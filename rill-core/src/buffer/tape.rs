//! # Tape loop — a large delay buffer for tape effects
//!
//! [`TapeLoop`] is a [`TapeRing`] over an owned `Vec` backing. Unlike
//! [`DelayLine`](super::DelayLine), the capacity is a runtime value, so tapes
//! can hold millions of samples without a const-generic inline array. The
//! backing storage is abstracted via [`RingStorage`]; a region-backed variant
//! (`RegionHandle` storage) is provided through the same ring.

use crate::buffer::registry::DelayBuffer;
use crate::buffer::tape_ring::{RingStorage, TapeRing};
use crate::math::Transcendental;

/// Heap-backed tape loop. The backing is a [`TapeRing`] over an owned `Vec`.
#[derive(Debug)]
pub struct TapeLoop<T: Transcendental> {
    inner: TapeRing<T, VecStorage<T>>,
}

/// Owned `Vec` storage implementing [`RingStorage`].
#[derive(Debug)]
pub(crate) struct VecStorage<T>(Vec<T>);

impl<T: Transcendental> RingStorage<T> for VecStorage<T> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn with_data(&self, f: impl FnOnce(&[T])) {
        f(&self.0);
    }
    fn with_data_mut(&mut self, f: impl FnOnce(&mut [T])) {
        f(&mut self.0);
    }
}

impl<T: Transcendental> TapeLoop<T> {
    /// Allocate a new tape loop with the given capacity (in samples).
    pub fn new(capacity: usize) -> Option<Self> {
        if capacity == 0 {
            return None;
        }
        let mut vec = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            vec.push(T::ZERO);
        }
        Some(Self {
            inner: TapeRing::new(VecStorage(vec)),
        })
    }

    /// Maximum capacity in samples.
    pub fn capacity(&self) -> usize {
        self.inner.len()
    }

    /// Current write cursor position.
    pub fn write_pos(&self) -> usize {
        self.inner.write_pos()
    }

    /// Write a single sample and advance the write cursor.
    #[inline(always)]
    pub fn write(&mut self, sample: T) {
        self.inner.write(sample);
    }

    /// Read a sample at `delay` samples behind the write position.
    #[inline(always)]
    pub fn read(&self, delay: usize) -> T {
        self.inner.read(delay)
    }

    /// Read with linear interpolation between samples.
    #[inline(always)]
    pub fn read_interpolated(&self, delay: f64) -> T {
        self.inner.read_interpolated(delay)
    }

    /// Write a full block of samples.
    #[inline(always)]
    pub fn write_block(&mut self, block: &[T]) {
        self.inner.write_block(block);
    }

    /// Read a full block starting at `delay` samples behind write position.
    #[inline(always)]
    pub fn read_block(&self, delay: usize, output: &mut [T]) {
        self.inner.read_block(delay, output);
    }

    /// Fill the entire buffer with a constant value.
    pub fn fill(&mut self, value: T) {
        self.inner.fill(value);
    }

    /// Reset write position and zero the buffer.
    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl<T: Transcendental> DelayBuffer<T> for TapeLoop<T> {
    fn capacity(&self) -> usize {
        self.capacity()
    }
    fn write(&mut self, sample: T) {
        self.write(sample);
    }
    fn write_block(&mut self, block: &[T]) {
        self.write_block(block);
    }
    fn read_interpolated(&self, delay: f64) -> T {
        self.read_interpolated(delay)
    }
    fn read_block(&self, delay: usize, out: &mut [T]) {
        self.read_block(delay, out);
    }
    fn clear(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tape_basic_write_read() {
        let mut tape = TapeLoop::<f32>::new(1024).unwrap();
        tape.write(1.0);
        tape.write(2.0);
        tape.write(3.0);
        assert_eq!(tape.read(0), 3.0);
        assert_eq!(tape.read(1), 2.0);
        assert_eq!(tape.read(2), 1.0);
    }

    #[test]
    fn test_tape_wraparound() {
        let mut tape = TapeLoop::<f32>::new(4).unwrap();
        for i in 0..10 {
            tape.write(i as f32);
        }
        assert_eq!(tape.read(0), 9.0);
        assert_eq!(tape.read(1), 8.0);
        assert_eq!(tape.read(2), 7.0);
        assert_eq!(tape.read(3), 6.0);
    }

    #[test]
    fn test_tape_block_ops() {
        let mut tape = TapeLoop::<f32>::new(64).unwrap();
        let block = [1.0f32; 64];
        tape.write_block(&block);
        let mut out = [0.0f32; 64];
        tape.read_block(63, &mut out);
        assert_eq!(out[0], 1.0);
    }

    #[test]
    fn test_tape_large_capacity() {
        let tape = TapeLoop::<f32>::new(1_000_000).unwrap();
        assert_eq!(tape.capacity(), 1_000_000);
    }

    #[test]
    fn test_tape_zero_capacity() {
        assert!(TapeLoop::<f32>::new(0).is_none());
    }

    #[test]
    fn test_read_interpolated() {
        let mut tape = TapeLoop::<f32>::new(1024).unwrap();
        tape.write(0.0);
        tape.write(1.0);
        let v = tape.read_interpolated(0.5);
        assert!((v - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_fill_and_clear() {
        let mut tape = TapeLoop::<f32>::new(4).unwrap();
        tape.fill(5.0);
        assert_eq!(tape.read(0), 5.0, "fill writes every slot");
        tape.write(1.0);
        assert_eq!(tape.read(0), 1.0, "write overwrites the head slot");
        assert_eq!(tape.read(1), 5.0, "other slots keep the fill value");
        tape.clear();
        assert_eq!(tape.read(0), 0.0);
    }
}
