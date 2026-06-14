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

use crate::buffer_view::DirectView;
use crate::config::AudioConfig;
use crate::error::IoResult;
use rill_core::io::IoBackend;
use rill_core::time::ClockTick;
use rill_core::traits::buffer_view::{BufferView, NullBufferView};

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
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(
            self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>,
        ));
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
    /// Pre-allocated capture buffer for output+input mode.
    /// Input callback copies DMA data here; output callback uses it for `DirectView`.
    capture_buf: Vec<f32>,
}

impl fmt::Debug for PortAudioBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PortAudioBackend")
            .field("config", &self.config)
            .field("running", &self.running.load(Ordering::Relaxed))
            .finish()
    }
}

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
        })
    }
}

// ============================================================================
// IoBackend impl
// ============================================================================

impl IoBackend for PortAudioBackend {
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

        // PortAudio's ALSA backend cannot start output-only streams on
        // virtual devices (PipeWire/JACK) — it negotiates buffer parameters
        // that the virtual device accepts but silently hangs on. Opening a
        // minimal input stream alongside the output enters duplex mode,
        // which uses a different parameter negotiation path that works.
        let force_in = out_channels > 0 && in_channels == 0;
        let effective_in = if force_in { 1 } else { in_channels };

        let pa = pa::PortAudio::new().map_err(|e| format!("PortAudio init: {e}"))?;

        // ── Output stream ────────────────────────────────────────────────────
        if out_channels > 0 {
            let oproc = process_cb;
            let ospos = sample_pos.clone();
            let orun = running.clone();
            let out_ch = out_channels as usize;
            let in_ch = in_channels as usize;

            let settings = pa
                .default_output_stream_settings::<f32>(
                    out_channels as i32,
                    sample_rate as f64,
                    buf_frames as u32,
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

                        let view: Arc<dyn BufferView> = if has_input {
                            Arc::new(DirectView::new_interleaved(
                                cap_ptr_usize as *const f32,
                                buffer.as_mut_ptr(),
                                in_ch,
                                out_ch,
                                n_frames,
                            ))
                        } else {
                            Arc::new(DirectView::new_output_only(
                                buffer.as_mut_ptr(),
                                out_ch,
                                n_frames,
                            ))
                        };

                        let pos = ospos.fetch_add(n_frames as u64, Ordering::Relaxed);
                        let mut tick = ClockTick::new(
                            pos,
                            n_frames as u32,
                            sample_rate as f32,
                            "portaudio".into(),
                            view,
                        );
                        tick.speed_ratio = 1.0;
                        unsafe {
                            oproc.call(&tick);
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
        if effective_in > 0 {
            let iproc = process_cb;
            let ispos = sample_pos;
            let irun = running.clone();

            let settings = pa
                .default_input_stream_settings::<f32>(
                    effective_in as i32,
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

                        if has_output {
                            // Copy DMA capture data into the shared buffer for the
                            // output callback to pick up when it fires the tick.
                            let n = args
                                .buffer
                                .len()
                                .min(buf_frames * effective_in.max(1) as usize);
                            unsafe {
                                std::ptr::copy_nonoverlapping(
                                    args.buffer.as_ptr(),
                                    cap_ptr_usize as *mut f32,
                                    n,
                                );
                            }
                        } else {
                            // Input-only: create DirectView and fire the process tick.
                            let in_ch = effective_in as usize;
                            let n_frames = args.buffer.len() / in_ch.max(1);
                            let view: Arc<dyn BufferView> = Arc::new(DirectView::new_interleaved(
                                args.buffer.as_ptr(),
                                std::ptr::null_mut(),
                                in_ch,
                                0,
                                n_frames,
                            ));

                            let pos = ispos.fetch_add(n_frames as u64, Ordering::Relaxed);
                            let mut tick = ClockTick::new(
                                pos,
                                n_frames as u32,
                                sample_rate as f32,
                                "portaudio".into(),
                                view,
                            );
                            tick.speed_ratio = 1.0;
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
            *self.out_stream.get() = None;
            *self.in_stream.get() = None;
        }
    }
}
