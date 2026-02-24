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

pub mod buffer;
pub mod algorithm;
pub mod filters;
pub mod context;
pub mod generators;
pub mod math;

//#[macro_use]
//pub mod macros;

// Re-exports
pub use math::AudioNum;
pub use buffer::{RingBuffer, FixedBuffer, DelayLine};
pub use algorithm::{Algorithm, ParameterizedAlgorithm, AlgorithmMetadata, AlgorithmCategory};
//pub use filters::{Filter, FilterType, FilterParams};
//pub use generators::{Generator, LFO, NoiseGenerator, EnvelopeGenerator};
pub use context::DspContext;

/// Prelude для удобного импорта
pub mod prelude {
    pub use crate::math::AudioNum;
    pub use crate::buffer::{RingBuffer, FixedBuffer, DelayLine};
    pub use crate::algorithm::Algorithm;
    pub use crate::context::DspContext;
    pub use crate::math::*;
    //pub use crate::macros::prelude::*;
}