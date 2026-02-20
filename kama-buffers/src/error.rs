use thiserror::Error;

#[derive(Error, Debug)]
pub enum BufferError {
    #[error("Invalid head ID: {0}")]
    InvalidHeadId(usize),
    #[error("Buffer full")]
    BufferFull,
    #[error("Buffer empty")]
    BufferEmpty,
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
}

pub type BufferResult<T> = Result<T, BufferError>;