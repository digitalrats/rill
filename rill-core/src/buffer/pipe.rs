use super::array_from_fn;
use crate::buffer::{AtomicStats, Buffer, BufferStats, CACHE_LINE_SIZE};
use crate::math::Transcendental;
use core::marker::PhantomData;
use std::fmt;

/// Single-producer, single-consumer buffer for intra-graph node connections.
///
/// Unlike its name suggests, this is **not** thread-safe — it is used
/// exclusively within the single-threaded signal graph. For cross-thread
/// communication use [`rill_core::queues`](crate::queues).
#[repr(align(64))]
pub struct PipeBuffer<T: Transcendental, const N: usize> {
    storage: [T; N],
    valid: bool,
    write_seq: usize,
    read_seq: usize,
    stats: AtomicStats,
    _phantom: PhantomData<[T; N]>,
}

impl<T: Transcendental, const N: usize> PipeBuffer<T, N> {
    /// Create a new empty pipe buffer.
    pub fn new() -> Self {
        let storage = array_from_fn(|_| T::ZERO);
        Self {
            storage,
            valid: false,
            write_seq: 0,
            read_seq: 0,
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }

    /// Write a block of data. Subsequent reads will return this data.
    #[inline(always)]
    pub fn write(&mut self, data: &[T; N]) {
        self.storage.copy_from_slice(data);
        self.valid = true;
        self.write_seq += 1;
        self.stats.record_write();
        self.stats.update_peak(1);
    }

    /// Read the latest written data (non-destructive).
    #[inline(always)]
    pub fn read(&mut self) -> Option<[T; N]> {
        if !self.valid {
            return None;
        }
        let mut result = [T::ZERO; N];
        result.copy_from_slice(&self.storage);
        self.read_seq += 1;
        self.stats.record_read();
        Some(result)
    }

    /// Read the latest written data (destructive — clears the valid flag).
    #[inline(always)]
    pub fn try_read(&mut self) -> Option<[T; N]> {
        if !self.valid {
            self.stats.record_underflow();
            return None;
        }
        let mut result = [T::ZERO; N];
        result.copy_from_slice(&self.storage);
        self.valid = false;
        self.read_seq += 1;
        self.stats.record_read();
        self.stats.update_peak(0);
        Some(result)
    }

    /// Busy-wait until data is available, then read destructively.
    pub fn read_blocking(&mut self) -> [T; N] {
        loop {
            if let Some(data) = self.try_read() {
                return data;
            }
            core::hint::spin_loop();
        }
    }

    /// Whether data is available for reading.
    pub fn has_data(&self) -> bool {
        self.valid
    }
    /// Number of writes performed.
    pub fn write_seq(&self) -> usize {
        self.write_seq
    }
    /// Number of reads performed.
    pub fn read_seq(&self) -> usize {
        self.read_seq
    }
    /// Whether all writes have been consumed by reads.
    pub fn is_caught_up(&self) -> bool {
        self.write_seq == self.read_seq
    }
    /// Number of times the buffer was overwritten before being read.
    pub fn overwrites(&self) -> usize {
        self.write_seq.saturating_sub(self.read_seq + 1)
    }

    /// Reset to initial empty state.
    pub fn reset(&mut self) {
        self.valid = false;
        self.stats.reset();
    }
}

impl<T: Transcendental, const N: usize> Buffer<T> for PipeBuffer<T, N> {
    fn capacity(&self) -> usize {
        N
    }
    fn len(&self) -> usize {
        if self.valid {
            1
        } else {
            0
        }
    }
    fn is_empty(&self) -> bool {
        !self.valid
    }
    fn is_full(&self) -> bool {
        self.valid
    }
    fn as_slice(&self) -> &[T] {
        &self.storage
    }
    fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.storage
    }
    fn fill(&mut self, value: T) {
        self.storage.fill(value);
    }
    fn copy_from(&mut self, src: &[T]) {
        let len = src.len().min(N);
        self.storage[..len].copy_from_slice(&src[..len]);
    }
    fn clear(&mut self) {
        self.valid = false;
        self.stats.reset();
    }
    fn stats(&self) -> BufferStats {
        let mut stats = self.stats.snapshot();
        stats.fill_level = if self.valid { 1.0 } else { 0.0 };
        stats
    }
    fn reset_stats(&mut self) {
        self.stats.reset();
    }
}

impl<T: Transcendental, const N: usize> Default for PipeBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental + fmt::Debug, const N: usize> fmt::Debug for PipeBuffer<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipeBuffer")
            .field("capacity", &N)
            .field("has_data", &self.valid)
            .field("write_seq", &self.write_seq)
            .field("read_seq", &self.read_seq)
            .field("overwrites", &self.overwrites())
            .field("stats", &self.stats.snapshot())
            .field("alignment", &CACHE_LINE_SIZE)
            .finish()
    }
}

impl<T: Transcendental + Copy, const N: usize> Clone for PipeBuffer<T, N> {
    fn clone(&self) -> Self {
        let mut new = Self::new();
        if self.valid {
            new.write(&self.storage);
        }
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipe_buffer_basic() {
        let mut buffer = PipeBuffer::<f32, 64>::new();
        let write_data = [42.0; 64];
        buffer.write(&write_data);
        assert!(buffer.has_data());
        assert_eq!(buffer.write_seq(), 1);
        let read_data = buffer.try_read().unwrap();
        assert_eq!(read_data[0], 42.0);
        assert_eq!(buffer.read_seq(), 1);
        assert!(buffer.is_caught_up());
    }
}
