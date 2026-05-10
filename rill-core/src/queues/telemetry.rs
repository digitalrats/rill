//! Telemetry types — event data from the audio world.

use crate::traits::{NodeId, ParameterId, PortId};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Event kind for clock tick telemetry.
pub const CLOCK_TICK: &str = "clock_tick";
/// Event kind for clock tempo telemetry.
pub const CLOCK_TEMPO: &str = "clock_tempo";

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
}
