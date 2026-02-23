//! # Типы сигналов

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::traits::{PortId, ParameterId};  // обновляем импорт

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

/// Сигнал изменения параметра.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ParameterChanged {
    pub port: PortId,
    pub parameter: ParameterId,
    pub value: f32,
    pub normalized: f32,
    pub timestamp: u64,
    pub source: SignalSource,
}

impl ParameterChanged {
    pub fn new(
        port: PortId,
        parameter: ParameterId,
        value: f32,
        normalized: f32,
        source: SignalSource,
    ) -> Self {
        Self {
            port,
            parameter,
            value,
            normalized,
            timestamp: current_timestamp(),
            source,
        }
    }

    pub fn node_parameter(
        node: crate::traits::NodeId,
        parameter: ParameterId,
        value: f32,
        normalized: f32,
        source: SignalSource,
    ) -> Self {
        Self::new(PortId::node(node), parameter, value, normalized, source)
    }
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Маркерный трейт для сигналов.
pub trait Signal: Send + Sync + 'static {}

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