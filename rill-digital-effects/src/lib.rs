//! Digital signal effects for Rill
//!
//! This crate provides common digital effects:
//! - Delay with feedback
//! - Distortion with multiple waveshaping types
//! - Limiter with lookahead
//! - Dry/Wet mix utility
//! - Tape write/read heads for delay lines
//! - More to come: Chorus, Flanger, Phaser, Reverb, Compressor

#![warn(missing_docs)]

/// Delay effect with configurable time, feedback, and mix.
pub mod delay;
/// Waveshaping distortion with multiple algorithm types.
pub mod distortion;
/// Lookahead limiter with configurable threshold and release.
pub mod limiter;

pub use delay::Delay;
pub use distortion::{Distortion, DistortionType};
pub use limiter::Limiter;
