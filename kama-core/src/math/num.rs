//! # Математические абстракции для Kama Core
//!
//! Этот модуль предоставляет унифицированный интерфейс для работы с числами
//! с плавающей точкой (f32 и f64) в реальном времени.

use core::ops::{Add, Sub, Mul, Div, Rem, Neg};
use core::fmt;

/// Числовой тип для аудио с полной поддержкой арифметических операций
pub trait AudioNum:
    Copy + Clone + Send + Sync + 'static + Default + PartialOrd +
    Add<Output = Self> +
    Sub<Output = Self> +
    Mul<Output = Self> +
    Div<Output = Self> +
    Rem<Output = Self> +
    Neg<Output = Self> +
    fmt::Debug
{
    /// Нулевое значение
    const ZERO: Self;
    
    /// Единичное значение
    const ONE: Self;
    
    /// Минимальное значение (для клиппинга)
    const MIN: Self;
    
    /// Максимальное значение (для клиппинга)
    const MAX: Self;
    
    /// Преобразование в f32
    fn to_f32(self) -> f32;
    
    /// Преобразование из f32
    fn from_f32(value: f32) -> Self;
    
    /// Преобразование в f64
    fn to_f64(self) -> f64 {
        self.to_f32() as f64
    }
    
    /// Абсолютное значение
    fn abs(self) -> Self;
    
    /// Минимум
    fn min(self, other: Self) -> Self;
    
    /// Максимум
    fn max(self, other: Self) -> Self;
    
    /// Клиппинг
    fn clamp(self, min: Self, max: Self) -> Self;
    
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
}

// -----------------------------------------------------------------------------
// Реализация для f32
// -----------------------------------------------------------------------------

impl AudioNum for f32 {
    const ZERO: f32 = 0.0;
    const ONE: f32 = 1.0;
    const MIN: f32 = -1.0;
    const MAX: f32 = 1.0;
    
    #[inline(always)]
    fn to_f32(self) -> f32 { self }
    
    #[inline(always)]
    fn from_f32(value: f32) -> f32 { value }
    
    #[inline(always)]
    fn abs(self) -> f32 { self.abs() }
    
    #[inline(always)]
    fn min(self, other: f32) -> f32 { self.min(other) }
    
    #[inline(always)]
    fn max(self, other: f32) -> f32 { self.max(other) }
    
    #[inline(always)]
    fn clamp(self, min: f32, max: f32) -> f32 { self.clamp(min, max) }
    
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
// Реализация для f64
// -----------------------------------------------------------------------------

impl AudioNum for f64 {
    const ZERO: f64 = 0.0;
    const ONE: f64 = 1.0;
    const MIN: f64 = -1.0;
    const MAX: f64 = 1.0;
    
    #[inline(always)]
    fn to_f32(self) -> f32 { self as f32 }
    
    #[inline(always)]
    fn from_f32(value: f32) -> f64 { value as f64 }
    
    #[inline(always)]
    fn abs(self) -> f64 { self.abs() }
    
    #[inline(always)]
    fn min(self, other: f64) -> f64 { self.min(other) }
    
    #[inline(always)]
    fn max(self, other: f64) -> f64 { self.max(other) }
    
    #[inline(always)]
    fn clamp(self, min: f64, max: f64) -> f64 { self.clamp(min, max) }
    
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