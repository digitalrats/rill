//! Типы ошибок для kama-control

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ControlError {
    #[error("Mapping error: {0}")]
    Mapping(String),
    
    #[error("Target error: {0}")]
    Target(String),
    
    #[error("Channel error")]
    Channel,
    
    #[error("Invalid value: {0}")]
    InvalidValue(String),
}

pub type ControlResult<T> = Result<T, ControlError>;