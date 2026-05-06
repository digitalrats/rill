//! CPAL бэкенд — callback-driven, без отдельного потока.
//!
//! Output пишет напрямую в CPAL-буфер через OutputWindow.
//! Process callback работает на CPAL audio thread.
//!
//! `run()` — non‑blocking: создаёт stream, запускает, сохраняет handle,
//! возвращается. Caller удерживает тред в park‑loop до `stop()`.
//! `stop()` — дропает stream.
//! Никаких `std::thread`, `std::sync`.

use std::cell::UnsafeCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::backend::{AudioBackend, BackendType};
use crate::buffer::IoRingBuffer;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use crate::output_window::{OutputSlot, OutputWindow};
use rill_core::io::IoBackend;

/// Callback slot.
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

/// CPAL бэкенд.
pub struct CpalBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    stream: UnsafeCell<Option<cpal::Stream>>,
    input_stream: UnsafeCell<Option<cpal::Stream>>,
    input_ring: Arc<IoRingBuffer>,
    output_slot: OutputSlot,
    xruns: Arc<AtomicU32>,
}

unsafe impl Send for CpalBackend {}
unsafe impl Sync for CpalBackend {}

impl fmt::Debug for CpalBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CpalBackend")
            .field("config", &self.config)
            .field("stream", &unsafe { (*self.stream.get()).is_some() })
            .finish()
    }
}

impl CpalBackend {
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        let buf_cap = (config.buffer_size * config.input_channels.max(1) * 4) as usize;
        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            stream: UnsafeCell::new(None),
            input_stream: UnsafeCell::new(None),
            input_ring: Arc::new(IoRingBuffer::new(buf_cap)),
            output_slot: OutputSlot::new(),
            xruns: Arc::new(AtomicU32::new(0)),
        })
    }
}

// ============================================================================
// IoBackend impl
// ============================================================================

