//! Passive real-time telemetry — peak/RMS/DC probes and non-RT collectors.

#![warn(missing_docs)]

/// Non-real-time collector that drains telemetry from a shared ring buffer.
pub mod collector;
#[cfg(feature = "debug")]
pub mod debug;
/// Real-time telemetry probe that captures per-block metrics.
pub mod probe;

/// Convenience re-exports for common telemetry types.
pub mod prelude {
    pub use crate::collector::TelemetryCollector;
    #[cfg(feature = "debug")]
    pub use crate::debug::collector_thread::CollectorThread;
    #[cfg(feature = "debug")]
    pub use crate::debug::formatter::{EventFormatter, JsonFormatter, TextFormatter};
    #[cfg(feature = "debug")]
    pub use crate::debug::protocol::{
        AnalyzerCommand, AnalyzerConfig, AnalyzerResponse, OutputMode,
    };
    #[cfg(feature = "debug")]
    pub use crate::debug::state::ProbeStateManager;
    pub use crate::probe::TelemetryProbe;
}
