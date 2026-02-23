//! # Типы ошибок сигнальной системы
//!
//! Специализированные ошибки, возникающие при работе с сигнальной шиной
//! и диспетчером сигналов.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SignalError {
    #[error("Signal type mismatch")]
    /// Несоответствие типа сигнала при обработке в диспетчере.
    TypeMismatch,
    #[error("Receiver not found")]
    /// Нет зарегистрированных получателей для данного типа сигнала.
    ReceiverNotFound,
    #[error("Signal channel full")]
    /// Канал переполнен (при политике DropNewest).
    ChannelFull,
    #[error("Signal bus disconnected")]
    /// Шина сигналов отключена (все отправители/получатели уничтожены).
    Disconnected,
}

pub type SignalResult<T> = Result<T, SignalError>;
