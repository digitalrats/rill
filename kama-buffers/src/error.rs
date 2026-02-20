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
    
    #[error("Pool is empty")]
    PoolEmpty,
    
    #[error("Size mismatch: expected {expected}, got {got}")]
    SizeMismatch { expected: usize, got: usize },
    
    #[error("Buffer not found: {0}")]
    BufferNotFound(String),
}

pub type BufferResult<T> = Result<T, BufferError>;