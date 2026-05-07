//! # Signal Buffers for single-threaded signal processing
//!
//! This module provides real-time safe buffers used by graph nodes inside
//! the signal thread. All buffers are **single-threaded** — they contain no
//! atomics or locks. Cross-thread communication goes through
//! [`rill_core::queues`](crate::queues).
//!
//! ## Buffer Types
//!
//! | Buffer | Description | Use Case |
//! |--------|-------------|----------|
//! | [`PipeBuffer`] | Single-producer, single-consumer | Point-to-point node connections |
//! | [`FanOutBuffer`] | One producer, multiple consumers | Broadcast signals to multiple nodes |
//! | [`FanInBuffer`] | Multiple producers, one consumer | Mix multiple signals |
//! | [`DelayLine`] | Circular buffer with delay | Effects like echo, reverb |
//! | [`RingBuffer`] | Multi-producer, multi-consumer | Generic queue for any scenario |
//! | [`TapeLoop`](crate::buffer::TapeLoop) | Heap-allocated circular buffer | Tape delay with large capacity |
//!
//! ## Features
//!
//! - **Real-time safe** - No allocations, no blocking, no system calls
//! - **Single-threaded** - No atomics, no locks, minimal overhead
//! - **Cache-line aligned** - Prevents false sharing
//! - **Statistically monitored** - Track performance metrics
//! - **Type-safe** - Generic over [`Scalar`](crate::math::Scalar) (f32, f64, integers)

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::fmt;

use crate::math::Transcendental;

// ============================================================================
// Submodules
// ============================================================================

mod buffer_trait;
mod delay;
mod fan;
mod pipe;
mod registry;
mod ring;
mod storage;
mod tape;

// ============================================================================
// Re-exports
// ============================================================================

pub use buffer_trait::{Buffer, FixedBuffer, HeapBuffer};
pub use delay::DelayLine;
pub use fan::{FanInBuffer, FanOutBuffer};
pub use pipe::PipeBuffer;
pub use registry::BufferRegistry;
pub use ring::RingBuffer;
pub use storage::{AtomicCell, AtomicCellError};
pub use tape::TapeLoop;

// ============================================================================
// Constants
// ============================================================================

/// Cache line size for alignment (64 bytes on x86_64)
///
/// This is the typical size of a CPU cache line. Aligning buffers to this
/// boundary prevents false sharing between threads running on different cores.
pub const CACHE_LINE_SIZE: usize = 64;

/// Default buffer size for most use cases
pub const DEFAULT_BUFFER_SIZE: usize = 1024;

/// Maximum buffer size (2^16 = 65536 samples)
pub const MAX_BUFFER_SIZE: usize = 65536;

/// Minimum buffer size (must be at least 16 for most algorithms)
pub const MIN_BUFFER_SIZE: usize = 16;

// ============================================================================
// Atomic Statistics
// ============================================================================

/// Atomic statistics for safe concurrent access
///
/// This structure provides lock-free atomic counters for buffer statistics.
/// It can be safely shared between threads without mutexes.
///
/// # Memory Layout
/// The structure is cache-line aligned to prevent false sharing.
///
/// # Thread Safety
/// All operations are atomic and use relaxed ordering where appropriate.
#[repr(align(64))]
pub struct AtomicStats {
    /// Total number of successful writes
    writes: AtomicU64,

    /// Total number of successful reads
    reads: AtomicU64,

    /// Number of underflow events (read when empty)
    underflows: AtomicU64,

    /// Number of overflow events (write when full)
    overflows: AtomicU64,

    /// Peak fill level (0-1000 representing 0.0-1.0)
    /// Stored as fixed-point for atomic operations
    peak_fill: AtomicUsize,
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
    ///
    /// # Arguments
    /// * `current_fill` - Current fill level (0-1000)
    ///
    /// This uses a compare-exchange loop to atomically update the peak.
    #[inline(always)]
    pub fn update_peak(&self, current_fill: usize) {
        let mut peak = self.peak_fill.load(Ordering::Relaxed);
        while current_fill > peak {
            match self.peak_fill.compare_exchange_weak(
                peak,
                current_fill,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(new_peak) => peak = new_peak,
            }
        }
    }

    /// Get a consistent snapshot of current statistics
    ///
    /// # Returns
    /// A `BufferStats` struct with a snapshot of all counters.
    /// Note that the snapshot may not be perfectly consistent due to
    /// concurrent updates, but it's good enough for monitoring.
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

impl Default for AtomicStats {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for AtomicStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AtomicStats")
            .field("writes", &self.writes.load(Ordering::Relaxed))
            .field("reads", &self.reads.load(Ordering::Relaxed))
            .field("underflows", &self.underflows.load(Ordering::Relaxed))
            .field("overflows", &self.overflows.load(Ordering::Relaxed))
            .field(
                "peak_fill",
                &(self.peak_fill.load(Ordering::Relaxed) as f32 / 1000.0),
            )
            .finish()
    }
}

