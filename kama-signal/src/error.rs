use thiserror::Error;

#[derive(Error, Debug)]
pub enum SignalError {
    #[error("Signal type mismatch")]
    TypeMismatch,
    #[error("Receiver not found")]
    ReceiverNotFound,
    #[error("Signal channel full")]
    ChannelFull,
    #[error("Signal bus disconnected")]
    Disconnected,
}

pub type SignalResult<T> = Result<T, SignalError>;