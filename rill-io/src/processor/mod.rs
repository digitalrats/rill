//! Processors for AudioEngine
//!
//! This module contains various implementations of the `AudioProcessor` trait
//! for real-time audio processing.

mod basic;

#[cfg(feature = "graph")]
mod graph;

#[cfg(feature = "examples")]
mod sine;

pub use basic::{
    GainProcessor,
    MonoMixerProcessor,
    PassThroughProcessor,
    SilenceProcessor,
};

#[cfg(feature = "examples")]
pub use basic::CaptureProcessor;

#[cfg(feature = "graph")]
pub use graph::GraphProcessor;

#[cfg(feature = "examples")]
pub use sine::SineProcessor;

pub use crate::engine::AudioProcessor;
