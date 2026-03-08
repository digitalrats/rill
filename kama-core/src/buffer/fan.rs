//! # Fan-out and fan-in buffers for complex routing

use crate::math::AudioNum;
use crate::buffer::{
    AudioBuffer, BufferStats, 
    AtomicStats, AtomicCell, CACHE_LINE_SIZE
};
use core::sync::atomic::{AtomicBool, Ordering};
use core::marker::PhantomData;
use std::fmt;
use super::array_from_fn;

// ============================================================================
// FanOutBuffer
// ============================================================================

/// Buffer for broadcasting from one producer to multiple consumers
#[repr(align(64))]
pub struct FanOutBuffer<T: AudioNum, const N: usize, const CONSUMERS: usize> {
    /// Shared data storage using AtomicCell for each sample
    storage: [AtomicCell<T>; N],
    
    /// Current version (incremented on each write)
    version: AtomicCell<usize>,
    
    /// Last read version for each consumer
    read_versions: [AtomicCell<usize>; CONSUMERS],
    
    /// Valid flag
    valid: AtomicBool,
    
    /// Atomic statistics
    stats: AtomicStats,
    
    /// Phantom data
    _phantom: PhantomData<T>,
}

impl<T: AudioNum, const N: usize, const CONSUMERS: usize> FanOutBuffer<T, N, CONSUMERS> {
    /// Create new fan-out buffer
    pub fn new() -> Self {
        assert!(CONSUMERS > 0, "FanOutBuffer must have at least one consumer");
        
        // Create storage with default values
        let storage = array_from_fn(|_| AtomicCell::new(T::ZERO));
        
        Self {
            storage,
            version: AtomicCell::new(0),
            read_versions: array_from_fn(|_| AtomicCell::new(0)),
            valid: AtomicBool::new(false),
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }
    
    /// Write data to all consumers
    #[inline(always)]
    pub fn write(&self, data: &[T; N]) {
        for i in 0..N {
            self.storage[i].store(data[i]);
        }
        
        self.version.store(self.version.load() + 1);
        self.valid.store(true, Ordering::Release);
        
        self.stats.record_write();
        self.stats.update_peak(1);
    }
    
    /// Try to read for a specific consumer
    #[inline(always)]
    pub fn try_read(&self, consumer_id: usize) -> Option<[T; N]> {
        if consumer_id >= CONSUMERS {
            return None;
        }
        
        let current_version = self.version.load();
        let last_read = self.read_versions[consumer_id].load();
        
        if last_read == current_version || !self.valid.load(Ordering::Acquire) {
            self.stats.record_underflow();
            return None;
        }
        
        let mut result = [T::ZERO; N];
        for i in 0..N {
            result[i] = self.storage[i].load();
        }
        
        self.read_versions[consumer_id].store(current_version);
        
        self.stats.record_read();
        
        Some(result)
    }
    
    /// Check if a specific consumer has new data
    #[inline(always)]
    pub fn has_new_data(&self, consumer_id: usize) -> bool {
        if consumer_id >= CONSUMERS {
            return false;
        }
        
        let current_version = self.version.load();
        let last_read = self.read_versions[consumer_id].load();
        
        current_version != last_read && self.valid.load(Ordering::Acquire)
    }
    
    /// Get the number of consumers
    pub const fn consumer_count(&self) -> usize {
        CONSUMERS
    }
    
    /// Get current version
    pub fn current_version(&self) -> usize {
        self.version.load()
    }
    
    /// Get last read version for consumer
    pub fn last_read_version(&self, consumer_id: usize) -> Option<usize> {
        if consumer_id >= CONSUMERS {
            None
        } else {
            Some(self.read_versions[consumer_id].load())
        }
    }
    
    /// Reset the buffer
    pub fn reset(&self) {
        self.valid.store(false, Ordering::Release);
        for i in 0..CONSUMERS {
            self.read_versions[i].store(0);
        }
        self.stats.reset();
    }
}

impl<T: AudioNum, const N: usize, const CONSUMERS: usize> AudioBuffer<T>
    for FanOutBuffer<T, N, CONSUMERS>
{
    fn capacity(&self) -> usize {
        N
    }
    
    fn len(&self) -> usize {
        if self.valid.load(Ordering::Relaxed) { 1 } else { 0 }
    }
    
    fn is_empty(&self) -> bool {
        !self.valid.load(Ordering::Relaxed)
    }
    
    fn is_full(&self) -> bool {
        self.valid.load(Ordering::Relaxed)
    }
    
    fn clear(&mut self) {
        self.valid.store(false, Ordering::Release);
        for i in 0..CONSUMERS {
            self.read_versions[i].store(0);
        }
        self.stats.reset();
    }
    
    fn stats(&self) -> BufferStats {
        let mut stats = self.stats.snapshot();
        stats.fill_level = if self.valid.load(Ordering::Relaxed) { 1.0 } else { 0.0 };
        stats
    }
    
    fn reset_stats(&mut self) {
        self.stats.reset();
    }
}

// ============================================================================
// FanInBuffer
// ============================================================================

/// Buffer for mixing multiple producers to one consumer
#[repr(align(64))]
pub struct FanInBuffer<T: AudioNum, const N: usize, const PRODUCERS: usize> {
    /// Storage for each producer, each using AtomicCell for samples
    storage: [[AtomicCell<T>; N]; PRODUCERS],
    
