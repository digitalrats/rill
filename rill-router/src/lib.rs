//! Signal routing, mixing, and equalization for Rill
//!
//! This crate provides:
//! - Equalizers (graphic, parametric) that work with any filter implementation
//! - Mixer with multiple channels, aux sends, and master output
//! - Future: matrix router for flexible signal routing

#![warn(missing_docs)]

/// Dry/wet mix utility node.
pub mod dry_wet;
/// Equalizer modules (graphic and parametric).
pub mod eq;
/// Multi-channel signal mixer with aux sends.
pub mod mixer;

// Re-export common types
pub use rill_core::traits::ParamValue;
pub use rill_core_dsp::filters::{Filter, FilterType};

// Re-export main types from eq module
pub use eq::{
    log_spaced_frequencies, BandType, BiquadFactory, EqBand, FilterFactory, GraphicEq,
    GraphicEqProcessor, ParametricEq, ParametricEqProcessor,
};

// Re-export main types from mixer module
pub use mixer::{ChannelConfig, ChannelMode, ChannelState, MixerNode, SendConfig, SendType};

// Re-export main types from dry_wet module
pub use dry_wet::DryWetMix;

pub mod register;

/// rill-lang builtins for router types.
#[cfg(feature = "lang")]
mod lang;
