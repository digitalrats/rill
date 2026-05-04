//! Telemetry queue — feedback data from the audio world.

use super::command::Command;
use crate::traits::{NodeId, ParameterId, PortId};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Константы для телеметрии часов (clock)
///
/// Формат `CLOCK_TICK` (Telemetry::Event с kind="clock_tick"):
/// - `data[0]` — `sample_pos` (абсолютная позиция сэмпла, f32)
/// - `data[1]` — `sample_rate` (частота дискретизации, Hz)
/// - `data[2]` — `tempo` (BPM, 0.0 если неизвестен)
/// - `data[3]` — `beat_position` (дробная позиция бита, 0.0 если нет темпа)
/// - `data[4]` — `is_new_beat` (1.0 если это начало нового бита, иначе 0.0)
/// - `data[5]` — `is_new_bar` (1.0 если это начало нового такта, иначе 0.0)
/// Event kind for clock tick telemetry.
pub const CLOCK_TICK: &str = "clock_tick";
/// Event kind for clock tempo telemetry.
pub const CLOCK_TEMPO: &str = "clock_tempo";

/// Lightweight wrapper around a telemetry sender.
///
/// Stored in nodes that wish to emit telemetry from `generate()` /
/// `process()` / `consume()`. Non-blocking `try_send` is safe for the
/// audio thread.
#[derive(Clone)]
pub struct TelemetryTx {
    /// Optional inner crossbeam sender (None = telemetry disabled).
    inner: Option<crossbeam_channel::Sender<Telemetry>>,
}

impl TelemetryTx {
    /// Create a disabled (no-op) telemetry sender.
    pub const fn empty() -> Self {
        Self { inner: None }
    }

    /// Create a new telemetry sender wrapping a crossbeam channel sender.
    pub fn new(tx: crossbeam_channel::Sender<Telemetry>) -> Self {
        Self { inner: Some(tx) }
    }

    /// Try to send a telemetry event (non-blocking, safe for RT threads).
    pub fn try_send(&self, event: Telemetry) {
        if let Some(ref tx) = self.inner {
            let _ = tx.try_send(event);
        }
    }

    /// Return a reference to the inner crossbeam sender, if present.
    pub fn sender(&self) -> Option<&crossbeam_channel::Sender<Telemetry>> {
        self.inner.as_ref()
    }
}

/// Telemetry type identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TelemetryKind {
    /// Parameter value telemetry.
    Parameter,
    /// Signal data (audio, sensor readings).
    Signal,
    /// Peak value telemetry.
    Peak,
    /// Event telemetry.
    Event,
    /// Micro-control violation telemetry.
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

/// Telemetry data emitted by signal graph nodes.
#[derive(Debug, Clone)]
pub enum Telemetry {
    /// A parameter value change.
    ParameterValue {
        /// Target port.
        port: PortId,
        /// Target parameter.
        parameter: ParameterId,
        /// Current parameter value.
        value: f32,
        /// Unix timestamp (microseconds).
        timestamp: u64,
    },

    /// Audio or sensor signal data.
    SignalData {
        /// Source node ID.
        node_id: NodeId,
        /// Channel index.
        channel: usize,
        /// Signal sample data.
        data: Vec<f32>,
        /// Unix timestamp (microseconds).
        timestamp: u64,
        /// Sample rate of the signal.
        sample_rate: f32,
    },

    /// Peak value reading.
    Peak {
        /// Target port.
        port: PortId,
        /// Peak value.
        value: f32,
        /// Unix timestamp (microseconds).
        timestamp: u64,
        /// Optional hold time in milliseconds.
        hold_time_ms: Option<u32>,
    },

    /// Named event with float payload.
    Event {
        /// Source component name.
        source: String,
        /// Event kind string.
        kind: String,
        /// Event data payload.
        data: Vec<f32>,
        /// Unix timestamp (microseconds).
        timestamp: u64,
        /// Optional human-readable description.
        description: Option<String>,
    },

    /// Micro-control timing violation.
    Violation {
        /// Component that exceeded its time budget.
        component: String,
        /// Expected execution time (nanoseconds).
        expected_ns: u64,
        /// Actual execution time (nanoseconds).
        actual_ns: u64,
        /// Optional associated value.
        value: Option<f32>,
        /// Unix timestamp (microseconds).
        timestamp: u64,
    },
}

// Реализуем трейт Command для Telemetry
impl Command for Telemetry {}

