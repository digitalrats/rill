//! # Скалярные реализации векторных операций
//!
//! Fallback реализации для платформ без SIMD поддержки или для отладки.

use super::traits::*;
use kama_core::AudioNum;

// -----------------------------------------------------------------------------
// Скалярные векторные типы
// -----------------------------------------------------------------------------

/// Скалярный вектор из 1 элемента
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScalarVector1<T: AudioNum>([T; 1]);

/// Скалярный вектор из 2 элементов
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScalarVector2<T: AudioNum>([T; 2]);

/// Скалярный вектор из 4 элементов
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScalarVector4<T: AudioNum>([T; 4]);

/// Скалярный вектор из 8 элементов
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScalarVector8<T: AudioNum>([T; 8]);

// -----------------------------------------------------------------------------
// Реализация Vector для ScalarVector4
// -----------------------------------------------------------------------------

impl<T: AudioNum> Vector<T, 4> for ScalarVector4<T> {
    fn splat(value: T) -> Self {
        ScalarVector4([value; 4])
    }

    fn load(slice: &[T]) -> Self {
        let mut arr = [T::ZERO; 4];
        arr.copy_from_slice(&slice[0..4]);
        ScalarVector4(arr)
    }

    fn store(&self, slice: &mut [T]) {
        slice[0..4].copy_from_slice(&self.0);
    }

    fn extract(&self, index: usize) -> T {
        self.0[index]
    }

    fn insert(&self, index: usize, value: T) -> Self {
        let mut arr = self.0;
        arr[index] = value;
        ScalarVector4(arr)
    }

    fn add(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i] + other.0[i];
        }
        ScalarVector4(arr)
    }

    fn sub(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i] - other.0[i];
        }
        ScalarVector4(arr)
    }

    fn mul(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i] * other.0[i];
        }
        ScalarVector4(arr)
    }

    fn div(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i] / other.0[i];
        }
        ScalarVector4(arr)
    }

    fn rem(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i] % other.0[i];
        }
        ScalarVector4(arr)
    }

    fn neg(&self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = -self.0[i];
        }
        ScalarVector4(arr)
    }

    fn abs(&self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i].abs();
        }
        ScalarVector4(arr)
    }

    fn min(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i].min(other.0[i]);
        }
        ScalarVector4(arr)
    }

    fn max(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i].max(other.0[i]);
        }
        ScalarVector4(arr)
    }

    fn clamp(&self, min: &Self, max: &Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i].clamp(min.0[i], max.0[i]);
        }
        ScalarVector4(arr)
    }

    fn sqrt(&self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i].sqrt();
        }
        ScalarVector4(arr)
    }

    fn exp(&self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i].exp();
        }
        ScalarVector4(arr)
    }

    fn ln(&self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i].ln();
        }
        ScalarVector4(arr)
    }

    fn sin(&self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i].sin();
        }
        ScalarVector4(arr)
    }

    fn cos(&self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i].cos();
        }
        ScalarVector4(arr)
    }

    fn tan(&self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i].tan();
        }
        ScalarVector4(arr)
    }
}

impl<T: AudioNum> Default for ScalarVector4<T> {
    fn default() -> Self {
        ScalarVector4([T::ZERO; 4])
    }
}

impl<T: AudioNum> Vector<T, 1> for ScalarVector1<T> {
    fn splat(value: T) -> Self {
        ScalarVector1([value; 1])
    }

    fn load(slice: &[T]) -> Self {
        let mut arr = [T::ZERO; 1];
        arr.copy_from_slice(&slice[0..1]);
        ScalarVector1(arr)
    }

    fn store(&self, slice: &mut [T]) {
        slice[0..1].copy_from_slice(&self.0);
    }

    fn extract(&self, index: usize) -> T {
        self.0[index]
    }

    fn insert(&self, index: usize, value: T) -> Self {
        let mut arr = self.0;
        arr[index] = value;
        ScalarVector1(arr)
    }

    fn add(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] + other.0[0];
        ScalarVector1(arr)
    }

    fn sub(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] - other.0[0];
        ScalarVector1(arr)
    }

    fn mul(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] * other.0[0];
        ScalarVector1(arr)
    }

    fn div(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] / other.0[0];
        ScalarVector1(arr)
    }

    fn rem(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] % other.0[0];
        ScalarVector1(arr)
    }

    fn neg(&self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = -self.0[0];
        ScalarVector1(arr)
    }

    fn abs(&self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0].abs();
        ScalarVector1(arr)
    }

    fn min(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0].min(other.0[0]);
        ScalarVector1(arr)
    }

    fn max(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0].max(other.0[0]);
        ScalarVector1(arr)
    }

    fn clamp(&self, min: &Self, max: &Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0].clamp(min.0[0], max.0[0]);
        ScalarVector1(arr)
    }

    fn sqrt(&self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0].sqrt();
        ScalarVector1(arr)
    }

    fn exp(&self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0].exp();
        ScalarVector1(arr)
    }

    fn ln(&self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0].ln();
        ScalarVector1(arr)
    }

    fn sin(&self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0].sin();
        ScalarVector1(arr)
    }

    fn cos(&self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0].cos();
        ScalarVector1(arr)
    }

    fn tan(&self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0].tan();
        ScalarVector1(arr)
    }
}

