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
pub mod context;
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
pub use algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm};
pub use context::DspContext;
pub use filters::{Filter, FilterParams, FilterType};
pub use generators::{
    EnvelopeGenerator, Generator, InterpolatedReader, LoopMode, NoiseGenerator, SamplePlayer,
    WavetableOscillator, LFO,
};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::algorithm::{
        Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm,
    };
    pub use crate::context::DspContext;
    pub use crate::filters::{Filter, FilterParams, FilterType};
    pub use crate::generators::{
        EnvelopeGenerator, Generator, InterpolatedReader, LoopMode, NoiseGenerator, SamplePlayer,
        WavetableOscillator, LFO,
    };
    pub use crate::mapping::{ControlMapper, MappingStrategy};
    pub use crate::math::*;
    pub use crate::smoothing::ParamSmoother;
    pub use crate::vector::prelude::*;
}
