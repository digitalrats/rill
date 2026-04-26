//! # CommandQueue — неблокирующая очередь команд
//!
//! [`CommandQueue`] обеспечивает безопасную передачу команд
//! из потока управления (control thread) в аудиопоток (audio thread)
//! через bounded crossbeam channel.

use std::fmt;
use crossbeam_channel::{self, Sender, Receiver, TrySendError, TryRecvError};

/// Базовый трейт для всех команд
///
/// Любой тип, реализующий этот трейт, может передаваться через очередь.
pub trait Command: Send + 'static + fmt::Debug {}

/// Две половинки CommandQueue
pub struct CommandSender<T> {
    tx: Sender<T>,
}

impl<T> Clone for CommandSender<T> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone() }
    }
}

impl<T: Send + 'static> CommandSender<T> {
    pub fn send(&self, value: T) -> Result<(), super::QueueError> {
        self.tx.try_send(value).map_err(|e| match e {
            TrySendError::Full(_) => super::QueueError::QueueFull,
            TrySendError::Disconnected(_) => super::QueueError::ChannelDisconnected,
        })
    }
}

/// Потребитель команд (audio thread)
pub struct CommandReceiver<T> {
    rx: Receiver<T>,
}

impl<T: Send + 'static> CommandReceiver<T> {
    pub fn try_recv(&self) -> Result<T, super::QueueError> {
        self.rx.try_recv().map_err(|e| match e {
            TryRecvError::Empty => super::QueueError::QueueEmpty,
            TryRecvError::Disconnected => super::QueueError::ChannelDisconnected,
        })
    }
}

/// Неблокирующая bounded очередь команд
///
/// # Пример
/// ```
/// use rill_core::queues::CommandQueue;
///
/// let queue = CommandQueue::<i32>::new("test", 16);
/// queue.send(42).unwrap();
/// assert_eq!(queue.try_recv(), Ok(42));
/// ```
pub struct CommandQueue<T> {
    tx: Sender<T>,
    rx: Receiver<T>,
    name: String,
    capacity: usize,
}

impl<T: Send + 'static> CommandQueue<T> {
    /// Создать новую очередь с фиксированной ёмкостью
    pub fn new(name: &str, capacity: usize) -> Self {
        let (tx, rx) = crossbeam_channel::bounded(capacity);
        Self {
            tx,
            rx,
            name: name.to_string(),
            capacity,
        }
    }

    /// Отправить команду (из control thread)
    pub fn send(&self, value: T) -> Result<(), super::QueueError> {
        self.tx.try_send(value).map_err(|e| match e {
            TrySendError::Full(_) => super::QueueError::QueueFull,
            TrySendError::Disconnected(_) => super::QueueError::ChannelDisconnected,
        })
    }

    /// Попытаться получить команду (из audio thread)
    pub fn try_recv(&self) -> Result<T, super::QueueError> {
        self.rx.try_recv().map_err(|e| match e {
            TryRecvError::Empty => super::QueueError::QueueEmpty,
            TryRecvError::Disconnected => super::QueueError::ChannelDisconnected,
        })
    }

    /// Получить отправителя
    pub fn sender(&self) -> Sender<T> {
        self.tx.clone()
    }

    /// Получить получателя
    pub fn receiver(&self) -> Receiver<T> {
        self.rx.clone()
    }

    /// Имя очереди
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Ёмкость
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Текущий размер
    pub fn len(&self) -> usize {
        self.rx.len()
    }

    /// Пуста ли
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
