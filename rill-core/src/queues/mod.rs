//! # Non-blocking queues for the dual-thread architecture
//!
//! This module provides queues for safe data exchange between the
//! control thread (soft RT) and the audio signal thread (hard RT).
//!
//! ## Components
//!
//! - [`SpscQueue`](crate::queues::SpscQueue) — Single-producer single-consumer queue (maximum throughput)
//! - [`RtQueueBase`](crate::queues::RtQueueBase) — Base trait for all queues
//! - [`QueueError`](crate::queues::QueueError) — Queue operation error type
//! - [`CommandQueue`](crate::queues::CommandQueue) — Commands from control thread to signal thread
//! - [`OverflowPolicy`](crate::queues::OverflowPolicy) — Overflow behaviour policies
//! - [`UnderflowPolicy`](crate::queues::UnderflowPolicy) — Underflow behaviour policies

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

// =============================================================================
// Подмодули
// =============================================================================

/// Bounded command queue using a crossbeam channel.
pub mod command;
/// Queue error types.
pub mod error;
/// Multi-producer single-consumer queue for automation.
pub mod mpsc;
/// Observer pattern helpers for queue monitoring.
pub mod observer;
/// Lock-free ring buffer for real-time use.
pub mod ring;
/// Base real-time queue implementation.
pub mod rt_queue;
/// Signal and command types for automation.
pub mod signal;
/// Lock-free single-producer single-consumer queue.
pub mod spsc;
/// Telemetry data types and senders.
pub mod telemetry;
/// Telemetry block batching utilities.
pub mod telemetry_block;

pub use command::CommandQueue;
pub use error::{QueueError, QueueResult};
pub use mpsc::MpscQueue;
pub use rt_queue::RtQueue;
pub use spsc::SpscQueue;
pub use telemetry_block::TelemetryBlock;

// Re-export key signal types
pub use signal::{
    AutomatonCommand, CalibrationKind, CommandEnum, MappingType, SensorCommand, ServoCommand,
    SetParameter, SignalOrigin,
};

// =============================================================================
// Политики поведения
// =============================================================================

/// Overflow behaviour policy for bounded queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowPolicy {
    /// Overwrite the oldest element (ring-buffer behaviour).
    OverwriteOldest,
    /// Discard the newest element (drop on full).
    DropNewest,
    /// Panic on overflow (debug only).
    Panic,
    /// Block the producer (not safe for RT threads).
    Block,
}

/// Underflow behaviour policy for bounded queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderflowPolicy {
    /// Return `None` on empty.
    ReturnNone,
    /// Panic on underflow (debug only).
    Panic,
}

// =============================================================================
// Статистика очереди
// =============================================================================

/// Live queue statistics collected inside the queue.
pub struct QueueStats {
    /// Total number of successful push operations.
    pushes: AtomicUsize,
    /// Total number of successful pop operations.
    pops: AtomicUsize,
    /// Total number of overflow events.
    overflows: AtomicUsize,
    /// Total number of underflow events.
    underflows: AtomicUsize,
    /// Maximum observed queue size.
    max_size: AtomicUsize,
}

impl QueueStats {
    /// Create a new empty statistics counter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a push operation and update the max size if needed.
    pub fn record_push(&self, current_size: usize) {
        self.pushes.fetch_add(1, Ordering::Relaxed);
        let prev = self.max_size.load(Ordering::Relaxed);
        if current_size > prev {
            let _ = self.max_size.compare_exchange(
                prev,
                current_size,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    /// Record a pop operation.
    pub fn record_pop(&self) {
        self.pops.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an overflow event.
    pub fn record_overflow(&self) {
        self.overflows.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an underflow event.
    pub fn record_underflow(&self) {
        self.underflows.fetch_add(1, Ordering::Relaxed);
    }

    /// Take an atomic snapshot of the current statistics.
    pub fn snapshot(&self) -> QueueStatsSnapshot {
        QueueStatsSnapshot {
            pushes: self.pushes.load(Ordering::Relaxed),
            pops: self.pops.load(Ordering::Relaxed),
            overflows: self.overflows.load(Ordering::Relaxed),
            underflows: self.underflows.load(Ordering::Relaxed),
            max_size: self.max_size.load(Ordering::Relaxed),
        }
    }
}

impl Default for QueueStats {
    fn default() -> Self {
        Self {
            pushes: AtomicUsize::new(0),
            pops: AtomicUsize::new(0),
            overflows: AtomicUsize::new(0),
            underflows: AtomicUsize::new(0),
            max_size: AtomicUsize::new(0),
        }
    }
}

/// Point-in-time snapshot of queue statistics.
#[derive(Debug, Clone, Copy, Default)]
pub struct QueueStatsSnapshot {
    /// Number of successful push operations.
    pub pushes: usize,
    /// Number of successful pop operations.
    pub pops: usize,
    /// Number of overflow events.
    pub overflows: usize,
    /// Number of underflow events.
    pub underflows: usize,
    /// Maximum observed queue size.
    pub max_size: usize,
}

impl QueueStatsSnapshot {
    /// Create a new empty snapshot.
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge two snapshots by summing counts and taking the max size.
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            pushes: self.pushes + other.pushes,
            pops: self.pops + other.pops,
            overflows: self.overflows + other.overflows,
            underflows: self.underflows + other.underflows,
            max_size: self.max_size.max(other.max_size),
        }
    }
}

impl fmt::Display for QueueStatsSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "pushes: {}, pops: {}, overflows: {}, underflows: {}, max_size: {}",
            self.pushes, self.pops, self.overflows, self.underflows, self.max_size
        )
    }
}

