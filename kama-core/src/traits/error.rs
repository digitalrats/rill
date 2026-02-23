use thiserror::Error;

/// Основная ошибка аудиосистемы
#[derive(Debug, Error)]
pub enum AudioError {
    #[error("Audio processing error: {0}")]
    Processing(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parameter error: {0}")]
    Parameter(String),

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("MIDI error: {0}")]
    Midi(String),

    #[error("Signal error: {0}")]
    Signal(String),

    #[error("Other error: {0}")]
    Other(String),
}

/// Результат операций с аудио
pub type AudioResult<T> = Result<T, AudioError>;
