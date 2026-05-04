use super::array_from_fn;
use crate::buffer::{AtomicStats, SignalBuffer, BufferStats, CACHE_LINE_SIZE};
use crate::math::Transcendental;
use core::marker::PhantomData;
use std::fmt;

// ============================================================================
// FanOutBuffer
// ============================================================================

/// Buffer for broadcasting from one producer to multiple consumers.
/// Single-threaded — use [`rill_core::queues`](crate::queues) for cross-thread.
#[repr(align(64))]
pub struct FanOutBuffer<T: Transcendental, const N: usize, const CONSUMERS: usize> {
    storage: [T; N],
    version: usize,
    read_versions: [usize; CONSUMERS],
    valid: bool,
    stats: AtomicStats,
    _phantom: PhantomData<T>,
}

impl<T: Transcendental, const N: usize, const CONSUMERS: usize> FanOutBuffer<T, N, CONSUMERS> {
    /// Create a new fan-out buffer.
    ///
    /// # Panics
    /// Panics if `CONSUMERS` is 0.
    pub fn new() -> Self {
        assert!(CONSUMERS > 0, "FanOutBuffer must have at least one consumer");
        Self {
            storage: array_from_fn(|_| T::ZERO),
            version: 0,
            read_versions: [0; CONSUMERS],
            valid: false,
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }

    /// Broadcast data to all consumers.
    #[inline(always)]
    pub fn write(&mut self, data: &[T; N]) {
        self.storage.copy_from_slice(data);
        self.version += 1;
        self.valid = true;
        self.stats.record_write();
        self.stats.update_peak(1);
    }

    /// Read data for a specific consumer, returning `None` if already read or no data available.
    #[inline(always)]
    pub fn try_read(&mut self, consumer_id: usize) -> Option<[T; N]> {
        if consumer_id >= CONSUMERS {
            return None;
        }
        let current_version = self.version;
        if self.read_versions[consumer_id] == current_version || !self.valid {
            self.stats.record_underflow();
            return None;
        }
        let mut result = [T::ZERO; N];
        result.copy_from_slice(&self.storage);
        self.read_versions[consumer_id] = current_version;
        self.stats.record_read();
        Some(result)
    }

    /// Whether unread data exists for the given consumer.
    pub fn has_new_data(&self, consumer_id: usize) -> bool {
        consumer_id < CONSUMERS && self.version != self.read_versions[consumer_id] && self.valid
    }

    /// Number of consumers (const generic parameter).
    pub const fn consumer_count(&self) -> usize { CONSUMERS }
    /// Current write version.
    pub fn current_version(&self) -> usize { self.version }
    /// Version last read by a consumer, or `None` if consumer ID is invalid.
    pub fn last_read_version(&self, consumer_id: usize) -> Option<usize> {
        if consumer_id >= CONSUMERS { None } else { Some(self.read_versions[consumer_id]) }
    }

    /// Reset to initial state (invalid, all consumers at version 0).
    pub fn reset(&mut self) {
        self.valid = false;
        self.read_versions.fill(0);
        self.stats.reset();
    }
}

impl<T: Transcendental, const N: usize, const CONSUMERS: usize> SignalBuffer<T>
    for FanOutBuffer<T, N, CONSUMERS>
{
    fn capacity(&self) -> usize { N }
    fn len(&self) -> usize { if self.valid { 1 } else { 0 } }
    fn is_empty(&self) -> bool { !self.valid }
    fn is_full(&self) -> bool { self.valid }
    fn clear(&mut self) { self.reset(); }
    fn stats(&self) -> BufferStats {
        let mut stats = self.stats.snapshot();
        stats.fill_level = if self.valid { 1.0 } else { 0.0 };
        stats
    }
    fn reset_stats(&mut self) { self.stats.reset(); }
}

impl<T: Transcendental, const N: usize, const CONSUMERS: usize> Default
    for FanOutBuffer<T, N, CONSUMERS>
{
    fn default() -> Self { Self::new() }
}

impl<T: Transcendental + fmt::Debug, const N: usize, const CONSUMERS: usize> fmt::Debug
    for FanOutBuffer<T, N, CONSUMERS>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FanOutBuffer")
            .field("capacity", &N)
            .field("consumers", &CONSUMERS)
            .field("has_data", &self.valid)
            .field("version", &self.version)
            .field("stats", &self.stats.snapshot())
            .field("alignment", &CACHE_LINE_SIZE)
            .finish()
    }
}

// ============================================================================
// FanInBuffer
// ============================================================================

/// Buffer for mixing multiple producers to one consumer.
/// Single-threaded — use [`rill_core::queues`](crate::queues) for cross-thread.
#[repr(align(64))]
pub struct FanInBuffer<T: Transcendental, const N: usize, const PRODUCERS: usize> {
    storage: [[T; N]; PRODUCERS],
    valid: [bool; PRODUCERS],
    write_seq: [usize; PRODUCERS],
    read_seq: usize,
    stats: AtomicStats,
    _phantom: PhantomData<T>,
}

