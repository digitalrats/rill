//! # Parameter-lock step sequencer
//!
//! A sample-accurate, telemetry-driven sequencer for scheduling parameter
//! changes over time.  Supports parameter-lock (p-lock) style programming:
//! each step specifies exactly which parameters change and to what value;
//! unlisted parameters are untouched.
//!
//! ## Concepts
//!
//! - [`ParameterTarget`] — a single (node, param, value) lock
//! - [`SequenceStep`] — a set of p-locks + duration
//! - [`Pattern`] — a sequence of steps with a playback mode
//! - [`Snapshot`] — a named collection of p-locks (convenience presets)
//! - [`SnapshotSequencer`] — tick-driven state machine
//!
//! ## Clock source
//!
//! The sequencer is designed to be driven by the audio thread's `CLOCK_TICK`
//! telemetry events.  Each tick carries `(sample_pos, sample_rate, tempo)`,
//! which the sequencer uses to determine step boundaries with sample accuracy.
//!
//! ## Integration
//!
//! Use [`Engine::attach_sequencer`](crate::engine::Engine::attach_sequencer)
//! or [`PatchbayEngine::attach_sequencer`](crate::engine::PatchbayEngine::attach_sequencer)
//! to spawn the sequencer task and start listening for clock ticks.
//! A [`SequencerHandle`] is returned for external control (start/stop/pattern
//! select).

mod engine;
mod pattern;
mod snapshot;
mod step;

pub use engine::{SequencerCommand, SequencerHandle, SnapshotSequencer};
pub use pattern::{Pattern, StepPlayMode};
pub use snapshot::{ParameterTarget, Snapshot};
pub use step::SequenceStep;

/// Serializable sequencer configuration (serde feature gate).
#[cfg(feature = "serde")]
mod def;
#[cfg(feature = "serde")]
pub use def::SequencerDef;
