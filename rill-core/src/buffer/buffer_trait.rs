//! # Buffer trait — common interface for all signal buffer types
//!
//! Unified trait covering both storage buffers (fixed-size, heap-allocated)
//! and queue-style buffers (pipe, delay, fan-out, fan-in, ring).
//!
//! Generic over [`crate::math::Scalar`] — supports f32, f64, and integer types.

use core::ops::{Deref, DerefMut};

use crate::buffer::BufferStats;
use crate::math::Scalar;

/// Common interface for all buffer types used in the signal graph.
pub trait Buffer<T: Scalar> {
    /// Maximum number of elements the buffer can hold.
    fn capacity(&self) -> usize;

    /// Current number of elements in the buffer.
    fn len(&self) -> usize;

    /// Whether the buffer is empty (`len() == 0`).
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the buffer is full (`len() == capacity()`).
    fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// Read-only access to the buffer data.
    fn as_slice(&self) -> &[T];

    /// Mutable access to the buffer data.
    fn as_mut_slice(&mut self) -> &mut [T];

    /// Fill the entire buffer with a value.
    fn fill(&mut self, value: T);

    /// Copy data from a slice. Copies `min(src.len(), self.len())` samples.
    fn copy_from(&mut self, src: &[T]);

    /// Remove all items from the buffer.
    fn clear(&mut self);

    /// Snapshot of performance statistics.
    fn stats(&self) -> BufferStats {
        BufferStats::new()
    }

    /// Reset performance counters (not the data).
    fn reset_stats(&mut self) {}
}

// ============================================================================
// FixedBuffer — compile-time fixed size, stack-allocated
// ============================================================================

/// Fixed-size buffer on the stack — default per-port buffer.
#[derive(Debug, Clone)]
pub struct FixedBuffer<T, const SIZE: usize> {
    data: [T; SIZE],
}

impl<T: Scalar, const SIZE: usize> FixedBuffer<T, SIZE> {
    /// Create a new buffer filled with `T::default()`.
    pub fn new() -> Self {
        Self {
            data: [T::default(); SIZE],
        }
    }

    /// Create a buffer from a fixed-size array.
    pub fn from_array(data: [T; SIZE]) -> Self {
        Self { data }
    }

    /// Create a buffer from a slice, truncating or padding with `T::default()` as needed.
    pub fn from_slice(slice: &[T]) -> Self {
        let mut data = [T::default(); SIZE];
        let len = slice.len().min(SIZE);
        data[..len].copy_from_slice(&slice[..len]);
        Self { data }
    }

    /// Return a reference to the inner array.
    pub fn as_array(&self) -> &[T; SIZE] {
        &self.data
    }

    /// Return a mutable reference to the inner array.
    pub fn as_mut_array(&mut self) -> &mut [T; SIZE] {
        &mut self.data
    }
}

impl<T: Scalar, const SIZE: usize> Default for FixedBuffer<T, SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Scalar, const SIZE: usize> Deref for FixedBuffer<T, SIZE> {
    type Target = [T; SIZE];
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T: Scalar, const SIZE: usize> DerefMut for FixedBuffer<T, SIZE> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl<T: Scalar, const SIZE: usize> From<[T; SIZE]> for FixedBuffer<T, SIZE> {
    fn from(data: [T; SIZE]) -> Self {
        Self::from_array(data)
    }
}

impl<T: Scalar, const SIZE: usize> Buffer<T> for FixedBuffer<T, SIZE> {
    fn capacity(&self) -> usize {
        SIZE
    }

    fn len(&self) -> usize {
        SIZE
    }

    fn as_slice(&self) -> &[T] {
        &self.data
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.data
    }

    fn fill(&mut self, value: T) {
        self.data.fill(value);
    }

    fn copy_from(&mut self, src: &[T]) {
        let len = src.len().min(SIZE);
        self.data[..len].copy_from_slice(&src[..len]);
    }

    fn clear(&mut self) {
        self.data.fill(T::default());
    }
}

// ============================================================================
// HeapBuffer — runtime-sized, heap-allocated
// ============================================================================

/// Heap-allocated buffer with runtime-determined size.
///
/// Used for resources whose size is determined from data
/// (loaded samples, configuration).
#[derive(Debug, Clone)]
pub struct HeapBuffer<T> {
    data: Vec<T>,
}

impl<T: Scalar> HeapBuffer<T> {
    /// Create a new buffer with `size` samples, all initialized to `T::default()`.
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![T::default(); size],
        }
    }

    /// Create a buffer from an existing `Vec`, taking ownership.
    pub fn from_vec(data: Vec<T>) -> Self {
        Self { data }
    }
}

impl<T: Scalar> Buffer<T> for HeapBuffer<T> {
    fn capacity(&self) -> usize {
        self.data.capacity()
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn as_slice(&self) -> &[T] {
        &self.data
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.data
    }

    fn fill(&mut self, value: T) {
        self.data.fill(value);
    }

    fn copy_from(&mut self, src: &[T]) {
        let len = src.len().min(self.data.len());
        self.data[..len].copy_from_slice(&src[..len]);
    }

    fn clear(&mut self) {
        self.data.clear();
    }
}
