//! # Multiple-Producer Single-Consumer queue
//!
//! Allows multiple producers to send data to a single consumer.
//! Uses atomic operations for producer synchronization.
#![allow(unsafe_code)]

use super::{QueueError, QueueResult, QueueStats};
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Linked list node for MPSC queue
struct Node<T> {
    value: Option<T>,
    next: AtomicPtr<Node<T>>,
}

impl<T> Node<T> {
    fn new(value: T) -> *mut Node<T> {
        Box::into_raw(Box::new(Node {
            value: Some(value),
            next: AtomicPtr::new(ptr::null_mut()),
        }))
    }

    fn stub() -> *mut Node<T> {
        Box::into_raw(Box::new(Node {
            value: None,
            next: AtomicPtr::new(ptr::null_mut()),
        }))
    }
}

/// Multiple-Producer Single-Consumer queue
///
/// Implemented as a Michael-Scott lock-free queue.
/// Producers never block, the consumer can wait for data.
pub struct MpscQueue<T> {
    /// Queue head (first element to read)
    head: AtomicPtr<Node<T>>,
    /// Queue tail (last element to write)
    tail: AtomicPtr<Node<T>>,
    /// Counter for statistics
    stats: QueueStats,
    /// Maximum capacity (0 = unlimited)
    max_capacity: usize,
    /// Current size (approximate)
    size: AtomicUsize,
}

impl<T> Default for MpscQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> MpscQueue<T> {
    /// Create a new queue
    pub fn new() -> Self {
        let stub = Node::<T>::stub();
        Self {
            head: AtomicPtr::new(stub),
            tail: AtomicPtr::new(stub),
            stats: QueueStats::new(),
            max_capacity: 0,
            size: AtomicUsize::new(0),
        }
    }

    /// Create a queue with a limited capacity
    pub fn with_capacity(capacity: usize) -> Self {
        let mut queue = Self::new();
        queue.max_capacity = capacity;
        queue
    }

    /// Push an element (can be called from multiple threads)
    pub fn push(&self, value: T) -> QueueResult<()> {
        // Check for overflow
        if self.max_capacity > 0 {
            let size = self.size.load(Ordering::Relaxed);
            if size >= self.max_capacity {
                self.stats.record_overflow();
                return Err(QueueError::QueueFull);
            }
        }

        let node = Node::new(value);
        let mut tail = self.tail.load(Ordering::Acquire);

        loop {
            let next = unsafe { (*tail).next.load(Ordering::Acquire) };

            if next.is_null() {
                // Try to add a new node
                match unsafe {
                    (*tail).next.compare_exchange_weak(
                        ptr::null_mut(),
                        node,
                        Ordering::Release,
                        Ordering::Relaxed,
                    )
                } {
                    Ok(_) => {
                        // Update tail
                        let _ = self.tail.compare_exchange(
                            tail,
                            node,
                            Ordering::Release,
                            Ordering::Relaxed,
                        );
                        self.size.fetch_add(1, Ordering::Relaxed);
                        self.stats.record_push(self.size());
                        return Ok(());
                    }
                    Err(new_next) => {
                        // Another thread already added a node, update tail
                        let _ = self.tail.compare_exchange(
                            tail,
                            new_next,
                            Ordering::Release,
                            Ordering::Relaxed,
                        );
                        tail = new_next;
                    }
                }
            } else {
                // Advance tail
                let _ =
                    self.tail
                        .compare_exchange(tail, next, Ordering::Release, Ordering::Relaxed);
                tail = next;
            }
        }
    }

    /// Pop an element (consumer only)
    pub fn pop(&self) -> Option<T> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Acquire);
            let next = unsafe { (*head).next.load(Ordering::Acquire) };

            if head == tail {
                if next.is_null() {
                    return None;
                }
                let _ =
                    self.tail
                        .compare_exchange(tail, next, Ordering::Release, Ordering::Relaxed);
            } else {
                if next.is_null() {
                    continue;
                }

                if self
                    .head
                    .compare_exchange(head, next, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    let value = unsafe { (*next).value.take() };
                    unsafe {
                        drop(Box::from_raw(head));
                    }
                    self.size.fetch_sub(1, Ordering::Relaxed);
                    self.stats.record_pop();
                    return value;
                }
            }
        }
    }

    /// Current size (approximate)
    pub fn size(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }

    /// Capacity (0 = unlimited)
    pub fn capacity(&self) -> usize {
        self.max_capacity
    }

    /// Check if the queue is empty
    pub fn is_empty(&self) -> bool {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        let next = unsafe { (*head).next.load(Ordering::Acquire) };

        head == tail && next.is_null()
    }
}

impl<T> Drop for MpscQueue<T> {
    fn drop(&mut self) {
        while self.pop().is_some() {}

        let head = self.head.load(Ordering::Relaxed);
        if !head.is_null() {
            unsafe {
                drop(Box::from_raw(head));
            }
        }
    }
}

unsafe impl<T: Send> Send for MpscQueue<T> {}
unsafe impl<T: Send> Sync for MpscQueue<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_mpsc_basic() {
        let queue = MpscQueue::new();

        queue.push(1).unwrap();
        queue.push(2).unwrap();
        queue.push(3).unwrap();

        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn test_mpsc_multiple_producers() {
        let queue = std::sync::Arc::new(MpscQueue::new());
        let mut handles = vec![];

        for i in 0..4 {
            let queue = queue.clone();
            handles.push(thread::spawn(move || {
                for j in 0..250 {
                    queue.push(i * 1000 + j).unwrap();
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let mut count = 0;
        while queue.pop().is_some() {
            count += 1;
        }

        assert_eq!(count, 1000);
    }
}