impl<T: AudioNum> Default for ScalarVector1<T> {
    fn default() -> Self {
        ScalarVector1([T::ZERO; 1])
    }
}

// Реализации для ScalarVector2 и ScalarVector8 аналогичны (опущены для краткости)

impl<T: AudioNum> Vector<T, 2> for ScalarVector2<T> {
    fn splat(value: T) -> Self {
        ScalarVector2([value; 2])
    }

    fn load(slice: &[T]) -> Self {
        let mut arr = [T::ZERO; 2];
        arr.copy_from_slice(&slice[0..2]);
        ScalarVector2(arr)
    }

    fn store(&self, slice: &mut [T]) {
        slice[0..2].copy_from_slice(&self.0);
    }

    fn extract(&self, index: usize) -> T {
        self.0[index]
    }

    fn insert(&self, index: usize, value: T) -> Self {
        let mut arr = self.0;
        arr[index] = value;
        ScalarVector2(arr)
    }

    fn add(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i] + other.0[i];
        }
        ScalarVector2(arr)
    }

    fn sub(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i] - other.0[i];
        }
        ScalarVector2(arr)
    }

    fn mul(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i] * other.0[i];
        }
        ScalarVector2(arr)
    }

    fn div(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i] / other.0[i];
        }
        ScalarVector2(arr)
    }

    fn rem(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i] % other.0[i];
        }
        ScalarVector2(arr)
    }

    fn neg(&self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = -self.0[i];
        }
        ScalarVector2(arr)
    }

    fn abs(&self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i].abs();
        }
        ScalarVector2(arr)
    }

    fn min(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i].min(other.0[i]);
        }
        ScalarVector2(arr)
    }

    fn max(&self, other: &Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i].max(other.0[i]);
        }
        ScalarVector2(arr)
    }

    fn clamp(&self, min: &Self, max: &Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i].clamp(min.0[i], max.0[i]);
        }
        ScalarVector2(arr)
    }

    fn sqrt(&self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i].sqrt();
        }
        ScalarVector2(arr)
    }

    fn exp(&self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i].exp();
        }
        ScalarVector2(arr)
    }

    fn ln(&self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i].ln();
        }
        ScalarVector2(arr)
    }

    fn sin(&self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i].sin();
        }
        ScalarVector2(arr)
    }

    fn cos(&self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i].cos();
        }
        ScalarVector2(arr)
    }

    fn tan(&self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i].tan();
        }
        ScalarVector2(arr)
    }
}

impl<T: AudioNum> Default for ScalarVector2<T> {
    fn default() -> Self {
        ScalarVector2([T::ZERO; 2])
    }
}

// -----------------------------------------------------------------------------
// Реализация операторов
// -----------------------------------------------------------------------------

use std::ops::{Add, Div, Mul, Neg, Rem, Sub};

impl<T: AudioNum> Add for ScalarVector4<T> {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i] + rhs.0[i];
        }
        ScalarVector4(arr)
    }
}

impl<T: AudioNum> Sub for ScalarVector4<T> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i] - rhs.0[i];
        }
        ScalarVector4(arr)
    }
}

impl<T: AudioNum> Mul for ScalarVector4<T> {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i] * rhs.0[i];
        }
        ScalarVector4(arr)
    }
}

impl<T: AudioNum> Div for ScalarVector4<T> {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i] / rhs.0[i];
        }
        ScalarVector4(arr)
    }
}

impl<T: AudioNum> Rem for ScalarVector4<T> {
    type Output = Self;

    fn rem(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = self.0[i] % rhs.0[i];
        }
        ScalarVector4(arr)
    }
}

impl<T: AudioNum> Neg for ScalarVector4<T> {
    type Output = Self;

    fn neg(self) -> Self {
        let mut arr = [T::ZERO; 4];
        for i in 0..4 {
            arr[i] = -self.0[i];
        }
        ScalarVector4(arr)
    }
}

impl<T: AudioNum> Add for ScalarVector2<T> {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i] + rhs.0[i];
        }
        ScalarVector2(arr)
    }
}

impl<T: AudioNum> Sub for ScalarVector2<T> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i] - rhs.0[i];
        }
        ScalarVector2(arr)
    }
}

impl<T: AudioNum> Mul for ScalarVector2<T> {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i] * rhs.0[i];
        }
        ScalarVector2(arr)
    }
}

impl<T: AudioNum> Div for ScalarVector2<T> {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i] / rhs.0[i];
        }
        ScalarVector2(arr)
    }
}

impl<T: AudioNum> Rem for ScalarVector2<T> {
    type Output = Self;

    fn rem(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = self.0[i] % rhs.0[i];
        }
        ScalarVector2(arr)
    }
}

impl<T: AudioNum> Neg for ScalarVector2<T> {
    type Output = Self;

