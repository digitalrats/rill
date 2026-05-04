//! # CommandQueue — non-blocking command queue
//!
//! [`CommandQueue`] provides safe command transfer from the control thread
//! to the audio signal thread via a bounded crossbeam channel.

use crossbeam_channel::{self, Receiver, Sender, TryRecvError, TrySendError};
use std::fmt;

/// Base trait for all commands.
///
/// Any type implementing this trait can be sent through a command queue.
pub trait Command: Send + 'static + fmt::Debug {}

/// Sender half of a command queue.
///
/// Cloned to share among multiple producer threads. Each clone references
/// the same underlying crossbeam channel.
pub struct CommandSender<T> {
    /// The underlying crossbeam channel sender.
    tx: Sender<T>,
}

impl<T> Clone for CommandSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

impl<T: Send + 'static> CommandSender<T> {
    /// Try to send a value into the queue.
    ///
    /// # Errors
    /// Returns `QueueFull` if the queue is at capacity, or
    /// `ChannelDisconnected` if the receiver has been dropped.
    pub fn send(&self, value: T) -> Result<(), super::QueueError> {
        self.tx.try_send(value).map_err(|e| match e {
            TrySendError::Full(_) => super::QueueError::QueueFull,
            TrySendError::Disconnected(_) => super::QueueError::ChannelDisconnected,
        })
    }
}

/// Receiver half of a command queue (consumed by the signal thread).
pub struct CommandReceiver<T> {
    /// The underlying crossbeam channel receiver.
    rx: Receiver<T>,
}

impl<T: Send + 'static> CommandReceiver<T> {
    /// Try to receive a value from the queue without blocking.
    ///
    /// # Errors
    /// Returns `QueueEmpty` if no value is available, or
    /// `ChannelDisconnected` if the sender has been dropped.
    pub fn try_recv(&self) -> Result<T, super::QueueError> {
        self.rx.try_recv().map_err(|e| match e {
            TryRecvError::Empty => super::QueueError::QueueEmpty,
            TryRecvError::Disconnected => super::QueueError::ChannelDisconnected,
        })
    }
}

/// Non-blocking bounded command queue.
///
/// Provides safe, lock-free transfer of commands from the control thread
/// to the audio signal thread via a bounded crossbeam channel.
///
/// # Example
/// ```
/// use rill_core::queues::CommandQueue;
///
/// let queue = CommandQueue::<i32>::new("test", 16);
/// queue.send(42).unwrap();
/// assert_eq!(queue.try_recv(), Ok(42));
/// ```
pub struct CommandQueue<T> {
    /// Inner crossbeam sender.
    tx: Sender<T>,
    /// Inner crossbeam receiver.
    rx: Receiver<T>,
    /// Human-readable queue name for debugging.
    name: String,
    /// Fixed capacity of the bounded channel.
    capacity: usize,
}

impl<T: Send + 'static> CommandQueue<T> {
    /// Create a new bounded queue with the given capacity.
    pub fn new(name: &str, capacity: usize) -> Self {
        let (tx, rx) = crossbeam_channel::bounded(capacity);
        Self {
            tx,
            rx,
            name: name.to_string(),
            capacity,
        }
    }

    /// Try to send a value into the queue (from the control thread).
    ///
    /// # Errors
    /// Returns `QueueFull` if the queue is at capacity, or
    /// `ChannelDisconnected` if the receiver has been dropped.
    pub fn send(&self, value: T) -> Result<(), super::QueueError> {
        self.tx.try_send(value).map_err(|e| match e {
            TrySendError::Full(_) => super::QueueError::QueueFull,
            TrySendError::Disconnected(_) => super::QueueError::ChannelDisconnected,
        })
    }

    /// Try to receive a value from the queue (from the signal thread).
    ///
    /// # Errors
    /// Returns `QueueEmpty` if no value is available, or
    /// `ChannelDisconnected` if the sender has been dropped.
    pub fn try_recv(&self) -> Result<T, super::QueueError> {
        self.rx.try_recv().map_err(|e| match e {
            TryRecvError::Empty => super::QueueError::QueueEmpty,
            TryRecvError::Disconnected => super::QueueError::ChannelDisconnected,
        })
    }

    /// Get a clone of the inner crossbeam sender.
    pub fn sender(&self) -> Sender<T> {
        self.tx.clone()
    }

    /// Get a clone of the inner crossbeam receiver.
    pub fn receiver(&self) -> Receiver<T> {
        self.rx.clone()
    }

    /// Return the human-readable queue name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the fixed capacity of the queue.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Return the number of elements currently in the queue.
    pub fn len(&self) -> usize {
        self.rx.len()
    }

    /// Return true if the queue is currently empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T: fmt::Debug + Send + 'static> fmt::Debug for CommandQueue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandQueue")
            .field("name", &self.name)
            .field("capacity", &self.capacity)
            .field("len", &self.len())
            .finish()
    }
}

impl<T> Clone for CommandQueue<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            rx: self.rx.clone(),
            name: self.name.clone(),
            capacity: self.capacity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queues::QueueError;
    use std::thread;

    #[test]
    fn test_command_queue_basic() {
        let queue = CommandQueue::<i32>::new("test", 16);

        queue.send(42).unwrap();
        queue.send(43).unwrap();

        assert_eq!(queue.try_recv(), Ok(42));
        assert_eq!(queue.try_recv(), Ok(43));
        assert_eq!(queue.try_recv(), Err(QueueError::QueueEmpty));
    }

    #[test]
    fn test_command_queue_sender_receiver() {
        let queue = CommandQueue::<i32>::new("test", 16);
        let sender = queue.sender();
        let receiver = queue.receiver();

        sender.send(1).unwrap();
        sender.send(2).unwrap();
        drop(sender);

        assert_eq!(receiver.try_recv().unwrap(), 1);
        assert_eq!(receiver.try_recv().unwrap(), 2);
    }

    #[test]
    fn test_command_queue_full() {
        let queue = CommandQueue::<i32>::new("test", 2);

        assert!(queue.send(1).is_ok());
        assert!(queue.send(2).is_ok());
        // Третья отправка должна вернуть QueueFull
        match queue.send(3) {
            Err(QueueError::QueueFull) => {}
            _ => panic!("Expected QueueFull"),
        }
    }

    #[test]
    fn test_command_queue_threaded() {
        let queue = std::sync::Arc::new(CommandQueue::<i32>::new("test", 1024));
        let q2 = queue.clone();

        let producer = thread::spawn(move || {
            for i in 0..100 {
                q2.send(i).unwrap();
            }
        });

        let consumer = thread::spawn(move || {
            let mut received = 0;
            while received < 100 {
                if let Ok(val) = queue.try_recv() {
                    assert_eq!(val, received);
                    received += 1;
                }
            }
        });

        producer.join().unwrap();
        consumer.join().unwrap();
    }
}
