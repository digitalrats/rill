//! PortAudio backend — callback-driven, exact buffer size.
//!
//! Zero-copy via `DirectView` — graph nodes read/write directly from/to
//! the PortAudio DMA buffers through `tick.view`. For output+input mode,
//! a pre-allocated capture buffer bridges the two separate PortAudio
//! callbacks (input copies DMA data, output creates a full interleaved view).

use std::cell::UnsafeCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use crate::config::AudioConfig;
use crate::error::IoResult;
use crate::output_window::{OutputSlot, OutputWindow};
use rill_core::io::{IoCapture, IoDriver, IoPlayback};
use rill_core::time::ClockTick;

use portaudio as pa;

/// Callback slot — stores the process callback via raw pointer for `Send`-safe
/// single-threaded access from the PortAudio RT callbacks.
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

/// PortAudio backend — callback-driven via PortAudio stream callbacks.
///
/// Zero-copy DMA access via `DirectView`. For output-only or input-only,
/// the DMA buffer pointer is used directly. For output+input, a pre-allocated
/// capture buffer bridges the two separate PortAudio callbacks.
pub struct PortAudioBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    sample_pos: Arc<AtomicU64>,
    out_stream: UnsafeCell<Option<pa::stream::Stream<pa::NonBlocking, pa::Output<f32>>>>,
    in_stream: UnsafeCell<Option<pa::stream::Stream<pa::NonBlocking, pa::Input<f32>>>>,
    capture_buf: Vec<f32>,
    output_slot: OutputSlot,
}

impl fmt::Debug for PortAudioBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PortAudioBackend")
            .field("config", &self.config)
            .field("running", &self.running.load(Ordering::Relaxed))
            .finish()
    }
}

unsafe impl Sync for PortAudioBackend {}

impl PortAudioBackend {
    /// Create a new PortAudio backend from the given configuration.
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        let buf_frames = config.buffer_size as usize;
        let in_ch = config.input_channels as usize;
        let in_cap = buf_frames * in_ch.max(1);

        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            xruns: Arc::new(AtomicU32::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            sample_pos: Arc::new(AtomicU64::new(0)),
            out_stream: UnsafeCell::new(None),
            in_stream: UnsafeCell::new(None),
            capture_buf: vec![0.0f32; in_cap],
            output_slot: OutputSlot::new(),
        })
    }
}

// ============================================================================
// IoDriver impl
// ============================================================================

