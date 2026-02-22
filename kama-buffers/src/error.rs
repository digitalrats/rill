//! # Типы ошибок для работы с буферами
//! 
//! Специализированные ошибки, возникающие при работе с буферами:

//! - неверные идентификаторы головок
//! - переполнение/опустошение буферов
//! - проблемы с пулом буферов
//! - несоответствие размеров

use thiserror::Error;

#[derive(Error, Debug)]
pub enum BufferError {
    #[error("Invalid head ID: {0}")]
    /// Неверный идентификатор головки воспроизведения.
    InvalidHeadId(usize),
    
    #[error("Buffer full")]
    /// Буфер переполнен.
    BufferFull,
    
    #[error("Buffer empty")]
    /// Буфер пуст.
    BufferEmpty,
    
    #[error("Invalid parameter: {0}")]
    /// Неверный параметр (например, слишком большая скорость).
    InvalidParameter(String),
    
    #[error("Pool is empty")]
    /// Пул буферов пуст.
    PoolEmpty,
    
    #[error("Size mismatch: expected {expected}, got {got}")]
    /// Несоответствие размера буфера ожидаемому.
    SizeMismatch { expected: usize, got: usize },
    
    #[error("Buffer not found: {0}")]
    /// Буфер с указанным именем не найден в реестре.
    BufferNotFound(String),
}

    /// Результат операций с буферами.
pub type BufferResult<T> = Result<T, BufferError>;