//! Queue operation error types.

use thiserror::Error;

/// Result type alias for queue operations.
pub type QueueResult<T> = Result<T, QueueError>;

/// Errors that can occur during queue operations.
#[derive(Error, Debug, PartialEq)]
pub enum QueueError {
    /// The queue is full and cannot accept more elements.
    #[error("Queue is full")]
    QueueFull,
    /// The queue is empty and has no elements to pop.
    #[error("Queue is empty")]
    QueueEmpty,
    /// The channel has been disconnected (all senders/receivers dropped).
    #[error("Channel disconnected")]
    ChannelDisconnected,
    /// A queue operation timed out.
    #[error("Operation timed out")]
    Timeout,
    /// The requested operation is not supported by this queue type.
    #[error("Operation not supported: {0}")]
    Unsupported(String),
    /// Failed to send data through the queue.
    #[error("Send failed: {0}")]
    SendFailed(String),
    /// Failed to receive data from the queue.
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
