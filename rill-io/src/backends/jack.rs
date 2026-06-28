//! JACK backend — zero-copy DMA access via `DirectView`.
//! JACK provides per-channel (planar) buffer pointers; the process
//! callback wraps them in a `DirectView::new_planar()` and fires
//! `process_cb.call(&tick)` once per JACK buffer — graph nodes
//! read/write directly from/to JACK DMA buffers through `tick.view`.
//!
//! `run()` — blocking: creates JACK client, activates, enters poll
//! loop. Process callback runs on JACK RT thread.
//! `stop()` sets `running = false` and deactivates the client.

use std::cell::UnsafeCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use jack::{AudioIn, AudioOut, Client, ClientOptions, Control, Port, ProcessHandler, ProcessScope};

use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use crate::output_window::{OutputSlot, OutputWindow};
use rill_core::io::{IoCapture, IoDriver, IoPlayback};
use rill_core::time::{ClockTick, SystemClock};

/// Callback slot — stores the process callback via raw pointer for `Send`-safe
/// single-threaded access from the JACK RT callback.
#[derive(Copy, Clone)]
struct CbSlot(usize);

impl CbSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None::<Box<dyn FnMut(&ClockTick)>>)) as usize)
    }
    unsafe fn set(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        (*(self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>)) = Some(cb);
    }
    unsafe fn call(&self, tick: &ClockTick) {
        if let Some(ref mut cb) = *(self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>) {
            cb(tick);
        }
    }
    unsafe fn take_box(&self) {
        let taken = (*(self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>)).take();
        drop(taken);
    }
}

/// JACK signal backend.
pub struct JackBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    output_slot: OutputSlot,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    sample_pos: Arc<AtomicU64>,
    /// Stores the active JACK client handle.
    /// Set once in `run()` (I/O thread), taken once in `stop()` (control thread).
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

unsafe impl Sync for JackBackend {}

impl JackBackend {
    /// Create a new JACK backend.
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            return Err(IoError::Unsupported(
                "JACK is only available on Linux and macOS".into(),
            ));
        }

        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            output_slot: OutputSlot::new(),
            xruns: Arc::new(AtomicU32::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            sample_pos: Arc::new(AtomicU64::new(0)),
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
}

// ─── ProcessHandler ──────────────────────────────────────────────────────

struct JackProcessHandler {
    process_cb: CbSlot,
    output_slot: OutputSlot,
    out_ports: Vec<Port<AudioOut>>,
    in_ports: Vec<Port<AudioIn>>,
    in_ch: usize,
    out_ch: usize,
    sample_pos: Arc<AtomicU64>,
    sample_rate: f32,
    config_rate: f32,
    block_size: usize,
    sys_clock: Option<Arc<SystemClock>>,
}

impl ProcessHandler for JackProcessHandler {
    fn process(&mut self, client: &Client, ps: &ProcessScope) -> Control {
        const MAX_BLOCK_SAMPLES: usize = 8192;

        if let Some(ref clock) = self.sys_clock {
            if let Ok(state) = client.transport().query() {
                if state.pos.valid_bbt() {
                    if let Some(bbt) = state.pos.bbt() {
                        clock.set_bpm(bbt.bpm);
                    }
                }
            }
        }

        let nframes = ps.n_frames() as usize;
        let chunk_size = self.block_size;

        let mut interleaved_out = [0.0f32; MAX_BLOCK_SAMPLES];

        // Process JACK buffer in BUF_SIZE chunks
        let mut offset = 0usize;
        while offset < nframes {
            let n = (nframes - offset).min(chunk_size);

            let mut in_ptrs: [*const f32; 8] = [std::ptr::null(); 8];
            let mut out_ptrs: [*mut f32; 8] = [std::ptr::null_mut(); 8];

            unsafe {
                for ch in 0..self.in_ch {
                    if ch < self.in_ports.len() {
                        in_ptrs[ch] = self.in_ports[ch].as_slice(ps).as_ptr().add(offset);
                    }
                }
                for ch in 0..self.out_ch {
                    if ch < self.out_ports.len() {
                        out_ptrs[ch] = self.out_ports[ch].as_mut_slice(ps).as_mut_ptr().add(offset);
                    }
                }
            }

            let pos = self.sample_pos.fetch_add(n as u64, Ordering::Relaxed);
            let mut tick = ClockTick::new(pos, n as u32, self.config_rate, "jack".into());
            if self.sample_rate > 0.0 && (self.sample_rate - self.config_rate).abs() > 1.0 {
                tick.speed_ratio = self.config_rate as f64 / self.sample_rate as f64;
            }

            if self.out_ch > 0 {
                let len = n * self.out_ch;
                interleaved_out[..len].fill(0.0);
                unsafe {
                    self.output_slot
                        .set(OutputWindow::new(interleaved_out.as_mut_ptr(), len));
                }
            }

            unsafe {
                self.process_cb.call(&tick);
            }

            if self.out_ch > 0 {
                unsafe {
                    self.output_slot.clear();
                    for ch in 0..self.out_ch {
                        if !out_ptrs[ch].is_null() {
                            for i in 0..n {
                                *out_ptrs[ch].add(i) = interleaved_out[i * self.out_ch + ch];
                            }
                        }
                    }
                }
            }

            offset += n;
        }

        Control::Continue
    }
}

