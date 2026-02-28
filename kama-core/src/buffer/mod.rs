//! # Audio Buffers with AudioNum Support
//!
//! This module provides lock-free, real-time safe buffers for audio processing
//! with full AudioNum support for both f32 and f64 sample types.
//!
//! ## Buffer Types
//!
//! - `PipeBuffer`: Single-producer, single-consumer (point-to-point)
//! - `FanOutBuffer`: One producer, multiple consumers (broadcast)
//! - `FanInBuffer`: Multiple producers, one consumer (mixing)
//! - `DelayLine`: Circular buffer for delay effects
//! - `RingBuffer`: Multi-producer, multi-consumer ring buffer

use core::sync::atomic::{AtomicUsize, AtomicU64, Ordering};
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use crate::math::AudioNum;

pub mod pipe;
pub mod fan;
pub mod delay;
pub mod ring;

pub use pipe::PipeBuffer;
pub use fan::{FanOutBuffer, FanInBuffer};
pub use delay::DelayLine;
pub use ring::RingBuffer;

/// Cache line size for alignment (64 bytes on x86_64)
pub const CACHE_LINE_SIZE: usize = 64;

/// Atomic statistics for safe concurrent access
///
/// This structure provides lock-free atomic counters for buffer statistics.
/// It can be safely shared between threads without mutexes.
#[repr(align(64))]
pub struct AtomicStats {
    writes: AtomicU64,
    reads: AtomicU64,
    underflows: AtomicU64,
    overflows: AtomicU64,
    peak_fill: AtomicUsize, // Store as fixed-point (*1000 for 0.1% precision)
}

impl AtomicStats {
    /// Create new atomic statistics with all counters set to zero
    pub const fn new() -> Self {
        Self {
            writes: AtomicU64::new(0),
            reads: AtomicU64::new(0),
            underflows: AtomicU64::new(0),
            overflows: AtomicU64::new(0),
            peak_fill: AtomicUsize::new(0),
        }
    }
    