impl IoBackend<f32> for CpalBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe {
            self.process_cb.set(cb);
        }
    }

    fn read(&self, channels: &mut [&mut [f32]]) -> usize {
        let frames = channels.first().map(|c| c.len()).unwrap_or(0);
        let cap = frames.min(256).saturating_mul(2);
        let mut temp = [0.0f32; 512];
        let n = self.input_ring.read(&mut temp[..cap]);
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
        if let Some(win) = unsafe { self.output_slot.as_mut() } {
            let out_ch = self.config.output_channels as usize;
            let cap = win.capacity().min(frames * out_ch);
            let dst = win.as_mut_slice();
            for i in 0..(cap / out_ch) {
                if let Some(ch) = channels.get(0) {
                    dst[i * out_ch] = ch[i];
                }
                if let Some(ch) = channels.get(1) {
                    dst[i * out_ch + 1] = ch[i];
                }
            }
            cap / out_ch
        } else {
            0
        }
    }

    fn run(&self, _running: Arc<AtomicBool>) -> Result<(), String> {
        let process_cb = self.process_cb;
        let oslot = self.output_slot.clone();
        let iring = self.input_ring.clone();
        let xruns = self.xruns.clone();
        let sample_rate = self.config.sample_rate;
        let out_channels = self.config.output_channels;
        let in_channels = self.config.input_channels;
        let buf_frames = self.config.buffer_size;
        let out_dev_name = self.config.output_device.clone();
        let in_dev_name = self.config.input_device.clone();

        let host = cpal::default_host();

        // ── Output stream ───────────────────────────────────────────────────
        if out_channels > 0 {
            let output_device = out_dev_name
                .as_deref()
                .and_then(|name| {
                    host.output_devices()
                        .ok()?
                        .find(|d| d.name().ok().as_deref() == Some(name))
                })
                .or_else(|| host.default_output_device())
                .ok_or_else(|| format!("No output device available"))?;

            let block = (buf_frames * out_channels) as usize;
            let mut temp_buf = vec![0.0f32; block * 16];

            let scfg = cpal::StreamConfig {
                channels: out_channels as u16,
                sample_rate: cpal::SampleRate(sample_rate),
                buffer_size: cpal::BufferSize::Fixed(buf_frames),
            };
            let stream = output_device
                .build_output_stream(
                    &scfg,
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        let total = data.len();
                        let mut written = 0usize;
                        let max_written = total.min(temp_buf.len());
                        while written + block <= max_written {
                            unsafe {
                                oslot.set(OutputWindow::new(
                                    temp_buf.as_mut_ptr().add(written),
                                    block,
                                ));
                                process_cb.call();
                                oslot.clear();
                            }
                            written += block;
                        }
                        data[..written].copy_from_slice(&temp_buf[..written]);
                        if written < total {
                            data[written..].fill(0.0);
                        }
                    },
                    {
                        let xruns = xruns.clone();
                        move |err| {
                            eprintln!("CPAL output stream error: {err}");
                            xruns.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                    None,
                )
                .map_err(|e| format!("CPAL output build: {e}"))?;

            stream
                .play()
                .map_err(|e| format!("CPAL output play: {e}"))?;
            unsafe {
                *self.stream.get() = Some(stream);
            }
        }

        // ── Input stream (capture callback only — no processing) ──────────
        if in_channels > 0 {
            let input_device = in_dev_name
                .as_deref()
                .and_then(|name| {
                    host.input_devices()
                        .ok()?
                        .find(|d| d.name().ok().as_deref() == Some(name))
                })
                .or_else(|| host.default_input_device());

            if let Some(input_device) = input_device {
                let block_samps = (buf_frames * in_channels) as usize;
                let has_output = out_channels > 0;

                let try_build = |bs: cpal::BufferSize| {
                    let icfg = cpal::StreamConfig {
                        channels: in_channels as u16,
                        sample_rate: cpal::SampleRate(sample_rate),
                        buffer_size: bs,
                    };
                    let ir = iring.clone();
                    let cb = process_cb;
                    input_device.build_input_stream(
                        &icfg,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            ir.write(data);
                            if !has_output && ir.len() >= block_samps {
                                unsafe {
                                    cb.call();
                                }
                            }
                        },
                        move |err| {
                            eprintln!("CPAL input stream error: {err}");
                        },
                        None,
                    )
                };

                let stream = try_build(cpal::BufferSize::Fixed(buf_frames))
                    .or_else(|_| try_build(cpal::BufferSize::Default));
                match stream {
                    Ok(s) => {
                        let _ = s.play();
                        unsafe {
                            *self.input_stream.get() = Some(s);
                        }
                    }
                    Err(e) => {
                        log::warn!("CPAL input stream disabled: {e}");
                    }
                }
            } else {
                log::warn!("CPAL input disabled: no input device available");
            }
        }

        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        if let Some(s) = unsafe { (*self.stream.get()).take() } {
            let _ = s.pause();
        }
        if let Some(s) = unsafe { (*self.input_stream.get()).take() } {
            let _ = s.pause();
        }
        Ok(())
    }
}

// ============================================================================
// AudioBackend impl
// ============================================================================

impl AudioBackend for CpalBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Cpal
    }

    fn config(&self) -> &AudioConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut AudioConfig {
        &mut self.config
    }

    fn init(&mut self) -> IoResult<()> {
        self.input_ring.clear_with_zeros();
        Ok(())
    }

    fn start(&mut self) -> IoResult<()> {
        Ok(())
    }

    fn stop(&mut self) -> IoResult<()> {
        Ok(())
    }

    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize> {
        let n = self.input_ring.read(buffer);
        Ok(n)
    }

    fn write(&mut self, _buffer: &[f32]) -> IoResult<usize> {
        Ok(0)
    }

    fn xruns(&self) -> u32 {
        self.xruns.load(Ordering::Acquire)
    }

    fn latency(&self) -> std::time::Duration {
        std::time::Duration::from_micros(
            (1_000_000.0 * self.config.buffer_size as f64 / self.config.sample_rate as f64) as u64,
        )
    }

    fn list_input_devices(&self) -> Vec<String> {
        cpal::default_host()
            .input_devices()
            .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    fn list_output_devices(&self) -> Vec<String> {
        cpal::default_host()
            .output_devices()
            .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }
}

impl Drop for CpalBackend {
    fn drop(&mut self) {
        if let Some(s) = unsafe { (*self.stream.get()).take() } {
            let _ = s.pause();
        }
        if let Some(s) = unsafe { (*self.input_stream.get()).take() } {
            let _ = s.pause();
        }
        unsafe {
            self.process_cb.drop_box();
        }
    }
}
