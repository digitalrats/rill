//! CPAL бэкенд — callback-driven, без отдельного потока, без crossbeam, без parking_lot.
//!
//! Output пишет напрямую в CPAL-буфер через OutputWindow (без ring buffer).
//! Единственный поток — тот, в котором CPAL дёргает output-коллбэк.

use std::cell::UnsafeCell;
use std::fmt;
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::backend::{AudioBackend, BackendType};
use crate::buffer::IoRingBuffer;
use crate::config::AudioConfig;
use crate::error::{IoError, IoResult};
use rill_core::io::IoBackend;

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

/// Mutable view into a CPAL output buffer chunk.
struct OutputWindow {
    ptr: *mut f32,
    capacity: usize,
}

impl OutputWindow {
    fn new(ptr: *mut f32, len: usize) -> Self {
        Self { ptr, capacity: len }
    }
    fn as_mut_slice(&mut self) -> &mut [f32] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.capacity) }
    }
}

/// Lock-free slot for the current output window, set during CPAL callback.
#[derive(Copy, Clone)]
struct OutputSlot(*mut Option<OutputWindow>);
unsafe impl Send for OutputSlot {}
unsafe impl Sync for OutputSlot {}

impl OutputSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None)))
    }
    unsafe fn set(&self, w: OutputWindow) {
        *self.0 = Some(w);
    }
    unsafe fn clear(&self) {
        *self.0 = None;
    }
    #[allow(clippy::mut_from_ref)]
    unsafe fn as_mut(&self) -> Option<&mut OutputWindow> {
        (*self.0).as_mut()
    }
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(self.0));
    }
}

/// CPAL бэкенд.
///
/// Владеет одним output-стримом. Не создаёт отдельного потока —
/// обработка живёт в CPAL-коллбэке. Output пишет напрямую в CPAL-буфер.
///
/// # Safety
/// `cpal::Stream` содержит `PhantomData<*mut ()>` → `!Send` на некоторых
/// платформах. `Send` корректен: `AudioIo` гарантирует последовательный доступ.
pub struct CpalBackend {
    config: AudioConfig,
    process_cb: CbSlot,
    stream: UnsafeCell<Option<cpal::Stream>>,
    input_ring: Arc<IoRingBuffer>,
    output_slot: OutputSlot,
    xruns: Arc<std::sync::atomic::AtomicU32>,
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
    /// Создать новый CPAL бэкенд.
    pub fn new(config: AudioConfig) -> IoResult<Self> {
        let buf_cap = (config.buffer_size * config.input_channels.max(1) * 4) as usize;
        Ok(Self {
            config,
            process_cb: CbSlot::new(),
            stream: UnsafeCell::new(None),
            input_ring: Arc::new(IoRingBuffer::new(buf_cap)),
            output_slot: OutputSlot::new(),
            xruns: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        })
    }

    fn build_streams(&self) -> IoResult<cpal::Stream> {
        let host = cpal::default_host();
        let output_device = self
            .config
            .output_device
            .as_deref()
            .and_then(|name| {
                host.output_devices()
                    .ok()?
                    .find(|d| d.name().ok().as_deref() == Some(name))
            })
            .or_else(|| host.default_output_device())
            .ok_or_else(|| IoError::DeviceNotFound("No output device available".into()))?;

        let stream_config = cpal::StreamConfig {
            channels: self.config.output_channels as u16,
            sample_rate: cpal::SampleRate(self.config.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let xruns = self.xruns.clone();
        let cb_addr = self.process_cb.0;
        let oslot = self.output_slot;

        let stream = output_device
            .build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let chunk = 256; // BUF_SIZE
                    let mut off = 0usize;

                    while off + chunk * 2 <= data.len() {
                        unsafe {
                            oslot.set(OutputWindow::new(data.as_mut_ptr().add(off), chunk * 2));
                            CbSlot(cb_addr).call();
                            oslot.clear();
                        }
                        off += chunk * 2;
                    }
                    if off < data.len() {
                        data[off..].fill(0.0);
                    }
                },
                move |err| {
                    eprintln!("CPAL output stream error: {}", err);
                    xruns.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                },
                None,
            )
            .map_err(|e| IoError::Backend(format!("CPAL output: {e}")))?;

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
        self.input_ring.clear_with_zeros();
        Ok(())
    }

    fn start(&mut self) -> IoResult<()> {
        // AudioIo::start() does the actual work. This path is unused
        // when the backend is used via AudioOutput (pull model).
        Ok(())
    }

    fn stop(&mut self) -> IoResult<()> {
        // AudioIo::stop() does the actual work.
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
            let cap = win.capacity.min(frames * 2);
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

    fn start(&self) -> Result<(), String> {
        let stream = match self.build_streams() {
            Ok(s) => s,
            Err(e) => return Err(format!("CPAL build: {e}")),
        };
        stream.play().map_err(|e| format!("CPAL play: {e}"))?;
        unsafe {
            *self.stream.get() = Some(stream);
        }
        Ok(())
    }

    fn stop(&self) -> Result<(), String> {
        if let Some(s) = unsafe { (*self.stream.get()).take() } {
            let _ = s.pause();
        }
        Ok(())
    }
}

impl Drop for CpalBackend {
    fn drop(&mut self) {
        if let Some(s) = unsafe { (*self.stream.get()).take() } {
            let _ = s.pause();
        }
        unsafe {
            self.process_cb.drop_box();
            self.output_slot.drop_box();
        }
    }
}