    fn neg(self) -> Self {
        let mut arr = [T::ZERO; 2];
        for i in 0..2 {
            arr[i] = -self.0[i];
        }
        ScalarVector2(arr)
    }
}

impl<T: AudioNum> Add for ScalarVector1<T> {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] + rhs.0[0];
        ScalarVector1(arr)
    }
}

impl<T: AudioNum> Sub for ScalarVector1<T> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] - rhs.0[0];
        ScalarVector1(arr)
    }
}

impl<T: AudioNum> Mul for ScalarVector1<T> {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] * rhs.0[0];
        ScalarVector1(arr)
    }
}

impl<T: AudioNum> Div for ScalarVector1<T> {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] / rhs.0[0];
        ScalarVector1(arr)
    }
}

impl<T: AudioNum> Rem for ScalarVector1<T> {
    type Output = Self;

    fn rem(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] % rhs.0[0];
        ScalarVector1(arr)
    }
}

impl<T: AudioNum> Neg for ScalarVector1<T> {
    type Output = Self;

    fn neg(self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = -self.0[0];
        ScalarVector1(arr)
    }
}

// -----------------------------------------------------------------------------
// Операции со скалярами
// -----------------------------------------------------------------------------

impl<T: AudioNum> Mul<T> for ScalarVector4<T> {
    type Output = Self;

    fn mul(self, rhs: T) -> Self {
        self * Self::splat(rhs)
    }
}

impl<T: AudioNum> Div<T> for ScalarVector4<T> {
    type Output = Self;

    fn div(self, rhs: T) -> Self {
        self / Self::splat(rhs)
    }
}

impl<T: AudioNum> Add<T> for ScalarVector4<T> {
    type Output = Self;

    fn add(self, rhs: T) -> Self {
        self + Self::splat(rhs)
    }
}

impl<T: AudioNum> Sub<T> for ScalarVector4<T> {
    type Output = Self;

    fn sub(self, rhs: T) -> Self {
        self - Self::splat(rhs)
    }
}

impl<T: AudioNum> Rem<T> for ScalarVector4<T> {
    type Output = Self;

    fn rem(self, rhs: T) -> Self {
        self % Self::splat(rhs)
    }
}

impl<T: AudioNum> Mul<T> for ScalarVector2<T> {
    type Output = Self;

    fn mul(self, rhs: T) -> Self {
        self * Self::splat(rhs)
    }
}

impl<T: AudioNum> Div<T> for ScalarVector2<T> {
    type Output = Self;

    fn div(self, rhs: T) -> Self {
        self / Self::splat(rhs)
    }
}

impl<T: AudioNum> Add<T> for ScalarVector2<T> {
    type Output = Self;

    fn add(self, rhs: T) -> Self {
        self + Self::splat(rhs)
    }
}

impl<T: AudioNum> Sub<T> for ScalarVector2<T> {
    type Output = Self;

    fn sub(self, rhs: T) -> Self {
        self - Self::splat(rhs)
    }
}

impl<T: AudioNum> Rem<T> for ScalarVector2<T> {
    type Output = Self;

    fn rem(self, rhs: T) -> Self {
        self % Self::splat(rhs)
    }
}

impl<T: AudioNum> Mul<T> for ScalarVector1<T> {
    type Output = Self;

    fn mul(self, rhs: T) -> Self {
        self * Self::splat(rhs)
    }
}

impl<T: AudioNum> Div<T> for ScalarVector1<T> {
    type Output = Self;

    fn div(self, rhs: T) -> Self {
        self / Self::splat(rhs)
    }
}

impl<T: AudioNum> Add<T> for ScalarVector1<T> {
    type Output = Self;

    fn add(self, rhs: T) -> Self {
        self + Self::splat(rhs)
    }
}

impl<T: AudioNum> Sub<T> for ScalarVector1<T> {
    type Output = Self;

    fn sub(self, rhs: T) -> Self {
        self - Self::splat(rhs)
    }
}

impl<T: AudioNum> Rem<T> for ScalarVector1<T> {
    type Output = Self;

    fn rem(self, rhs: T) -> Self {
        self % Self::splat(rhs)
    }
}

// -----------------------------------------------------------------------------
// Тесты
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scalar_vector4_basic() {
        let a = ScalarVector4::<f32>::splat(2.0);
        let b = ScalarVector4::<f32>::splat(3.0);
        let c = a + b;
        assert_eq!(c.extract(0), 5.0);
        assert_eq!(c.extract(3), 5.0);
    }

    #[test]
    fn test_scalar_vector4_math() {
        let a = ScalarVector4::<f32>::splat(0.0);
        let b = a.sin();
        assert_eq!(b.extract(0), 0.0);

        let c = ScalarVector4::<f32>::splat(1.0);
        let d = c.sqrt();
        assert_eq!(d.extract(0), 1.0);
    }

    #[test]
    fn test_scalar_vector2() {
        let a = ScalarVector2::<f64>::splat(5.0);
        let b = ScalarVector2::<f64>::splat(2.0);
        let c = a * b;
        assert_eq!(c.extract(0), 10.0);
        assert_eq!(c.extract(1), 10.0);
    }
}
