//! JACK backend — OutputWindow, no ring buffer for output.
//!
//! `run()` — non-blocking: creates JACK client, activates, saves
//! the handle and returns. Process callback runs on JACK RT thread.
//! `stop()` drops the handle → JACK deactivates.
//! No `std::thread`, `std::sync`.

use std::cell::UnsafeCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use jack::{AudioIn, AudioOut, Client, ClientOptions, Control, Port, ProcessHandler, ProcessScope};

use crate::buffer::IoRingBuffer;

use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use crate::output_window::{OutputSlot, OutputWindow};
use rill_core::io::IoBackend;
use rill_core::time::{ClockTick, SystemClock};
use rill_core::traits::buffer_view::{BufferView, NullBufferView};

/// Callback slot.
#[derive(Copy, Clone)]
struct CbSlot(usize);

impl CbSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None::<Box<dyn Fn(f32)>>)) as usize)
    }
    unsafe fn set(&self, cb: Box<dyn Fn(f32)>) {
        (*(self.0 as *mut Option<Box<dyn Fn(f32)>>)) = Some(cb);
    }
    unsafe fn call(&self, sr: f32) {
        if let Some(ref cb) = *(self.0 as *mut Option<Box<dyn Fn(f32)>>) {
            cb(sr);
        }
    }
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(self.0 as *mut Option<Box<dyn Fn(f32)>>));
    }
}

/// JACK audio backend.
pub struct JackBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    output_slot: OutputSlot,
    input_ring: Arc<IoRingBuffer>,
    #[allow(dead_code)]
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    /// Stores the active JACK client handle.
    /// Set once in `run()` (audio thread), taken once in `stop()` (control thread).
    active_client: UnsafeCell<Option<jack::AsyncClient<(), JackProcessHandler>>>,
    /// Optional shared system clock for JACK transport sync.
    /// When set, the process callback queries JACK transport state and
    /// updates BPM atomically.
    sys_clock: Option<Arc<SystemClock>>,
}

impl fmt::Debug for JackBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JackBackend")
            .field("config", &self.config)
            .field("running", &self.running)
            .finish()
    }
}

impl JackBackend {
    /// Create a new JACK backend.
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            return Err(IoError::Unsupported(
                "JACK is only available on Linux and macOS".into(),
            ));
        }

        // JACK periods (nframes) can be 1024-4096, so ring buffer needs
        // to hold multiple periods. Use 32x multiplier like PipeWire.
        let buf_cap = (config.buffer_size * config.input_channels.max(1) * 32) as usize;
        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            output_slot: OutputSlot::new(),
            input_ring: Arc::new(IoRingBuffer::new(buf_cap)),
            xruns: Arc::new(AtomicU32::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            active_client: UnsafeCell::new(None),
            sys_clock: None,
        })
    }

    /// Attach a shared [`SystemClock`] for JACK transport sync.
    ///
    /// When set, the JACK process callback queries `client.transport().query()`
    /// on every block and updates the clock's BPM atomically if the transport
    /// is rolling and BBT data is valid.
    ///
    /// Call this **before** `run()` so the process handler receives the clock.
    pub fn set_system_clock(&mut self, clock: Arc<SystemClock>) {
        self.sys_clock = Some(clock);
    }

    /// Common setup: create JACK client, register ports, activate.
    /// Called from `run()` (non‑blocking).
    fn setup(&self) -> Result<(), String> {
        let client_name = self.config.output_device.as_deref().unwrap_or("rill");

        let (client, _status) = Client::new(client_name, ClientOptions::NO_START_SERVER)
            .map_err(|e| format!("JACK client new: {e:?}"))?;

        let out_port: Option<Port<AudioOut>> = if self.config.output_channels > 0 {
            Some(
                client
                    .register_port("output", AudioOut)
                    .map_err(|e| format!("JACK output port: {e:?}"))?,
            )
        } else {
            None
        };

        let in_ports: Vec<Port<AudioIn>> = if self.config.input_channels > 0 {
            let n = self.config.input_channels.min(2) as usize;
            let mut ports = Vec::with_capacity(n);
            for i in 0..n {
                let name = if n == 1 {
                    "input".into()
                } else {
                    format!("input_{}", i + 1)
                };
                ports.push(
                    client
                        .register_port(&name, AudioIn)
                        .map_err(|e| format!("JACK input port {name}: {e:?}"))?,
                );
            }
            ports
        } else {
            Vec::new()
        };

        // Auto-connect ports (before activation, ports are already registered)
        let out_port_name = out_port.as_ref().and_then(|p| p.name().ok());
        let in_port_names: Vec<_> = in_ports.iter().filter_map(|p| p.name().ok()).collect();

        let in_ch = self.config.input_channels.max(1) as usize;
        let sample_rate = client.sample_rate() as f32;
        let handler = JackProcessHandler {
            process_cb: self.process_cb,
            out_port,
            in_ports,
            in_ch,
            output_slot: self.output_slot.clone(),
            input_ring: self.input_ring.clone(),
            sample_rate,
            sys_clock: self.sys_clock.clone(),
        };

        let active_client = client
            .activate_async((), handler)
            .map_err(|e| format!("JACK activate: {e:?}"))?;

        let jack_client = active_client.as_client();
        if let Some(name) = out_port_name {
            for target in &["system:playback_1", "system:playback_2"] {
                if let Err(e) = jack_client.connect_ports_by_name(&name, target) {
                    log::info!("JACK connect {name} → {target}: {e}");
                }
            }
        }
        for (i, name) in in_port_names.iter().enumerate() {
            let src = format!("system:capture_{}", i + 1);
            if let Err(e) = jack_client.connect_ports_by_name(&src, name) {
                log::info!("JACK connect {src} → {name}: {e}");
            }
        }

        self.running.store(true, Ordering::Release);
        unsafe {
            *self.active_client.get() = Some(active_client);
        }
        Ok(())
    }
}

