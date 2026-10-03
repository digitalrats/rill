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

/// The tape as a passive in-memory delay backend (ring buffer + heads).
pub mod tape;
/// Re-export of the tape head resource-backed builtins registration.
#[cfg(feature = "lang")]
pub use tape::lang::register_tape_builtins;
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

/// Register graph nodes and lang builtins for sampler.
pub mod register;

/// Register sampler backends (passive — no driver/callback) into a
/// [`BackendFactory`](rill_graph::backend_factory::BackendFactory).
#[cfg(feature = "graph")]
pub fn register_backends(factory: &mut rill_graph::backend_factory::BackendFactory) {
    factory.register("sampler", rill_core::io::BackendMeta::passive(), |_| {
        Err("passive backends produce no driver; not constructed via factory".into())
    });
}

/// rill-lang builtins for sampler types.
#[cfg(feature = "lang")]
mod lang;
