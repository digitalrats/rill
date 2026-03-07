//! Prelude для удобного импорта векторных типов и трейтов.
//!
//! Использование:
//! ```
//! use kama_core_dsp::vector::prelude::*;
//! ```

pub use super::traits::{
    Vector, VectorScalarOps, VectorReduce, VectorMask,
};

// Реэкспорт скалярных векторных типов (всегда доступны)
pub use super::scalar::{
    ScalarVector2, ScalarVector4, ScalarVector8,
};

// Реэкспорт SIMD типов, если фича включена
#[cfg(feature = "simd")]
pub use super::simd::*;

// Реэкспорт операций
pub use super::ops::{
    add_slices, sub_slices, mul_slices, div_slices,
    mul_scalar_slice, add_scalar_slice,
};

// Реэкспорт математических функций
pub use super::math::{
    sin_slice, cos_slice, tan_slice, exp_slice, ln_slice, sqrt_slice, abs_slice,
    min_slice, max_slice, clamp_slice,
};

// Реэкспорт системы выражений
pub use super::expr::{
    VectorExpr, ConstantExpr, LoadExpr, BinaryExpr, UnaryExpr,
    vector_expr,
};