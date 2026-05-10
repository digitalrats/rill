//! # SIMD implementations for vector operations
//!
//! This module contains platform-dependent SIMD implementations of vector operations.
//!
//! ## CPU feature detection
//! The system automatically determines available SIMD instructions at runtime
//! and selects the optimal implementation.
//!
//! ## Supported architectures
//! - x86/x86_64: SSE2, SSE4.1, AVX, AVX2, AVX512
//! - ARM: NEON (AArch64)
//! - WebAssembly: SIMD128
//!
//! ## Usage
//! Users typically don't interact with this module directly,
//! but use the high-level abstractions from `vector::traits`.

#![allow(unused_imports)]
#![allow(dead_code)]

// Cross-platform SIMD implementation via the wide crate (requires simd feature)
#[cfg(feature = "simd")]
pub mod wide;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod x86;

#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
pub mod arm;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

/// SIMD capability detector for the CPU
pub struct SimdDetector {
    has_sse2: bool,
    has_sse4_1: bool,
    has_avx: bool,
    has_avx2: bool,
    has_avx512: bool,
    has_neon: bool,
    has_wasm_simd128: bool,
}

impl Default for SimdDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl SimdDetector {
    /// Creates a detector and determines the current CPU's capabilities.
    ///
    /// Uses `std::arch::is_x86_feature_detected!` on x86/x86_64
    /// and `std::arch::is_aarch64_feature_detected!` on AArch64.
    /// On other targets, all features are reported as unavailable.
    pub fn new() -> Self {
        Self {
            #[cfg(target_arch = "x86_64")]
            has_sse2: std::arch::is_x86_feature_detected!("sse2"),
            #[cfg(target_arch = "x86")]
            has_sse2: std::arch::is_x86_feature_detected!("sse2"),
            #[cfg(target_arch = "x86_64")]
            has_sse4_1: std::arch::is_x86_feature_detected!("sse4.1"),
            #[cfg(target_arch = "x86")]
            has_sse4_1: std::arch::is_x86_feature_detected!("sse4.1"),
            #[cfg(target_arch = "x86_64")]
            has_avx: std::arch::is_x86_feature_detected!("avx"),
            #[cfg(target_arch = "x86")]
            has_avx: std::arch::is_x86_feature_detected!("avx"),
            #[cfg(target_arch = "x86_64")]
            has_avx2: std::arch::is_x86_feature_detected!("avx2"),
            #[cfg(target_arch = "x86")]
            has_avx2: std::arch::is_x86_feature_detected!("avx2"),
            #[cfg(target_arch = "x86_64")]
            has_avx512: std::arch::is_x86_feature_detected!("avx512f"),
            #[cfg(target_arch = "x86")]
            has_avx512: std::arch::is_x86_feature_detected!("avx512f"),
            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
            has_sse2: false,
            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
            has_sse4_1: false,
            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
            has_avx: false,
            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
            has_avx2: false,
            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
            has_avx512: false,
            #[cfg(target_arch = "aarch64")]
            has_neon: std::arch::is_aarch64_feature_detected!("neon"),
            #[cfg(not(target_arch = "aarch64"))]
            has_neon: false,
            #[cfg(target_arch = "wasm32")]
            has_wasm_simd128: true,
            #[cfg(not(target_arch = "wasm32"))]
            has_wasm_simd128: false,
        }
    }

    /// Returns the maximum recommended SIMD width for the current platform.
    ///
    /// On x86_64: 8 if AVX available, 4 if SSE2 available.
    /// On AArch64: 4 if NEON available.
    /// On wasm32: 4.
    /// Fallback: 1 (scalar).
    pub fn recommended_simd_width<T: crate::Transcendental>() -> usize {
        let det = Self::new();
        if det.has_avx {
            return 8;
        }
        if det.has_sse2 || det.has_neon || det.has_wasm_simd128 {
            return 4;
        }
        1
    }
}

// Re-exports
#[cfg(feature = "simd")]
pub use wide::*;

// Platform-specific re-exports (not yet implemented)
// #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
// pub use x86::*;

// #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
// pub use arm::*;

// #[cfg(target_arch = "wasm32")]
// pub use wasm::*;
