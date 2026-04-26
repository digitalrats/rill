//! Ошибки, связанные с очередями

use thiserror::Error;

/// Результат операций с очередями
pub type QueueResult<T> = Result<T, QueueError>;

/// Ошибки очередей
#[derive(Error, Debug, PartialEq)]
pub enum QueueError {
    /// Очередь переполнена (при попытке отправить в ограниченную очередь)
    #[error("Queue is full")]
    QueueFull,
    
    /// Очередь пуста (при попытке получить из пустой очереди)
    #[error("Queue is empty")]
    QueueEmpty,
    
    /// Канал отключен (все отправители/получатели уничтожены)
    #[error("Channel disconnected")]
    ChannelDisconnected,
    
    /// Таймаут при ожидании
    #[error("Operation timed out")]
    Timeout,
    
    /// Неподдерживаемая операция для данного типа очереди
    #[error("Operation not supported: {0}")]
    Unsupported(String),
    
    /// Ошибка отправки с потерей данных
    #[error("Send failed: {0}")]
    SendFailed(String),
    
    /// Ошибка получения
    #[error("Receive failed: {0}")]
    ReceiveFailed(String),
}

impl<T> From<crossbeam_channel::TrySendError<T>> for QueueError {
    fn from(err: crossbeam_channel::TrySendError<T>) -> Self {
        match err {
            crossbeam_channel::TrySendError::Full(_) => QueueError::QueueFull,
            crossbeam_channel::TrySendError::Disconnected(_) => QueueError::ChannelDisconnected,
        }
    }
}

impl From<crossbeam_channel::TryRecvError> for QueueError {
    fn from(err: crossbeam_channel::TryRecvError) -> Self {
        match err {
            crossbeam_channel::TryRecvError::Empty => QueueError::QueueEmpty,
            crossbeam_channel::TryRecvError::Disconnected => QueueError::ChannelDisconnected,
        }
    }
}

impl From<crossbeam_channel::RecvTimeoutError> for QueueError {
    fn from(err: crossbeam_channel::RecvTimeoutError) -> Self {
        match err {
            crossbeam_channel::RecvTimeoutError::Timeout => QueueError::Timeout,
            crossbeam_channel::RecvTimeoutError::Disconnected => QueueError::ChannelDisconnected,
        }
    }
}