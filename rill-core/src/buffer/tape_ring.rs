//! # TapeRing — unified non-consuming write-head ring
//!
//! The shared ring math behind `TapeLoop`, `DelayLine`, and rill-lang's `@`
//! delay rings: `write` advances a single head; reads are at a delay behind it
//! (delay 0 = most recent); nothing is ever consumed. Storage is abstracted via
//! [`RingStorage`], so the same ring works over a
//! [`RegionHandle`](crate::memory::RegionHandle), a `Box<[T]>`, or an inline
//! `[T; N]`.
//!
//! `RingBuffer` (the consuming FIFO queue) is a different data structure and is
//! not part of this type.

use crate::math::Transcendental;

/// Abstract ring storage: the contiguous backing plus its usable length.
pub trait RingStorage<T> {
    /// Usable length of the ring (tape capacity / delay length).
    fn len(&self) -> usize;
    /// Whether the ring has zero usable length.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Read access to the whole backing.
    fn with_data(&self, f: impl FnOnce(&[T]));
    /// Write access to the whole backing.
    fn with_data_mut(&mut self, f: impl FnOnce(&mut [T]));
}

/// A non-consuming write-head ring over any [`RingStorage`].
#[derive(Debug)]
pub struct TapeRing<T, S: RingStorage<T>> {
    storage: S,
    head: usize,
    _marker: std::marker::PhantomData<T>,
}

impl<T: Transcendental, S: RingStorage<T>> TapeRing<T, S> {
    /// Create a ring over `storage`, with the head at the start.
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            head: 0,
            _marker: std::marker::PhantomData,
        }
    }

    /// Write a single sample and advance the head.
    #[inline(always)]
    pub fn write(&mut self, sample: T) {
        let n = self.storage.len();
        self.storage.with_data_mut(|buf| buf[self.head] = sample);
        self.head = (self.head + 1) % n;
    }

    /// Read a sample at `delay` behind the head (0 = most recent). Delays
    /// beyond the ring length clamp to the oldest sample (matching the
    /// original `TapeLoop` behavior).
    #[inline(always)]
    pub fn read(&self, delay: usize) -> T {
        let n = self.storage.len();
        let d = delay.min(n - 1);
        let pos = (self.head + n - d - 1) % n;
        let mut out = T::ZERO;
        self.storage.with_data(|buf| out = buf[pos]);
        out
    }

    /// Write a whole block, advancing the head.
    pub fn write_block(&mut self, block: &[T]) {
        let n = self.storage.len();
        let len = block.len().min(n);
        self.storage.with_data_mut(|buf| {
            for (i, &b) in block.iter().enumerate().take(len) {
                buf[(self.head + i) % n] = b;
            }
        });
        self.head = (self.head + len) % n;
    }

    /// Read a whole block at `delay` behind the head (oldest first).
    pub fn read_block(&self, delay: usize, out: &mut [T]) {
        let n = self.storage.len();
        let d = delay.min(n - 1);
        let len = out.len().min(n);
        for (i, o) in out.iter_mut().enumerate().take(len) {
            *o = self.read(d + len - 1 - i);
        }
    }

    /// Read a single sample at a fractional `delay` with linear interpolation.
    pub fn read_interpolated(&self, delay: f64) -> T {
        let d = delay as usize;
        let frac = T::from_f64(delay.fract());
        let s1 = self.read(d);
        let s2 = self.read(d + 1);
        s1 + (s2 - s1) * frac
    }

    /// Zero the ring and reset the head.
    pub fn clear(&mut self) {
        self.storage.with_data_mut(|buf| buf.fill(T::ZERO));
        self.head = 0;
    }

    /// Fill the entire backing with `value` without moving the head.
    pub fn fill(&mut self, value: T) {
        self.storage.with_data_mut(|buf| buf.fill(value));
    }

    /// Current write cursor position.
    #[inline(always)]
    pub fn write_pos(&self) -> usize {
        self.head
    }

    /// Usable length.
    pub fn len(&self) -> usize {
        self.storage.len()
    }

    /// Whether the ring is empty (always false for a non-consuming ring).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test storage: an owned Vec.
    struct V<T>(Vec<T>);
    impl<T: Transcendental> RingStorage<T> for V<T> {
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

    fn ring<const N: usize>() -> TapeRing<f32, V<f32>> {
        TapeRing::new(V(vec![0.0f32; N]))
    }

    #[test]
    fn write_read_wraparound() {
        let mut t = ring::<4>();
        for i in 0..10 {
            t.write(i as f32);
        }
        assert_eq!(t.read(0), 9.0);
        assert_eq!(t.read(1), 8.0);
        assert_eq!(t.read(2), 7.0);
        assert_eq!(t.read(3), 6.0);
    }

    #[test]
    fn block_roundtrip() {
        let mut t = ring::<4>();
        let block = [1.0f32, 2.0, 3.0, 4.0];
        t.write_block(&block);
        let mut out = [0.0f32; 4];
        t.read_block(0, &mut out);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn read_interpolated() {
        let mut t = ring::<4>();
        t.write(0.0);
        t.write(1.0);
        t.write(2.0);
        t.write(3.0);
        // head = 0; read(1) = buf[2] = 2.0, read(2) = buf[1] = 1.0.
        let v = t.read_interpolated(1.5);
        assert!((v - 1.5).abs() < 0.001);
    }

    #[test]
    fn clear_resets() {
        let mut t = ring::<4>();
        t.write_block(&[1.0, 2.0, 3.0, 4.0]);
        t.clear();
        assert_eq!(t.read(0), 0.0);
        assert_eq!(t.read(1), 0.0);
    }

    #[test]
    fn write_block_advances_head() {
        let mut t = ring::<4>();
        t.write_block(&[1.0, 2.0]);
        // head now at 2; most recent written is 2.0.
        assert_eq!(t.read(0), 2.0);
        assert_eq!(t.read(1), 1.0);
        assert_eq!(t.read(2), 0.0);
    }
}