// ─── ProcessHandler ──────────────────────────────────────────────────────

struct JackProcessHandler {
    process_cb: CbSlot,
    out_port: Option<Port<AudioOut>>,
    in_ports: Vec<Port<AudioIn>>,
    in_ch: usize,
    output_slot: OutputSlot,
    input_ring: Arc<IoRingBuffer>,
    sample_rate: f32,
    sys_clock: Option<Arc<SystemClock>>,
}

impl ProcessHandler for JackProcessHandler {
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        // JACK Transport sync — update BPM from transport master
        if let Some(ref clock) = self.sys_clock {
            if let Ok(state) = _client.transport().query() {
                if state.pos.valid_bbt() {
                    if let Some(bbt) = state.pos.bbt() {
                        clock.set_bpm(bbt.bpm);
                    }
                }
            }
        }

        let nframes = ps.n_frames() as usize;

        // Capture: read input ports → ring buffer (interleaved)
        if !self.in_ports.is_empty() {
            let n_samp = nframes * self.in_ch;
            let max_samp = n_samp.min(4096);
            let mut temp = [0.0f32; 4096];
            let len = max_samp;
            for i in 0..nframes.min(len / self.in_ch) {
                for ch in 0..self.in_ch.min(2) {
                    if ch < self.in_ports.len() {
                        let src = self.in_ports[ch].as_slice(ps);
                        if i < src.len() {
                            temp[i * self.in_ch + ch] = src[i];
                        }
                    }
                }
            }
            self.input_ring.write(&temp[..len]);
        }

        // Playback: process graph → output port
        if let Some(ref mut out) = self.out_port {
            let buf = out.as_mut_slice(ps);
            let chunk = 256usize;
            let mut off = 0usize;
            while off + chunk <= nframes {
                unsafe {
                    self.output_slot
                        .set(OutputWindow::new(buf.as_mut_ptr().add(off), chunk));
                    self.process_cb.call(self.sample_rate);
                    self.output_slot.clear();
                }
                off += chunk;
            }
            if off < nframes {
                buf[off..nframes].fill(0.0);
            }
        } else if !self.in_ports.is_empty() {
            // Capture-only: process all available blocks
            let chunk_samps = 256 * self.in_ch;
            while self.input_ring.len() >= chunk_samps {
                unsafe {
                    self.process_cb.call(self.sample_rate);
                }
            }
        }

        Control::Continue
    }
}

// ============================================================================
// IoBackend impl
// ============================================================================

impl IoBackend for JackBackend {
    fn create_view(&self) -> Arc<dyn BufferView> {
        Arc::new(NullBufferView::new(0, 0))
    }

    fn set_process_callback(&self, _cb: Box<dyn FnMut(&ClockTick)>) {}

    fn run(&self, _running: Arc<AtomicBool>) -> Result<(), String> {
        self.setup()
    }

    fn stop(&self) -> Result<(), String> {
        self.running.store(false, Ordering::Release);
        unsafe {
            if let Some(client) = (*self.active_client.get()).take() {
                drop(client);
            }
        }
        Ok(())
    }
}

impl Drop for JackBackend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        unsafe {
            if let Some(client) = (*self.active_client.get()).take() {
                drop(client);
            }
            self.process_cb.drop_box();
        }
    }
}