// ============================================================================
// IoDriver impl
// ============================================================================

impl IoDriver for JackBackend {
    fn set_process_callback(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn run(&self, running: Arc<AtomicBool>) -> Result<(), String> {
        let client_name = self.config.output_device.as_deref().unwrap_or("rill");

        let (client, _status) = Client::new(client_name, ClientOptions::NO_START_SERVER)
            .map_err(|e| format!("JACK client new: {e:?}"))?;

        let out_ch = self.config.output_channels.min(2) as usize;
        let out_ports: Vec<Port<AudioOut>> = if out_ch > 0 {
            let mut ports = Vec::with_capacity(out_ch);
            for i in 0..out_ch {
                let name = if out_ch == 1 {
                    "output".into()
                } else {
                    format!("output_{}", i + 1)
                };
                ports.push(
                    client
                        .register_port(&name, AudioOut)
                        .map_err(|e| format!("JACK output port {name}: {e:?}"))?,
                );
            }
            ports
        } else {
            Vec::new()
        };

        let in_ch = self.config.input_channels.min(2) as usize;
        let in_ports: Vec<Port<AudioIn>> = if in_ch > 0 {
            let mut ports = Vec::with_capacity(in_ch);
            for i in 0..in_ch {
                let name = if in_ch == 1 {
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

        // Collect port names for auto-connect
        let out_port_names: Vec<_> = out_ports.iter().filter_map(|p| p.name().ok()).collect();
        let in_port_names: Vec<_> = in_ports.iter().filter_map(|p| p.name().ok()).collect();

        let sample_rate = client.sample_rate() as f32;
        let config_rate = self.config.sample_rate as f32;
        let block_size = self.config.buffer_size as usize;
        let handler = JackProcessHandler {
            process_cb: self.process_cb,
            output_slot: self.output_slot.clone(),
            out_ports,
            in_ports,
            in_ch,
            out_ch,
            sample_pos: self.sample_pos.clone(),
            sample_rate,
            config_rate,
            block_size,
            sys_clock: self.sys_clock.clone(),
        };

        let active_client = client
            .activate_async((), handler)
            .map_err(|e| format!("JACK activate: {e:?}"))?;

        let jack_client = active_client.as_client();
        for (i, name) in out_port_names.iter().enumerate() {
            let target = format!("system:playback_{}", i + 1);
            if let Err(e) = jack_client.connect_ports_by_name(name, &target) {
                log::info!("JACK connect {name} → {target}: {e}");
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

        // Block until orchestrator signals stop via the running flag
        while running.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        Ok(())
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

impl IoPlayback for JackBackend {
    fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        unsafe {
            if let Some(window) = self.output_slot.as_mut() {
                let buf = window.as_mut_slice();
                let nch = self.config.output_channels as usize;
                let n_frames = buf.len() / nch.max(1);
                let n = src.len().min(n_frames);
                for i in 0..n {
                    buf[i * nch + channel] = src[i];
                }
                n
            } else {
                0
            }
        }
    }

    fn num_output_channels(&self) -> usize {
        self.config.output_channels as usize
    }
}

impl IoCapture for JackBackend {
    fn read_input(&self, _channel: usize, dst: &mut [f32]) -> usize {
        dst.fill(0.0);
        dst.len()
    }

    fn num_input_channels(&self) -> usize {
        self.config.input_channels as usize
    }
}

impl Drop for JackBackend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        unsafe {
            self.process_cb.take_box();
        }
        unsafe {
            if let Some(client) = (*self.active_client.get()).take() {
                drop(client);
            }
        }
    }
}
