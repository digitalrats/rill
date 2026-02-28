//! Числовые абстракции для аудиообработки

use std::fmt;
use std::ops::{Add, Sub, Mul, Div, Neg};

/// Маркерный трейт для числовых типов в аудиообработке
///
/// Позволяет писать обобщенный код, работающий и с f32, и с f64.
/// Включает базовые арифметические операции и конвертацию.
pub trait AudioNum:
    Copy +
    Clone +
    Send +
    Sync +
    'static +
    fmt::Debug +
    PartialEq +
    PartialOrd +
    Add<Output = Self> +
    Sub<Output = Self> +
    Mul<Output = Self> +
    Div<Output = Self> +
    Neg<Output = Self>
{
    /// Ноль
    const ZERO: Self;
    
    /// Единица
    const ONE: Self;
    
    /// Создать из f32
    fn from_f32(x: f32) -> Self;
    
    /// Конвертировать в f32
    fn as_f32(self) -> f32;
    
    /// Абсолютное значение
    fn abs(self) -> Self;
    
    /// Минимум
    fn min(self, other: Self) -> Self;
    
    /// Максимум
    fn max(self, other: Self) -> Self;
    
    /// Квадратный корень
    fn sqrt(self) -> Self;
    
    /// Экспонента
    fn exp(self) -> Self;
    
    /// Натуральный логарифм
    fn ln(self) -> Self;
    
    /// Синус
    fn sin(self) -> Self;
    
    /// Косинус
    fn cos(self) -> Self;
    
    /// Тангенс
    fn tan(self) -> Self;
    
    /// Степень
    fn powf(self, exp: Self) -> Self;
}

impl AudioNum for f32 {
    const ZERO: f32 = 0.0;
    const ONE: f32 = 1.0;
    
    #[inline(always)]
    fn from_f32(x: f32) -> Self { x }
    
    #[inline(always)]
    fn as_f32(self) -> f32 { self }
    
    #[inline(always)]
    fn abs(self) -> Self { self.abs() }
    
    #[inline(always)]
    fn min(self, other: Self) -> Self { self.min(other) }
    
    #[inline(always)]
    fn max(self, other: Self) -> Self { self.max(other) }
    
    #[inline(always)]
    fn sqrt(self) -> Self { self.sqrt() }
    
    #[inline(always)]
    fn exp(self) -> Self { self.exp() }
    
    #[inline(always)]
    fn ln(self) -> Self { self.ln() }
    
    #[inline(always)]
    fn sin(self) -> Self { self.sin() }
    
    #[inline(always)]
    fn cos(self) -> Self { self.cos() }
    
    #[inline(always)]
    fn tan(self) -> Self { self.tan() }
    
    #[inline(always)]
    fn powf(self, exp: Self) -> Self { self.powf(exp) }
}

impl AudioNum for f64 {
    const ZERO: f64 = 0.0;
    const ONE: f64 = 1.0;
    
    #[inline(always)]
    fn from_f32(x: f32) -> Self { x as f64 }
    
    #[inline(always)]
    fn as_f32(self) -> f32 { self as f32 }
    
    #[inline(always)]
    fn abs(self) -> Self { self.abs() }
    
    #[inline(always)]
    fn min(self, other: Self) -> Self { self.min(other) }
    
    #[inline(always)]
    fn max(self, other: Self) -> Self { self.max(other) }
    
    #[inline(always)]
    fn sqrt(self) -> Self { self.sqrt() }
    
    #[inline(always)]
    fn exp(self) -> Self { self.exp() }
    
    #[inline(always)]
    fn ln(self) -> Self { self.ln() }
    
    #[inline(always)]
    fn sin(self) -> Self { self.sin() }
    
    #[inline(always)]
    fn cos(self) -> Self { self.cos() }
    
    #[inline(always)]
    fn tan(self) -> Self { self.tan() }
    
    #[inline(always)]
    fn powf(self, exp: Self) -> Self { self.powf(exp) }
}