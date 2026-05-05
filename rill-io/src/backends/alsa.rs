//! ALSA бэкенд для Linux — без crossbeam, без parking_lot.
//!
//! Thread запускается сразу, ждёт `Start` через `Arc<AtomicBool>` +
//! `thread::park`/`unpark`. После старта — event-driven ALSA loop
//! (`snd_pcm_wait`), никакого `thread::sleep`.
//!
//! Все буферы в RT-пути — стековые фиксированного размера.

const MAX_BLOCK_SAMPLES: usize = 8192;

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use alsa::pcm::{Access, Format, HwParams};
use alsa::{Direction, ValueOr, PCM};

use crate::audio_io::AudioIo;
use crate::buffer::IoRingBuffer;

use crate::backend::{AudioBackend, BackendType};
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};

/// Callback slot — `*mut Option<Box<dyn Fn()>>` as `usize` (Send-friendly).
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

/// ALSA бэкенд
pub struct AlsaBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    input_buffer: Arc<IoRingBuffer>,
    output_buffer: Arc<IoRingBuffer>,
    thread_handle: Option<thread::JoinHandle<()>>,
    running: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
}

// Send+Sync: AudioIo требует Send, доступ последовательный.
unsafe impl Send for AlsaBackend {}
unsafe impl Sync for AlsaBackend {}

impl fmt::Debug for AlsaBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlsaBackend")
            .field("config", &self.config)
            .field("running", &self.running.load(Ordering::Relaxed))
            .field("thread_handle", &self.thread_handle.is_some())
            .finish()
    }
}

impl AlsaBackend {
    /// Create a new ALSA backend.
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        let buffer_size = (config.buffer_size * config.output_channels * 4) as usize;
        let xruns = Arc::new(AtomicU32::new(0));
        let input_buffer = Arc::new(IoRingBuffer::new(buffer_size));
        let output_buffer = Arc::new(IoRingBuffer::new(buffer_size));
        let running = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicBool::new(false));
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
        let thread_output = output_buffer.clone();
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
            xruns,
            input_buffer,
            output_buffer,
            thread_handle: Some(handle),
            running,
            started,
        })
    }
}

fn alsa_thread(
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    input_buffer: Arc<IoRingBuffer>,
    output_buffer: Arc<IoRingBuffer>,
    config: AudioConfig,
    running: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
    device_name: Arc<Mutex<String>>,
) {
    // Ждём первого Start (unpark от start()).
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
                log::warn!("ALSA capture {}: {} — disabled", dev_name, e);
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
            log::warn!("ALSA configure capture: {} — disabled", e);
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

    let out_buffer_size = (config.buffer_size * config.output_channels) as usize;
    let in_buffer_size = (config.buffer_size * config.input_channels) as usize;
    let out_sz = out_buffer_size.min(MAX_BLOCK_SAMPLES);
    let in_sz = in_buffer_size.max(1).min(MAX_BLOCK_SAMPLES);
    let mut pb = [0i16; MAX_BLOCK_SAMPLES];
    let mut cb = [0i16; MAX_BLOCK_SAMPLES];

    while running.load(Ordering::Acquire) {
        match pcm_playback.wait(None) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                eprintln!("ALSA wait: {}", e);
                if let Err(r) = pcm_playback.try_recover(e, true) {
                    eprintln!("ALSA recover failed: {}", r);
                    break;
                }
                continue;
            }
        }

        // Capture → input ring
        if let Some(ref pcm) = pcm_capture {
            if let Ok(io) = pcm.io_i16() {
                if let Ok(n_read) = io.readi(&mut cb[..in_sz]) {
                    let n = (n_read * config.input_channels as usize).min(in_sz);
                    let mut temp = [0.0f32; MAX_BLOCK_SAMPLES];
                    for (i, s) in cb[..n].iter().enumerate() {
                        temp[i] = *s as f32 / 32768.0;
                    }
                    input_buffer.write(&temp[..n]);
                }
            }
        }

        // Process graph
        unsafe {
            process_cb.call();
        }

        // Output ring → ALSA
        {
            let mut temp = [0.0f32; MAX_BLOCK_SAMPLES];
            let n = output_buffer.read(&mut temp[..out_sz]);
            for i in 0..out_sz {
                pb[i] = if i < n {
                    (temp[i].clamp(-1.0, 1.0) * 32767.0) as i16
                } else {
                    0
                };
            }
        }

        if let Ok(io) = pcm_playback.io_i16() {
            if let Err(e) = io.writei(&pb) {
                eprintln!("ALSA write: {}", e);
                xruns.fetch_add(1, Ordering::Relaxed);
                let _ = pcm_playback.try_recover(e, true);
            }
        }
    }

    let _ = pcm_playback.drain();
    if let Some(ref pcm) = pcm_capture {
        let _ = pcm.drain();
    }
}

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
        let zeros = vec![0.0f32; cap];
        self.input_buffer.write(&zeros);
        self.output_buffer.write(&zeros);
        Ok(())
    }

    fn start(&mut self) -> IoResult<()> {
        self.running.store(true, Ordering::Release);
        if let Some(handle) = &self.thread_handle {
            handle.thread().unpark();
        }
        // Ждём, пока тред подтвердит старт
        while !self.started.load(Ordering::Acquire) {
            thread::yield_now();
        }
        Ok(())
    }

    fn stop(&mut self) -> IoResult<()> {
        self.running.store(false, Ordering::Release);
        self.started.store(false, Ordering::Release);
        // unpark, чтобы тред проснулся и вышел из wait
        if let Some(handle) = &self.thread_handle {
            handle.thread().unpark();
        }
        Ok(())
    }

    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize> {
        let n = self.input_buffer.read(buffer);
        Ok(n)
    }

    fn write(&mut self, buffer: &[f32]) -> IoResult<usize> {
        let n = self.output_buffer.write(buffer);
        Ok(n)
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
            "default".to_string(),
            "hw:0,0".to_string(),
            "hw:1,0".to_string(),
            "plughw:0,0".to_string(),
            "plughw:1,0".to_string(),
        ]
    }

    fn list_output_devices(&self) -> Vec<String> {
        vec![
            "default".to_string(),
            "hw:0,0".to_string(),
            "hw:1,0".to_string(),
            "plughw:0,0".to_string(),
            "plughw:1,0".to_string(),
            "dmix:0".to_string(),
        ]
    }
}

impl AudioIo for AlsaBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn read_input(&self, left: &mut [f32], right: &mut [f32]) -> usize {
        let frames = left.len().min(right.len());
        let cap = frames.min(256).saturating_mul(2);
        let mut temp = [0.0f32; 512];
        let n = self.input_buffer.read(&mut temp[..cap]);
        let frames_out = n / 2;
        for i in 0..frames_out.min(frames) {
            left[i] = temp[i * 2];
            right[i] = temp[i * 2 + 1];
        }
        frames_out
    }

    fn write_output(&self, left: &[f32], right: &[f32]) -> usize {
        let frames = left.len().min(right.len());
        let cap = frames.min(256).saturating_mul(2);
        let mut temp = [0.0f32; 512];
        for i in 0..(cap / 2) {
            temp[i * 2] = left[i];
            temp[i * 2 + 1] = right[i];
        }
        self.output_buffer.write(&temp[..cap]) / 2
    }

    fn start(&self) -> crate::audio_io::IoResult<()> {
        self.running.store(true, Ordering::Release);
        if let Some(handle) = &self.thread_handle {
            handle.thread().unpark();
        }
        Ok(())
    }

    fn stop(&self) -> crate::audio_io::IoResult<()> {
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
    }
}
