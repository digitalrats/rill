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
//! Use [`Patchbay::attach_sequencer`](crate::engine::Patchbay::attach_sequencer)
//! to spawn the sequencer task and start listening for clock ticks.
//! Returns parameter commands to be dispatched via `ActorRef<SetParameter>`.
//! select).

mod engine;
mod pattern;
mod snapshot;
mod step;

pub use engine::SnapshotSequencer;
pub use pattern::{Pattern, StepPlayMode};
pub use snapshot::{ParameterTarget, Snapshot};
pub use step::SequenceStep;
