//! Digital filters for Rill
//!
//! This crate provides digital filter implementations:
//! - Biquad filter (LowPass, HighPass, BandPass, Notch, Peak, LowShelf, HighShelf, AllPass)
//! - Moog ladder 4-pole lowpass filter with resonance

#![warn(missing_docs)]

pub mod biquad;
/// Moog ladder filter — 4-pole digital lowpass with resonance
pub mod moog_ladder;

// Re-export main types from rill-core-dsp
pub use biquad::{BiquadExt, BiquadFilter, BiquadProcessor};
pub use moog_ladder::MoogLadderProcessor;
pub use rill_core_dsp::filters::{Filter, FilterParams, FilterType, MoogLadder};

pub mod register;
