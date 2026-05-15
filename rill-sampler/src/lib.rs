//! Sample playback and time-series reading for the Rill signal graph.
//!
//! Provides:
//! - `SamplePlayerNode` — stereo sample playback with loop modes
//! - `SampleBuffer` — sample container with WAV loading (feature `"wav"`)
//! - `TimeSeriesReader` / `TimeSeriesNode` — irregular time series playback from CSV
//!
//! Depends on `rill-core` and `rill-core-dsp`.

#![warn(missing_docs)]

/// Sample buffer container for mono/stereo sample data.
pub mod buffer;

/// Sample playback source node with loop modes.
pub mod player;

/// Recording sink node — captures signal for offline analysis and WAV export.
pub mod recorder;
/// Unevenly-sampled time series reader and source node.
pub mod timeseries;

#[cfg(feature = "wav")]
/// WAV file loading (requires feature `"wav"`).
pub mod wav;

/// Re-exported convenience items (`SampleBuffer`, key traits).
pub mod prelude;

/// Re-export of the `rill_core` crate.
pub use rill_core;
/// Re-export of the `rill_core_dsp` crate.
pub use rill_core_dsp;
