//! ALSA backend for Linux
//!
//! `run()` — blocking: opens PCM, configures, enters a poll-driven loop.
//! Signal data flows through `IoRingBuffer` pairs bridged by `DeinterleavedView`.
//! The process callback receives a `ClockTick` carrying the view — graph nodes
//! read/write through `tick.view` uniformly.
//!
//! Exits when `running` becomes false. Cleanup happens inside `run()` before returning.

const MAX_BLOCK_SAMPLES: usize = 8192;

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use alsa::pcm::{Access, Format, HwParams};
use alsa::{Direction, ValueOr, PCM};

use crate::buffer::IoRingBuffer;
use crate::buffer_view::DeinterleavedView;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use rill_core::io::IoBackend;
use rill_core::math::functions::{f32_to_i16_chunk, i16_to_f32_chunk};
use rill_core::time::ClockTick;
use rill_core::traits::buffer_view::BufferView;

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
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(
            self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>,
        ));
    }
}

// ============================================================================
// AlsaBackend
// ============================================================================

/// ALSA audio backend — poll-driven I/O loop.
///
/// Signal data flows through `IoRingBuffer` → `DeinterleavedView` →
/// `tick.view` in graph nodes.  The poll loop reads capture data into the
/// input ring, triggers graph processing via the view, then drains the
/// output ring to the playback PCM.
pub struct AlsaBackend {
    config: AudioConfig,
    input_ring: Arc<IoRingBuffer>,
    output_ring: Arc<IoRingBuffer>,
    view: Arc<dyn BufferView>,
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
        let input_channels = config.input_channels;
        let output_channels = config.output_channels;
        let block_size = config.buffer_size as usize;
        let ring_cap = (block_size * output_channels.max(1) as usize * 32).next_power_of_two();
        let in_ring_cap = (block_size * input_channels.max(1) as usize * 32).next_power_of_two();

        let input_ring = Arc::new(IoRingBuffer::new(in_ring_cap));
        let output_ring = Arc::new(IoRingBuffer::new(ring_cap));
        let view: Arc<dyn BufferView> = Arc::new(DeinterleavedView::new(
            input_ring.clone(),
            output_ring.clone(),
            input_channels as usize,
            output_channels as usize,
            block_size,
        ));

        Ok(Self {
            config,
            input_ring,
            output_ring,
            view,
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
    input_ring: Arc<IoRingBuffer>,
    output_ring: Arc<IoRingBuffer>,
    view: Arc<dyn BufferView>,
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

    let mut f32_buf = [0.0f32; MAX_BLOCK_SAMPLES];
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

        // ── Capture: read interleaved i16 → convert to f32 → write to input ring ──
        if has_capture {
            let pcm = pcm_capture.as_ref().unwrap();
            match pcm.io_i16() {
                Ok(io) => match io.readi(&mut cb_i16[..in_sz]) {
                    Ok(n_read) => {
                        let n = (n_read * in_ch).min(in_sz);
                        let mut temp = [0.0f32; MAX_BLOCK_SAMPLES];
                        i16_to_f32_chunk(&cb_i16[..n], &mut temp[..n]);
                        input_ring.write(&temp[..n]);
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

        // ── Process block: create ClockTick → call graph callback ──
        let pos = sample_pos.fetch_add(buf_frames as u64, Ordering::Relaxed);
        let tick = ClockTick::new(
            pos,
            buf_frames as u32,
            negotiated_rate,
            "alsa".into(),
            view.clone(),
        );
        unsafe {
            process_cb.call(&tick);
        }

        // ── Playback: read interleaved f32 from output ring → convert to i16 → write ──
        if has_playback {
            let pcm = pcm_playback.as_ref().unwrap();
            let total_samps = chunk_samples;
            let n = output_ring.read(&mut f32_buf[..total_samps]);
            if n < total_samps {
                f32_buf[n..total_samps].fill(0.0);
            }
            f32_to_i16_chunk(&f32_buf[..total_samps], &mut i16_buf[..total_samps]);

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
        self.view.clone()
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
            self.input_ring.clone(),
            self.output_ring.clone(),
            self.view.clone(),
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
            self.process_cb.drop_box();
        }
    }
}
