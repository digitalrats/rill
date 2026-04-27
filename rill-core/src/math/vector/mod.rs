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
//! use rill_core::vector::prelude::*;
//!
//! let a = ScalarVector4::splat(1.0);
//! let b = ScalarVector4::splat(2.0);
//! let c = a + b;
//! assert_eq!(c, ScalarVector4::splat(3.0));
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

pub mod math;
pub mod ops;
pub mod traits;
// pub mod expr;  // временно отключено из-за ошибок компиляции
pub mod macros;
pub mod scalar;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod simd;

// Re-exports
pub use math::*;
pub use ops::*;
pub use traits::*;
// pub use expr::*;  // временно отключено
pub use macros::*;
pub use scalar::*;

/// Prelude для удобного импорта
pub mod prelude {
    pub use crate::math::vector::math::*;
    pub use crate::math::vector::ops::*;
    pub use crate::math::vector::traits::*;
    // pub use crate::math::vector::expr::*;  // временно отключено
    pub use crate::math::vector::macros::*;
    pub use crate::math::vector::scalar::*;

    // Типы векторов
    #[cfg(feature = "simd")]
    pub use crate::math::vector::simd::*;

    // Скалярные типы
    pub use crate::math::vector::scalar::{ScalarVector1, ScalarVector2, ScalarVector4, ScalarVector8};
}
