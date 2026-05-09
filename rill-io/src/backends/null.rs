use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::config::AudioConfig;

use rill_core::io::IoBackend;

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
    unsafe fn drop_box(&self) {
        drop(Box::from_raw(self.0 as *mut Option<Box<dyn Fn(f32)>>));
    }
}

/// A no-op audio backend that produces silence and discards output.
///
/// Useful for testing and offline processing.
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
    /// Create a new null backend with the given audio config.
    pub fn new(config: AudioConfig) -> Self {
        Self {
            config,
            cb: CbSlot::new(),
            is_running: false,
            xruns: 0,
        }
    }
}

impl IoBackend<f32> for NullBackend {
    fn set_process_callback(&self, cb: Box<dyn Fn(f32)>) {
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
            self.cb.call(self.config.sample_rate as f32);
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
