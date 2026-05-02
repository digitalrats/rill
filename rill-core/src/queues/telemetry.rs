//! Очередь телеметрии — данные обратной связи из звукового мира

use super::command::Command;
use crate::traits::{NodeId, ParameterId, PortId};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Тип телеметрии (для идентификации)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TelemetryKind {
    /// Значение параметра
    Parameter,
    /// Сигнальные данные (аудио, сенсоры)
    Signal,
    /// Пиковое значение
    Peak,
    /// Событие
    Event,
    /// Нарушение микро-контроля
    Violation,
}

impl fmt::Display for TelemetryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TelemetryKind::Parameter => write!(f, "parameter"),
            TelemetryKind::Signal => write!(f, "signal"),
            TelemetryKind::Peak => write!(f, "peak"),
            TelemetryKind::Event => write!(f, "event"),
            TelemetryKind::Violation => write!(f, "violation"),
        }
    }
}

/// Данные телеметрии
#[derive(Debug, Clone)]
pub enum Telemetry {
    /// Значение параметра
    ParameterValue {
        port: PortId,
        parameter: ParameterId,
        value: f32,
        timestamp: u64,
    },

    /// Аудио данные
    SignalData {
        node_id: NodeId,
        channel: usize,
        data: Vec<f32>,
        timestamp: u64,
        sample_rate: f32,
    },

    /// Пиковое значение
    Peak {
        port: PortId,
        value: f32,
        timestamp: u64,
        hold_time_ms: Option<u32>,
    },

    /// Событие
    Event {
        source: String,
        kind: String,
        data: Vec<f32>,
        timestamp: u64,
        description: Option<String>,
    },

    /// Нарушение микро-контроля
    Violation {
        component: String,
        expected_ns: u64,
        actual_ns: u64,
        value: Option<f32>,
        timestamp: u64,
    },
}

// Реализуем трейт Command для Telemetry
impl Command for Telemetry {}

impl Telemetry {
    /// Создать метку времени (текущее время в микросекундах)
    pub fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }

    /// Создать телеметрию значения параметра
    pub fn parameter(port: PortId, parameter: ParameterId, value: f32) -> Self {
        Telemetry::ParameterValue {
            port,
            parameter,
            value,
            timestamp: Self::now(),
        }
    }

    /// Создать телеметрию с указанной временной меткой (для тестов)
    pub fn parameter_with_time(
        port: PortId,
        parameter: ParameterId,
        value: f32,
        timestamp: u64,
    ) -> Self {
        Telemetry::ParameterValue {
            port,
            parameter,
            value,
            timestamp,
        }
    }

    /// Создать телеметрию аудиоданных
    pub fn audio(node_id: NodeId, channel: usize, data: Vec<f32>) -> Self {
        Telemetry::SignalData {
            node_id,
            channel,
            data,
            timestamp: Self::now(),
            sample_rate: 44100.0,
        }
    }

    /// Создать телеметрию аудиоданных с частотой дискретизации
    pub fn audio_with_sample_rate(
        node_id: NodeId,
        channel: usize,
        data: Vec<f32>,
        sample_rate: f32,
    ) -> Self {
        Telemetry::SignalData {
            node_id,
            channel,
            data,
            timestamp: Self::now(),
            sample_rate,
        }
    }

    /// Создать телеметрию пика
    pub fn peak(port: PortId, value: f32) -> Self {
        Telemetry::Peak {
            port,
            value,
            timestamp: Self::now(),
            hold_time_ms: None,
        }
    }

    /// Создать телеметрию пика с удержанием
    pub fn peak_with_hold(port: PortId, value: f32, hold_time_ms: u32) -> Self {
        Telemetry::Peak {
            port,
            value,
            timestamp: Self::now(),
            hold_time_ms: Some(hold_time_ms),
        }
    }

    /// Создать телеметрию события
    pub fn event(source: impl Into<String>, kind: impl Into<String>, data: Vec<f32>) -> Self {
        Telemetry::Event {
            source: source.into(),
            kind: kind.into(),
            data,
            timestamp: Self::now(),
            description: None,
        }
    }

    /// Создать телеметрию события с описанием
    pub fn event_with_description(
        source: impl Into<String>,
        kind: impl Into<String>,
        data: Vec<f32>,
        description: impl Into<String>,
    ) -> Self {
        Telemetry::Event {
            source: source.into(),
            kind: kind.into(),
            data,
            timestamp: Self::now(),
            description: Some(description.into()),
        }
    }

    /// Создать телеметрию нарушения
    pub fn violation(
        component: impl Into<String>,
        expected_ns: u64,
        actual_ns: u64,
        value: Option<f32>,
    ) -> Self {
        Telemetry::Violation {
            component: component.into(),
            expected_ns,
            actual_ns,
            value,
            timestamp: Self::now(),
        }
    }

    /// Получить тип телеметрии
    pub fn kind(&self) -> TelemetryKind {
        match self {
            Telemetry::ParameterValue { .. } => TelemetryKind::Parameter,
            Telemetry::SignalData { .. } => TelemetryKind::Signal,
            Telemetry::Peak { .. } => TelemetryKind::Peak,
            Telemetry::Event { .. } => TelemetryKind::Event,
            Telemetry::Violation { .. } => TelemetryKind::Violation,
        }
    }

    /// Получить временную метку
    pub fn timestamp(&self) -> u64 {
        match self {
            Telemetry::ParameterValue { timestamp, .. } => *timestamp,
            Telemetry::SignalData { timestamp, .. } => *timestamp,
            Telemetry::Peak { timestamp, .. } => *timestamp,
            Telemetry::Event { timestamp, .. } => *timestamp,
            Telemetry::Violation { timestamp, .. } => *timestamp,
        }
    }
}

