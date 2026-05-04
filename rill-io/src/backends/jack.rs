//! JACK бэкенд — OutputWindow, без ring buffer для output.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use jack::{AudioOut, Client, ClientOptions, Control, Port, ProcessHandler, ProcessScope};

use crate::audio_io::{AudioIo, IoResult as AudioIoResult};
use crate::backend::{AudioBackend, BackendType};
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};

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

/// Mutable view into a JACK port buffer chunk.
struct OutputWindow {
    ptr: *mut f32,
    capacity: usize,
}

impl OutputWindow {
    fn new(ptr: *mut f32, len: usize) -> Self {
        Self { ptr, capacity: len }
    }
    fn as_mut_slice(&mut self) -> &mut [f32] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.capacity) }
    }
}

/// Lock-free slot for the current output window.
#[derive(Copy, Clone)]
struct OutputSlot(*mut Option<OutputWindow>);
unsafe impl Send for OutputSlot {}
unsafe impl Sync for OutputSlot {}

impl OutputSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None)))
    }
    unsafe fn set(&self, w: OutputWindow) {
        *self.0 = Some(w);
    }
    unsafe fn clear(&self) {
        *self.0 = None;
    }
    unsafe fn as_mut(&self) -> Option<&mut OutputWindow> {
        (*self.0).as_mut()
    }
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(self.0));
    }
}

/// JACK audio backend.
pub struct JackBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    output_slot: OutputSlot,
    thread_handle: Option<thread::JoinHandle<()>>,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
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

        let xruns = Arc::new(AtomicU32::new(0));
        let running = Arc::new(AtomicBool::new(false));
        let process_cb = CbSlot::new();
        let output_slot = OutputSlot::new();

        let t_config = config.clone();
        let t_running = running.clone();
        let t_process_cb = process_cb;
        let t_slot = output_slot;

        let handle = thread::Builder::new()
            .name("rill-jack".into())
            .spawn(move || {
                run_jack_thread(t_process_cb, t_config, t_running, t_slot);
            })
            .map_err(|e| IoError::Backend(e.to_string()))?;

        Ok(Self {
            config,
            process_cb,
            output_slot,
            thread_handle: Some(handle),
            xruns,
            running,
        })
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

        // Split JACK output buffer into BUF_SIZE chunks and process each.
        // write_output() sums stereo to mono and writes directly into out.
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
        // Zero remaining frames
        if off < nframes {
            out[off..nframes].fill(0.0);
        }

        Control::Continue
    }
}

fn run_jack_thread(
    process_cb: CbSlot,
    config: AudioConfig,
    running: Arc<AtomicBool>,
    output_slot: OutputSlot,
) {
    loop {
        thread::park();
        if running.load(Ordering::Acquire) {
            break;
        }
        return;
    }

    let client_name = config.output_device.as_deref().unwrap_or("rill");

    let (client, _status) = match Client::new(client_name, ClientOptions::NO_START_SERVER) {
        Ok(c) => c,
        Err(e) => {
            log::error!("JACK client new: {e:?}");
            return;
        }
    };

    let out_port: Port<AudioOut> = match client.register_port("output", AudioOut) {
        Ok(p) => p,
        Err(e) => {
            log::error!("JACK output port: {e:?}");
            return;
        }
    };

    let handler = JackProcessHandler {
        process_cb,
        out_port,
        output_slot,
    };

    let active_client = match client.activate_async((), handler) {
        Ok(a) => a,
        Err(e) => {
            log::error!("JACK activate: {e:?}");
            return;
        }
    };

    // Auto-connect output port to JACK system playback ports.
    let jack_client = active_client.as_client();
    let out_port_name = format!("{}:output", client_name);
    for target in &["system:playback_1", "system:playback_2"] {
        if let Err(e) = jack_client.connect_ports_by_name(&out_port_name, target) {
            log::info!("JACK connect {} → {}: {}", out_port_name, target, e);
        }
    }

    while running.load(Ordering::Acquire) {
        thread::park();
    }

    drop(active_client);
}

// ============================================================================
// AudioIo impl
// ============================================================================

impl AudioIo for JackBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn read_input(&self, _left: &mut [f32], _right: &mut [f32]) -> usize {
        0
    }

    fn write_output(&self, left: &[f32], right: &[f32]) -> usize {
        let n = left.len().min(right.len());
        if let Some(win) = unsafe { self.output_slot.as_mut() } {
            let cap = win.capacity.min(n);
            let dst = win.as_mut_slice();
            // Sum stereo to mono
            for i in 0..cap {
                dst[i] = (left[i] + right[i]) * 0.5;
            }
            cap
        } else {
            0
        }
    }

    fn start(&self) -> AudioIoResult<()> {
        self.running.store(true, Ordering::Release);
        if let Some(handle) = &self.thread_handle {
            handle.thread().unpark();
        }
        Ok(())
    }

    fn stop(&self) -> AudioIoResult<()> {
        self.running.store(false, Ordering::Release);
        if let Some(handle) = &self.thread_handle {
            handle.thread().unpark();
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
        self.running.store(true, Ordering::Release);
        if let Some(handle) = &self.thread_handle {
            handle.thread().unpark();
        }
        Ok(())
    }

    fn stop(&mut self) -> IoResult<()> {
        self.running.store(false, Ordering::Release);
        if let Some(handle) = &self.thread_handle {
            handle.thread().unpark();
        }
        thread::sleep(Duration::from_millis(50));
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

    fn latency(&self) -> Duration {
        Duration::from_micros(
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
        if let Some(handle) = &self.thread_handle {
            handle.thread().unpark();
        }
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        unsafe {
            self.process_cb.drop_box();
            self.output_slot.drop_box();
        }
    }
}
