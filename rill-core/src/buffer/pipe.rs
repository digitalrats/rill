//! # Point-to-point buffer for single producer, single consumer connections

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
// PipeBuffer
// ============================================================================

/// Single-producer, single-consumer buffer for node connections
///
/// This buffer provides wait-free operations and minimal overhead.
/// It is ideal for point-to-point connections between audio nodes.
///
/// # Type Parameters
/// - `T`: Audio sample type (f32 or f64) implementing `AudioNum`
/// - `N`: Buffer size (number of samples per block)
#[repr(align(64))]
pub struct PipeBuffer<T: AudioNum, const N: usize> {
    /// Storage for the buffer using AtomicCell for each sample
    /// This provides safe concurrent access without unsafe code
    storage: [AtomicCell<T>; N],
    
    /// Flag indicating if buffer contains valid data
    /// - `true`: data available for reading
    /// - `false`: buffer empty
    valid: AtomicBool,
    
    /// Write sequence number (monotonically increasing)
    /// Used for debugging and detecting overwrites
    write_seq: AtomicCell<usize>,
    
    /// Read sequence number (monotonically increasing)
    /// Used for debugging and detecting underruns
    read_seq: AtomicCell<usize>,
    
    /// Atomic statistics for performance monitoring
    stats: AtomicStats,
    
    /// Phantom data to satisfy const generic
    _phantom: PhantomData<[T; N]>,
}

impl<T: AudioNum, const N: usize> PipeBuffer<T, N> {
    /// Create a new pipe buffer
    ///
    /// The buffer starts empty with no data available.
    pub fn new() -> Self {
        // Create storage with default values (T::ZERO)
        let storage = array_from_fn(|_| AtomicCell::new(T::ZERO));
        
        Self {
            storage,
            valid: AtomicBool::new(false),
            write_seq: AtomicCell::new(0),
            read_seq: AtomicCell::new(0),
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }
    
    /// Write a block of data to the buffer
    ///
    /// This operation is wait-free and will overwrite any existing data.
    /// The buffer holds at most one block at a time - new writes always
    /// overwrite the previous block, regardless of whether it was read.
    ///
    /// # Arguments
    /// * `data` - Array of samples to write (must be exactly `N` samples)
    #[inline(always)]
    pub fn write(&self, data: &[T; N]) {
        // Copy data to storage using AtomicCell's store
        // This is safe and doesn't require unsafe code
        for i in 0..N {
            self.storage[i].store(data[i]);
        }
        
        // Mark as valid (release ordering ensures data is visible)
        self.valid.store(true, Ordering::Release);
        self.write_seq.store(self.write_seq.load() + 1);
        
        // Update statistics
        self.stats.record_write();
        self.stats.update_peak(1);
    }
    
    /// Try to read a block of data from the buffer
    ///
    /// Returns `Some(data)` if data is available, `None` otherwise.
    /// This operation is wait-free and non-blocking.
    #[inline(always)]
    pub fn try_read(&self) -> Option<[T; N]> {
        if !self.valid.load(Ordering::Acquire) {
            self.stats.record_underflow();
            return None;
        }
        
        let mut result = [T::ZERO; N];
        for i in 0..N {
            // AtomicCell's load is safe and doesn't require unsafe code
            result[i] = self.storage[i].load();
        }
        
        self.valid.store(false, Ordering::Release);
        self.read_seq.store(self.read_seq.load() + 1);
        
        self.stats.record_read();
        self.stats.update_peak(0);
        
        Some(result)
    }
    
    /// Read data, blocking until available (for non-real-time use)
    ///
    /// This is a convenience method for non-real-time contexts like
    /// testing or offline processing. It spins until data is available.
    pub fn read_blocking(&self) -> [T; N] {
        loop {
            if let Some(data) = self.try_read() {
                return data;
            }
            core::hint::spin_loop();
        }
    }
    
    /// Check if buffer has valid data available
    #[inline(always)]
    pub fn has_data(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }
    
    /// Get write sequence number (for debugging)
    pub fn write_seq(&self) -> usize {
        self.write_seq.load()
    }
    
    /// Get read sequence number (for debugging)
    pub fn read_seq(&self) -> usize {
        self.read_seq.load()
    }
    
    /// Check if reader is caught up with writer
    pub fn is_caught_up(&self) -> bool {
        self.write_seq() == self.read_seq()
    }
    
    /// Get the number of overwritten blocks (for debugging)
    pub fn overwrites(&self) -> usize {
        self.write_seq().saturating_sub(self.read_seq() + 1)
    }
    
    /// Reset the buffer to empty state
    ///
    /// Clears the valid flag and resets statistics.
    /// Does not actually zero the memory (not needed for correctness).
    pub fn reset(&self) {
        self.valid.store(false, Ordering::Release);
        self.stats.reset();
    }
}

// ============================================================================
// AudioBuffer Implementation
// ============================================================================

impl<T: AudioNum, const N: usize> AudioBuffer<T> for PipeBuffer<T, N> {
    fn capacity(&self) -> usize {
        N
    }
    
    fn len(&self) -> usize {
        if self.has_data() { 1 } else { 0 }
    }
    
    fn is_empty(&self) -> bool {
        !self.has_data()
    }
    
    fn is_full(&self) -> bool {
        self.has_data()
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

// ============================================================================
// Default Implementation
// ============================================================================

impl<T: AudioNum, const N: usize> Default for PipeBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Debug Implementation
// ============================================================================

impl<T: AudioNum + fmt::Debug, const N: usize> fmt::Debug for PipeBuffer<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipeBuffer")
            .field("capacity", &N)
            .field("has_data", &self.has_data())
            .field("write_seq", &self.write_seq())
            .field("read_seq", &self.read_seq())
            .field("overwrites", &self.overwrites())
            .field("stats", &self.stats.snapshot())
            .field("alignment", &CACHE_LINE_SIZE)
            .finish()
    }
}

// ============================================================================
// Clone Implementation
// ============================================================================

impl<T: AudioNum + Copy, const N: usize> Clone for PipeBuffer<T, N> {
    fn clone(&self) -> Self {
        let new = Self::new();
        
        // If this buffer has data, copy it
        if let Some(data) = self.try_read() {
            new.write(&data);
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
    fn test_pipe_buffer_basic() {
        let buffer = PipeBuffer::<f32, 64>::new();
        
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