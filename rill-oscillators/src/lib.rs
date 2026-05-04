//! Unified oscillators for Rill
//!
//! Audio-frequency oscillators (20 Hz – 20 kHz): Sine, Saw, Square,
//! Triangle, Noise, FM synthesis, and wavetable.

#![warn(missing_docs)]

pub mod audio;

// Re-export common types for convenience
pub use audio::*;
