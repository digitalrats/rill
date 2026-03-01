// kama-core-dsp/src/lib.rs
//! # Kama Core DSP
//!
//! Ядро DSP-абстракций для Kama Audio.
//!
//! ## Особенности
//! - Полная параметризация типами (f32/f64) через `AudioNum`
//! - RT-safe буферы с const generics (стабильная фича)
//! - Базовые алгоритмы (Delay, Biquad, и т.д.)
//! - Макросы для генерации узлов

#![warn(missing_docs)]
#![deny(unsafe_code)]

// Для сложных const expr (опционально)
#![cfg_attr(feature = "unstable", feature(generic_const_exprs))]

pub mod algorithm;
pub mod filters;
pub mod context;
pub mod generators;
pub mod math;
pub mod vector;

#[macro_use]
pub mod macros;

// Re-exports
pub use algorithm::{Algorithm, ParameterizedAlgorithm, AlgorithmMetadata, AlgorithmCategory};
pub use filters::{Filter, FilterType, FilterParams};
pub use generators::{Generator, LFO, NoiseGenerator, EnvelopeGenerator};
pub use context::DspContext;

/// Prelude для удобного импорта
pub mod prelude {
    pub use crate::algorithm::{Algorithm, ParameterizedAlgorithm, AlgorithmMetadata, AlgorithmCategory};
    pub use crate::context::DspContext;
    pub use crate::filters::{Filter, FilterType, FilterParams};
    pub use crate::generators::{Generator, LFO, NoiseGenerator, EnvelopeGenerator};
    pub use crate::math::*;
    pub use crate::vector::prelude::*;
    pub use crate::macros::prelude::*;
}