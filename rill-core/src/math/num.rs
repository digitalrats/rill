use core::fmt;
use core::ops::*;

/// Базовый числовой трейт для любых скалярных типов.
///
/// Включает арифметику, min/max/clamp, abs.
/// Реализован для f32, f64 и всех целых типов (i8/i16/i32/i64, u8/u16/u32/u64).
pub trait Scalar:
    Copy
    + Clone
    + Send
    + Sync
    + 'static
    + Default
    + PartialOrd
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Rem<Output = Self>
    + Neg<Output = Self>
    + AddAssign
    + SubAssign
    + MulAssign
    + DivAssign
    + fmt::Debug
{
    const ZERO: Self;
    const ONE: Self;
    const MIN: Self;
    const MAX: Self;

    fn abs(self) -> Self;
    fn min(self, other: Self) -> Self;
    fn max(self, other: Self) -> Self;
    fn clamp(self, min: Self, max: Self) -> Self;
}

/// Трансцендентные операции (sin, cos, sqrt, exp, ln).
///
/// Расширяет `Scalar` функциями, доступными только для типов
/// с плавающей точкой (f32, f64).
pub trait Transcendental: Scalar {
    const PI: Self;

    fn to_f32(self) -> f32;
    fn from_f32(value: f32) -> Self;

    fn to_f64(self) -> f64 {
        self.to_f32() as f64
    }

    fn from_f64(value: f64) -> Self {
        Self::from_f32(value as f32)
    }

    fn sqrt(self) -> Self;
    fn exp(self) -> Self;
    fn ln(self) -> Self;
    fn sin(self) -> Self;
    fn cos(self) -> Self;
    fn tan(self) -> Self;
}

// -----------------------------------------------------------------------------
// Scalar — f32
// -----------------------------------------------------------------------------

impl Scalar for f32 {
    const ZERO: f32 = 0.0;
    const ONE: f32 = 1.0;
    const MIN: f32 = -1.0;
    const MAX: f32 = 1.0;

    #[inline(always)]
    fn abs(self) -> f32 { self.abs() }

    #[inline(always)]
    fn min(self, other: f32) -> f32 { self.min(other) }

    #[inline(always)]
    fn max(self, other: f32) -> f32 { self.max(other) }

    #[inline(always)]
    fn clamp(self, min: f32, max: f32) -> f32 { self.clamp(min, max) }
}

impl Transcendental for f32 {
    const PI: f32 = std::f32::consts::PI;

    #[inline(always)]
    fn to_f32(self) -> f32 { self }

    #[inline(always)]
    fn from_f32(value: f32) -> f32 { value }

    #[inline(always)]
    fn from_f64(value: f64) -> f32 { value as f32 }

    #[inline(always)]
    fn sqrt(self) -> f32 { self.sqrt() }

    #[inline(always)]
    fn exp(self) -> f32 { self.exp() }

    #[inline(always)]
    fn ln(self) -> f32 { self.ln() }

    #[inline(always)]
    fn sin(self) -> f32 { self.sin() }

    #[inline(always)]
    fn cos(self) -> f32 { self.cos() }

    #[inline(always)]
    fn tan(self) -> f32 { self.tan() }
}

// -----------------------------------------------------------------------------
// Scalar + Transcendental — f64
// -----------------------------------------------------------------------------

impl Scalar for f64 {
    const ZERO: f64 = 0.0;
    const ONE: f64 = 1.0;
    const MIN: f64 = -1.0;
    const MAX: f64 = 1.0;

    #[inline(always)]
    fn abs(self) -> f64 { self.abs() }

    #[inline(always)]
    fn min(self, other: f64) -> f64 { self.min(other) }

    #[inline(always)]
    fn max(self, other: f64) -> f64 { self.max(other) }

    #[inline(always)]
    fn clamp(self, min: f64, max: f64) -> f64 { self.clamp(min, max) }
}

impl Transcendental for f64 {
    const PI: f64 = std::f64::consts::PI;

    #[inline(always)]
    fn to_f32(self) -> f32 { self as f32 }

    #[inline(always)]
    fn from_f32(value: f32) -> f64 { value as f64 }

    #[inline(always)]
    fn from_f64(value: f64) -> f64 { value }

    #[inline(always)]
    fn sqrt(self) -> f64 { self.sqrt() }

    #[inline(always)]
    fn exp(self) -> f64 { self.exp() }

    #[inline(always)]
    fn ln(self) -> f64 { self.ln() }

    #[inline(always)]
    fn sin(self) -> f64 { self.sin() }

    #[inline(always)]
    fn cos(self) -> f64 { self.cos() }

    #[inline(always)]
    fn tan(self) -> f64 { self.tan() }
}

// -----------------------------------------------------------------------------
// Scalar — целые типы
// -----------------------------------------------------------------------------

macro_rules! impl_scalar_int {
    ($ty:ty, $zero:expr, $one:expr, $min:expr, $max:expr) => {
        impl Scalar for $ty {
            const ZERO: $ty = $zero;
            const ONE: $ty = $one;
            const MIN: $ty = $min;
            const MAX: $ty = $max;

            #[inline(always)]
            fn abs(self) -> $ty { if self >= 0 { self } else { -self } }

            #[inline(always)]
            fn min(self, other: $ty) -> $ty { core::cmp::Ord::min(self, other) }

            #[inline(always)]
            fn max(self, other: $ty) -> $ty { core::cmp::Ord::max(self, other) }

            #[inline(always)]
            fn clamp(self, lo: $ty, hi: $ty) -> $ty { core::cmp::Ord::clamp(self, lo, hi) }
        }
    };
}

impl_scalar_int!(i8, 0, 1, i8::MIN, i8::MAX);
impl_scalar_int!(i16, 0, 1, i16::MIN, i16::MAX);
impl_scalar_int!(i32, 0, 1, i32::MIN, i32::MAX);
impl_scalar_int!(i64, 0, 1, i64::MIN, i64::MAX);
