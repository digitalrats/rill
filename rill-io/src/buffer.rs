//! Dynamic-sized ring buffer for cross-thread I/O.
//!
//! Unlike [`rill_core::buffer::RingBuffer<T, N>`] which uses compile-time
//! fixed sizes, this buffer uses `Vec<f32>` and atomic indices so the
//! capacity can be configured at runtime — necessary for audio backends
//! whose buffer sizes are not known at compile time.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Lock-free single-producer single-consumer ring buffer for `f32` samples.
pub struct IoRingBuffer {
    buffer: Vec<f32>,
    write_index: AtomicUsize,
    read_index: AtomicUsize,
    mask: usize,
}

impl IoRingBuffer {
    /// Create a new ring buffer with `capacity` elements (rounded up to next power of 2).
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two();
        Self {
            buffer: vec![0.0f32; cap],
            write_index: AtomicUsize::new(0),
            read_index: AtomicUsize::new(0),
            mask: cap - 1,
        }
    }

    /// Maximum capacity (power of two).
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// Number of samples available for reading.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        let w = self.write_index.load(Ordering::Acquire);
        let r = self.read_index.load(Ordering::Acquire);
        w.wrapping_sub(r) & self.mask
    }

    /// Whether the buffer is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Write samples into the buffer. Returns number of samples written.
    pub fn write(&mut self, data: &[f32]) -> usize {
        let cap = self.buffer.len();
        let w = self.write_index.load(Ordering::Relaxed);
        let r = self.read_index.load(Ordering::Acquire);
        let available = cap - (w.wrapping_sub(r) & self.mask) - 1;
        let to_write = data.len().min(available);
        for i in 0..to_write {
            let idx = (w + i) & self.mask;
            self.buffer[idx] = data[i];
        }
        self.write_index
            .store(w.wrapping_add(to_write), Ordering::Release);
        to_write
    }

    /// Read up to `data.len()` samples from the buffer. Returns number of samples read.
    pub fn read(&mut self, data: &mut [f32]) -> usize {
        let r = self.read_index.load(Ordering::Relaxed);
        let w = self.write_index.load(Ordering::Acquire);
        let available = w.wrapping_sub(r) & self.mask;
        let to_read = data.len().min(available);
        for i in 0..to_read {
            let idx = (r + i) & self.mask;
            data[i] = self.buffer[idx];
        }
        self.read_index
            .store(r.wrapping_add(to_read), Ordering::Release);
        to_read
    }

    /// Clear all data from the buffer.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.read_index
            .store(self.write_index.load(Ordering::Acquire), Ordering::Release);
    }

    /// Fill the entire buffer with zeros and reset indices.
    #[allow(dead_code)]
    pub fn clear_with_zeros(&mut self) {
        self.buffer.fill(0.0);
        self.write_index.store(0, Ordering::Release);
        self.read_index.store(0, Ordering::Release);
    }
}
