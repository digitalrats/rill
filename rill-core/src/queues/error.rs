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
