//! Базовые типы для мира автоматов

use rill_core::traits::{NodeId, ParameterId, PortId};
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Время в мире автоматов
#[derive(Debug, Clone, Copy)]
pub struct WorldTime {
    /// Абсолютное время в секундах
    pub absolute: f64,
    /// Время с последнего обновления
    pub delta: f64,
    /// Количество тиков
    pub tick: u64,
}

impl WorldTime {
    pub fn new() -> Self {
        Self {
            absolute: 0.0,
            delta: 0.0,
            tick: 0,
        }
    }
    
    pub fn advance(&mut self, delta_seconds: f64) {
        self.delta = delta_seconds;
        self.absolute += delta_seconds;
        self.tick += 1;
    }
}

/// Источник сигнала в мире автоматов
#[derive(Debug, Clone)]
pub enum SignalOrigin {
    /// Автомат (LFO, envelope)
    Automaton(String),
    /// Сенсор (ручка, микрофон)
    Sensor(String),
    /// Серво (исполнитель)
    Servo(String),
    /// Внешний мир (MIDI, OSC)
    External(String),
}

impl fmt::Display for SignalOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignalOrigin::Automaton(name) => write!(f, "⚙️ {}", name),
            SignalOrigin::Sensor(name) => write!(f, "👁️ {}", name),
            SignalOrigin::Servo(name) => write!(f, "🦾 {}", name),
            SignalOrigin::External(name) => write!(f, "🌍 {}", name),
        }
    }
}

/// Сигнал в мире автоматов (расширение ParameterChanged)
#[derive(Debug, Clone)]
pub struct WorldSignal {
    /// Откуда пришел сигнал
    pub origin: SignalOrigin,
    /// Куда направлен
    pub target: Option<SignalTarget>,
    /// Значение (нормализованное 0-1)
    pub value: f32,
    /// Время возникновения
    pub time: WorldTime,
}

impl WorldSignal {
    pub fn new(origin: SignalOrigin, value: f32) -> Self {
        Self {
            origin,
            target: None,
            value: value.clamp(0.0, 1.0),
            time: WorldTime::new(), // Будет заполнено позже
        }
    }
    
    pub fn with_target(mut self, target: SignalTarget) -> Self {
        self.target = Some(target);
        self
    }
}

/// Цель сигнала
#[derive(Debug, Clone)]
pub enum SignalTarget {
    /// Параметр в SignalGraph
    Parameter(PortId, ParameterId),
    /// Другой автомат
    Automaton(String),
    /// Шина (группа)
    Bus(String),
}

/// Контекст выполнения автомата
#[derive(Clone)]
pub struct AutomatonContext {
    /// Текущее время
    pub time: WorldTime,
    /// Входящие сигналы за этот тик
    pub inputs: Vec<WorldSignal>,
    /// Память автомата (для stateful)
    pub memory: Arc<parking_lot::RwLock<Vec<f32>>>,
}

impl AutomatonContext {
    pub fn new() -> Self {
        Self {
            time: WorldTime::new(),
            inputs: Vec::new(),
            memory: Arc::new(parking_lot::RwLock::new(Vec::with_capacity(16))),
        }
    }
}

/// Ошибки мира автоматов
#[derive(Debug, thiserror::Error)]
pub enum WorldError {
    #[error("Sensor {0} not found")]
    SensorNotFound(String),
    
    #[error("Automaton {0} not found")]
    AutomatonNotFound(String),
    
    #[error("Servo {0} not found")]
    ServoNotFound(String),
    
    #[error("Signal target not found")]
    TargetNotFound,
    
    #[error("Channel error")]
    ChannelError,
}