//! ALSA бэкенд для Linux — без crossbeam, без parking_lot.
//!
//! Thread запускается сразу, ждёт `Start` через `Arc<AtomicBool>` +
//! `thread::park`/`unpark`. После старта — event-driven ALSA loop
//! (`snd_pcm_wait`), никакого `thread::sleep`.
//!
//! Output пишет напрямую в ALSA-буфер через OutputWindow (без ring buffer).
//! Input по-прежнему использует IoRingBuffer.

const MAX_BLOCK_SAMPLES: usize = 8192;

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use alsa::pcm::{Access, Format, HwParams};
use alsa::{Direction, ValueOr, PCM};

use crate::backend::{AudioBackend, BackendType};
use crate::buffer::IoRingBuffer;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use rill_core::io::IoBackend;

// ============================================================================
// OutputWindow — writable slice into ALSA's interleaved buffer
// ============================================================================

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

// ============================================================================
// Callback slot
// ============================================================================

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

// ============================================================================
// AlsaBackend
// ============================================================================

/// ALSA audio backend.
pub struct AlsaBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    output_slot: OutputSlot,
    xruns: Arc<AtomicU32>,
    input_buffer: Arc<IoRingBuffer>,
    thread_handle: Option<thread::JoinHandle<()>>,
    running: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
}

unsafe impl Send for AlsaBackend {}
unsafe impl Sync for AlsaBackend {}

impl fmt::Debug for AlsaBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlsaBackend")
            .field("config", &self.config)
            .field("running", &self.running.load(Ordering::Relaxed))
            .finish()
    }
}

impl AlsaBackend {
    /// Create a new ALSA backend.
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        let buffer_size = (config.buffer_size * config.output_channels * 4) as usize;
        let xruns = Arc::new(AtomicU32::new(0));
        let input_buffer = Arc::new(IoRingBuffer::new(buffer_size));
        let running = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicBool::new(false));
        let output_slot = OutputSlot::new();
        let device_name = Arc::new(Mutex::new(
            config
                .output_device
                .clone()
                .unwrap_or_else(|| "default".to_string()),
        ));

        let process_cb = CbSlot::new();
        let thread_cb = process_cb;
        let thread_xruns = xruns.clone();
        let thread_input = input_buffer.clone();
        let thread_output = output_slot.clone();
        let thread_config = config.clone();
        let thread_running = running.clone();
        let thread_started = started.clone();
        let thread_device_name = device_name.clone();

        let handle = thread::spawn(move || {
            alsa_thread(
                thread_cb,
                thread_xruns,
                thread_input,
                thread_output,
                thread_config,
                thread_running,
                thread_started,
                thread_device_name,
            );
        });

        Ok(Self {
            config,
            process_cb,
            output_slot,
            xruns,
            input_buffer,
            thread_handle: Some(handle),
            running,
            started,
        })
    }
}

// ============================================================================
// ALSA thread
// ============================================================================