impl fmt::Display for Telemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Telemetry::ParameterValue {
                port,
                parameter,
                value,
                timestamp,
            } => {
                write!(
                    f,
                    "[{}] 📊 {}::{} = {:.3}",
                    timestamp, port, parameter, value
                )
            }
            Telemetry::SignalData {
                node_id,
                channel,
                data,
                timestamp,
                sample_rate,
            } => {
                let duration_ms = data.len() as f32 / sample_rate * 1000.0;
                write!(
                    f,
                    "[{}] 🎵 {}:{} ({} samples, {:.1}ms)",
                    timestamp,
                    node_id,
                    channel,
                    data.len(),
                    duration_ms
                )
            }
            Telemetry::Peak {
                port,
                value,
                timestamp,
                hold_time_ms,
            } => {
                if let Some(hold) = hold_time_ms {
                    write!(
                        f,
                        "[{}] 📈 {} = {:.3} (hold {}ms)",
                        timestamp, port, value, hold
                    )
                } else {
                    write!(f, "[{}] 📈 {} = {:.3}", timestamp, port, value)
                }
            }
            Telemetry::Event {
                source,
                kind,
                data,
                timestamp,
                description,
            } => {
                if let Some(desc) = description {
                    write!(
                        f,
                        "[{}] 📢 {}:{} ({}) {:?}",
                        timestamp, source, kind, desc, data
                    )
                } else {
                    write!(f, "[{}] 📢 {}:{} {:?}", timestamp, source, kind, data)
                }
            }
            Telemetry::Violation {
                component,
                expected_ns,
                actual_ns,
                value,
                timestamp,
            } => {
                if let Some(v) = value {
                    write!(
                        f,
                        "[{}] ⚠️ {} нарушение: {}нс > {}нс, value={:.3}",
                        timestamp, component, actual_ns, expected_ns, v
                    )
                } else {
                    write!(
                        f,
                        "[{}] ⚠️ {} нарушение: {}нс > {}нс",
                        timestamp, component, actual_ns, expected_ns
                    )
                }
            }
        }
    }
}

// TelemetryQueue - это просто тип-алиас на CommandQueue<Telemetry>
pub type TelemetryQueue = super::command::CommandQueue<Telemetry>;

// Удобные методы расширения для TelemetryQueue
pub trait TelemetryQueueExt {
    fn send_parameter(
        &self,
        port: PortId,
        parameter: ParameterId,
        value: f32,
    ) -> Result<(), super::error::QueueError>;
    fn send_audio(
        &self,
        node_id: NodeId,
        channel: usize,
        data: Vec<f32>,
    ) -> Result<(), super::error::QueueError>;
    fn send_audio_with_sample_rate(
        &self,
        node_id: NodeId,
        channel: usize,
        data: Vec<f32>,
        sample_rate: f32,
    ) -> Result<(), super::error::QueueError>;
    fn send_peak(&self, port: PortId, value: f32) -> Result<(), super::error::QueueError>;
    fn send_peak_with_hold(
        &self,
        port: PortId,
        value: f32,
        hold_time_ms: u32,
    ) -> Result<(), super::error::QueueError>;
    fn send_event(
        &self,
        source: &str,
        kind: &str,
        data: Vec<f32>,
    ) -> Result<(), super::error::QueueError>;
    fn send_event_with_description(
        &self,
        source: &str,
        kind: &str,
        data: Vec<f32>,
        description: &str,
    ) -> Result<(), super::error::QueueError>;
    fn send_violation(
        &self,
        component: &str,
        expected_ns: u64,
        actual_ns: u64,
        value: Option<f32>,
    ) -> Result<(), super::error::QueueError>;
}

impl TelemetryQueueExt for super::command::CommandQueue<Telemetry> {
    fn send_parameter(
        &self,
        port: PortId,
        parameter: ParameterId,
        value: f32,
    ) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::parameter(port, parameter, value))
            .map_err(|e| e.into())
    }

    fn send_audio(
        &self,
        node_id: NodeId,
        channel: usize,
        data: Vec<f32>,
    ) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::audio(node_id, channel, data))
            .map_err(|e| e.into())
    }

    fn send_audio_with_sample_rate(
        &self,
        node_id: NodeId,
        channel: usize,
        data: Vec<f32>,
        sample_rate: f32,
    ) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::audio_with_sample_rate(
            node_id,
            channel,
            data,
            sample_rate,
        ))
        .map_err(|e| e.into())
    }

    fn send_peak(&self, port: PortId, value: f32) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::peak(port, value))
            .map_err(|e| e.into())
    }

    fn send_peak_with_hold(
        &self,
        port: PortId,
        value: f32,
        hold_time_ms: u32,
    ) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::peak_with_hold(port, value, hold_time_ms))
            .map_err(|e| e.into())
    }

    fn send_event(
        &self,
        source: &str,
        kind: &str,
        data: Vec<f32>,
    ) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::event(source, kind, data))
            .map_err(|e| e.into())
    }

    fn send_event_with_description(
        &self,
        source: &str,
        kind: &str,
        data: Vec<f32>,
        description: &str,
    ) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::event_with_description(
            source,
            kind,
            data,
            description,
        ))
        .map_err(|e| e.into())
    }

    fn send_violation(
        &self,
        component: &str,
        expected_ns: u64,
        actual_ns: u64,
        value: Option<f32>,
    ) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::violation(
            component,
            expected_ns,
            actual_ns,
            value,
        ))
        .map_err(|e| e.into())
    }
}
