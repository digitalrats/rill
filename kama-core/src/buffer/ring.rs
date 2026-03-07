//! Generic ring buffer for multi-producer, multi-consumer scenarios
//!
//! Provides a lock-free ring buffer that can handle multiple producers
//! and multiple consumers simultaneously.

use crate::math::AudioNum;
use crate::buffer::{AudioBuffer, BufferStats, AlignedStorage, AtomicStats, BufferError, BufferResult};
use core::sync::atomic::{AtomicUsize, Ordering};
use core::marker::PhantomData;
use std::fmt;

/// Multi-producer, multi-consumer ring buffer
///
/// # Type Parameters
/// - `T`: Audio sample type (f32 or f64)
/// - `N`: Buffer size (must be power of two for efficient masking)
///
/// # Example
/// ```
/// use kama_core::buffer::ring::RingBuffer;
///
/// let buffer = RingBuffer::<f32, 256>::new();
///
/// // Producer
/// buffer.write(42.0).unwrap();
/// buffer.write(43.0).unwrap();
///
/// // Consumer
/// assert_eq!(buffer.read().unwrap(), 42.0);
/// assert_eq!(buffer.read().unwrap(), 43.0);
/// ```
#[repr(align(64))]
pub struct RingBuffer<T: AudioNum, const N: usize> {
    /// Storage
    storage: AlignedStorage<T, N>,
    
    /// Write index
    write_idx: AtomicUsize,
    
    /// Read index
    read_idx: AtomicUsize,
    
    /// Mask for efficient indexing (N-1, works when N is power of two)
    mask: usize,
    
    /// Atomic statistics
    stats: AtomicStats,
    
    /// Phantom data
    _phantom: PhantomData<T>,
}

impl<T: AudioNum, const N: usize> RingBuffer<T, N> {
    /// Create new ring buffer
    ///
    /// # Panics
    /// Panics if N is not a power of two
    pub fn new() -> Self {
        assert!(N.is_power_of_two(), "RingBuffer size must be power of two");
        
        Self {
            storage: AlignedStorage::new(),
            write_idx: AtomicUsize::new(0),
            read_idx: AtomicUsize::new(0),
            mask: N - 1,
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }
    
    /// Write a single sample
    ///
    /// Returns `Err(BufferError::Full)` if the buffer is full.
    #[inline(always)]
    pub fn write(&self, sample: T) -> BufferResult<()> {
        let write = self.write_idx.load(Ordering::Relaxed);
        let read = self.read_idx.load(Ordering::Acquire);
        
        // Check if buffer is full
        if write.wrapping_sub(read) >= N {
            self.stats.record_overflow();
            return Err(BufferError::Full);
        }
        
        // Write sample
        let idx = write & self.mask;
        self.storage.write(idx, sample);
        
        self.write_idx.store(write.wrapping_add(1), Ordering::Release);
        self.stats.record_write();
        self.stats.update_peak(self.available());
        
        Ok(())
    }
    
    /// Read a single sample
    ///
    /// Returns `Err(BufferError::Empty)` if the buffer is empty.
    #[inline(always)]
    pub fn read(&self) -> BufferResult<T> {
        let write = self.write_idx.load(Ordering::Acquire);
        let read = self.read_idx.load(Ordering::Relaxed);
        
        // Check if buffer is empty
        if read == write {
            self.stats.record_underflow();
            return Err(BufferError::Empty);
        }
        
        // Read sample
        let idx = read & self.mask;
        let sample = unsafe { self.storage.read(idx) };
        
        self.read_idx.store(read.wrapping_add(1), Ordering::Release);
        self.stats.record_read();
        
        Ok(sample)
    }
    
    /// Write a block of samples
    ///
    /// Returns the number of samples successfully written.
    #[inline(always)]
    pub fn write_block(&self, samples: &[T]) -> usize {
        let mut written = 0;
        
        for &sample in samples {
            match self.write(sample) {
                Ok(()) => written += 1,
                Err(BufferError::Full) => break,
                Err(_) => break,
            }
        }
        
        written
    }
    
    /// Read a block of samples
    ///
    /// Returns the number of samples successfully read.
    #[inline(always)]
    pub fn read_block(&self, samples: &mut [T]) -> usize {
        let mut read = 0;
        
        for sample in samples.iter_mut() {
            match self.read() {
                Ok(val) => {
                    *sample = val;
                    read += 1;
                }
                Err(BufferError::Empty) => break,
                Err(_) => break,
            }
        }
        
        read
    }
    
    /// Get number of available samples for reading
    #[inline(always)]
    pub fn available(&self) -> usize {
        let write = self.write_idx.load(Ordering::Acquire);
        let read = self.read_idx.load(Ordering::Relaxed);
        write.wrapping_sub(read)
    }
    
    /// Get remaining space for writing
    #[inline(always)]
    pub fn remaining(&self) -> usize {
        N - self.available()
    }
    
    /// Check if buffer is empty
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.available() == 0
    }
    
