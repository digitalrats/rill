//! ALSA backend for Linux
//!
//! `run()` — blocking: opens PCM, configures, enters a poll-driven loop.
//! Zero-copy via `DirectView` — graph nodes read/write directly from/to
//! per-block f32 buffers through `tick.view`.
//!
//! Exits when `running` becomes false. Cleanup happens inside `run()` before returning.

const MAX_BLOCK_SAMPLES: usize = 8192;

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use alsa::pcm::{Access, Format, HwParams};
use alsa::{Direction, ValueOr, PCM};

use crate::buffer_view::DirectView;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use rill_core::io::IoBackend;
use rill_core::math::functions::{f32_to_i16_chunk, i16_to_f32_chunk};
use rill_core::time::ClockTick;
use rill_core::traits::buffer_view::{BufferView, NullBufferView};

// ============================================================================
// Callback slot
// ============================================================================

/// Callback slot — stores the process callback via raw pointer for `Send`-safe
/// single-threaded access from the poll loop.
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

// ============================================================================
// AlsaBackend
// ============================================================================

/// ALSA audio backend — poll-driven I/O loop.
///
/// Zero-copy DMA access via `DirectView` — per-block f32 buffers are
/// allocated on the stack inside the poll loop.  Graph nodes read/write
/// through `tick.view` directly without ring buffers.
pub struct AlsaBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    sample_pos: Arc<AtomicU64>,
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
        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            xruns: Arc::new(AtomicU32::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            sample_pos: Arc::new(AtomicU64::new(0)),
        })
    }
}

// ============================================================================
// ALSA I/O loop — called from `run()`
// ============================================================================

fn alsa_io_loop(
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    sample_pos: Arc<AtomicU64>,
    config: &AudioConfig,
    running: &AtomicBool,
) {
    let out_dev = config.output_device.as_deref().unwrap_or("default");

    let mut negotiated_rate = config.sample_rate as f32;
    let period_frames: usize;
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
    let has_capture = pcm_capture.is_some();
    let out_ch = config.output_channels as usize;
    let in_ch = config.input_channels as usize;
    let chunk_samples = buf_frames * out_ch.max(1);
    let in_sz = (buf_frames * in_ch).clamp(1, MAX_BLOCK_SAMPLES);

    let mut cap_f32 = [0.0f32; MAX_BLOCK_SAMPLES];
    let mut play_f32 = [0.0f32; MAX_BLOCK_SAMPLES];
    let mut i16_buf = [0i16; MAX_BLOCK_SAMPLES];
    let mut cb_i16 = [0i16; MAX_BLOCK_SAMPLES];

    while running.load(Ordering::Acquire) {
        // Wait for ALSA device readiness via snd_pcm_wait — one period = one block
        if has_playback {
            match pcm_playback.as_ref().unwrap().wait(Some(10u32)) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    if let Err(r) = pcm_playback.as_ref().unwrap().try_recover(e, true) {
                        eprintln!("ALSA wait recover: {r}");
                        xruns.fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                    continue;
                }
            }
        } else if has_capture {
            match pcm_capture.as_ref().unwrap().wait(Some(10u32)) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    if let Err(r) = pcm_capture.as_ref().unwrap().try_recover(e, true) {
                        eprintln!("ALSA capture wait recover: {r}");
                        xruns.fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                    continue;
                }
            }
        }

        if !running.load(Ordering::Acquire) {
            break;
        }

        // ── Capture: read interleaved i16 → convert to f32 → fill cap_f32 ──
        if has_capture {
            let pcm = pcm_capture.as_ref().unwrap();
            match pcm.io_i16() {
                Ok(io) => match io.readi(&mut cb_i16[..in_sz]) {
                    Ok(n_read) => {
                        let n = (n_read * in_ch).min(in_sz);
                        cap_f32[..n].fill(0.0);
                        i16_to_f32_chunk(&cb_i16[..n], &mut cap_f32[..n]);
                    }
                    Err(e) => {
                        eprintln!("ALSA capture read: {e}");
                        xruns.fetch_add(1, Ordering::Relaxed);
                    }
                },
                Err(e) => {
                    eprintln!("ALSA capture io_i16: {e}");
                }
            }
        }

        // ── Process block: create DirectView → ClockTick → call graph callback ──
        {
            let view: Arc<dyn BufferView> = if has_capture && has_playback {
                Arc::new(DirectView::new_interleaved(
                    cap_f32.as_ptr(),
                    play_f32.as_mut_ptr(),
                    in_ch,
                    out_ch,
                    buf_frames,
                ))
            } else if has_capture {
                Arc::new(DirectView::new_interleaved(
                    cap_f32.as_ptr(),
                    std::ptr::null_mut(),
                    in_ch,
                    0,
                    buf_frames,
                ))
            } else {
                Arc::new(DirectView::new_output_only(
                    play_f32.as_mut_ptr(),
                    out_ch,
                    buf_frames,
                ))
            };

            let pos = sample_pos.fetch_add(buf_frames as u64, Ordering::Relaxed);
            let mut tick =
                ClockTick::new(pos, buf_frames as u32, negotiated_rate, "alsa".into(), view);
            let config_rate = config.sample_rate as f64;
            let actual_rate = negotiated_rate as f64;
            tick.speed_ratio = if (config_rate - actual_rate).abs() > 1.0 {
                config_rate / actual_rate
            } else {
                1.0
            };
            unsafe {
                process_cb.call(&tick);
            }
        }

        // ── Playback: convert play_f32 → i16 → write to PCM ──
        if has_playback {
            let pcm = pcm_playback.as_ref().unwrap();
            let total_samps = chunk_samples;
            f32_to_i16_chunk(&play_f32[..total_samps], &mut i16_buf[..total_samps]);

            let mut retries = 3usize;
            loop {
                match pcm.io_i16() {
                    Ok(io) => match io.writei(&i16_buf[..total_samps]) {
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

impl IoBackend for AlsaBackend {
    fn create_view(&self) -> Arc<dyn BufferView> {
        // Real views are created per-callback with DMA pointers.
        Arc::new(NullBufferView::new(
            self.config.input_channels as usize,
            self.config.output_channels as usize,
        ))
    }

    fn set_process_callback(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn run(&self, running: Arc<AtomicBool>) -> Result<(), String> {
        self.running.store(true, Ordering::Release);
        alsa_io_loop(
            self.process_cb,
            self.xruns.clone(),
            self.sample_pos.clone(),
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
            self.process_cb.take_box();
        }
    }
}
