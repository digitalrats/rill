//! # Main RT-safe queue for dual-thread architecture
//!
//! [`RtQueue`] — the main queue for communication between
//! the control thread and the signal thread. Combines the functionality
//! of SPSC and MPSC queues with a convenient API.

use super::spsc::SpscQueue;
use super::{QueueResult, QueueStatsSnapshot};

/// Queue type
#[derive(Debug, Clone, Copy)]
pub enum QueueType {
    /// One producer, one consumer (maximum throughput)
    SingleProducer,
    /// Multiple producers, one consumer
    MultiProducer,
}

/// Main RT-safe queue
///
/// # Example
/// ```
/// use rill_core::queues::RtQueue;
///
/// // Create a queue for commands
/// let queue = RtQueue::<i32>::new(1024);
///
/// // Control thread (soft RT)
/// queue.push(42).unwrap();
///
/// // Signal thread (hard RT)
/// if let Some(cmd) = queue.pop() {
///     println!("Got command: {}", cmd);
/// }
/// ```
pub struct RtQueue<T: Copy> {
    /// Inner implementation
    inner: RtQueueInner<T>,
}

enum RtQueueInner<T: Copy> {
    Spsc(SpscQueue<T, 1024>),        // For single producer
    Mpsc(super::mpsc::MpscQueue<T>), // For multiple producers
}

impl<T: Copy + Default + Send + 'static> RtQueue<T> {
    /// Create a new queue with a fixed size
    pub fn new(capacity: usize) -> Self {
        // By default use SPSC for maximum performance
        if capacity <= 1024 {
            Self {
                inner: RtQueueInner::Spsc(SpscQueue::new()),
            }
        } else {
            Self {
                inner: RtQueueInner::Mpsc(super::mpsc::MpscQueue::with_capacity(capacity)),
            }
        }
    }

    /// Create a queue for a single producer
    pub fn new_spsc() -> Self {
        Self {
            inner: RtQueueInner::Spsc(SpscQueue::new()),
        }
    }

    /// Create a queue for multiple producers
    pub fn new_mpsc(capacity: usize) -> Self {
        Self {
            inner: RtQueueInner::Mpsc(super::mpsc::MpscQueue::with_capacity(capacity)),
        }
    }

    /// Push an element (from the control thread)
    pub fn push(&self, value: T) -> QueueResult<()> {
        match &self.inner {
            RtQueueInner::Spsc(q) => q.push(value),
            RtQueueInner::Mpsc(q) => q.push(value),
        }
    }

    /// Pop an element (from the signal thread)
    pub fn pop(&self) -> Option<T> {
        match &self.inner {
            RtQueueInner::Spsc(q) => q.pop(),
            RtQueueInner::Mpsc(q) => q.pop(),
        }
    }

    /// Current size
    pub fn len(&self) -> usize {
        match &self.inner {
            RtQueueInner::Spsc(q) => q.len(),
            RtQueueInner::Mpsc(q) => q.size(),
        }
    }

    /// Capacity
    pub fn capacity(&self) -> usize {
        match &self.inner {
            RtQueueInner::Spsc(q) => q.capacity(),
            RtQueueInner::Mpsc(q) => q.capacity(),
        }
    }

    /// Check if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get statistics
    pub fn stats(&self) -> QueueStatsSnapshot {
        match &self.inner {
            RtQueueInner::Spsc(q) => q.stats(),
            RtQueueInner::Mpsc(_q) => {
                // Stub for MPSC
                QueueStatsSnapshot {
                    pushes: 0,
                    pops: 0,
                    overflows: 0,
                    underflows: 0,
                    max_size: 0,
                }
            }
        }
    }
}

impl<T: Copy> Clone for RtQueue<T> {
    fn clone(&self) -> Self {
        // Only for MPSC queues, SPSC cannot be cloned
        match &self.inner {
            RtQueueInner::Spsc(_) => panic!("Cannot clone SPSC queue"),
            RtQueueInner::Mpsc(q) => Self {
                inner: RtQueueInner::Mpsc(super::mpsc::MpscQueue::with_capacity(q.capacity())),
            },
        }
    }
}

#[allow(unsafe_code)]
unsafe impl<T: Copy + Send> Send for RtQueue<T> {}
#[allow(unsafe_code)]
unsafe impl<T: Copy + Sync> Sync for RtQueue<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rt_queue_spsc() {
        let queue = RtQueue::<i32>::new_spsc();

        queue.push(42).unwrap();
        assert_eq!(queue.pop(), Some(42));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn test_rt_queue_mpsc() {
        let queue = RtQueue::<i32>::new_mpsc(16);

        queue.push(1).unwrap();
        queue.push(2).unwrap();
        queue.push(3).unwrap();

        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
    }
}
