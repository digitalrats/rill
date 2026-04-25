//! Digital audio effects for Rill
//!
//! This crate provides common digital effects:
//! - Delay with feedback
//! - Distortion with multiple waveshaping types
//! - Limiter with lookahead
//! - More to come: Chorus, Flanger, Phaser, Reverb, Compressor

#![warn(missing_docs)]

pub mod delay;
pub mod distortion;
pub mod limiter;

pub use delay::Delay;
pub use distortion::{Distortion, DistortionType};
pub use limiter::Limiter;