// ============================================================================
// Buffer Statistics
// ============================================================================

/// Buffer statistics snapshot for monitoring and debugging
///
/// This struct provides a read-only snapshot of buffer performance metrics.
/// It's typically obtained via `Buffer::stats()`.
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

impl BufferStats {
    /// Create a new zeroed statistics snapshot
    pub fn new() -> Self {
        Self::default()
    }

    /// Calculate the success rate (reads / writes)
    ///
    /// Returns 1.0 if no writes, otherwise reads/writes.
    pub fn success_rate(&self) -> f32 {
        if self.writes == 0 {
            1.0
        } else {
            self.reads as f32 / self.writes as f32
        }
    }

    /// Calculate the error rate (underflows + overflows) / operations
    pub fn error_rate(&self) -> f32 {
        let total = self.writes + self.reads;
        if total == 0 {
            0.0
        } else {
            (self.underflows + self.overflows) as f32 / total as f32
        }
    }
}

// The unified Buffer trait (defined in buffer_trait) replaces both the
// original Buffer and the former SignalBuffer trait.

// ============================================================================
// Aligned Storage
// ============================================================================

// Cache-line aligned storage for lock-free buffers
//
// This type provides aligned storage that can be safely shared between threads.
// It is not `Copy` or `Clone` by design - use references or pointers.
//
// # Type Parameters
// - `T`: The sample type (must implement `Transcendental`)
// - `N`: The number of elements
// # Safety
// This type uses `UnsafeCell` for interior mutability and `MaybeUninit`
// for uninitialized data. Users must ensure proper initialization before reading.

// ============================================================================
// Utility Functions
// ============================================================================

/// Utility functions for common buffer operations
pub mod utils {
    use super::*;

    /// Copy data from one slice to another with bounds checking
    ///
    /// # Arguments
    /// * `src` - Source slice
    /// * `dst` - Destination slice
    ///
    /// # Returns
    /// The number of elements copied
    #[inline(always)]
    pub fn copy_safe<T: Copy>(src: &[T], dst: &mut [T]) -> usize {
        let len = src.len().min(dst.len());
        dst[..len].copy_from_slice(&src[..len]);
        len
    }

    /// Fill slice with zeroes
    ///
    /// # Arguments
    /// * `slice` - The slice to fill
    #[inline(always)]
    pub fn zero_fill<T: Default + Copy>(slice: &mut [T]) {
        for item in slice.iter_mut() {
            *item = T::default();
        }
    }

    /// Mix two slices with gain
    ///
    /// # Arguments
    /// * `src` - Source slice to mix in
    /// * `dst` - Destination slice (will be modified)
    /// * `gain` - Gain to apply to source
    #[inline(always)]
    pub fn mix_with_gain<T>(src: &[T], dst: &mut [T], gain: T)
    where
        T: Transcendental + core::ops::Mul<Output = T> + core::ops::Add<Output = T>,
    {
        let len = src.len().min(dst.len());
        for i in 0..len {
            dst[i] += src[i] * gain;
        }
    }

    /// Apply gain to slice
    ///
    /// # Arguments
    /// * `slice` - The slice to modify
    /// * `gain` - Gain to apply
    #[inline(always)]
    pub fn apply_gain<T>(slice: &mut [T], gain: T)
    where
        T: Transcendental + core::ops::Mul<Output = T>,
    {
        for item in slice.iter_mut() {
            *item *= gain;
        }
    }

    /// Calculate RMS of slice
    ///
    /// # Arguments
    /// * `slice` - The slice to analyze
    ///
    /// # Returns
    /// The RMS value
    #[inline(always)]
    pub fn calculate_rms<T>(slice: &[T]) -> f64
    where
        T: Transcendental + core::ops::Mul<Output = T> + core::iter::Sum,
    {
        let sum_squares: T = slice.iter().map(|&x| x * x).sum();
        let sum_f64: f64 = sum_squares.to_f64();
        (sum_f64 / slice.len() as f64).sqrt()
    }

    /// Calculate peak of slice
    ///
    /// # Arguments
    /// * `slice` - The slice to analyze
    ///
    /// # Returns
    /// The peak absolute value
    #[inline(always)]
    pub fn calculate_peak<T>(slice: &[T]) -> f64
    where
        T: Transcendental + PartialOrd,
    {
        slice.iter().map(|&x| x.to_f64().abs()).fold(0.0, f64::max)
    }
}

// ============================================================================
// Prelude
// ============================================================================

