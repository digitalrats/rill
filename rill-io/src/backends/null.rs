use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::config::AudioConfig;

use rill_core::io::IoBackend;
use rill_core::time::ClockTick;
use rill_core::traits::buffer_view::{BufferView, NullBufferView};

#[derive(Copy, Clone)]
struct CbSlot(usize);

impl CbSlot {
    fn new() -> Self {
        Self(Box::into_raw(Box::new(None::<Box<dyn FnMut(&ClockTick)>>)) as usize)
    }
    unsafe fn set(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        (*(self.0 as *mut Option<Box<dyn FnMut(&ClockTick)>>)) = Some(cb);
    }
    unsafe fn call(&mut self, tick: &ClockTick) {
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

/// A no-op audio backend that produces silence and discards output.
///
/// Fires the process callback once inside `run()` for testing purposes,
/// then returns immediately (no I/O loop).
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

impl IoBackend for NullBackend {
    fn create_view(&self) -> Arc<dyn BufferView> {
        Arc::new(NullBufferView::new(
            self.config.input_channels as usize,
            self.config.output_channels as usize,
        ))
    }

    fn set_process_callback(&self, cb: Box<dyn FnMut(&ClockTick)>) {
        unsafe {
            self.cb.set(cb);
        }
    }

    fn run(&self, _running: Arc<AtomicBool>) -> Result<(), String> {
        let tick = ClockTick::new(
            0,
            self.config.buffer_size,
            self.config.sample_rate as f32,
            "null".into(),
            Arc::new(NullBufferView::new(
                self.config.input_channels as usize,
                self.config.output_channels as usize,
            )),
        );
        // Fire the callback once for testing — this triggers graph processing,
        // which drains the actor mailbox (applies queued SetParameter commands).
        unsafe {
            let mut_ref: *mut CbSlot = &self.cb as *const CbSlot as *mut CbSlot;
            (*mut_ref).call(&tick);
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