    /// Valid flags for each producer
    valid: [AtomicBool; PRODUCERS],
    
    /// Write sequence for each producer
    write_seq: [AtomicCell<usize>; PRODUCERS],
    
    /// Last read sequence
    read_seq: AtomicCell<usize>,
    
    /// Atomic statistics
    stats: AtomicStats,
    
    /// Phantom data
    _phantom: PhantomData<T>,
}

impl<T: AudioNum, const N: usize, const PRODUCERS: usize> FanInBuffer<T, N, PRODUCERS> {
    /// Create new fan-in buffer
    pub fn new() -> Self {
        assert!(PRODUCERS > 0, "FanInBuffer must have at least one producer");
        
        // Create storage with default values
        let storage = array_from_fn(|_| {
            array_from_fn(|_| AtomicCell::new(T::ZERO))
        });
        
        Self {
            storage,
            valid: array_from_fn(|_| AtomicBool::new(false)),
            write_seq: array_from_fn(|_| AtomicCell::new(0)),
            read_seq: AtomicCell::new(0),
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }
    
    /// Write data from a specific producer
    #[inline(always)]
    pub fn write(&self, producer_id: usize, data: &[T; N]) {
        if producer_id >= PRODUCERS {
            return;
        }
        
        for i in 0..N {
            self.storage[producer_id][i].store(data[i]);
        }
        
        self.valid[producer_id].store(true, Ordering::Release);
        self.write_seq[producer_id].store(
            self.write_seq[producer_id].load() + 1
        );
        
        self.stats.record_write();
    }
    
    /// Try to read mixed data from all producers
    #[inline(always)]
    pub fn try_read(&self) -> Option<[T; N]> {
        let mut result = [T::ZERO; N];
        let mut any_valid = false;
        let mut active_producers = 0;
        let current_seq = self.read_seq.load();
        
        for producer in 0..PRODUCERS {
            if self.valid[producer].load(Ordering::Acquire) {
                let write_seq = self.write_seq[producer].load();
                
                if write_seq > current_seq {
                    any_valid = true;
                    active_producers += 1;
                    for i in 0..N {
                        result[i] = result[i] + self.storage[producer][i].load();
                    }
                }
            }
        }
        
        if any_valid {
            self.read_seq.store(self.read_seq.load() + 1);
            
            self.stats.record_read();
            self.stats.update_peak(active_producers);
            Some(result)
        } else {
            self.stats.record_underflow();
            None
        }
    }
    
    /// Get number of producers
    pub const fn producer_count(&self) -> usize {
        PRODUCERS
    }
    
    /// Check if producer has new data
    pub fn producer_has_data(&self, producer_id: usize) -> bool {
        if producer_id >= PRODUCERS {
            return false;
        }
        
        let write_seq = self.write_seq[producer_id].load();
        let read_seq = self.read_seq.load();
        
        write_seq > read_seq && self.valid[producer_id].load(Ordering::Acquire)
    }
    
    /// Get read sequence
    pub fn read_seq(&self) -> usize {
        self.read_seq.load()
    }
    
    /// Get write sequence for producer
    pub fn write_seq(&self, producer_id: usize) -> Option<usize> {
        if producer_id >= PRODUCERS {
            None
        } else {
            Some(self.write_seq[producer_id].load())
        }
    }
    
    /// Reset buffer
    pub fn reset(&self) {
        for producer in 0..PRODUCERS {
            self.valid[producer].store(false, Ordering::Release);
            self.write_seq[producer].store(0);
        }
        self.read_seq.store(0);
        self.stats.reset();
    }
    
    /// Clear specific producer
    pub fn clear_producer(&self, producer_id: usize) {
        if producer_id < PRODUCERS {
            self.valid[producer_id].store(false, Ordering::Release);
            self.write_seq[producer_id].store(0);
        }
    }
}

impl<T: AudioNum, const N: usize, const PRODUCERS: usize> AudioBuffer<T>
    for FanInBuffer<T, N, PRODUCERS>
{
    fn capacity(&self) -> usize {
        N * PRODUCERS
    }
    
    fn len(&self) -> usize {
        let read_seq = self.read_seq.load();
        let mut count = 0;
        
        for producer in 0..PRODUCERS {
            let write_seq = self.write_seq[producer].load();
            if write_seq > read_seq && self.valid[producer].load(Ordering::Acquire) {
                count += 1;
            }
        }
        
        count
    }
    
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    
    fn is_full(&self) -> bool {
        self.len() == PRODUCERS
    }
    
    fn clear(&mut self) {
        for producer in 0..PRODUCERS {
            self.valid[producer].store(false, Ordering::Release);
            self.write_seq[producer].store(0);
        }
        self.read_seq.store(0);
        self.stats.reset();
    }
    
    fn stats(&self) -> BufferStats {
        let mut stats = self.stats.snapshot();
        stats.fill_level = self.len() as f32 / PRODUCERS as f32;
        stats
    }
    
    fn reset_stats(&mut self) {
        self.stats.reset();
    }
}

// ============================================================================
// Default implementations
// ============================================================================

impl<T: AudioNum, const N: usize, const CONSUMERS: usize> Default
    for FanOutBuffer<T, N, CONSUMERS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T: AudioNum, const N: usize, const PRODUCERS: usize> Default
    for FanInBuffer<T, N, PRODUCERS>
{
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Debug implementations
// ============================================================================

impl<T: AudioNum + fmt::Debug, const N: usize, const CONSUMERS: usize> fmt::Debug
    for FanOutBuffer<T, N, CONSUMERS>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FanOutBuffer")
            .field("capacity", &N)
            .field("consumers", &CONSUMERS)
            .field("has_data", &self.valid.load(Ordering::Relaxed))
            .field("version", &self.version.load())
            .field("stats", &self.stats.snapshot())
            .field("alignment", &CACHE_LINE_SIZE)
            .finish()
    }
}

impl<T: AudioNum + fmt::Debug, const N: usize, const PRODUCERS: usize> fmt::Debug
    for FanInBuffer<T, N, PRODUCERS>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut active = 0;
        for i in 0..PRODUCERS {
            if self.valid[i].load(Ordering::Relaxed) {
                active += 1;
            }
        }
        
