//! Типы сигналов

#[cfg(feature = "serde")]
use serde::{Serialize, Deserialize};

/// Базовый трейт сигнала
pub trait Signal: Send + Sync + 'static {}

/// Источник сигнала
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SignalSource {
    UserInterface,
    Automation,
    Midi { channel: u8, controller: u8 },
    Osc { address: String },
    Script,
    External,
}

/// Сигнал изменения параметра
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ParameterChanged {
    pub node_id: String,
    pub parameter_id: String,
    pub value: f32,
    pub normalized_value: f32,
    pub timestamp: u64,
    pub source: SignalSource,
}

impl Signal for ParameterChanged {}

/// Сигнал тактового синхроимпульса
#[derive(Debug, Clone)]
pub struct ClockTick {
    pub sample_pos: u64,
    pub samples_since_last: u32,
}

impl Signal for ClockTick {}

/// Системное событие
#[derive(Debug, Clone)]
pub enum SystemEvent {
    GraphChanged,
    TransportStarted,
    TransportStopped,
    Error(String),
}

impl Signal for SystemEvent {}