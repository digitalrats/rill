//! # Векторные операции для DSP
//!
//! Этот модуль предоставляет встроенный предметно-ориентированный язык (eDSL) для векторных операций,
//! оптимизированных под SIMD инструкции.
//!
//! ## Основные возможности
//! - Базовые векторные типы для f32 и f64 с различной шириной SIMD
//! - Арифметические операции (+, -, *, /, %)
//! - Математические функции (sin, cos, exp, ln, sqrt, ...)
//! - Система выражений для ленивых вычислений и оптимизаций
//! - Автоматическая детекция SIMD возможностей процессора
//!
//! ## Использование
//! ```
//! use kama_core_dsp::vector::prelude::*;
//!
//! let a = Vector4::splat(1.0);
//! let b = Vector4::splat(2.0);
//! let c = a + b;
//! assert_eq!(c, Vector4::splat(3.0));
//! ```
//!
//! ## Архитектура
//! Модуль организован следующим образом:
//! - `traits` - основные трейты (`Vector`, `VectorOps`, `VectorMath`)
//! - `ops` - реализации арифметических операций
//! - `math` - математические функции
//! - `simd` - SIMD реализации для разных архитектур
//! - `expr` - система выражений и оптимизации
//! - `scalar` - скалярные fallback реализации
//!
//! ## Поддерживаемые платформы
//! - x86/x86_64: SSE2, SSE4.1, AVX, AVX2, AVX512 (через детекцию во время выполнения)
//! - ARM: NEON (AArch64)
//! - WebAssembly: SIMD128
//! - Скалярный fallback для платформ без SIMD

#![allow(unused_imports)]
#![allow(dead_code)]

pub mod traits;
pub mod ops;
pub mod math;
// pub mod expr;  // временно отключено из-за ошибок компиляции
pub mod scalar;
pub mod macros;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod simd;

#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
pub mod simd_arm;

#[cfg(target_arch = "wasm32")]
pub mod simd_wasm;

// Re-exports
pub use traits::*;
pub use ops::*;
pub use math::*;
// pub use expr::*;  // временно отключено
pub use scalar::*;
pub use macros::*;

/// Prelude для удобного импорта
pub mod prelude {
    pub use crate::vector::traits::*;
    pub use crate::vector::ops::*;
    pub use crate::vector::math::*;
    // pub use crate::vector::expr::*;  // временно отключено
    pub use crate::vector::scalar::*;
    pub use crate::vector::macros::*;
    
    // Типы векторов
    #[cfg(feature = "simd")]
    pub use crate::vector::simd::*;
    
    // Скалярные типы
    pub use crate::vector::scalar::{ScalarVector2, ScalarVector4, ScalarVector8};
}