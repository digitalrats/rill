//! # Типы ошибок сигнальной системы

use thiserror::Error;

/// Ошибки сигнальной системы
#[derive(Error, Debug)]
pub enum SignalError {
    /// Несоответствие типа сигнала при обработке в диспетчере
    #[error("Signal type mismatch")]
    TypeMismatch,
    
    /// Нет зарегистрированных получателей для данного типа сигнала
    #[error("Receiver not found")]
    ReceiverNotFound,
    
    /// Канал переполнен (при политике DropNewest)
    #[error("Signal channel full")]
    ChannelFull,
    
    /// Шина сигналов отключена
    #[error("Signal bus disconnected")]
    Disconnected,
}

/// Результат операций с сигналами
pub type SignalResult<T> = Result<T, SignalError>;