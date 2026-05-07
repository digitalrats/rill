//! Passive real-time telemetry — peak/RMS/DC probes and non-RT collectors.

#![warn(missing_docs)]

/// Non-real-time collector that drains telemetry from a shared ring buffer.
pub mod collector;
/// Real-time telemetry probe that captures per-block metrics.
pub mod probe;

/// Convenience re-exports for common telemetry types.
pub mod prelude {
    pub use crate::collector::TelemetryCollector;
    pub use crate::probe::TelemetryProbe;
}
