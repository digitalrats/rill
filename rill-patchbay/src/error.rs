//! Типы ошибок для крейта rill-automation

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AutomationError {
    #[error("Automaton error: {0}")]
    Automaton(String),
    
    #[error("Parameter error: {0}")]
    Parameter(String),
    
    #[error("Servo error: {0}")]
    Servo(String),
    
    #[error("Clock error: {0}")]
    Clock(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type AutomationResult<T> = Result<T, AutomationError>;