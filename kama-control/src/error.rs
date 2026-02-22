//! # Типы ошибок для kama-control
//! 
//! Специализированные ошибки, возникающие при работе с контроллерами:
//! - ошибки бэкендов (MIDI, HID, OSC)
//! - ошибки маппинга
//! - проблемы с устройствами

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ControlError {
    /// Ошибка бэкенда (MIDI, HID, OSC).
    #[error("Backend error: {0}")]
    /// Ошибка бэкенда (MIDI, HID, OSC).
    Backend(String),
    
    #[error("Device not found: {0}")]
    /// Устройство не найдено.
    DeviceNotFound(String),
    
    #[error("MIDI error: {0}")]
    /// Ошибка MIDI подсистемы.
    Midi(String),
    
    #[error("HID error: {0}")]
    /// Ошибка HID подсистемы.
    Hid(String),
    
    #[error("OSC error: {0}")]
    /// Ошибка OSC подсистемы.
    Osc(String),
    
    /// Ошибка маппинга событий.
    #[error("Mapping error: {0}")]
    /// Ошибка маппинга событий.
    Mapping(String),
    
    #[error("IO error: {0}")]
    /// Ошибка ввода-вывода.
    Io(#[from] std::io::Error),
    
    /// Ошибка канала связи (отправка/получение).
    #[error("Channel error")]
    /// Ошибка канала связи (отправка/получение).
    Channel,
}

pub type ControlResult<T> = Result<T, ControlError>;