// rill-fft/src/lib.rs
//! # Rill FFT
//!
//! Fast Fourier Transform and frequency-domain signal processing for the Rill ecosystem.
//!
//! ## Modules
//!
//! | Module | Key types | Purpose |
//! |---|---|---|
//! | `complex_fft` | `ComplexFft<T>` | Radix-2 DIT complex FFT (forward + inverse) |
//! | `real_fft` | `RealFft<T>` | Real-valued FFT via half-size complex packing |
//! | `overlap_add` | `OverlapAddConvolver<T, BUF>` | Frequency‑domain convolution (medium IRs) |
//! | `partitioned_conv` | `PartitionedConvolver<T, BUF>` | Partitioned convolution (long IRs) |
//! | `spectrum` | `FftSpectrumAnalyzer<T>` | FFT‑based spectrum analyser |
//! | `effects` | `SpectralGate`, `SpectralDelay` | Frequency‑domain effects |
//! | `nodes` | `ConvolverNode` | Graph‑node wrappers |
//!
//! ## RT safety
//!
//! All scratch buffers (twiddle tables, delay lines, overlap buffers) are
//! pre‑allocated in constructors. `process()` methods perform **zero heap
//! allocations** — verified by a custom panic‑on‑alloc tests in `tests/rt_safety.rs`.
//!
//! ## Performance (f32, x86_64, release profile)
//!
//! | Operation | Size | Time | Throughput |
//! |---|---|---|---|
//! | `ComplexFft::forward` | 1024 | 6.7 µs | 153 Melem/s |
//! | `RealFft::forward` | 1024 | 6.2 µs | 165 Melem/s |
//! | `ComplexFft::forward` | 16384 | 177 µs | 92 Melem/s |
//! | `OverlapAddConvolver` | IR 2048, BUF 128 | 61 µs/block | ~2100 blocks/s |
//! | `PartitionedConvolver` | IR 65536, BUF 128 | 104 µs/block | ~9600 blocks/s |
//! | `DirectConvolver` | 128 taps, BUF 128 | 10 µs/block | 12.7 Melem/s |
//!
//! ### f64 precision
//!
//! | Operation | Size | Time | Throughput |
//! |---|---|---|---|
//! | `ComplexFft::forward` | 1024 | 7.9 µs | 129 Melem/s |
//! | `ComplexFft::forward` | 4096 | 39.7 µs | 103 Melem/s |
//! | `ComplexFft::forward` | 8192 | 93.5 µs | 88 Melem/s |
//!
//! f64 is ~15–20 % slower than f32, consistent with double‑width memory and cache pressure.
//! 64‑bit transforms are still well within the real‑time budget for typical block sizes.
//!
//! At 44.1 kHz with block size 128 the per‑block budget is ~2.9 ms.
//! All operations fit comfortably within the real‑time budget.
//!
//! ## Examples
//!
//! ### Complex FFT
//!
//! ```rust,no_run
//! use rill_fft::complex_fft::ComplexFft;
//! use num_complex::Complex;
//!
//! let fft = ComplexFft::<f32>::new(1024);
//! let mut data: Vec<Complex<f32>> = (0..1024)
//!     .map(|i| Complex::new((i as f32 * 0.1).sin(), 0.0))
//!     .collect();
//!
//! fft.forward(&mut data);
//! // ... manipulate spectrum ...
//! fft.inverse(&mut data);
//! ```
//!
//! ### Convolution
//!
//! ```rust,no_run
//! use rill_fft::partitioned_conv::PartitionedConvolver;
//!
//! // IR length 16384 samples, BUF_SIZE = 128
//! let mut conv = PartitionedConvolver::<f32, 128>::new(16384);
//!
//! // Load impulse response (e.g., from a WAV file)
//! let ir: Vec<f32> = vec![0.0; 16384];
//! conv.set_ir(&ir);
//!
//! // Process audio blocks in the signal thread
//! let input = [0.5f32; 128];
//! let mut output = [0.0f32; 128];
//! conv.process(&input, &mut output);
//! ```
//!
//! ### Spectral gate
//!
//! ```rust,no_run
//! use rill_fft::effects::spectral_gate::SpectralGate;
//!
//! let mut gate = SpectralGate::<f32, 128>::new();
//! gate.set_threshold(0.01);
//! gate.set_ratio(0.0);  // hard gate below threshold
//!
//! let input = [0.5f32; 128];
//! let mut output = [0.0f32; 128];
//! gate.process(&input, &mut output);
//! ```
//!
//! ## Features
//! - Generic over `T: Transcendental` (f32, f64)
//! - SIMD acceleration behind `simd` feature flag (via `rill-core/wide`)
//! - `#![deny(unsafe_code)]` — pure safe Rust

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
    pub use crate::effects::{spectral_delay::SpectralDelay, spectral_gate::SpectralGate};
    pub use crate::nodes::convolver_node::ConvolverNode;
    pub use crate::overlap_add::OverlapAddConvolver;
    pub use crate::partitioned_conv::PartitionedConvolver;
    pub use crate::real_fft::RealFft;
    pub use crate::spectrum::FftSpectrumAnalyzer;
}

pub mod register;

pub mod lang;
