//! # События и сигналы
//!
//! Типы для обмена событиями между компонентами системы.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::node::NodeId;
use crate::port::PortId;

/// Тип источника события
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSource {
    /// Пользовательский интерфейс
    UserInterface,
    /// Автоматизация (LFO, огибающая)
    Automation,
    /// MIDI устройство
    Midi,
    /// OSC сообщение
    Osc,
    /// Скрипт
    Script,
    /// Внешний источник
    External,
}

/// Событие изменения параметра
#[derive(Debug, Clone)]
pub struct ParameterChange {
    /// Целевой порт
    pub target: PortId,
    /// Имя параметра
    pub param: String,
    /// Новое значение
    pub value: f32,
    /// Нормализованное значение (0.0-1.0)
    pub normalized: f32,
    /// Временная метка (микросекунды)
    pub timestamp: u64,
    /// Источник события
    pub source: EventSource,
}

impl ParameterChange {
    /// Создать новое событие изменения параметра
    pub fn new(target: PortId, param: impl Into<String>, value: f32) -> Self {
        Self {
            target,
            param: param.into(),
            value,
            normalized: value.clamp(0.0, 1.0),
            timestamp: current_timestamp(),
            source: EventSource::External,
        }
    }
    
    /// Установить источник
    pub fn with_source(mut self, source: EventSource) -> Self {
        self.source = source;
        self
    }
}

/// MIDI событие
#[derive(Debug, Clone, Copy)]
pub enum MidiEvent {
    /// Note On
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    /// Note Off
    NoteOff {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    /// Control Change
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    /// Pitch Bend
    PitchBend {
        channel: u8,
        value: i16,
    },
    /// Program Change
    ProgramChange {
        channel: u8,
        program: u8,
    },
    /// Clock (для синхронизации)
    Clock,
    /// Start
    Start,
    /// Stop
    Stop,
    /// Continue
    Continue,
}

impl MidiEvent {
    /// Получить нормализованное значение (0.0-1.0) для CC
    pub fn normalized_value(&self) -> Option<f32> {
        match self {
            MidiEvent::ControlChange { value, .. } => Some(*value as f32 / 127.0),
            MidiEvent::NoteOn { velocity, .. } => Some(*velocity as f32 / 127.0),
            MidiEvent::PitchBend { value, .. } => Some((*value as f32 + 8192.0) / 16383.0),
            _ => None,
        }
    }
}

/// OSC событие
#[derive(Debug, Clone)]
pub struct OscEvent {
    /// OSC адрес
    pub address: String,
    /// Аргументы (как f32 для простоты)
    pub args: Vec<f32>,
    /// Временная метка
    pub timestamp: u64,
}

/// Системное событие
#[derive(Debug, Clone)]
pub enum SystemEvent {
    /// Граф изменился
    GraphChanged,
    /// Транспорт запущен
    TransportStarted,
    /// Транспорт остановлен
    TransportStopped,
    /// BPM изменился
    BpmChanged(f32),
    /// Позиция изменилась
    PositionChanged(u64),
    /// Ошибка
    Error(String),
}

/// Общий тип события
#[derive(Debug, Clone)]
pub enum Event {
    /// Изменение параметра
    ParameterChange(ParameterChange),
    /// MIDI событие
    Midi(MidiEvent),
    /// OSC событие
    Osc(OscEvent),
    /// Системное событие
    System(SystemEvent),
}

/// Получить текущую временную метку
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Сигнатура для обработчика событий
pub type EventHandler = Box<dyn Fn(&Event) + Send + Sync>;