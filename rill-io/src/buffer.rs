//! Lock-free single-producer single-consumer ring buffer for `f32` samples.
//!
//! Uses `UnsafeCell` for interior mutability so all methods take `&self`,
//! allowing lock-free access from multiple threads (SPSC guaranteed by
//! protocol — the two sides never touch the same atomics concurrently
//! for conflicting operations).
//!
//! # Safety
//! - One writer (calls `write`) and one reader (calls `read`) at a time.
//! - The writer and reader may run on different threads.
//! - No其它 concurrent access from additional threads.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Lock-free single-producer single-consumer ring buffer for `f32` samples.
pub struct IoRingBuffer {
    buffer: UnsafeCell<Vec<f32>>,
    write_index: AtomicUsize,
    read_index: AtomicUsize,
    mask: usize,
}

unsafe impl Send for IoRingBuffer {}
unsafe impl Sync for IoRingBuffer {}

impl IoRingBuffer {
    /// Create a new ring buffer with `capacity` elements (rounded up to next power of 2).
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two();
        Self {
            buffer: UnsafeCell::new(vec![0.0f32; cap]),
            write_index: AtomicUsize::new(0),
            read_index: AtomicUsize::new(0),
            mask: cap - 1,
        }
    }

    /// Maximum capacity (power of two).
    pub fn capacity(&self) -> usize {
        unsafe { (*self.buffer.get()).len() }
    }

    /// Number of samples available for reading.
    pub fn len(&self) -> usize {
        let w = self.write_index.load(Ordering::Acquire);
        let r = self.read_index.load(Ordering::Acquire);
        w.wrapping_sub(r) & self.mask
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Write samples into the buffer. Returns number of samples written.
    pub fn write(&self, data: &[f32]) -> usize {
        let cap = unsafe { (*self.buffer.get()).len() };
        let w = self.write_index.load(Ordering::Relaxed);
        let r = self.read_index.load(Ordering::Acquire);
        let available = cap - (w.wrapping_sub(r) & self.mask) - 1;
        let to_write = data.len().min(available);
        let buf = unsafe { &mut *self.buffer.get() };
        #[allow(clippy::needless_range_loop)]
        for i in 0..to_write {
            let idx = (w + i) & self.mask;
            buf[idx] = data[i];
        }
        self.write_index
            .store(w.wrapping_add(to_write), Ordering::Release);
        to_write
    }

    /// Read up to `data.len()` samples from the buffer. Returns number of samples read.
    pub fn read(&self, data: &mut [f32]) -> usize {
        let r = self.read_index.load(Ordering::Relaxed);
        let w = self.write_index.load(Ordering::Acquire);
        let available = w.wrapping_sub(r) & self.mask;
        let to_read = data.len().min(available);
        let buf = unsafe { &*self.buffer.get() };
        #[allow(clippy::needless_range_loop)]
        for i in 0..to_read {
            let idx = (r + i) & self.mask;
            data[i] = buf[idx];
        }
        self.read_index
            .store(r.wrapping_add(to_read), Ordering::Release);
        to_read
    }

    /// Clear all data from the buffer.
    pub fn clear(&self) {
        self.read_index
            .store(self.write_index.load(Ordering::Acquire), Ordering::Release);
    }

    /// Fill the entire buffer with zeros and reset indices.
    pub fn clear_with_zeros(&self) {
        let buf = unsafe { &mut *self.buffer.get() };
        buf.fill(0.0);
        self.write_index.store(0, Ordering::Release);
        self.read_index.store(0, Ordering::Release);
    }
}