    /// Check if buffer is full
    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.available() == N
    }
    
    /// Clear the buffer (reset indices)
    pub fn clear(&mut self) {
        self.write_idx.store(0, Ordering::Release);
        self.read_idx.store(0, Ordering::Release);
        self.stats.reset();
    }
}

impl<T: AudioNum, const N: usize> AudioBuffer<T> for RingBuffer<T, N> {
    fn capacity(&self) -> usize {
        N
    }
    
    fn len(&self) -> usize {
        self.available()
    }
    
    fn clear(&mut self) {
        self.clear();
    }
    
    fn stats(&self) -> BufferStats {
        let mut stats = self.stats.snapshot();
        stats.fill_level = self.available() as f32 / N as f32;
        stats
    }
    
    fn reset_stats(&mut self) {
        self.stats.reset();
    }
}

impl<T: AudioNum, const N: usize> Default for RingBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: AudioNum, const N: usize> fmt::Debug for RingBuffer<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RingBuffer")
            .field("capacity", &N)
            .field("available", &self.available())
            .field("remaining", &self.remaining())
            .field("stats", &self.stats.snapshot())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ring_buffer_f32() {
        let buffer = RingBuffer::<f32, 64>::new();
        
        // Write
        buffer.write(42.0).unwrap();
        assert_eq!(buffer.available(), 1);
        
        // Read
        let val = buffer.read().unwrap();
        assert_eq!(val, 42.0);
        assert_eq!(buffer.available(), 0);
    }
    
    #[test]
    fn test_ring_buffer_f64() {
        let buffer = RingBuffer::<f64, 64>::new();
        
        buffer.write(42.0).unwrap();
        let val = buffer.read().unwrap();
        assert!((val - 42.0).abs() < 1e-10);
    }
    
    #[test]
    fn test_ring_buffer_full() {
        let buffer = RingBuffer::<f32, 4>::new();
        
        // Fill buffer
        for i in 0..4 {
            assert!(buffer.write(i as f32).is_ok());
        }
        
        // Next write should fail
        assert!(buffer.write(4.0).is_err());
        assert_eq!(buffer.stats().overflows, 1);
        assert!(buffer.is_full());
    }
    
    #[test]
    fn test_ring_buffer_empty() {
        let buffer = RingBuffer::<f32, 4>::new();
        
        // Read from empty buffer
        assert!(buffer.read().is_err());
        assert_eq!(buffer.stats().underflows, 1);
        assert!(buffer.is_empty());
    }
    
    #[test]
    fn test_ring_buffer_block_ops() {
        let buffer = RingBuffer::<f32, 64>::new();
        
        let write_data = [1.0, 2.0, 3.0, 4.0];
        let written = buffer.write_block(&write_data);
        assert_eq!(written, 4);
        
        let mut read_data = [0.0; 4];
        let read = buffer.read_block(&mut read_data);
        assert_eq!(read, 4);
        assert_eq!(read_data, write_data);
    }
    
    #[test]
    #[should_panic(expected = "RingBuffer size must be power of two")]
    fn test_ring_buffer_invalid_size() {
        let _ = RingBuffer::<f32, 10>::new();
    }
    
    #[test]
    fn test_ring_buffer_wrap_around() {
        let buffer = RingBuffer::<f32, 4>::new();
        
        // Fill and read to create wrap-around
        for i in 0..3 {
            buffer.write(i as f32).unwrap();
        }
        
        for _ in 0..2 {
            let _ = buffer.read().unwrap();
        }
        
        // Write more
        buffer.write(3.0).unwrap();
        buffer.write(4.0).unwrap();
        
        // Should still work
        assert_eq!(buffer.read().unwrap(), 2.0);
        assert_eq!(buffer.read().unwrap(), 3.0);
        assert_eq!(buffer.read().unwrap(), 4.0);
        assert!(buffer.read().is_err());
    }
}