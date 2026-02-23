//! # Типы сигналов

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::traits::{NodeId, ParameterId};  // обновляем импорт

/// Маркерный трейт для типов, которые могут использоваться как сигналы
pub trait Signal: Send + Sync + 'static {}

/// Источник сигнала изменения параметра
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SignalSource {
    /// Изменение через пользовательский интерфейс
    UserInterface,
    /// Автоматическое изменение (LFO, огибающая)
    Automation,
    /// MIDI-сообщение с указанием канала и контроллера
    Midi { 
        /// MIDI-канал (0-15)
        channel: u8, 
        /// Номер контроллера (0-127)
        controller: u8 
    },
    /// OSC-сообщение с указанием адреса
    Osc { 
        /// OSC-адрес
        address: String 
    },
    /// Изменение через скрипт
    Script,
    /// Внешний источник
    External,
}

/// Сигнал об изменении параметра узла
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ParameterChanged {
    /// ID узла
    pub node_id: NodeId,
    /// ID параметра
    pub parameter_id: ParameterId,  // заменяем String на ParameterId
    /// Значение
    pub value: f32,
    /// Нормализованное значение (0-1)
    pub normalized_value: f32,
    /// Временная метка
    pub timestamp: u64,
    /// Источник сигнала
    pub source: SignalSource,
}

impl Signal for ParameterChanged {}

/// Тактовый сигнал от транспорта
#[derive(Debug, Clone)]
pub struct ClockTick {
    /// Позиция в сэмплах
    pub sample_pos: u64,
    /// Сэмплов с прошлого тика
    pub samples_since_last: u32,
}

impl Signal for ClockTick {}

/// Системные события
#[derive(Debug, Clone)]
pub enum SystemEvent {
    /// Граф обработки изменился
    GraphChanged,
    /// Транспорт запущен
    TransportStarted,
    /// Транспорт остановлен
    TransportStopped,
    /// Ошибка в системе с описанием
    Error(String),
}

impl Signal for SystemEvent {}