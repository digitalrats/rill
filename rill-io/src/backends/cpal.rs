//! CPAL бэкенд — callback-driven, без отдельного потока, без crossbeam, без parking_lot.
//!
//! Единственный поток — тот, в котором CPAL дёргает output-коллбэк.
//! Процессинг живёт внутри этого коллбэка. Управление (start/stop) —
//! синхронное, без каналов.

use std::fmt;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio_io::{AudioIo, IoResult as AudioIoResult};
use crate::backend::{AudioBackend, BackendType};
use crate::buffer::IoRingBuffer;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};

/// Callback slot — stores a `*mut Option<Box<dyn Fn()>>` as `usize`
/// so the field type itself is `Send`.
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
///
/// Владеет одним output-стримом (и опционально input-стримом).
/// Не создаёт отдельного потока — обработка живёт в CPAL-коллбэке.
///
/// # Safety
/// `cpal::Stream` на некоторых платформах (ALSA) содержит `PhantomData<*mut ()>`
/// и не реализует `Send` автоматически. `Send` корректен, поскольку `AudioIo`
/// гарантирует последовательный доступ (stop вызывается после join RT-потока).
pub struct CpalBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    stream: Option<cpal::Stream>,
    output_ring: Arc<IoRingBuffer>,
    input_ring: Arc<IoRingBuffer>,
    xruns: Arc<std::sync::atomic::AtomicU32>,
}

unsafe impl Send for CpalBackend {}
unsafe impl Sync for CpalBackend {}

impl fmt::Debug for CpalBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CpalBackend")
            .field("config", &self.config)
            .field("stream", &self.stream.is_some())
            .finish()
    }
}

impl CpalBackend {
    /// Создать новый CPAL бэкенд.
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        let buf_cap = (config.buffer_size * config.output_channels.max(config.input_channels).max(1) * 4) as usize;
        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            stream: None,
            output_ring: Arc::new(IoRingBuffer::new(buf_cap)),
            input_ring: Arc::new(IoRingBuffer::new(buf_cap)),
            xruns: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        })
    }

    fn build_streams(&mut self) -> IoResult<cpal::Stream> {
        let host = cpal::default_host();
        let output_device = self.config.output_device.as_deref()
            .and_then(|name| host.output_devices().ok()?.find(|d| d.name().ok().as_deref() == Some(name)))
            .or_else(|| host.default_output_device())
            .ok_or_else(|| IoError::DeviceNotFound("No output device available".into()))?;

        let stream_config = cpal::StreamConfig {
            channels: self.config.output_channels as u16,
            sample_rate: cpal::SampleRate(self.config.sample_rate),
            buffer_size: cpal::BufferSize::Fixed(self.config.buffer_size),
        };

        let out_ring = self.output_ring.clone();
        let xruns = self.xruns.clone();
        // Store slot address as usize — the field type IS Send.
        let cb_addr = self.process_cb.0;

        let stream = output_device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // 1. Call graph processing callback
                let slot = CbSlot(cb_addr);
                unsafe { slot.call(); }
                // 2. Read from output ring → CPAL buffer
                let n = out_ring.read(data);
                if n < data.len() {
                    data[n..].fill(0.0);
                }
            },
            move |err| {
                eprintln!("CPAL output stream error: {}", err);
                xruns.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            },
            None,
        ).map_err(|e| IoError::Backend(format!("CPAL output: {e}")))?;

        Ok(stream)
    }
}

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
        self.output_ring.clear_with_zeros();
        self.input_ring.clear_with_zeros();
        Ok(())
    }

    fn start(&mut self) -> IoResult<()> {
        let stream = self.build_streams()?;
        stream.play().map_err(|e| IoError::Backend(format!("CPAL play: {e}")))?;
        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) -> IoResult<()> {
        if let Some(s) = self.stream.take() {
            let _ = s.pause();
        }
        Ok(())
    }

    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize> {
        let n = self.input_ring.read(buffer);
        Ok(n)
    }

    fn write(&mut self, buffer: &[f32]) -> IoResult<usize> {
        let n = self.output_ring.write(buffer);
        Ok(n)
    }

    fn xruns(&self) -> u32 {
        self.xruns.load(std::sync::atomic::Ordering::Acquire)
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

impl AudioIo for CpalBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe { self.process_cb.set(cb); }
    }

    fn read_input(&self, left: &mut [f32], right: &mut [f32]) -> usize {
        let frames = left.len().min(right.len());
        let cap = frames.min(256).saturating_mul(2);
        let mut temp = [0.0f32; 512];
        let n = self.input_ring.read(&mut temp[..cap]);
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
        self.output_ring.write(&temp[..cap]) / 2
    }

    fn start(&self) -> AudioIoResult<()> {
        // AudioIo::start is a no-op here — AudioBackend::start does the work.
        // In the AudioIo path the backend is started via AudioBackend.
        Ok(())
    }

    fn stop(&self) -> AudioIoResult<()> {
        // Stopped via AudioBackend::stop or Drop.
        Ok(())
    }
}

impl Drop for CpalBackend {
    fn drop(&mut self) {
        let _ = self.stop();
        unsafe { self.process_cb.drop_box(); }
    }
}