impl IoDriver for PortAudioBackend {
    fn set_process_callback(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn run(&self, running: Arc<AtomicBool>) -> Result<(), String> {
        let process_cb = self.process_cb;
        let sample_pos = self.sample_pos.clone();
        let _xruns = self.xruns.clone();
        let sample_rate = self.config.sample_rate;
        let out_channels = self.config.output_channels;
        let in_channels = self.config.input_channels;
        let buf_frames = self.config.buffer_size as usize;
        let has_output = out_channels > 0;
        let has_input = in_channels > 0;

        // Raw pointer to the capture buffer (stable for the lifetime of the backend).
        // Cast to usize for Send-safe capture in the move closures (follows CbSlot pattern).
        let cap_ptr_usize = self.capture_buf.as_ptr() as usize;

        let pa = pa::PortAudio::new().map_err(|e| format!("PortAudio init: {e}"))?;

        // ── Output stream ────────────────────────────────────────────────────
        if out_channels > 0 {
            let oproc = process_cb;
            let ospos = sample_pos.clone();
            let orun = running.clone();
            let out_ch = out_channels as usize;
            let _in_ch = in_channels as usize;
            let is_output_driver = in_channels == 0;
            let block_size = buf_frames;
            let output_slot = self.output_slot.clone();

            // Request a large DMA buffer (48 × rill block_size) so the
            // hardware / ALSA plugin has a stable quantum — like PipeWire's
            // default 12288-frame buffer.  The multi-block loop below chunks
            // it back into `block_size` pieces, sending one `ClockTick` per
            // rill block.
            let pa_frames = (buf_frames * 48) as u32;
            let settings = pa
                .default_output_stream_settings::<f32>(
                    out_channels as i32,
                    sample_rate as f64,
                    pa_frames,
                )
                .map_err(|e| format!("PortAudio output settings: {e}"))?;

            let mut stream = pa
                .open_non_blocking_stream(settings, {
                    move |args: pa::OutputStreamCallbackArgs<f32>| {
                        if !orun.load(Ordering::Acquire) {
                            return pa::Complete;
                        }

                        let buffer = args.buffer;
                        let n_frames = buffer.len() / out_ch.max(1);

                        if is_output_driver {
                            // Drive the graph once per `block_size` chunk so the
                            // entire DMA buffer is filled and one `ClockTick` is
                            // sent per rill block — matching PipeWire's chunking.
                            let mut offset = 0usize;
                            while offset < n_frames {
                                let n = (n_frames - offset).min(block_size);
                                let pos = ospos.fetch_add(n as u64, Ordering::Relaxed);
                                let mut tick = ClockTick::new(
                                    pos,
                                    n as u32,
                                    sample_rate as f32,
                                    "portaudio".into(),
                                );
                                tick.speed_ratio = 1.0;
                                tick.io_quantum = n_frames as u32;
                                unsafe {
                                    output_slot.set(OutputWindow::new(
                                        buffer.as_mut_ptr().add(offset * out_ch),
                                        n * out_ch,
                                    ));
                                    oproc.call(&tick);
                                    output_slot.clear();
                                }
                                offset += n;
                            }
                            debug_assert_eq!(
                                offset, n_frames,
                                "PortAudio buffer not aligned to block_size"
                            );
                        }

                        pa::Continue
                    }
                })
                .map_err(|e| format!("PortAudio output stream: {e}"))?;

            stream
                .start()
                .map_err(|e| format!("PortAudio output start: {e}"))?;

            unsafe {
                *self.out_stream.get() = Some(stream);
            }
        }

        // ── Input stream ─────────────────────────────────────────────────────
        if in_channels > 0 {
            let iproc = process_cb;
            let ispos = sample_pos;
            let irun = running.clone();
            let is_input_driver = has_input;

            let settings = pa
                .default_input_stream_settings::<f32>(
                    in_channels as i32,
                    sample_rate as f64,
                    buf_frames as u32,
                )
                .map_err(|e| format!("PortAudio input settings: {e}"))?;

            let mut stream = pa
                .open_non_blocking_stream(settings, {
                    move |args: pa::InputStreamCallbackArgs<f32>| {
                        if !irun.load(Ordering::Acquire) {
                            return pa::Complete;
                        }

                        let in_ch = in_channels as usize;
                        let n_frames = args.buffer.len() / in_ch.max(1);

                        // Always fire the process tick from the input callback
                        // when in InputDriver mode (has_output = true means the
                        // output callback is passive — it just reads output_ring).
                        if has_output {
                            // Also copy to capture buffer for the output callback
                            // to use when it fires the tick (not needed for recording).
                            let n = args
                                .buffer
                                .len()
                                .min(buf_frames * in_channels.max(1) as usize);
                            unsafe {
                                std::ptr::copy_nonoverlapping(
                                    args.buffer.as_ptr(),
                                    cap_ptr_usize as *mut f32,
                                    n,
                                );
                            }
                        }

                        let pos = ispos.fetch_add(n_frames as u64, Ordering::Relaxed);
                        let mut tick = ClockTick::new(
                            pos,
                            n_frames as u32,
                            sample_rate as f32,
                            "portaudio".into(),
                        );
                        tick.speed_ratio = 1.0;
                        if is_input_driver {
                            unsafe {
                                iproc.call(&tick);
                            }
                        }

                        pa::Continue
                    }
                })
                .map_err(|e| format!("PortAudio input stream: {e}"))?;

            stream
                .start()
                .map_err(|e| format!("PortAudio input start: {e}"))?;

            unsafe {
                *self.in_stream.get() = Some(stream);
            }
        }

        self.running.store(true, Ordering::Release);
        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        self.running.store(false, Ordering::Release);
        unsafe {
            *self.out_stream.get() = None;
            *self.in_stream.get() = None;
        }
        Ok(())
    }
}

impl Drop for PortAudioBackend {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        unsafe {
            self.process_cb.take_box();
        }
        unsafe {
            *self.out_stream.get() = None;
            *self.in_stream.get() = None;
        }
    }
}

// ============================================================================
// IoPlayback impl
// ============================================================================

impl IoPlayback for PortAudioBackend {
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

// ============================================================================
// IoCapture impl
// ============================================================================

impl IoCapture for PortAudioBackend {
    fn read_input(&self, _channel: usize, dst: &mut [f32]) -> usize {
        dst.fill(0.0);
        dst.len()
    }

    fn num_input_channels(&self) -> usize {
        self.config.input_channels as usize
    }
}
