//! PortAudio backend — callback-driven, exact buffer size.
//!
//! Signal data flows through `IoRingBuffer` pairs bridged by `DeinterleavedView`.
//! The output callback reads from the output ring into the DMA buffer, creates a
//! `ClockTick`, and fires the process callback — the graph then reads input and
//! writes output through `tick.view`.

use std::cell::UnsafeCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use crate::buffer::IoRingBuffer;
use crate::buffer_view::DeinterleavedView;
use crate::config::AudioConfig;
use crate::error::IoResult;
use rill_core::io::IoBackend;
use rill_core::time::ClockTick;
use rill_core::traits::buffer_view::BufferView;

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
/// Signal data flows through `IoRingBuffer` → `DeinterleavedView` →
/// `tick.view` in graph nodes. The input stream callback writes capture
/// data into the input ring; the output stream callback drains the output
/// ring to the playback DMA buffer and fires the process tick.
pub struct PortAudioBackend {
    config: AudioConfig,
    input_ring: Arc<IoRingBuffer>,
    output_ring: Arc<IoRingBuffer>,
    view: Arc<dyn BufferView>,
    process_cb: CbSlot,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
    sample_pos: Arc<AtomicU64>,
    out_stream: UnsafeCell<Option<pa::stream::Stream<pa::NonBlocking, pa::Output<f32>>>>,
    in_stream: UnsafeCell<Option<pa::stream::Stream<pa::NonBlocking, pa::Input<f32>>>>,
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
            out_stream: UnsafeCell::new(None),
            in_stream: UnsafeCell::new(None),
        })
    }
}

// ============================================================================
// IoBackend impl
// ============================================================================

impl IoBackend for PortAudioBackend {
    fn create_view(&self) -> Arc<dyn BufferView> {
        self.view.clone()
    }

    fn set_process_callback(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn run(&self, running: Arc<AtomicBool>) -> Result<(), String> {
        let process_cb = self.process_cb;
        let iring = self.input_ring.clone();
        let oring = self.output_ring.clone();
        let view = self.view.clone();
        let sample_pos = self.sample_pos.clone();
        let _xruns = self.xruns.clone();
        let sample_rate = self.config.sample_rate;
        let out_channels = self.config.output_channels;
        let in_channels = self.config.input_channels;
        let buf_frames = self.config.buffer_size as usize;

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
            let obuf = oring.clone();
            let oview = view.clone();
            let oproc = process_cb;
            let ospos = sample_pos.clone();
            let orun = running.clone();

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
                        let n = obuf.read(buffer);
                        if n < buffer.len() {
                            buffer[n..].fill(0.0);
                        }

                        // Fire process tick — the graph reads input_ring
                        // and writes output_ring for the next cycle.
                        let pos = ospos.fetch_add(buf_frames as u64, Ordering::Relaxed);
                        let tick = ClockTick::new(
                            pos,
                            buf_frames as u32,
                            sample_rate as f32,
                            "portaudio".into(),
                            oview.clone(),
                        );
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
            let ibuf = iring.clone();
            let iview = view.clone();
            let iproc = process_cb;
            let ispos = sample_pos;
            let irun = running.clone();
            let has_output = out_channels > 0;

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

                        ibuf.write(args.buffer);

                        // When there is no output stream, the input
                        // callback drives the process tick.
                        if !has_output {
                            let pos = ispos.fetch_add(buf_frames as u64, Ordering::Relaxed);
                            let tick = ClockTick::new(
                                pos,
                                buf_frames as u32,
                                sample_rate as f32,
                                "portaudio".into(),
                                iview.clone(),
                            );
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
            self.process_cb.drop_box();
        }
    }
}
