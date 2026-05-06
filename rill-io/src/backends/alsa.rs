//! ALSA бэкенд для Linux — без `std::thread`, без `std::sync`.
//!
//! `run()` — блокирующий: открывает PCM, конфигурирует, входит в
//! `snd_pcm_wait()` loop. Выходит когда `running` становится false.
//! Очистка происходит внутри `run()` перед возвратом.

const MAX_BLOCK_SAMPLES: usize = 8192;

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alsa::pcm::{Access, Format, HwParams};
use alsa::{Direction, ValueOr, PCM};

use crate::backend::{AudioBackend, BackendType};
use crate::buffer::IoRingBuffer;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use crate::output_window::{OutputSlot, OutputWindow};
use rill_core::io::IoBackend;

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
    running: Arc<AtomicBool>,
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
        let ch = config.input_channels.max(config.output_channels).max(1);
        let buffer_size = (config.buffer_size * ch * 4) as usize;
        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            output_slot: OutputSlot::new(),
            xruns: Arc::new(AtomicU32::new(0)),
            input_buffer: Arc::new(IoRingBuffer::new(buffer_size)),
            running: Arc::new(AtomicBool::new(false)),
        })
    }
}

// ============================================================================
// ALSA I/O loop — called from `run()`
// ============================================================================

fn alsa_io_loop(
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    input_buffer: Arc<IoRingBuffer>,
    output_slot: OutputSlot,
    config: &AudioConfig,
    running: &AtomicBool,
) {
    let out_dev = config.output_device.as_deref().unwrap_or("default");

    // Open playback PCM only if output is configured
    let pcm_playback: Option<PCM> = if config.output_channels > 0 {
        match PCM::new(out_dev, Direction::Playback, false) {
            Ok(pcm) => {
                if let Err(e) = configure_pcm(&pcm, config.output_channels, config) {
                    eprintln!("ALSA configure playback: {}", e);
                    return;
                }
                // Start playback after buffer has 2 processing blocks.
                if let Ok(sw) = pcm.sw_params_current() {
                    let _ = sw.set_start_threshold((config.buffer_size * 2) as alsa::pcm::Frames);
                    let _ = pcm.sw_params(&sw);
                }
                Some(pcm)
            }
            Err(e) => {
                eprintln!("ALSA open {}: {}", out_dev, e);
                return;
            }
        }
    } else {
        None
    };

    let cap_dev = config.input_device.as_deref().unwrap_or("default");

    let pcm_capture: Option<PCM> = if config.input_channels > 0 {
        match PCM::new(cap_dev, Direction::Capture, true) {
            Ok(pcm) => {
                if let Err(e) = configure_pcm(&pcm, config.input_channels, config) {
                    log::warn!("ALSA configure capture: {}", e);
                }
                let _ = pcm.start();
                Some(pcm)
            }
            Err(e) => {
                log::warn!("ALSA capture {}: disabled", e);
                None
            }
        }
    } else {
        None
    };

    let buf_frames = config.buffer_size as usize;
    let has_playback = pcm_playback.is_some();
    let out_ch = config.output_channels as usize;
    let in_ch = config.input_channels as usize;
    let chunk_samples = buf_frames * out_ch.max(1);
    let in_sz = (buf_frames * in_ch).max(1).min(MAX_BLOCK_SAMPLES);
    let mut f32_buf = [0.0f32; 2048];
    let mut i16_buf = [0i16; 4096];
    let mut cb_i16 = [0i16; 4096];

    while running.load(Ordering::Acquire) {
        // Capture → input ring
        if let Some(ref pcm) = pcm_capture {
            if let Ok(io) = pcm.io_i16() {
                if let Ok(n_read) = io.readi(&mut cb_i16[..in_sz]) {
                    let n = (n_read * in_ch).min(in_sz);
                    let mut temp = [0.0f32; MAX_BLOCK_SAMPLES];
                    for (i, s) in cb_i16[..n].iter().enumerate() {
                        temp[i] = *s as f32 / 32768.0;
                    }
                    input_buffer.write(&temp[..n]);
                }
            }
        }

        // Throttle loop to audio rate — wait for playback space or capture data.
        if has_playback {
            match pcm_playback.as_ref().unwrap().wait(Some(10u32)) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    if let Err(r) = pcm_playback.as_ref().unwrap().try_recover(e, true) {
                        eprintln!("ALSA wait recover: {r}");
                        break;
                    }
                    continue;
                }
            }
        } else if let Some(ref pcm) = pcm_capture {
            match pcm.wait(Some(10u32)) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    if let Err(r) = pcm.try_recover(e, true) {
                        eprintln!("ALSA capture wait recover: {r}");
                        break;
                    }
                    continue;
                }
            }
        }

        if !running.load(Ordering::Acquire) {
            break;
        }

        // Generate one block
        unsafe {
            output_slot.set(OutputWindow::new(f32_buf.as_mut_ptr(), chunk_samples));
            process_cb.call();
            output_slot.clear();
        }

        // Write to playback PCM if present
        if let Some(ref pcm) = pcm_playback {
            for i in 0..chunk_samples {
                i16_buf[i] = (f32_buf[i].clamp(-1.0, 1.0) * 32767.0) as i16;
            }
            let mut retries = 3usize;
            loop {
                match pcm.io_i16() {
                    Ok(io) => match io.writei(&i16_buf[..chunk_samples]) {
                        Ok(_) => break,
                        Err(e) => {
                            eprintln!("ALSA write: {e}");
                            xruns.fetch_add(1, Ordering::Relaxed);
                            if retries == 0 {
                                break;
                            }
                            retries -= 1;
                            if let Err(r) = pcm.try_recover(e, true) {
                                eprintln!("ALSA recover: {r}");
                                break;
                            }
                        }
                    },
                    Err(e) => {
                        eprintln!("ALSA io_i16: {e}");
                        break;
                    }
                }
            }
        }
    }

    if let Some(ref pcm) = pcm_playback {
        let _ = pcm.drain();
    }
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
    // Buffer large enough for 4 processing blocks; period = 1 block.
    // This ensures each snd_pcm_wait signals exactly one block of space.
    hw.set_buffer_size(config.buffer_size as alsa::pcm::Frames * 4)
        .map_err(|e| IoError::Config(e.to_string()))?;
    hw.set_period_size(config.buffer_size as alsa::pcm::Frames, ValueOr::Nearest)
        .map_err(|e| IoError::Config(e.to_string()))?;
    pcm.hw_params(&hw)
        .map_err(|e| IoError::Config(e.to_string()))?;
    Ok(())
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
                let cap = win.capacity().min(frames * 2);
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

    fn run(&self, running: Arc<AtomicBool>) -> Result<(), String> {
        self.running.store(true, Ordering::Release);
        alsa_io_loop(
            self.process_cb,
            self.xruns.clone(),
            self.input_buffer.clone(),
            self.output_slot.clone(),
            &self.config,
            &running,
        );
        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        self.running.store(false, Ordering::Release);
        Ok(())
    }
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
        Ok(())
    }

    fn stop(&mut self) -> IoResult<()> {
        self.running.store(false, Ordering::Release);
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

impl Drop for AlsaBackend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        unsafe {
            self.process_cb.drop_box();
        }
    }
}
