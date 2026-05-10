//! ALSA backend for Linux — no `std::thread`, no `std::sync`.
//!
//! `run()` — blocking: opens PCM, configures, enters
//! `snd_pcm_wait()` loop. Exits when `running` becomes false.
//! Cleanup happens inside `run()` before returning.

const MAX_BLOCK_SAMPLES: usize = 8192;

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use alsa::pcm::{Access, Format, HwParams};
use alsa::{Direction, ValueOr, PCM};

use crate::buffer::IoRingBuffer;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use crate::output_window::{OutputSlot, OutputWindow};
use rill_core::io::IoBackend;
use rill_core::math::functions::{f32_to_i16_chunk, i16_to_f32_chunk};

// ============================================================================
// Callback slot
// ============================================================================

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
    let mut negotiated_rate = config.sample_rate as f32;
    let period_frames;
    let pcm_playback: Option<PCM> = if config.output_channels > 0 {
        match PCM::new(out_dev, Direction::Playback, false) {
            Ok(pcm) => {
                match configure_pcm(&pcm, config.output_channels, config) {
                    Ok((rate, period)) => {
                        negotiated_rate = rate as f32;
                        period_frames = period as usize;
                    }
                    Err(e) => {
                        eprintln!("ALSA configure playback: {}", e);
                        return;
                    }
                }
                // Start playback after buffer has 2 processing blocks.
                if let Ok(sw) = pcm.sw_params_current() {
                    let _ = sw.set_start_threshold((period_frames * 2) as alsa::pcm::Frames);
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
    let in_sz = (buf_frames * in_ch).clamp(1, MAX_BLOCK_SAMPLES);
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
                    i16_to_f32_chunk(&cb_i16[..n], &mut temp[..n]);
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
            process_cb.call(negotiated_rate);
            output_slot.clear();
        }

        // Write to playback PCM if present
        if let Some(ref pcm) = pcm_playback {
            f32_to_i16_chunk(&f32_buf[..chunk_samples], &mut i16_buf[..chunk_samples]);
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

fn configure_pcm(pcm: &PCM, channels: u32, config: &AudioConfig) -> IoResult<(u32, u32)> {
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
    let negotiated_rate = hw.get_rate().map_err(|e| IoError::Config(e.to_string()))?;
    let negotiated_period = hw
        .get_period_size()
        .map_err(|e| IoError::Config(e.to_string()))?;
    if negotiated_period != config.buffer_size as alsa::pcm::Frames {
        return Err(IoError::Config(format!(
            "ALSA period mismatch: requested {}, got {}. Use a different backend (portaudio, pipewire, jack).",
            config.buffer_size, negotiated_period
        )));
    }
    pcm.hw_params(&hw)
        .map_err(|e| IoError::Config(e.to_string()))?;
    Ok((negotiated_rate, negotiated_period as u32))
}

// ============================================================================
// IoBackend impl
// ============================================================================

impl IoBackend<f32> for AlsaBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn(f32)>) {
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
        let nch = self.config.output_channels as usize;
        if nch == 0 {
            return 0;
        }
        let frames = channels[0].len();
        unsafe {
            if let Some(win) = self.output_slot.as_mut() {
                let cap = win.capacity().min(frames * nch);
                let dst = win.as_mut_slice();
                for i in 0..frames {
                    for ch in 0..nch {
                        dst[i * nch + ch] = channels[ch][i];
                    }
                }
                cap / nch
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

impl Drop for AlsaBackend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        unsafe {
            self.process_cb.drop_box();
        }
    }
}
