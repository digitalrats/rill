//! Digital audio effects for Rill
//!
//! This crate provides common digital effects:
//! - Delay with feedback
//! - Distortion with multiple waveshaping types
//! - Limiter with lookahead
//! - Dry/Wet mix utility
//! - Tape write/read heads for delay lines
//! - More to come: Chorus, Flanger, Phaser, Reverb, Compressor

#![warn(missing_docs)]

pub mod delay;
pub mod dry_wet_mix;
pub mod distortion;
pub mod limiter;
pub mod read_head;
pub mod write_head;

pub use delay::Delay;
pub use dry_wet_mix::DryWetMix;
pub use distortion::{Distortion, DistortionType};
pub use limiter::Limiter;
pub use read_head::ReadHead;
pub use write_head::WriteHead;