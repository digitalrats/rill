use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use crate::backend::{AudioBackend, BackendType};
use crate::config::AudioConfig;
use crate::error::IoResult;
use rill_core::io::IoBackend;

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

pub struct NullBackend {
    config: AudioConfig,
    cb: CbSlot,
    is_running: bool,
    xruns: u32,
}

impl fmt::Debug for NullBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NullBackend")
            .field("config", &self.config)
            .field("is_running", &self.is_running)
            .field("xruns", &self.xruns)
            .finish()
    }
}

impl NullBackend {
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            cb: CbSlot::new(),
            is_running: false,
            xruns: 0,
        }
    }
}

impl AudioBackend for NullBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Null
    }
    fn config(&self) -> &AudioConfig {
        &self.config
    }
    fn config_mut(&mut self) -> &mut AudioConfig {
        &mut self.config
    }
    fn init(&mut self) -> IoResult<()> {
        Ok(())
    }
    fn start(&mut self) -> IoResult<()> {
        self.is_running = true;
        Ok(())
    }
    fn stop(&mut self) -> IoResult<()> {
        self.is_running = false;
        Ok(())
    }
    fn read(&mut self, buffer: &mut [f32]) -> IoResult<usize> {
        buffer.fill(0.0);
        Ok(buffer.len())
    }
    fn write(&mut self, buffer: &[f32]) -> IoResult<usize> {
        Ok(buffer.len())
    }
    fn xruns(&self) -> u32 {
        self.xruns
    }
    fn latency(&self) -> Duration {
        Duration::from_micros(
            (1_000_000.0 * self.config.buffer_size as f64 / self.config.sample_rate as f64) as u64,
        )
    }
    fn list_input_devices(&self) -> Vec<String> {
        vec!["Null Input".into()]
    }
    fn list_output_devices(&self) -> Vec<String> {
        vec!["Null Output".into()]
    }
}

impl IoBackend<f32> for NullBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn()>) {
        unsafe {
            self.cb.set(cb);
        }
    }
    fn read(&self, channels: &mut [&mut [f32]]) -> usize {
        let n = channels.first().map(|c| c.len()).unwrap_or(0);
        for ch in channels.iter_mut() {
            ch[..n].fill(0.0);
        }
        n
    }
    fn write(&self, channels: &[&[f32]]) -> usize {
        channels.first().map(|c| c.len()).unwrap_or(0)
    }
    fn run(&self, _running: Arc<AtomicBool>) -> Result<(), String> {
        unsafe {
            self.cb.call();
        }
        Ok(())
    }
    fn stop(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Drop for NullBackend {
    fn drop(&mut self) {
        unsafe {
            self.cb.drop_box();
        }
    }
}