/// Prelude for convenient imports
///
/// Import this module to get all the common buffer types and traits:
/// ```
/// use rill_core::buffer::prelude::*;
/// ```
pub mod prelude {
    pub use super::{
        // Utility functions
        utils,

        // AtomicCell
        AtomicCell,
        AtomicCellError,

        // Core trait (unified Buffer replaces SignalBuffer + Buffer)
        Buffer,

        // Error types
        BufferError,
        BufferResult,

        // Statistics
        BufferStats,

        DelayLine,
        FanInBuffer,
        FanOutBuffer,
        // Buffer types
        PipeBuffer,
        RingBuffer,

        // Constants
        CACHE_LINE_SIZE,
        DEFAULT_BUFFER_SIZE,
        MAX_BUFFER_SIZE,
        MIN_BUFFER_SIZE,
    };
}

// ============================================================================
// Buffer Error Types
// ============================================================================

/// Buffer error types
///
/// These errors can occur during buffer operations. They are designed to be
/// `Copy` and `Eq` for efficient handling in real-time contexts.
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

    /// Operation would block (for non-blocking operations)
    #[error("Operation would block")]
    WouldBlock,

    /// Buffer overflow (data was lost)
    #[error("Buffer overflow")]
    Overflow,

    /// Buffer underflow (no data available)
    #[error("Buffer underflow")]
    Underflow,

    /// Invalid buffer size
    #[error("Invalid buffer size: {0}")]
    InvalidSize(usize),
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Helper function to create arrays without requiring `Copy`
#[allow(unsafe_code)]
pub fn array_from_fn<T, const N: usize>(mut f: impl FnMut(usize) -> T) -> [T; N] {
    use core::mem::MaybeUninit;

    let mut array: [MaybeUninit<T>; N] = unsafe { MaybeUninit::uninit().assume_init() };

    for (i, item) in array.iter_mut().enumerate() {
        *item = MaybeUninit::new(f(i));
    }

    unsafe { core::mem::transmute_copy(&array) }
}

/// Result type for buffer operations
pub type BufferResult<T> = Result<T, BufferError>;

// ============================================================================
// Tests
// ============================================================================

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
    fn test_buffer_stats() {
        let stats = BufferStats {
            writes: 100,
            reads: 95,
            underflows: 3,
            overflows: 2,
            fill_level: 0.5,
            peak_fill: 0.8,
        };

        // success_rate = reads/writes = 95/100 = 0.95
        assert!((stats.success_rate() - 0.95).abs() < 0.001);

        // error_rate = (underflows + overflows) / (writes + reads) = 5/195 ≈ 0.02564
        assert!((stats.error_rate() - 0.02564).abs() < 0.001);
    }

    #[test]
    fn test_utils() {
        let mut dst = [0.0; 4];
        let src = [1.0, 2.0, 3.0];

        let copied = utils::copy_safe(&src, &mut dst);
        assert_eq!(copied, 3);
        assert_eq!(dst[0], 1.0);
        assert_eq!(dst[1], 2.0);
        assert_eq!(dst[2], 3.0);

        utils::zero_fill(&mut dst[..3]);
        assert_eq!(dst[0], 0.0);
        assert_eq!(dst[1], 0.0);
        assert_eq!(dst[2], 0.0);

        let mut mix_dst = [1.0, 1.0, 1.0];
        utils::mix_with_gain(&[2.0, 2.0, 2.0], &mut mix_dst, 0.5);
        assert_eq!(mix_dst[0], 2.0);

        let rms = utils::calculate_rms(&[1.0, -1.0, 1.0, -1.0]);
        assert!((rms - 1.0).abs() < 1e-6);

        let peak = utils::calculate_peak(&[0.5, -0.8, 0.3, -0.9]);
        assert!((peak - 0.9).abs() < 1e-6);
    }

    #[test]
    fn test_constants() {
        assert_eq!(CACHE_LINE_SIZE, 64);
        assert!(MAX_BUFFER_SIZE > MIN_BUFFER_SIZE);
        assert!(DEFAULT_BUFFER_SIZE >= MIN_BUFFER_SIZE);
        assert!(DEFAULT_BUFFER_SIZE <= MAX_BUFFER_SIZE);
    }

    #[test]
    fn test_buffer_error_display() {
        assert_eq!(format!("{}", BufferError::Empty), "Buffer is empty");
        assert_eq!(format!("{}", BufferError::Full), "Buffer is full");
        assert_eq!(
            format!("{}", BufferError::InvalidIndex(5)),
            "Invalid index: 5"
        );
    }

    #[test]
    fn test_atomic_cell_basic() {
        let cell = AtomicCell::new(42);
        assert_eq!(cell.load(), 42);

        cell.store(100);
        assert_eq!(cell.load(), 100);
    }

    #[test]
    fn test_atomic_cell_try_new() {
        let cell = AtomicCell::try_new(42).unwrap();
        assert_eq!(cell.load(), 42);
    }

    #[test]
    fn test_atomic_cell_default() {
        let cell = AtomicCell::<i32>::default();
        assert_eq!(cell.load(), 0);
    }
}