fn alsa_thread(
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    input_buffer: Arc<IoRingBuffer>,
    output_slot: OutputSlot,
    config: AudioConfig,
    running: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
    device_name: Arc<Mutex<String>>,
) {
    loop {
        thread::park();
        if running.load(Ordering::Acquire) {
            break;
        }
    }

    let dev_name = device_name.lock().unwrap().clone();

    let pcm_playback = match PCM::new(&dev_name, Direction::Playback, false) {
        Ok(pcm) => pcm,
        Err(e) => {
            eprintln!("ALSA open {}: {}", dev_name, e);
            return;
        }
    };

    let pcm_capture: Option<PCM> = if config.input_channels > 0 {
        match PCM::new(&dev_name, Direction::Capture, false) {
            Ok(pcm) => Some(pcm),
            Err(e) => {
                log::warn!("ALSA capture {}: disabled", e);
                None
            }
        }
    } else {
        None
    };

    if let Err(e) = configure_pcm(&pcm_playback, config.output_channels, &config) {
        eprintln!("ALSA configure playback: {}", e);
        return;
    }
    if let Some(ref pcm) = pcm_capture {
        if let Err(e) = configure_pcm(pcm, config.input_channels, &config) {
            log::warn!("ALSA configure capture: {}", e);
        }
    }

    if let Err(e) = pcm_playback.start() {
        eprintln!("ALSA start: {}", e);
        return;
    }
    if let Some(ref pcm) = pcm_capture {
        let _ = pcm.start();
    }

    started.store(true, Ordering::Release);

    let buf_frames = config.buffer_size as usize;
    let in_sz = (buf_frames * config.input_channels as usize)
        .max(1)
        .min(MAX_BLOCK_SAMPLES);
    let mut f32_buf = [0.0f32; 2048]; // max 1024 stereo frames
    let mut i16_buf = [0i16; 4096]; // max 2048 stereo frames × 2 ch
    let mut cb_i16 = [0i16; 4096];

    while running.load(Ordering::Acquire) {
        match pcm_playback.wait(None) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                eprintln!("ALSA wait: {}", e);
                if let Err(r) = pcm_playback.try_recover(e, true) {
                    break;
                }
                continue;
            }
        }

        // Capture → input ring
        if let Some(ref pcm) = pcm_capture {
            if let Ok(io) = pcm.io_i16() {
                if let Ok(n_read) = io.readi(&mut cb_i16[..in_sz]) {
                    let n = (n_read * config.input_channels as usize).min(in_sz);
                    let mut temp = [0.0f32; MAX_BLOCK_SAMPLES];
                    for (i, s) in cb_i16[..n].iter().enumerate() {
                        temp[i] = *s as f32 / 32768.0;
                    }
                    input_buffer.write(&temp[..n]);
                }
            }
        }

        // Available frames for playback
        let avail = match pcm_playback.avail_update() {
            Ok(a) => a as usize,
            Err(e) => {
                if let Err(r) = pcm_playback.try_recover(e, true) {
                    break;
                }
                continue;
            }
        };

        let out_channels = config.output_channels as usize;
        let mut written = 0usize;
        while written + buf_frames <= avail {
            let chunk_frames = buf_frames;
            let chunk_samples = chunk_frames * out_channels;

            // Set OutputWindow → call process_cb → write directly into f32_buf
            unsafe {
                output_slot.set(OutputWindow::new(f32_buf.as_mut_ptr(), chunk_samples));
                process_cb.call();
                output_slot.clear();
            }

            // Convert f32 → i16 interleaved
            for i in 0..chunk_samples {
                i16_buf[i] = (f32_buf[i].clamp(-1.0, 1.0) * 32767.0) as i16;
            }

            // Write to ALSA
            match pcm_playback.io_i16() {
                Ok(io) => match io.writei(&i16_buf[..chunk_samples]) {
                    Ok(n) => written += n,
                    Err(e) => {
                        eprintln!("ALSA write: {}", e);
                        xruns.fetch_add(1, Ordering::Relaxed);
                        if let Err(r) = pcm_playback.try_recover(e, true) {
                            break;
                        }
                    }
                },
                Err(e) => {
                    eprintln!("ALSA io_i16: {}", e);
                    break;
                }
            }
        }
    }

    let _ = pcm_playback.drain();
    if let Some(ref pcm) = pcm_capture {
        let _ = pcm.drain();
    }
}

// ============================================================================
// PCM configuration
// ============================================================================

fn configure_pcm(pcm: &PCM, channels: u32, config: &AudioConfig) -> IoResult<()> {
    let hw = HwParams::any(pcm).map_err(|e| IoError::Config(e.to_string()))?;
    hw.set_access(Access::RWInterleaved)
        .map_err(|e| IoError::Config(e.to_string()))?;
    hw.set_format(Format::s16())
        .map_err(|e| IoError::Config(e.to_string()))?;
    hw.set_rate(config.sample_rate, ValueOr::Nearest)
        .map_err(|e| IoError::Config(e.to_string()))?;
    hw.set_channels(channels)
        .map_err(|e| IoError::Config(e.to_string()))?;
    hw.set_buffer_size(config.buffer_size as alsa::pcm::Frames)
        .map_err(|e| IoError::Config(e.to_string()))?;
    pcm.hw_params(&hw)
        .map_err(|e| IoError::Config(e.to_string()))?;
    Ok(())
}