impl<T: Transcendental, const N: usize, const PRODUCERS: usize> FanInBuffer<T, N, PRODUCERS> {
    /// Create a new fan-in buffer.
    ///
    /// # Panics
    /// Panics if `PRODUCERS` is 0.
    pub fn new() -> Self {
        assert!(PRODUCERS > 0, "FanInBuffer must have at least one producer");
        Self {
            storage: array_from_fn(|_| [T::ZERO; N]),
            valid: [false; PRODUCERS],
            write_seq: [0; PRODUCERS],
            read_seq: 0,
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }

    /// Write a block of data from one producer.
    #[inline(always)]
    pub fn write(&mut self, producer_id: usize, data: &[T; N]) {
        if producer_id >= PRODUCERS { return; }
        self.storage[producer_id].copy_from_slice(data);
        self.valid[producer_id] = true;
        self.write_seq[producer_id] += 1;
        self.stats.record_write();
    }

    /// Read and sum all producers' data that have new writes since last read.
    #[inline(always)]
    pub fn try_read(&mut self) -> Option<[T; N]> {
        let mut result = [T::ZERO; N];
        let mut any_valid = false;
        let mut active_producers = 0;
        let current_seq = self.read_seq;
        for producer in 0..PRODUCERS {
            if self.valid[producer] && self.write_seq[producer] > current_seq {
                any_valid = true;
                active_producers += 1;
                for i in 0..N {
                    result[i] += self.storage[producer][i];
                }
            }
        }
        if any_valid {
            self.read_seq += 1;
            self.stats.record_read();
            self.stats.update_peak(active_producers);
            Some(result)
        } else {
            self.stats.record_underflow();
            None
        }
    }

    /// Number of producers (const generic parameter).
    pub const fn producer_count(&self) -> usize { PRODUCERS }

    /// Whether a specific producer has unread data.
    pub fn producer_has_data(&self, producer_id: usize) -> bool {
        if producer_id >= PRODUCERS { return false; }
        self.write_seq[producer_id] > self.read_seq && self.valid[producer_id]
    }

    /// Current read sequence counter.
    pub fn read_seq(&self) -> usize { self.read_seq }
    /// Write sequence counter for a specific producer, or `None` if ID is invalid.
    pub fn write_seq(&self, producer_id: usize) -> Option<usize> {
        if producer_id >= PRODUCERS { None } else { Some(self.write_seq[producer_id]) }
    }

    /// Reset all producers and the read counter.
    pub fn reset(&mut self) {
        self.valid.fill(false);
        self.write_seq.fill(0);
        self.read_seq = 0;
        self.stats.reset();
    }

    /// Clear a specific producer's data without affecting others.
    pub fn clear_producer(&mut self, producer_id: usize) {
        if producer_id < PRODUCERS {
            self.valid[producer_id] = false;
            self.write_seq[producer_id] = 0;
        }
    }
}

impl<T: Transcendental, const N: usize, const PRODUCERS: usize> SignalBuffer<T>
    for FanInBuffer<T, N, PRODUCERS>
{
    fn capacity(&self) -> usize { N * PRODUCERS }
    fn len(&self) -> usize {
        let mut count = 0;
        for producer in 0..PRODUCERS {
            if self.write_seq[producer] > self.read_seq && self.valid[producer] {
                count += 1;
            }
        }
        count
    }
    fn is_empty(&self) -> bool { self.len() == 0 }
    fn is_full(&self) -> bool { self.len() == PRODUCERS }
    fn clear(&mut self) { self.reset(); }
    fn stats(&self) -> BufferStats {
        let mut stats = self.stats.snapshot();
        stats.fill_level = self.len() as f32 / PRODUCERS as f32;
        stats
    }
    fn reset_stats(&mut self) { self.stats.reset(); }
}

impl<T: Transcendental, const N: usize, const PRODUCERS: usize> Default for FanInBuffer<T, N, PRODUCERS> {
    fn default() -> Self { Self::new() }
}

impl<T: Transcendental + fmt::Debug, const N: usize, const PRODUCERS: usize> fmt::Debug
    for FanInBuffer<T, N, PRODUCERS>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let active = self.valid.iter().filter(|v| **v).count();
        f.debug_struct("FanInBuffer")
            .field("capacity", &(N * PRODUCERS))
            .field("producers", &PRODUCERS)
            .field("active_producers", &active)
            .field("len", &self.len())
            .field("read_seq", &self.read_seq)
            .field("stats", &self.stats.snapshot())
            .field("alignment", &CACHE_LINE_SIZE)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fan_out_buffer_basic() {
        let mut buffer = FanOutBuffer::<f32, 64, 3>::new();
        let data = [42.0; 64];
        buffer.write(&data);
        for i in 0..3 {
            let read = buffer.try_read(i).unwrap();
            assert_eq!(read[0], 42.0);
        }
    }

    #[test]
    fn test_fan_in_buffer_basic() {
        let mut buffer = FanInBuffer::<f32, 64, 2>::new();
        buffer.write(0, &[1.0; 64]);
        buffer.write(1, &[2.0; 64]);
        let mixed = buffer.try_read().unwrap();
        assert_eq!(mixed[0], 3.0);
    }
}
