//! # Vector operations for DSP
//!
//! This module provides an embedded domain-specific language (eDSL) for vector
//! operations optimised with SIMD instructions.
//!
//! ## Features
//! - Basic vector types for f32 and f64 with various SIMD lane widths
//! - Arithmetic operations (+, -, *, /, %)
//! - Math functions (sin, cos, exp, ln, sqrt, ...)
//! - Automatic CPU SIMD capability detection
//!
//! ## Usage
//! ```
//! use rill_core::vector::prelude::*;
//!
//! let a = ScalarVector4::splat(1.0);
//! let b = ScalarVector4::splat(2.0);
//! let c = a + b;
//! assert_eq!(c, ScalarVector4::splat(3.0));
//! ```
//!
//! ## Architecture
//! - `traits` — core traits (`Vector`, `VectorOps`, `VectorMath`)
//! - `ops` — arithmetic operator implementations
//! - `math` — math function implementations
//! - `simd` — SIMD backends for different architectures
//! - `scalar` — scalar fallback implementations
//!
//! ## Supported platforms
//! - x86/x86_64: SSE2, SSE4.1, AVX, AVX2, AVX512 (runtime detection)
//! - ARM: NEON (AArch64)
//! - WebAssembly: SIMD128
//! - Scalar fallback for platforms without SIMD

#![allow(unused_imports)]
#![allow(dead_code)]

/// Vector construction macros.
pub mod macros;
/// Vector math functions (sin, cos, etc.).
pub mod math;
/// Arithmetic operator implementations for vectors.
pub mod ops;
/// Scalar fallback vector implementations.
pub mod scalar;
/// Core vector traits (`Vector`, `VectorTranscendental`, etc.).
pub mod traits;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
/// SIMD-accelerated vector implementations.
pub mod simd;

// Re-exports
pub use macros::*;
pub use math::*;
pub use ops::*;
pub use scalar::*;
pub use traits::*;

/// Convenience prelude for importing all vector types and traits.
pub mod prelude {
    pub use crate::math::vector::macros::*;
    pub use crate::math::vector::math::*;
    pub use crate::math::vector::ops::*;
    pub use crate::math::vector::scalar::*;
    pub use crate::math::vector::traits::*;

    /// SIMD vector types (conditionally available).
    #[cfg(feature = "simd")]
    pub use crate::math::vector::simd::*;

    /// Scalar (non-SIMD) vector types.
    pub use crate::math::vector::scalar::{
        ScalarVector1, ScalarVector2, ScalarVector4, ScalarVector8,
    };
}
