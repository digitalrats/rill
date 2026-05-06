//! JACK бэкенд — OutputWindow, без ring buffer для output.
//!
//! `run()` — non‑blocking: создаёт JACK client, активирует, сохраняет
//! handle и возвращается. Process callback работает на JACK RT thread.
//! `stop()` дропает handle → JACK деактивируется.
//! Никаких `std::thread`, `std::sync`.

use std::cell::UnsafeCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use jack::{AudioIn, AudioOut, Client, ClientOptions, Control, Port, ProcessHandler, ProcessScope};

use crate::buffer::IoRingBuffer;

use crate::backend::{AudioBackend, BackendType};
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use crate::output_window::{OutputSlot, OutputWindow};
use rill_core::io::IoBackend;

/// Callback slot.
#[derive(Copy, Clone)]
struct CbSlot(usize);
unsafe impl Send for CbSlot {}
unsafe impl Sync for CbSlot {}

impl CbSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None::<Box<dyn Fn()>>)) as usize)
    }
    unsafe fn set(&self, cb: Box<dyn Fn()>) {
        (*(self.0 as *mut Option<Box<dyn Fn()>>)) = Some(cb);
    }
    unsafe fn call(&self) {
        if let Some(ref cb) = *(self.0 as *mut Option<Box<dyn Fn()>>) {
            cb();
        }
    }
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(self.0 as *mut Option<Box<dyn Fn()>>));
    }
}

/// JACK audio backend.
pub struct JackBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    output_slot: OutputSlot,
    input_ring: Arc<IoRingBuffer>,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    /// Stores the active JACK client handle.
    /// Set once in `run()` (audio thread), taken once in `stop()` (control thread).
    active_client: UnsafeCell<Option<jack::AsyncClient<(), JackProcessHandler>>>,
}

unsafe impl Send for JackBackend {}
unsafe impl Sync for JackBackend {}

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
        })
    }

    /// Common setup: create JACK client, register ports, activate.
    /// Called from `run()` (non‑blocking) and `AudioBackend::start()`.
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
        let handler = JackProcessHandler {
            process_cb: self.process_cb,
            out_port,
            in_ports,
            in_ch,
            output_slot: self.output_slot.clone(),
            input_ring: self.input_ring.clone(),
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
}

impl ProcessHandler for JackProcessHandler {
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
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
                    self.process_cb.call();
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
                    self.process_cb.call();
                }
            }
        }

        Control::Continue
    }
}

// ============================================================================
// IoBackend impl
// ============================================================================

impl IoBackend<f32> for JackBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn read(&self, channels: &mut [&mut [f32]]) -> usize {
        let frames = channels.first().map(|c| c.len()).unwrap_or(0);
        if frames == 0 {
            return 0;
        }
        let out_ch = self.config.input_channels.max(1) as usize;
        let cap = frames.saturating_mul(out_ch).min(4096);
        let mut temp = [0.0f32; 4096];
        let n = self.input_ring.read(&mut temp[..cap]);
        let frames_out = n / out_ch;
        let out = frames_out.min(frames);
        if out_ch >= 2 {
            for i in 0..out {
                if let Some(c) = channels.get_mut(0) {
                    c[i] = temp[i * out_ch];
                }
                if let Some(c) = channels.get_mut(1) {
                    c[i] = temp[i * out_ch + 1];
                }
            }
        } else {
            for i in 0..out {
                if let Some(c) = channels.get_mut(0) {
                    c[i] = temp[i];
                }
            }
        }
        out
    }

    fn write(&self, channels: &[&[f32]]) -> usize {
        let frames = channels.first().map(|c| c.len()).unwrap_or(0);
        if let Some(win) = unsafe { self.output_slot.as_mut() } {
            let cap = win.capacity().min(frames);
            let dst = win.as_mut_slice();
            let left = channels.first().copied().unwrap_or(&[]);
            let right = channels.get(1).copied().unwrap_or(left);
            for i in 0..cap {
                dst[i] = (left[i] + right[i]) * 0.5;
            }
            cap
        } else {
            0
        }
    }

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

// ============================================================================
// AudioBackend impl
// ============================================================================

impl AudioBackend for JackBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Jack
    }
    fn config(&self) -> &AudioConfig {
        &self.config
    }
    fn config_mut(&mut self) -> &mut AudioConfig {
        &mut self.config
    }
    fn init(&mut self) -> IoResult<()> {
        Ok(())
    }

    fn start(&mut self) -> IoResult<()> {
        self.setup().map_err(|e| IoError::Backend(e))
    }

    fn stop(&mut self) -> IoResult<()> {
        self.running.store(false, Ordering::Release);
        unsafe {
            if let Some(client) = (*self.active_client.get()).take() {
                drop(client);
            }
        }
        Ok(())
    }

    fn read(&mut self, _buffer: &mut [f32]) -> IoResult<usize> {
        Ok(0)
    }
    fn write(&mut self, _buffer: &[f32]) -> IoResult<usize> {
        Ok(0)
    }
    fn xruns(&self) -> u32 {
        self.xruns.load(Ordering::Acquire)
    }

    fn latency(&self) -> std::time::Duration {
        std::time::Duration::from_micros(
            (1_000_000.0 * self.config.buffer_size as f64 / self.config.sample_rate as f64) as u64,
        )
    }

    fn list_input_devices(&self) -> Vec<String> {
        vec!["default".to_string()]
    }
    fn list_output_devices(&self) -> Vec<String> {
        vec!["default".to_string()]
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