// =============================================================================
// Базовый трейт для всех очередей
// =============================================================================

/// Base trait for all real-time safe queues.
///
/// Implementations must be:
/// - Lock-free (no mutexes)
/// - RT-safe (no allocations, no blocking)
pub trait RtQueueBase<T>: Send + Sync {
    /// Push a value into the queue.
    ///
    /// # Errors
    /// Returns `QueueFull` if the queue is at capacity.
    fn push(&self, value: T) -> QueueResult<()>;

    /// Pop a value from the queue, or `None` if empty.
    fn pop(&self) -> Option<T>;

    /// Current number of elements in the queue.
    fn len(&self) -> usize;

    /// Maximum capacity of the queue.
    fn capacity(&self) -> usize;

    /// Return true if the queue is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return true if the queue is full.
    fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// Clear all elements from the queue.
    fn clear(&self);
}

// =============================================================================
// Вспомогательные функции
// =============================================================================

/// Return true if `n` is a power of two.
#[inline]
pub const fn is_power_of_two(n: usize) -> bool {
    n != 0 && (n & (n - 1)) == 0
}

/// Compute the next power of two greater than or equal to `n`.
///
/// # Panics
/// Panics when `n` is 0.
#[inline]
pub const fn next_power_of_two(n: usize) -> usize {
    let mut n = n - 1;
    n |= n >> 1;
    n |= n >> 2;
    n |= n >> 4;
    n |= n >> 8;
    n |= n >> 16;
    n += 1;
    n
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_snapshot() {
        let stats1 = QueueStatsSnapshot {
            pushes: 10,
            pops: 5,
            overflows: 1,
            underflows: 0,
            max_size: 8,
        };

        let stats2 = QueueStatsSnapshot {
            pushes: 20,
            pops: 15,
            overflows: 0,
            underflows: 2,
            max_size: 16,
        };

        let merged = stats1.merge(&stats2);
        assert_eq!(merged.pushes, 30);
        assert_eq!(merged.pops, 20);
        assert_eq!(merged.overflows, 1);
        assert_eq!(merged.underflows, 2);
        assert_eq!(merged.max_size, 16);
    }

    #[test]
    fn test_power_of_two() {
        assert!(is_power_of_two(1));
        assert!(is_power_of_two(2));
        assert!(is_power_of_two(4));
        assert!(is_power_of_two(8));
        assert!(is_power_of_two(16));
        assert!(!is_power_of_two(3));
        assert!(!is_power_of_two(5));
        assert!(!is_power_of_two(6));
        assert!(!is_power_of_two(7));
    }

    #[test]
    fn test_next_power_of_two() {
        assert_eq!(next_power_of_two(1), 1);
        assert_eq!(next_power_of_two(2), 2);
        assert_eq!(next_power_of_two(3), 4);
        assert_eq!(next_power_of_two(4), 4);
        assert_eq!(next_power_of_two(5), 8);
        assert_eq!(next_power_of_two(6), 8);
        assert_eq!(next_power_of_two(7), 8);
        assert_eq!(next_power_of_two(8), 8);
        assert_eq!(next_power_of_two(9), 16);
    }

    #[test]
    fn test_queue_stats_record() {
        let stats = QueueStats::new();
        stats.record_push(5);
        stats.record_push(8);
        stats.record_overflow();
        stats.record_pop();

        let snap = stats.snapshot();
        assert_eq!(snap.pushes, 2);
        assert_eq!(snap.pops, 1);
        assert_eq!(snap.overflows, 1);
        assert_eq!(snap.max_size, 8);
    }
}
