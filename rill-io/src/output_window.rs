//! # OutputWindow — обобщённое окно для прямой записи в DMA-буфер
//!
//! Используется бэкэндами ALSA, CPAL, JACK, PipeWire для записи
//! аудиоданных напрямую в аппаратный буфер (без промежуточного ring buffer).

use std::slice;

/// Writable window into an interleaved `f32` output buffer.
pub struct OutputWindow {
    ptr: *mut f32,
    capacity: usize,
}

impl OutputWindow {
    pub fn new(ptr: *mut f32, len: usize) -> Self {
        Self { ptr, capacity: len }
    }

    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        unsafe { slice::from_raw_parts_mut(self.ptr, self.capacity) }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Lock-free slot for the current output window, set by the audio thread
/// before calling the process callback and cleared after.
#[derive(Clone)]
pub struct OutputSlot(*mut Option<OutputWindow>);

unsafe impl Send for OutputSlot {}
unsafe impl Sync for OutputSlot {}

impl OutputSlot {
    pub fn new() -> Self {
        Self(Box::into_raw(Box::new(None)))
    }

    pub unsafe fn set(&self, w: OutputWindow) {
        *self.0 = Some(w);
    }

    pub unsafe fn clear(&self) {
        *self.0 = None;
    }

    pub unsafe fn as_mut(&self) -> Option<&mut OutputWindow> {
        (*self.0).as_mut()
    }

    unsafe fn drop_box(&self) {
        drop(Box::from_raw(self.0));
    }
}

impl Drop for OutputSlot {
    fn drop(&mut self) {
        unsafe {
            self.drop_box();
        }
    }
}
