//! Unified oscillators for Kama Audio
//!
//! This crate provides all types of oscillators:
//! - Audio oscillators (20Hz - 20kHz): Sine, Saw, Square, Triangle, Noise, FM
//! - Control oscillators (0.01Hz - 100Hz): LFO, Envelope, Random, Sample & Hold
//! - Sync generators: Clock, Trigger, Pulse

#![warn(missing_docs)]

pub mod audio;
pub mod control;
pub mod sync;

// Re-export common types for convenience
pub use audio::*;
pub use control::*;
pub use sync::*;
