// rill-core-dsp/src/lib.rs
//! # Rill Core DSP
//!
//! Ядро DSP-абстракций для Rill.
//!
//! ## Особенности
//! - Полная параметризация типами (f32/f64) через `Transcendental`
//! - RT-safe буферы с const generics (стабильная фича)
//! - Базовые алгоритмы (Delay, Biquad, и т.д.)
//! - Макросы для генерации узлов

#![warn(missing_docs)]
#![deny(unsafe_code)]
// Для сложных const expr (опционально)
#![cfg_attr(feature = "unstable", feature(generic_const_exprs))]

pub mod algorithm;
pub mod context;
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
    EnvelopeGenerator, Generator, InterpolatedReader, LoopMode, NoiseGenerator,
    SamplePlayer, LFO, WavetableOscillator,
};

/// Prelude для удобного импорта
pub mod prelude {
    pub use crate::algorithm::{
        Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm,
    };
    pub use crate::context::DspContext;
    pub use crate::filters::{Filter, FilterParams, FilterType};
    pub use crate::generators::{
        EnvelopeGenerator, Generator, InterpolatedReader, LoopMode, NoiseGenerator,
        SamplePlayer, LFO, WavetableOscillator,
    };
    pub use crate::macros::prelude::*;
    pub use crate::mapping::{ControlMapper, MappingStrategy};
    pub use crate::math::*;
    pub use crate::smoothing::ParamSmoother;
    pub use crate::vector::prelude::*;
}
