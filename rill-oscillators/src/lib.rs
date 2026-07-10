//! Unified oscillators for Rill
//!
//! Signal-frequency generators (20 Hz – 20 kHz): Sine, Saw, Square,
//! Triangle, Noise, FM synthesis, and wavetable.

#![warn(missing_docs)]

pub mod signal;

// Re-export common types for convenience
pub use signal::*;

pub mod register;
