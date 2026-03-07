//! Point-to-point buffer for single producer, single consumer connections
//!
//! `PipeBuffer` provides a lock-free, wait-free buffer for connecting
//! two audio nodes directly. It is optimized for the common case of
//! one producer and one consumer.

use crate::math::AudioNum;
use crate::buffer::{AudioBuffer, BufferStats, AlignedStorage, Sequence, AtomicStats};
use core::sync::atomic::{AtomicBool, Ordering};
use core::marker::PhantomData;
use std::fmt;

/// Single-producer, single-consumer buffer for node connections
///
/// This buffer provides wait-free operations and minimal overhead.
/// It is ideal for point-to-point connections between audio nodes.
///
/// # Type Parameters
/// - `T`: Audio sample type (f32 or f64)
/// - `N`: Buffer size (number of samples per block)
///
/// # Example
/// ```
/// use kama_core::buffer::pipe::PipeBuffer;
///
/// let buffer = PipeBuffer::<f32, 64>::new();
///
/// // Producer thread
/// let data = [1.0; 64];
/// buffer.write(&data);
///
/// // Consumer thread
/// if let Some(read_data) = buffer.try_read() {
///     assert_eq!(read_data[0], 1.0);
/// }
/// ```
#[repr(align(64))]
pub struct PipeBuffer<T: AudioNum, const N: usize> {
    /// Storage for the buffer (aligned to cache line)
    storage: AlignedStorage<T, N>,
    
    /// Flag indicating if buffer contains valid data
    valid: AtomicBool,
    
    /// Write sequence number (for debugging)
    write_seq: Sequence,
    
    /// Read sequence number (for debugging)
    read_seq: Sequence,
    
    /// Atomic statistics
    stats: AtomicStats,
    
    /// Phantom data
    _phantom: PhantomData<[T; N]>,
}

impl<T: AudioNum, const N: usize> PipeBuffer<T, N> {
    /// Create a new pipe buffer
    pub fn new() -> Self {
        Self {
            storage: AlignedStorage::new(),
            valid: AtomicBool::new(false),
            write_seq: Sequence::new(),
            read_seq: Sequence::new(),
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }
    
    /// Write data to the buffer
    ///
    /// This operation is wait-free and will overwrite any existing data.
    /// The buffer holds at most one block at a time.
    ///
    /// # Arguments
    /// * `data` - Array of samples to write
    #[inline(always)]
    pub fn write(&self, data: &[T; N]) {
        // Copy data to storage
        for i in 0..N {
            self.storage.write(i, data[i]);
        }
        
        // Mark as valid (release ordering ensures data is visible)
        self.valid.store(true, Ordering::Release);
        self.write_seq.fetch_add(1, Ordering::Relaxed);
        
        // Update statistics
        self.stats.record_write();
        self.stats.update_peak(1);
    }
    
    /// Try to read data from the buffer
    ///
    /// Returns `None` if no data is available.
    #[inline(always)]
    pub fn try_read(&self) -> Option<[T; N]> {
        if !self.valid.load(Ordering::Acquire) {
            self.stats.record_underflow();
            return None;
        }
        
        let mut result = [T::ZERO; N];
        for i in 0..N {
            unsafe {
                result[i] = self.storage.read(i);
            }
        }
        
        self.valid.store(false, Ordering::Release);
        self.read_seq.fetch_add(1, Ordering::Relaxed);
        
        self.stats.record_read();
        self.stats.update_peak(0);
        
        Some(result)
    }
    
    /// Read data, blocking until available (for non-real-time use)
    ///
    /// This is a convenience method for non-real-time contexts.
    /// It spins until data is available.
    pub fn read_blocking(&self) -> [T; N] {
        loop {
            if let Some(data) = self.try_read() {
                return data;
            }
            core::hint::spin_loop();
        }
    }
    
    /// Check if buffer has valid data
    #[inline(always)]
    pub fn has_data(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }
    
    /// Get write sequence number (for debugging)
    pub fn write_seq(&self) -> usize {
        self.write_seq.load(Ordering::Relaxed)
    }
    
    /// Get read sequence number (for debugging)
    pub fn read_seq(&self) -> usize {
        self.read_seq.load(Ordering::Relaxed)
    }
    
    /// Check if reader is caught up with writer
    pub fn is_caught_up(&self) -> bool {
        self.write_seq.load(Ordering::Relaxed) == self.read_seq.load(Ordering::Relaxed) + 1
    }
}

impl<T: AudioNum, const N: usize> AudioBuffer<T> for PipeBuffer<T, N> {
    fn capacity(&self) -> usize {
        N
    }
    
    fn len(&self) -> usize {
        if self.has_data() { 1 } else { 0 }
    }
    
    fn clear(&mut self) {
        self.valid.store(false, Ordering::Release);
        self.stats.reset();
    }
    
    fn stats(&self) -> BufferStats {
        let mut stats = self.stats.snapshot();
        stats.fill_level = if self.has_data() { 1.0 } else { 0.0 };
        stats
    }
    
    fn reset_stats(&mut self) {
        self.stats.reset();
    }
}

impl<T: AudioNum, const N: usize> Default for PipeBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: AudioNum, const N: usize> fmt::Debug for PipeBuffer<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipeBuffer")
            .field("capacity", &N)
            .field("has_data", &self.has_data())
            .field("write_seq", &self.write_seq())
            .field("read_seq", &self.read_seq())
            .field("stats", &self.stats.snapshot())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pipe_buffer_f32() {
        let buffer = PipeBuffer::<f32, 64>::new();
        
        assert!(!buffer.has_data());
        assert_eq!(buffer.len(), 0);
        
        let data = [42.0; 64];
        buffer.write(&data);
        
        assert!(buffer.has_data());
        assert_eq!(buffer.len(), 1);
        
        let read = buffer.try_read().unwrap();
        assert_eq!(read[0], 42.0);
        assert_eq!(read[63], 42.0);
        
        assert!(!buffer.has_data());
        assert_eq!(buffer.len(), 0);
    }
    
    #[test]
    fn test_pipe_buffer_f64() {
        let buffer = PipeBuffer::<f64, 64>::new();
        
        let data = [42.0; 64];
        buffer.write(&data);
        
        let read = buffer.try_read().unwrap();
        assert!((read[0] - 42.0).abs() < 1e-10);
    }
    
    #[test]
    fn test_pipe_buffer_overwrite() {
        let buffer = PipeBuffer::<f32, 64>::new();
        
        let data1 = [1.0; 64];
        let data2 = [2.0; 64];
        
        buffer.write(&data1);
        buffer.write(&data2); // Overwrites without reading
        
        let read = buffer.try_read().unwrap();
        assert_eq!(read[0], 2.0); // Should get latest data
    }
    
    #[test]
    fn test_pipe_buffer_empty_read() {
        let buffer = PipeBuffer::<f32, 64>::new();
        
        assert!(buffer.try_read().is_none());
        assert_eq!(buffer.stats().underflows, 1);
    }
    
    #[test]
    fn test_pipe_buffer_stats() {
        let buffer = PipeBuffer::<f32, 64>::new();
        
        let data = [1.0; 64];
        buffer.write(&data);
        
        let stats = buffer.stats();
        assert_eq!(stats.writes, 1);
        assert_eq!(stats.fill_level, 1.0);
        
        let _ = buffer.try_read();
        
        let stats = buffer.stats();
        assert_eq!(stats.reads, 1);
        assert_eq!(stats.fill_level, 0.0);
    }
}