//! Error types for rill-io

use thiserror::Error;

/// I/O errors
#[derive(Error, Debug)]
pub enum IoError {
    /// Backend-specific error
    #[error("Backend error: {0}")]
    Backend(String),

    /// Device not found
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Stream error
    #[error("Stream error: {0}")]
    Stream(String),

    /// Initialization error
    #[error("Initialization error: {0}")]
    Init(String),

    /// Unsupported feature
    #[error("Unsupported feature: {0}")]
    Unsupported(String),

    /// Operation timed out
    #[error("Timeout")]
    Timeout,

    /// Wrapped `std::io::Error`
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Channel communication error
    #[error("Channel error")]
    Channel,
}

/// Result of I/O operations
pub type IoResult<T> = Result<T, IoError>;