impl Telemetry {
    /// Return the current Unix time in microseconds.
    pub fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }

    /// Create a parameter value telemetry event.
    pub fn parameter(port: PortId, parameter: ParameterId, value: f32) -> Self {
        Telemetry::ParameterValue {
            port,
            parameter,
            value,
            timestamp: Self::now(),
        }
    }

    /// Create a parameter value telemetry event with an explicit timestamp (for testing).
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

    /// Create a signal data telemetry event.
    pub fn signal(node_id: NodeId, channel: usize, data: Vec<f32>) -> Self {
        Telemetry::SignalData {
            node_id,
            channel,
            data,
            timestamp: Self::now(),
            sample_rate: 44100.0,
        }
    }

    /// Create a signal data telemetry event with an explicit sample rate.
    pub fn signal_with_sample_rate(
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

    /// Create a peak value telemetry event.
    pub fn peak(port: PortId, value: f32) -> Self {
        Telemetry::Peak {
            port,
            value,
            timestamp: Self::now(),
            hold_time_ms: None,
        }
    }

    /// Create a peak value telemetry event with a hold time.
    pub fn peak_with_hold(port: PortId, value: f32, hold_time_ms: u32) -> Self {
        Telemetry::Peak {
            port,
            value,
            timestamp: Self::now(),
            hold_time_ms: Some(hold_time_ms),
        }
    }

    /// Create an event telemetry event.
    pub fn event(source: impl Into<String>, kind: impl Into<String>, data: Vec<f32>) -> Self {
        Telemetry::Event {
            source: source.into(),
            kind: kind.into(),
            data,
            timestamp: Self::now(),
            description: None,
        }
    }

    /// Create an event telemetry event with a description.
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

    /// Create a micro-control violation telemetry event.
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

    /// Return the telemetry type category.
    pub fn kind(&self) -> TelemetryKind {
        match self {
            Telemetry::ParameterValue { .. } => TelemetryKind::Parameter,
            Telemetry::SignalData { .. } => TelemetryKind::Signal,
            Telemetry::Peak { .. } => TelemetryKind::Peak,
            Telemetry::Event { .. } => TelemetryKind::Event,
            Telemetry::Violation { .. } => TelemetryKind::Violation,
        }
    }

    /// Return the timestamp (microseconds since Unix epoch).
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

/// Alias for a [`CommandQueue`] specialised for telemetry data.
///
/// [`CommandQueue`]: super::command::CommandQueue
pub type TelemetryQueue = super::command::CommandQueue<Telemetry>;

/// Convenience extension methods for [`TelemetryQueue`].
pub trait TelemetryQueueExt {
    /// Send a parameter value telemetry event.
    fn send_parameter(
        &self,
        port: PortId,
        parameter: ParameterId,
        value: f32,
    ) -> Result<(), super::error::QueueError>;
    /// Send a signal data telemetry event.
    fn send_signal(
        &self,
        node_id: NodeId,
        channel: usize,
        data: Vec<f32>,
    ) -> Result<(), super::error::QueueError>;
    /// Send a signal data telemetry event with explicit sample rate.
    fn send_signal_with_sample_rate(
        &self,
        node_id: NodeId,
        channel: usize,
        data: Vec<f32>,
        sample_rate: f32,
    ) -> Result<(), super::error::QueueError>;
    /// Send a peak value telemetry event.
    fn send_peak(&self, port: PortId, value: f32) -> Result<(), super::error::QueueError>;
    /// Send a peak value telemetry event with hold time.
    fn send_peak_with_hold(
        &self,
        port: PortId,
        value: f32,
        hold_time_ms: u32,
    ) -> Result<(), super::error::QueueError>;
    /// Send an event telemetry event.
    fn send_event(
        &self,
        source: &str,
        kind: &str,
        data: Vec<f32>,
    ) -> Result<(), super::error::QueueError>;
    /// Send an event telemetry event with description.
    fn send_event_with_description(
        &self,
        source: &str,
        kind: &str,
        data: Vec<f32>,
        description: &str,
    ) -> Result<(), super::error::QueueError>;
    /// Send a micro-control violation telemetry event.
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
    }

    fn send_signal(
        &self,
        node_id: NodeId,
        channel: usize,
        data: Vec<f32>,
    ) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::signal(node_id, channel, data))
    }

    fn send_signal_with_sample_rate(
        &self,
        node_id: NodeId,
        channel: usize,
        data: Vec<f32>,
        sample_rate: f32,
    ) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::signal_with_sample_rate(
            node_id,
            channel,
            data,
            sample_rate,
        ))
    }

    fn send_peak(&self, port: PortId, value: f32) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::peak(port, value))
    }

    fn send_peak_with_hold(
        &self,
        port: PortId,
        value: f32,
        hold_time_ms: u32,
    ) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::peak_with_hold(port, value, hold_time_ms))
    }

    fn send_event(
        &self,
        source: &str,
        kind: &str,
        data: Vec<f32>,
    ) -> Result<(), super::error::QueueError> {
        self.send(Telemetry::event(source, kind, data))
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
    }
}
