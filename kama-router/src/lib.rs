//! Audio routing, mixing, and equalization for Kama Audio
//!
//! This crate provides:
//! - Equalizers (graphic, parametric) that work with any filter implementation
//! - Mixer with multiple channels, aux sends, and master output
//! - Future: matrix router for flexible signal routing

#![warn(missing_docs)]

pub mod eq;
pub mod mixer;

// Re-export common types
pub use kama_core::traits::ParamValue;
pub use kama_core_dsp::filters::{Filter, FilterType};

// Re-export main types from eq module
pub use eq::{
    BandType, EqBand, GraphicEq, ParametricEq, 
    BiquadFactory, GraphicEqProcessor, ParametricEqProcessor, 
    log_spaced_frequencies, FilterFactory
};

// Re-export main types from mixer module
pub use mixer::{
    ChannelConfig, ChannelMode, ChannelState, 
    MixerNode, SendConfig, SendType
};