//! # OutputWindow — обобщённое окно для прямой записи в DMA-буфер
//!
//! Используется бэкэндами ALSA, CPAL, JACK, PipeWire для записи
//! аудиоданных напрямую в аппаратный буфер (без промежуточного ring buffer).

use std::cell::UnsafeCell;
use std::slice;
use std::sync::Arc;

/// Writable window into an interleaved `f32` output buffer.
pub struct OutputWindow {
    ptr: *mut f32,
    capacity: usize,
}

impl OutputWindow {
    /// Create a new output window wrapping a raw pointer + length.
    pub fn new(ptr: *mut f32, len: usize) -> Self {
        Self { ptr, capacity: len }
    }

    /// Get the window contents as a mutable slice.
    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        unsafe { slice::from_raw_parts_mut(self.ptr, self.capacity) }
    }

    /// The capacity (number of `f32` samples) of this window.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Lock-free slot for the current output window, set by the audio thread
/// before calling the process callback and cleared after.
///
/// Uses `Arc<UnsafeCell>` so every clone shares the same heap allocation
/// with correct reference counting — no double-free / use-after-free
/// when a clone is dropped before another clone is used.
#[derive(Clone)]
pub struct OutputSlot(Arc<UnsafeCell<Option<OutputWindow>>>);

unsafe impl Send for OutputSlot {}
unsafe impl Sync for OutputSlot {}

impl Default for OutputSlot {
    #[allow(clippy::arc_with_non_send_sync)]
    fn default() -> Self {
        Self(Arc::new(UnsafeCell::new(None)))
    }
}

impl OutputSlot {
    /// Create a new empty output slot.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new() -> Self {
        Self(Arc::new(UnsafeCell::new(None)))
    }

    /// Set the current output window (called by the audio thread).
    ///
    /// # Safety
    ///
    /// Caller must ensure this is invoked from the audio I/O thread only,
    /// and that no other thread reads or writes the slot concurrently.
    pub unsafe fn set(&self, w: OutputWindow) {
        *self.0.get() = Some(w);
    }

    /// Clear the current output window.
    ///
    /// # Safety
    ///
    /// Caller must ensure this is invoked from the audio I/O thread only,
    /// and that no other thread reads or writes the slot concurrently.
    pub unsafe fn clear(&self) {
        *self.0.get() = None;
    }

    /// Get a mutable reference to the current output window, if set.
    ///
    /// # Safety
    ///
    /// Caller must ensure that no other thread reads or writes the slot
    /// concurrently, and that the returned mutable reference does not
    /// alias with any other reference.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn as_mut(&self) -> Option<&mut OutputWindow> {
        (*self.0.get()).as_mut()
    }
}
