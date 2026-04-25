//! Prelude для удобного импорта векторных типов и трейтов.
//!
//! Использование:
//! ```
//! use rill_core_dsp::vector::prelude::*;
//! ```

pub use super::traits::{Vector, VectorMask, VectorReduce, VectorScalarOps};

// Реэкспорт скалярных векторных типов (всегда доступны)
pub use super::scalar::{ScalarVector1, ScalarVector2, ScalarVector4, ScalarVector8};

// Реэкспорт SIMD типов, если фича включена
#[cfg(feature = "simd")]
pub use super::simd::*;

// Реэкспорт операций
pub use super::ops::{
    add_scalar_slice, add_slices, div_slices, mul_scalar_slice, mul_slices, sub_slices,
};

// Реэкспорт математических функций
pub use super::math::{
    abs_slice, clamp_slice, cos_slice, exp_slice, ln_slice, max_slice, min_slice, sin_slice,
    sqrt_slice, tan_slice,
};

// Реэкспорт системы выражений
pub use super::expr::{vector_expr, BinaryExpr, ConstantExpr, LoadExpr, UnaryExpr, VectorExpr};
