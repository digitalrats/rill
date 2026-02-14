use thiserror::Error;

#[derive(Error, Debug)]
pub enum IoError {
    #[error("Backend error: {0}")]
    Backend(String),
    
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("Stream error: {0}")]
    Stream(String),
    
    #[error("Initialization error: {0}")]
    Init(String),
    
    #[error("Unsupported feature: {0}")]
    Unsupported(String),
    
    #[error("Timeout")]
    Timeout,
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type IoResult<T> = Result<T, IoError>;