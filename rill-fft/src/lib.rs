//! # Rill FFT
//!
//! Fast Fourier Transform and frequency-domain signal processing for the Rill ecosystem.
//!
//! ## Modules
//! - `complex_fft` — Radix-2 complex FFT (forward and inverse)
//! - `real_fft` — Real-valued FFT using complex FFT with packing
//!
//! ## Features
//! - Generic over `T: Transcendental` (f32, f64)
//! - RT-safe: all scratch buffers pre-allocated in constructors, zero allocations in `process()`
//! - SIMD acceleration behind `simd` feature flag (via `rill-core`)

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod complex_fft;
pub mod effects;
pub mod nodes;
pub mod overlap_add;
pub mod partitioned_conv;
pub mod real_fft;
pub mod spectrum;

/// Prelude for convenient imports.
pub mod prelude {
    pub use crate::complex_fft::ComplexFft;
    pub use crate::overlap_add::OverlapAddConvolver;
    pub use crate::partitioned_conv::PartitionedConvolver;
    pub use crate::real_fft::RealFft;
    pub use crate::spectrum::FftSpectrumAnalyzer;
}
