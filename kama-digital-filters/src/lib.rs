//! Digital filters for Kama Audio
//!
//! This crate provides digital filter implementations:
//! - Biquad filter (LowPass, HighPass, BandPass, Notch, Peak, LowShelf, HighShelf, AllPass)
//! - More filters coming soon: OnePole, StateVariable, Comb, etc.

#![warn(missing_docs)]

pub mod biquad;

// Re-export main types from kama-core-dsp
pub use biquad::{BiquadFilter, BiquadProcessor, BiquadExt};
pub use kama_core_dsp::filters::{Filter, FilterParams, FilterType};

