//! Типы ошибок для экосистемы Kama Audio

use thiserror::Error;

/// Основная ошибка аудиосистемы
#[derive(Debug, Error)]
pub enum AudioError {
    /// Ошибка обработки аудио
    #[error("Audio processing error: {0}")]
    Processing(String),
    
    /// Ошибка ввода-вывода
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    /// Ошибка параметра
    #[error("Parameter error: {0}")]
    Parameter(String),
    
    /// Ошибка графа обработки
    #[error("Graph error: {0}")]
    Graph(String),
    
    /// Ошибка MIDI
    #[error("MIDI error: {0}")]
    Midi(String),
    
    /// Ошибка сигнальной системы
    #[error("Signal error: {0}")]
    Signal(String),
    
    /// Прочие ошибки
    #[error("Other error: {0}")]
    Other(String),
}

/// Результат операций с аудио
pub type AudioResult<T> = Result<T, AudioError>;