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

use jack::{AudioOut, Client, ClientOptions, Control, Port, ProcessHandler, ProcessScope};

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

        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            output_slot: OutputSlot::new(),
            xruns: Arc::new(AtomicU32::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            active_client: UnsafeCell::new(None),
        })
    }

    /// Common setup: create JACK client, register port, activate.
    /// Called from `run()` (non‑blocking) and `AudioBackend::start()`.
    fn setup(&self) -> Result<(), String> {
        let client_name = self.config.output_device.as_deref().unwrap_or("rill");

        let (client, _status) = Client::new(client_name, ClientOptions::NO_START_SERVER)
            .map_err(|e| format!("JACK client new: {e:?}"))?;

        let out_port: Port<AudioOut> = client
            .register_port("output", AudioOut)
            .map_err(|e| format!("JACK output port: {e:?}"))?;

        let handler = JackProcessHandler {
            process_cb: self.process_cb,
            out_port,
            output_slot: self.output_slot.clone(),
        };

        let active_client = client
            .activate_async((), handler)
            .map_err(|e| format!("JACK activate: {e:?}"))?;

        // Auto-connect to system playback ports
        let jack_client = active_client.as_client();
        let out_port_name = format!("{client_name}:output");
        for target in &["system:playback_1", "system:playback_2"] {
            if let Err(e) = jack_client.connect_ports_by_name(&out_port_name, target) {
                log::info!("JACK connect {out_port_name} → {target}: {e}");
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
    out_port: Port<AudioOut>,
    output_slot: OutputSlot,
}

impl ProcessHandler for JackProcessHandler {
    fn process(&mut self, _client: &Client, ps: &ProcessScope) -> Control {
        let nframes = ps.n_frames() as usize;
        let out = self.out_port.as_mut_slice(ps);

        let chunk = 256usize;
        let mut off = 0usize;
        while off + chunk <= nframes {
            unsafe {
                self.output_slot
                    .set(OutputWindow::new(out.as_mut_ptr().add(off), chunk));
                self.process_cb.call();
                self.output_slot.clear();
            }
            off += chunk;
        }
        if off < nframes {
            out[off..nframes].fill(0.0);
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

    fn read(&self, _channels: &mut [&mut [f32]]) -> usize {
        0
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
