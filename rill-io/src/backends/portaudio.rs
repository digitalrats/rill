//! PortAudio backend — callback-driven, exact buffer size.
//!
//! Output writes directly to the PortAudio buffer via OutputWindow.
//! Unlike CPAL, PortAudio guarantees the requested sample rate and
//! buffer size — no `BufferSize::Default` negotiation issues.

use std::cell::UnsafeCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use crate::buffer::IoRingBuffer;
use crate::config::AudioConfig;
use crate::error::IoResult;
use crate::output_window::{OutputSlot, OutputWindow};
use rill_core::io::IoBackend;
use rill_core::time::ClockTick;
use rill_core::traits::buffer_view::{BufferView, NullBufferView};

use portaudio as pa;

/// Callback slot.
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
}

/// PortAudio backend.
pub struct PortAudioBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    output_slot: OutputSlot,
    input_ring: Arc<IoRingBuffer>,
    xruns: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
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
        let ch = config.input_channels.max(config.output_channels).max(1);
        let buffer_size = (config.buffer_size * ch * 4) as usize;
        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            output_slot: OutputSlot::new(),
            xruns: Arc::new(AtomicU32::new(0)),
            input_ring: Arc::new(IoRingBuffer::new(buffer_size)),
            running: Arc::new(AtomicBool::new(false)),
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
        Arc::new(NullBufferView::new(0, 0))
    }

    fn set_process_callback(&self, _cb: Box<dyn FnMut(&ClockTick)>) {}

    fn run(&self, running: Arc<AtomicBool>) -> Result<(), String> {
        let process_cb = self.process_cb;
        let oslot = self.output_slot.clone();
        let iring = self.input_ring.clone();
        let _xruns = self.xruns.clone();
        let sample_rate = self.config.sample_rate;
        let out_channels = self.config.output_channels;
        // PortAudio's ALSA backend cannot start output-only streams on
        // virtual devices (PipeWire/JACK) — it negotiates buffer parameters
        // that the virtual device accepts but silently hangs on. Opening a
        // minimal input stream alongside the output enters duplex mode,
        // which uses a different parameter negotiation path that works.
        let in_channels = if out_channels > 0 {
            self.config.input_channels.max(1)
        } else {
            self.config.input_channels
        };
        let buf_frames = self.config.buffer_size as usize;

        let pa = pa::PortAudio::new().map_err(|e| format!("PortAudio init: {e}"))?;

        // Output stream
        if out_channels > 0 {
            let dev = pa
                .default_output_device()
                .map_err(|e| format!("PortAudio output device: {e}"))?;
            let info = pa
                .device_info(dev)
                .map_err(|e| format!("PA device info: {e}"))?;
            let params = pa::StreamParameters::new(
                dev,
                out_channels as i32,
                true,
                info.default_low_output_latency,
            );
            let flags = pa::StreamFlags::CLIP_OFF | pa::StreamFlags::DITHER_OFF;
            let settings = pa::OutputStreamSettings::with_flags(
                params,
                sample_rate as f64,
                buf_frames as u32,
                flags,
            );
            let mut stream = pa
                .open_non_blocking_stream(settings, {
                    let oslot = oslot.clone();
                    let running = running.clone();
                    move |args: pa::OutputStreamCallbackArgs<f32>| {
                        if !running.load(Ordering::Acquire) {
                            return pa::Complete;
                        }
                        let buffer = args.buffer;
                        let total = buffer.len();
                        let block = buf_frames * out_channels as usize;
                        let mut temp_buf = [0.0f32; 8192];
                        let mut off = 0usize;
                        while off + block <= total {
                            unsafe {
                                oslot.set(OutputWindow::new(temp_buf.as_mut_ptr(), block));
                                process_cb.call(sample_rate as f32);
                                oslot.clear();
                            }
                            buffer[off..off + block].copy_from_slice(&temp_buf[..block]);
                            off += block;
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

        // Input stream
        if in_channels > 0 {
            let settings = pa
                .default_input_stream_settings(
                    in_channels as i32,
                    sample_rate as f64,
                    buf_frames as u32,
                )
                .map_err(|e| format!("PortAudio input settings: {e}"))?;
            let mut stream = pa
                .open_non_blocking_stream(settings, {
                    let iring = iring.clone();
                    let has_output = out_channels > 0;
                    let block_samps = buf_frames * in_channels as usize;
                    move |args: pa::InputStreamCallbackArgs<f32>| {
                        iring.write(args.buffer);
                        if !has_output && iring.len() >= block_samps {
                            unsafe {
                                process_cb.call(sample_rate as f32);
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
