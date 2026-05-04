//! # Vector operations for DSP
//!
//! This module provides an embedded domain-specific language (eDSL) for vector
//! operations optimised with SIMD instructions.
//!
//! ## Features
//! - Basic vector types for f32 and f64 with various SIMD lane widths
//! - Arithmetic operations (+, -, *, /, %)
//! - Math functions (sin, cos, exp, ln, sqrt, ...)
//! - Expression system for lazy evaluation and optimisations
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
//! - `expr` — expression system and optimisations
//! - `scalar` — scalar fallback implementations
//!
//! ## Supported platforms
//! - x86/x86_64: SSE2, SSE4.1, AVX, AVX2, AVX512 (runtime detection)
//! - ARM: NEON (AArch64)
//! - WebAssembly: SIMD128
//! - Scalar fallback for platforms without SIMD

#![allow(unused_imports)]
#![allow(dead_code)]

/// Vector math functions (sin, cos, etc.).
pub mod math;
/// Arithmetic operator implementations for vectors.
pub mod ops;
/// Core vector traits (`Vector`, `VectorTranscendental`, etc.).
pub mod traits;
// pub mod expr;  // temporarily disabled due to compilation errors
/// Vector construction macros.
pub mod macros;
/// Scalar fallback vector implementations.
pub mod scalar;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
/// SIMD-accelerated vector implementations.
pub mod simd;

// Re-exports
pub use math::*;
pub use ops::*;
pub use traits::*;
// pub use expr::*;
pub use macros::*;
pub use scalar::*;

/// Convenience prelude for importing all vector types and traits.
pub mod prelude {
    pub use crate::math::vector::math::*;
    pub use crate::math::vector::ops::*;
    pub use crate::math::vector::traits::*;
    // pub use crate::math::vector::expr::*;  // temporarily disabled
    pub use crate::math::vector::macros::*;
    pub use crate::math::vector::scalar::*;

    /// SIMD vector types (conditionally available).
    #[cfg(feature = "simd")]
    pub use crate::math::vector::simd::*;

    /// Scalar (non-SIMD) vector types.
    pub use crate::math::vector::scalar::{ScalarVector1, ScalarVector2, ScalarVector4, ScalarVector8};
}
