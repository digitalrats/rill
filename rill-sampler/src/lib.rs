//! Sample playback and time-series reading for the Rill signal graph.
//!
//! Provides:
//! - `SamplePlayerNode` — stereo sample playback with loop modes
//! - `SampleBuffer` — sample container with WAV loading (feature `"wav"`)
//! - `TimeSeriesReader` / `TimeSeriesNode` — irregular time series playback from CSV
//!
//! Depends on `rill-core` and `rill-core-dsp`.

#![warn(missing_docs)]

pub mod buffer;
pub mod player;
pub mod timeseries;

#[cfg(feature = "wav")]
pub mod wav;

pub mod prelude;

pub use rill_core;
pub use rill_core_dsp;