        f.debug_struct("FanInBuffer")
            .field("capacity", &(N * PRODUCERS))
            .field("producers", &PRODUCERS)
            .field("active_producers", &active)
            .field("len", &self.len())
            .field("read_seq", &self.read_seq.load())
            .field("stats", &self.stats.snapshot())
            .field("alignment", &CACHE_LINE_SIZE)
            .finish()
    }
}

// ============================================================================
// Clone implementations (where possible)
// ============================================================================

impl<T: AudioNum + Copy, const N: usize, const CONSUMERS: usize> Clone
    for FanOutBuffer<T, N, CONSUMERS>
{
    fn clone(&self) -> Self {
        let new = Self::new();
        
        if self.valid.load(Ordering::Acquire) {
            let mut data = [T::ZERO; N];
            for i in 0..N {
                data[i] = self.storage[i].load();
            }
            new.write(&data);
        }
        
        new
    }
}

impl<T: AudioNum + Copy, const N: usize, const PRODUCERS: usize> Clone
    for FanInBuffer<T, N, PRODUCERS>
{
    fn clone(&self) -> Self {
        let new = Self::new();
        
        for producer in 0..PRODUCERS {
            if self.valid[producer].load(Ordering::Acquire) {
                let mut data = [T::ZERO; N];
                for i in 0..N {
                    data[i] = self.storage[producer][i].load();
                }
                new.write(producer, &data);
            }
        }
        
        new
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fan_out_buffer_basic() {
        let buffer = FanOutBuffer::<f32, 64, 3>::new();
        
        let data = [42.0; 64];
        buffer.write(&data);
        
        for i in 0..3 {
            let read = buffer.try_read(i).unwrap();
            assert_eq!(read[0], 42.0);
        }
    }
    
    #[test]
    fn test_fan_in_buffer_basic() {
        let buffer = FanInBuffer::<f32, 64, 2>::new();
        
        buffer.write(0, &[1.0; 64]);
        buffer.write(1, &[2.0; 64]);
        
        let mixed = buffer.try_read().unwrap();
        assert_eq!(mixed[0], 3.0);
    }
}