// ============================================================================
// AudioBackend impl
// ============================================================================

impl AudioBackend for AlsaBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Alsa
    }
    fn config(&self) -> &AudioConfig {
        &self.config
    }
    fn config_mut(&mut self) -> &mut AudioConfig {
        &mut self.config
    }

    fn init(&mut self) -> IoResult<()> {
        let cap = self.input_buffer.capacity();
        self.input_buffer.write(&vec![0.0f32; cap]);
        Ok(())
    }

    fn start(&mut self) -> IoResult<()> {
        self.running.store(true, Ordering::Release);
        if let Some(handle) = &self.thread_handle {
            handle.thread().unpark();
        }
        while !self.started.load(Ordering::Acquire) {
            thread::yield_now();
        }
        Ok(())
    }

    fn stop(&mut self) -> IoResult<()> {
        self.running.store(false, Ordering::Release);
        self.started.store(false, Ordering::Release);
        if let Some(handle) = &self.thread_handle {
            handle.thread().unpark();
        }
        Ok(())
    }

    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize> {
        Ok(self.input_buffer.read(buffer))
    }

    fn write(&mut self, buffer: &[f32]) -> IoResult<usize> {
        Ok(buffer.len())
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
        vec![
            "default".into(),
            "hw:0,0".into(),
            "hw:1,0".into(),
            "plughw:0,0".into(),
            "plughw:1,0".into(),
        ]
    }

    fn list_output_devices(&self) -> Vec<String> {
        vec![
            "default".into(),
            "hw:0,0".into(),
            "hw:1,0".into(),
            "plughw:0,0".into(),
            "plughw:1,0".into(),
            "dmix:0".into(),
        ]
    }
}

// ============================================================================
// IoBackend impl
// ============================================================================

impl IoBackend<f32> for AlsaBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn read(&self, channels: &mut [&mut [f32]]) -> usize {
        let frames = channels.first().map(|c| c.len()).unwrap_or(0);
        let cap = frames.min(256).saturating_mul(2);
        let mut temp = [0.0f32; 512];
        let n = self.input_buffer.read(&mut temp[..cap]);
        let frames_out = n / 2;
        for i in 0..frames_out.min(frames) {
            if let Some(ch) = channels.get_mut(0) {
                ch[i] = temp[i * 2];
            }
            if let Some(ch) = channels.get_mut(1) {
                ch[i] = temp[i * 2 + 1];
            }
        }
        frames_out
    }

    fn write(&self, channels: &[&[f32]]) -> usize {
        let frames = channels.first().map(|c| c.len()).unwrap_or(0);
        if frames == 0 {
            return 0;
        }
        unsafe {
            if let Some(win) = self.output_slot.as_mut() {
                let cap = win.capacity.min(frames * 2);
                let dst = win.as_mut_slice();
                for i in 0..(cap / 2) {
                    if let Some(ch) = channels.get(0) {
                        dst[i * 2] = ch[i];
                    }
                    if let Some(ch) = channels.get(1) {
                        dst[i * 2 + 1] = ch[i];
                    }
                }
                cap / 2
            } else {
                0
            }
        }
    }

    fn start(&self) -> Result<(), String> {
        self.running.store(true, Ordering::Release);
        if let Some(handle) = &self.thread_handle {
            handle.thread().unpark();
        }
        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        self.running.store(false, Ordering::Release);
        self.started.store(false, Ordering::Release);
        Ok(())
    }
}

impl Drop for AlsaBackend {
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
        }
        unsafe {
            self.output_slot.drop_box();
        }
    }
}
