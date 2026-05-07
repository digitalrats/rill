//! # Ring queue with random access
//!
//! [`RingQueue`](crate::queues::ring::RingQueue) — a hybrid between a ring buffer and a queue,
//! allowing data to be read with arbitrary delay.

use super::QueueStats;
use crate::buffer::AtomicCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Ring queue with random access
///
/// Allows reading data not only from the head, but also with arbitrary
/// delay. Useful for delay effects and reverb.
#[repr(C, align(64))]
pub struct RingQueue<T: Copy, const CAP: usize> {
    /// Data
    data: [AtomicCell<T>; CAP],
    /// Write index
    write_pos: AtomicUsize,
    /// Mask for fast computation
    mask: usize,
    /// Statistics
    stats: QueueStats,
}

impl<T: Copy + Default, const CAP: usize> Default for RingQueue<T, CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Default, const CAP: usize> RingQueue<T, CAP> {
    /// Create a new ring queue
    pub fn new() -> Self {
        assert!(CAP.is_power_of_two(), "CAP must be a power of two");

        let data = std::array::from_fn(|_| AtomicCell::new(T::default()));

        Self {
            data,
            write_pos: AtomicUsize::new(0),
            mask: CAP - 1,
            stats: QueueStats::new(),
        }
    }

    /// Push an element (always succeeds)
    pub fn push(&self, value: T) {
        let pos = self.write_pos.load(Ordering::Relaxed);
        self.data[pos].store(value);
        self.write_pos
            .store((pos + 1) & self.mask, Ordering::Release);
        self.stats.record_push(self.len());
    }

    /// Read an element with delay
    ///
    /// # Arguments
    /// * `delay` - delay in samples (0 = most recently written)
    pub fn read_delayed(&self, delay: usize) -> T {
        assert!(delay < CAP, "Delay must be less than CAP");

        let write_pos = self.write_pos.load(Ordering::Acquire);
        let read_pos = (write_pos + CAP - delay - 1) & self.mask;

        self.data[read_pos].load()
    }

    /// Read an element with fractional delay (linear interpolation)
    pub fn read_interpolated(&self, delay_frac: f64) -> T
    where
        T: From<f64> + Into<f64>,
    {
        let delay_int = delay_frac.floor() as usize;
        let frac = delay_frac.fract();

        let s1: f64 = self.read_delayed(delay_int).into();
        let s2: f64 = self.read_delayed(delay_int + 1).into();

        T::from(s1 * (1.0 - frac) + s2 * frac)
    }

    /// Read an element by absolute index
    pub fn read_at(&self, index: usize) -> T {
        let write_pos = self.write_pos.load(Ordering::Acquire);
        let read_pos = (write_pos + CAP - index - 1) & self.mask;
        self.data[read_pos].load()
    }

    /// Push a slice of data
    pub fn push_slice(&self, slice: &[T]) {
        for &value in slice {
            self.push(value);
        }
    }

    /// Read a slice of data with delay
    pub fn read_slice_delayed(&self, delay: usize, output: &mut [T]) {
        for (i, out) in output.iter_mut().enumerate() {
            *out = self.read_delayed(delay + i);
        }
    }

    /// Current write position
    pub fn write_pos(&self) -> usize {
        self.write_pos.load(Ordering::Acquire)
    }

    /// Capacity
    pub const fn capacity(&self) -> usize {
        CAP
    }

    /// Number of written elements (no more than CAP)
    pub fn len(&self) -> usize {
        CAP
    }

    /// Returns `true` if no elements have been written.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Reset the write position
    pub fn reset(&self) {
        self.write_pos.store(0, Ordering::Release);
    }

    /// Get statistics
    pub fn stats(&self) -> &QueueStats {
        &self.stats
    }
}

#[allow(unsafe_code)]
unsafe impl<T: Copy + Send, const CAP: usize> Send for RingQueue<T, CAP> {}
#[allow(unsafe_code)]
unsafe impl<T: Copy + Sync, const CAP: usize> Sync for RingQueue<T, CAP> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_queue_basic() {
        let queue = RingQueue::<i32, 4>::new();

        queue.push(1);
        queue.push(2);
        queue.push(3);
        queue.push(4);

        assert_eq!(queue.read_delayed(0), 4);
        assert_eq!(queue.read_delayed(1), 3);
        assert_eq!(queue.read_delayed(2), 2);
        assert_eq!(queue.read_delayed(3), 1);
    }

    #[test]
    fn test_ring_queue_wraparound() {
        let queue = RingQueue::<i32, 4>::new();

        for i in 0..10 {
            queue.push(i);
        }

        // After overflow, should contain the last 4 values
        assert_eq!(queue.read_delayed(0), 9);
        assert_eq!(queue.read_delayed(1), 8);
        assert_eq!(queue.read_delayed(2), 7);
        assert_eq!(queue.read_delayed(3), 6);
    }

    #[test]
    fn test_ring_queue_interpolated() {
        let queue = RingQueue::<f64, 4>::new();

        queue.push(1.0);
        queue.push(2.0);
        queue.push(3.0);
        queue.push(4.0);

        let val = queue.read_interpolated(1.5);
        assert!((val - 2.5).abs() < 0.001);
    }
}