    /// Record a successful write operation
    #[inline(always)]
    pub fn record_write(&self) {
        self.writes.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Record a successful read operation
    #[inline(always)]
    pub fn record_read(&self) {
        self.reads.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Record an underflow event (read when empty)
    #[inline(always)]
    pub fn record_underflow(&self) {
        self.underflows.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Record an overflow event (write when full)
    #[inline(always)]
    pub fn record_overflow(&self) {
        self.overflows.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Update peak fill level (0-1000 representing 0.0-1.0)
    #[inline(always)]
    pub fn update_peak(&self, current_fill: usize) {
        let mut peak = self.peak_fill.load(Ordering::Relaxed);
        while current_fill > peak {
            match self.peak_fill.compare_exchange_weak(
                peak, 
                current_fill, 
                Ordering::Relaxed, 
                Ordering::Relaxed
            ) {
                Ok(_) => break,
                Err(new_peak) => peak = new_peak,
            }
        }
    }
    
    /// Get a consistent snapshot of current statistics
    pub fn snapshot(&self) -> BufferStats {
        BufferStats {
            writes: self.writes.load(Ordering::Relaxed),
            reads: self.reads.load(Ordering::Relaxed),
            underflows: self.underflows.load(Ordering::Relaxed),
            overflows: self.overflows.load(Ordering::Relaxed),
            fill_level: 0.0, // To be filled by caller with current fill level
            peak_fill: self.peak_fill.load(Ordering::Relaxed) as f32 / 1000.0,
        }
    }
    
    /// Reset all statistics to zero
    pub fn reset(&self) {
        self.writes.store(0, Ordering::Relaxed);
        self.reads.store(0, Ordering::Relaxed);
        self.underflows.store(0, Ordering::Relaxed);
        self.overflows.store(0, Ordering::Relaxed);
        self.peak_fill.store(0, Ordering::Relaxed);
    }
}

/// Buffer statistics snapshot for monitoring and debugging
#[derive(Debug, Default, Clone, Copy)]
pub struct BufferStats {
    /// Total number of successful write operations
    pub writes: u64,
    
    /// Total number of successful read operations
    pub reads: u64,
    
    /// Number of underflow events (read when empty)
    pub underflows: u64,
    
    /// Number of overflow events (write when full)
    pub overflows: u64,
    
    /// Current fill level (0.0 to 1.0)
    pub fill_level: f32,
    
    /// Peak fill level since last reset (0.0 to 1.0)
    pub peak_fill: f32,
}

/// Common trait for all audio buffers
pub trait AudioBuffer<T: AudioNum> {
    /// Get the total capacity of the buffer in samples
    fn capacity(&self) -> usize;
    
    /// Get the current number of items in the buffer
    fn len(&self) -> usize;
    
    /// Check if the buffer is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    
    /// Check if the buffer is full
    fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }
    
    /// Clear all items from the buffer
    fn clear(&mut self);
    
    /// Get a snapshot of current buffer statistics
    fn stats(&self) -> BufferStats;
    
    /// Reset all statistics to zero
    fn reset_stats(&mut self);
}

/// Cache-line aligned storage for lock-free buffers
///
/// This type provides aligned storage that can be safely shared between threads.
/// It is not `Copy` or `Clone` by design - use references or pointers.
#[repr(align(64))]
pub struct AlignedStorage<T: AudioNum, const N: usize> {
    data: [UnsafeCell<MaybeUninit<T>>; N],
}

impl<T: AudioNum, const N: usize> AlignedStorage<T, N> {
    /// Create new aligned storage with uninitialized data
    pub fn new() -> Self {
        let data = array_from_fn(|_| UnsafeCell::new(MaybeUninit::uninit()));
        Self { data }
    }
    
    /// Write a value at the specified index
    ///
    /// # Safety
    /// The caller must ensure that the index is within bounds and that
    /// there is no concurrent write to the same index.
    #[inline(always)]
    pub fn write(&self, index: usize, value: T) {
        debug_assert!(index < N, "Index {} out of bounds for size {}", index, N);
        unsafe {
            *(self.data[index].get()) = MaybeUninit::new(value);
        }
    }
    
    /// Read a value from the specified index
    ///
    /// # Safety
    /// The caller must ensure that the index is within bounds and that
    /// the cell has been initialized with a valid value.
    #[inline(always)]
    pub unsafe fn read(&self, index: usize) -> T {
        debug_assert!(index < N, "Index {} out of bounds for size {}", index, N);
        (*(self.data[index].get())).assume_init_read()
    }
    
    /// Get a raw pointer to the element at index
    #[inline(always)]
    pub fn get_ptr(&self, index: usize) -> *mut T {
        debug_assert!(index < N);
        self.data[index].get() as *mut T
    }
}

impl<T: AudioNum, const N: usize> Default for AlignedStorage<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to create arrays without requiring `Copy`
///
/// This is needed because `UnsafeCell` and `MaybeUninit` are not `Copy`,
/// but we need to create arrays of them.
fn array_from_fn<T, const N: usize>(mut f: impl FnMut(usize) -> T) -> [T; N] {
    use core::mem::MaybeUninit;
    
    // Create an uninitialized array
    let mut array: [MaybeUninit<T>; N] = unsafe { MaybeUninit::uninit().assume_init() };
    
    // Initialize each element
    for (i, item) in array.iter_mut().enumerate() {
        *item = MaybeUninit::new(f(i));
    }
    
    // Transmute to initialized array (safe because we initialized all elements)
    unsafe { core::mem::transmute_copy(&array) }
}

/// Atomic sequence number for lock-free operations
///
/// Provides atomic operations on a sequence counter with cache-line alignment
/// to prevent false sharing.
#[repr(align(64))]
pub struct Sequence(AtomicUsize);

impl Sequence {
    /// Create a new sequence starting at 0
    pub const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }
    
    /// Load the current value
    #[inline(always)]
    pub fn load(&self, order: Ordering) -> usize {
        self.0.load(order)
    }
    
    /// Store a new value
    #[inline(always)]
    pub fn store(&self, val: usize, order: Ordering) {
        self.0.store(val, order)
    }
    
    /// Atomic fetch-and-add
    #[inline(always)]
    pub fn fetch_add(&self, val: usize, order: Ordering) -> usize {
        self.0.fetch_add(val, order)
    }
    
    /// Atomic compare-and-exchange
    #[inline(always)]
    pub fn compare_exchange(
        &self,
        current: usize,
        new: usize,
        success: Ordering,
        failure: Ordering,
    ) -> Result<usize, usize> {
        self.0.compare_exchange(current, new, success, failure)
    }
}

/// Buffer error types
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BufferError {
    /// Buffer is empty (tried to read when no data available)
    #[error("Buffer is empty")]
    Empty,
    
    /// Buffer is full (tried to write when no space available)
    #[error("Buffer is full")]
    Full,
    
    /// Invalid index access
    #[error("Invalid index: {0}")]
    InvalidIndex(usize),
    
    /// Buffer is disconnected (other end is gone)
    #[error("Buffer is disconnected")]
    Disconnected,
}

/// Result type for buffer operations
pub type BufferResult<T> = Result<T, BufferError>;

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_atomic_stats() {
        let stats = AtomicStats::new();
        
        stats.record_write();
        stats.record_read();
        stats.record_underflow();
        stats.record_overflow();
        stats.update_peak(500);
        
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.writes, 1);
        assert_eq!(snapshot.reads, 1);
        assert_eq!(snapshot.underflows, 1);
        assert_eq!(snapshot.overflows, 1);
        assert!((snapshot.peak_fill - 0.5).abs() < 0.001);
    }
    
    #[test]
    fn test_aligned_storage() {
        let storage = AlignedStorage::<f32, 64>::new();
        
        // Check alignment
        let ptr = &storage as *const _ as usize;
        assert_eq!(ptr % CACHE_LINE_SIZE, 0);
        
        // Write and read
        storage.write(0, 42.0);
        unsafe {
            assert_eq!(storage.read(0), 42.0);
        }
    }
    
    #[test]
    fn test_sequence() {
        let seq = Sequence::new();
        assert_eq!(seq.load(Ordering::SeqCst), 0);
        
        seq.store(42, Ordering::SeqCst);
        assert_eq!(seq.load(Ordering::SeqCst), 42);
        
        seq.fetch_add(10, Ordering::SeqCst);
        assert_eq!(seq.load(Ordering::SeqCst), 52);
    }
}