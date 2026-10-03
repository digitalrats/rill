// rill-core-dsp/src/lib.rs
//! # Rill Core DSP
//!
//! Core DSP abstractions and algorithms for the Rill ecosystem.
//!
//! ## Modules
//! - `algorithm` — DSP algorithm trait (`Algorithm`, `ParameterizedAlgorithm`) and categories
//! - `context` — DSP context with sample rate, block size, and time info
//! - `filters` — Filter trait and types (Biquad coefficients, MoogLadder, etc.)
//! - `generators` — signal generators (WavetableOscillator, SamplePlayer, LFO, Envelope, Noise)
//! - `mapping` — control mapping strategies
//! - `math` — extra math utilities
//! - `smoothing` — parameter smoothing (ParamSmoother)
//! - `vector` — SIMD vector abstractions
//! - `macros` — macros for algorithm-to-node conversion and prelude
//!
//! ## Features
//! - Full type parameterization (f32/f64) via `Transcendental`
//! - RT-safe buffers with const generics
//! - SIMD acceleration behind `simd` feature flag

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod algorithm;
pub mod analyzer;
pub mod complex_mat;
pub mod context;
pub mod direct_conv;
pub mod effect;
pub mod filters;
pub mod generators;
pub mod mapping;
pub mod math;
pub mod smoothing;
pub mod vector;

#[macro_use]
pub mod macros;

// Re-exports
pub use algorithm::ParameterizedAlgorithm;
pub use context::DspContext;
pub use direct_conv::DirectConvolver;
pub use filters::{Filter, FilterParams, FilterType};
pub use generators::{
    BasicOscillator, EnvelopeGenerator, Generator, InterpolatedReader, LoopMode, NoiseGenerator,
    NoiseType, Resampler, SamplePlayer, Waveform, WavetableOscillator, LFO,
};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::algorithm::ParameterizedAlgorithm;
    pub use crate::complex_mat::{
        mul_complex, mul_complex_4, mul_complex_add, mul_complex_add_4, ComplexMat2, ComplexMat3,
    };
    pub use crate::context::DspContext;
    pub use crate::direct_conv::DirectConvolver;
    pub use crate::filters::{Filter, FilterParams, FilterType};
    pub use crate::generators::{
        EnvelopeGenerator, Generator, InterpolatedReader, LoopMode, NoiseGenerator, Resampler,
        SamplePlayer, WavetableOscillator, LFO,
    };
    pub use crate::mapping::{ControlMapper, MappingStrategy};
    pub use crate::math::*;
    pub use crate::smoothing::ParamSmoother;
    pub use crate::vector::prelude::*;
}
