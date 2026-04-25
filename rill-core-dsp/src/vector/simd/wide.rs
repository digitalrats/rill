//! Кроссплатформенные SIMD реализации через крейт `wide`
//!
//! Этот модуль предоставляет типы векторов, использующие библиотеку `wide`,
//! которая обеспечивает переносимые SIMD операции с fallback на скалярные реализации.
//!
//! Типы:
//! - `F32x4`, `F32x8` для `f32`
//! - `F64x2`, `F64x4` для `f64`

use rill_core::AudioNum;
use wide::{f32x4, f32x8, f64x2, f64x4};
use std::ops::{Add, Sub, Mul, Div, Rem, Neg};

use crate::vector::traits::Vector;

// -----------------------------------------------------------------------------
// Обёртки над типами wide для реализации трейта Vector
// -----------------------------------------------------------------------------

/// SIMD вектор из 4 элементов `f32`
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct F32x4(f32x4);

/// SIMD вектор из 8 элементов `f32`
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct F32x8(f32x8);

/// SIMD вектор из 2 элементов `f64`
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct F64x2(f64x2);

/// SIMD вектор из 4 элементов `f64`
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct F64x4(f64x4);

// -----------------------------------------------------------------------------
// Реализации Default
// -----------------------------------------------------------------------------

impl Default for F32x4 {
    fn default() -> Self {
        Self(f32x4::splat(0.0))
    }
}

impl Default for F32x8 {
    fn default() -> Self {
        Self(f32x8::splat(0.0))
    }
}

impl Default for F64x2 {
    fn default() -> Self {
        Self(f64x2::splat(0.0))
    }
}

impl Default for F64x4 {
    fn default() -> Self {
        Self(f64x4::splat(0.0))
    }
}

// -----------------------------------------------------------------------------
// Реализация Vector для F32x4
// -----------------------------------------------------------------------------

impl Vector<f32, 4> for F32x4 {
    fn splat(value: f32) -> Self {
        F32x4(f32x4::splat(value))
    }
    
    fn load(slice: &[f32]) -> Self {
        let mut arr = [0.0f32; 4];
        arr.copy_from_slice(&slice[0..4]);
        F32x4(f32x4::from(arr))
    }
    
    fn store(&self, slice: &mut [f32]) {
        let arr: [f32; 4] = self.0.into();
        slice[0..4].copy_from_slice(&arr);
    }
    
    fn extract(&self, index: usize) -> f32 {
        let arr: [f32; 4] = self.0.into();
        arr[index]
    }
    
    fn insert(&self, index: usize, value: f32) -> Self {
        let mut arr: [f32; 4] = self.0.into();
        arr[index] = value;
        F32x4(f32x4::from(arr))
    }
    
    fn add(&self, other: &Self) -> Self {
        F32x4(self.0 + other.0)
    }
    
    fn sub(&self, other: &Self) -> Self {
        F32x4(self.0 - other.0)
    }
    
    fn mul(&self, other: &Self) -> Self {
        F32x4(self.0 * other.0)
    }
    
    fn div(&self, other: &Self) -> Self {
        F32x4(self.0 / other.0)
    }
    
    fn rem(&self, other: &Self) -> Self {
        // wide не предоставляет операцию остатка, реализуем покомпонентно
        let a: [f32; 4] = self.0.into();
        let b: [f32; 4] = other.0.into();
        let mut arr = [0.0f32; 4];
        for i in 0..4 {
            arr[i] = a[i] % b[i];
        }
        F32x4(f32x4::from(arr))
    }
    
    fn neg(&self) -> Self {
        F32x4(-self.0)
    }
    
    fn abs(&self) -> Self {
        F32x4(self.0.abs())
    }
    
    fn min(&self, other: &Self) -> Self {
        F32x4(self.0.min(other.0))
    }
    
    fn max(&self, other: &Self) -> Self {
        F32x4(self.0.max(other.0))
    }
    
    fn clamp(&self, min: &Self, max: &Self) -> Self {
        // clamp = self.max(min).min(max)
        F32x4(self.0.max(min.0).min(max.0))
    }
    
    fn sqrt(&self) -> Self {
        F32x4(self.0.sqrt())
    }
    
    fn exp(&self) -> Self {
        F32x4(self.0.exp())
    }
    
    fn ln(&self) -> Self {
        F32x4(self.0.ln())
    }
    
    fn sin(&self) -> Self {
        F32x4(self.0.sin())
    }
    
    fn cos(&self) -> Self {
        F32x4(self.0.cos())
    }
    
    fn tan(&self) -> Self {
        F32x4(self.0.tan())
    }
}

// -----------------------------------------------------------------------------
// Реализация Vector для F32x8
// -----------------------------------------------------------------------------

impl Vector<f32, 8> for F32x8 {
    fn splat(value: f32) -> Self {
        F32x8(f32x8::splat(value))
    }
    
    fn load(slice: &[f32]) -> Self {
        let mut arr = [0.0f32; 8];
        arr.copy_from_slice(&slice[0..8]);
        F32x8(f32x8::from(arr))
    }
    
    fn store(&self, slice: &mut [f32]) {
        let arr: [f32; 8] = self.0.into();
        slice[0..8].copy_from_slice(&arr);
    }
    
    fn extract(&self, index: usize) -> f32 {
        let arr: [f32; 8] = self.0.into();
        arr[index]
    }
    
    fn insert(&self, index: usize, value: f32) -> Self {
        let mut arr: [f32; 8] = self.0.into();
        arr[index] = value;
        F32x8(f32x8::from(arr))
    }
    
    fn add(&self, other: &Self) -> Self {
        F32x8(self.0 + other.0)
    }
    
    fn sub(&self, other: &Self) -> Self {
        F32x8(self.0 - other.0)
    }
    
    fn mul(&self, other: &Self) -> Self {
        F32x8(self.0 * other.0)
    }
    
    fn div(&self, other: &Self) -> Self {
        F32x8(self.0 / other.0)
    }
    
    fn rem(&self, other: &Self) -> Self {
        let a: [f32; 8] = self.0.into();
        let b: [f32; 8] = other.0.into();
        let mut arr = [0.0f32; 8];
        for i in 0..8 {
            arr[i] = a[i] % b[i];
        }
        F32x8(f32x8::from(arr))
    }
    
    fn neg(&self) -> Self {
        F32x8(-self.0)
    }
    
    fn abs(&self) -> Self {
        F32x8(self.0.abs())
    }
    
    fn min(&self, other: &Self) -> Self {
        F32x8(self.0.min(other.0))
    }
    
    fn max(&self, other: &Self) -> Self {
        F32x8(self.0.max(other.0))
    }
    
    fn clamp(&self, min: &Self, max: &Self) -> Self {
        F32x8(self.0.max(min.0).min(max.0))
    }
    
    fn sqrt(&self) -> Self {
        F32x8(self.0.sqrt())
    }
    
    fn exp(&self) -> Self {
        F32x8(self.0.exp())
    }
    
    fn ln(&self) -> Self {
        F32x8(self.0.ln())
    }
    
    fn sin(&self) -> Self {
        F32x8(self.0.sin())
    }
    
    fn cos(&self) -> Self {
        F32x8(self.0.cos())
    }
    
    fn tan(&self) -> Self {
        F32x8(self.0.tan())
    }
}

// -----------------------------------------------------------------------------
// Реализация Vector для F64x2
// -----------------------------------------------------------------------------

impl Vector<f64, 2> for F64x2 {
    fn splat(value: f64) -> Self {
        F64x2(f64x2::splat(value))
    }
    
    fn load(slice: &[f64]) -> Self {
        let mut arr = [0.0f64; 2];
        arr.copy_from_slice(&slice[0..2]);
        F64x2(f64x2::from(arr))
    }
    
    fn store(&self, slice: &mut [f64]) {
        let arr: [f64; 2] = self.0.into();
        slice[0..2].copy_from_slice(&arr);
    }
    
    fn extract(&self, index: usize) -> f64 {
        let arr: [f64; 2] = self.0.into();
        arr[index]
    }
    
    fn insert(&self, index: usize, value: f64) -> Self {
        let mut arr: [f64; 2] = self.0.into();
        arr[index] = value;
        F64x2(f64x2::from(arr))
    }
    
    fn add(&self, other: &Self) -> Self {
        F64x2(self.0 + other.0)
    }
    
    fn sub(&self, other: &Self) -> Self {
        F64x2(self.0 - other.0)
    }
    
    fn mul(&self, other: &Self) -> Self {
        F64x2(self.0 * other.0)
    }
    
    fn div(&self, other: &Self) -> Self {
        F64x2(self.0 / other.0)
    }
    
    fn rem(&self, other: &Self) -> Self {
        let a: [f64; 2] = self.0.into();
        let b: [f64; 2] = other.0.into();
        let mut arr = [0.0f64; 2];
        for i in 0..2 {
            arr[i] = a[i] % b[i];
        }
        F64x2(f64x2::from(arr))
    }
    
    fn neg(&self) -> Self {
        F64x2(-self.0)
    }
    
    fn abs(&self) -> Self {
        F64x2(self.0.abs())
    }
    
    fn min(&self, other: &Self) -> Self {
        F64x2(self.0.min(other.0))
    }
    
    fn max(&self, other: &Self) -> Self {
        F64x2(self.0.max(other.0))
    }
    
    fn clamp(&self, min: &Self, max: &Self) -> Self {
        F64x2(self.0.max(min.0).min(max.0))
    }
    
    fn sqrt(&self) -> Self {
        F64x2(self.0.sqrt())
    }
    
    fn exp(&self) -> Self {
        F64x2(self.0.exp())
    }
    
    fn ln(&self) -> Self {
        F64x2(self.0.ln())
    }
    
    fn sin(&self) -> Self {
        F64x2(self.0.sin())
    }
    
    fn cos(&self) -> Self {
        F64x2(self.0.cos())
    }
    
    fn tan(&self) -> Self {
        F64x2(self.0.tan())
    }
}

// -----------------------------------------------------------------------------
// Реализация Vector для F64x4
// -----------------------------------------------------------------------------

impl Vector<f64, 4> for F64x4 {
    fn splat(value: f64) -> Self {
        F64x4(f64x4::splat(value))
    }
    
    fn load(slice: &[f64]) -> Self {
        let mut arr = [0.0f64; 4];
        arr.copy_from_slice(&slice[0..4]);
        F64x4(f64x4::from(arr))
    }
    
    fn store(&self, slice: &mut [f64]) {
        let arr: [f64; 4] = self.0.into();
        slice[0..4].copy_from_slice(&arr);
    }
    
    fn extract(&self, index: usize) -> f64 {
        let arr: [f64; 4] = self.0.into();
        arr[index]
    }
    
    fn insert(&self, index: usize, value: f64) -> Self {
        let mut arr: [f64; 4] = self.0.into();
        arr[index] = value;
        F64x4(f64x4::from(arr))
    }
    
    fn add(&self, other: &Self) -> Self {
        F64x4(self.0 + other.0)
    }
    
    fn sub(&self, other: &Self) -> Self {
        F64x4(self.0 - other.0)
    }
    
    fn mul(&self, other: &Self) -> Self {
        F64x4(self.0 * other.0)
    }
    
    fn div(&self, other: &Self) -> Self {
        F64x4(self.0 / other.0)
    }
    
    fn rem(&self, other: &Self) -> Self {
        let a: [f64; 4] = self.0.into();
        let b: [f64; 4] = other.0.into();
        let mut arr = [0.0f64; 4];
        for i in 0..4 {
            arr[i] = a[i] % b[i];
        }
        F64x4(f64x4::from(arr))
    }
    
    fn neg(&self) -> Self {
        F64x4(-self.0)
    }
    
    fn abs(&self) -> Self {
        F64x4(self.0.abs())
    }
    
    fn min(&self, other: &Self) -> Self {
        F64x4(self.0.min(other.0))
    }
    
    fn max(&self, other: &Self) -> Self {
        F64x4(self.0.max(other.0))
    }
    
    fn clamp(&self, min: &Self, max: &Self) -> Self {
        F64x4(self.0.max(min.0).min(max.0))
    }
    
    fn sqrt(&self) -> Self {
        F64x4(self.0.sqrt())
    }
    
    fn exp(&self) -> Self {
        F64x4(self.0.exp())
    }
    
    fn ln(&self) -> Self {
        F64x4(self.0.ln())
    }
    
    fn sin(&self) -> Self {
        F64x4(self.0.sin())
    }
    
    fn cos(&self) -> Self {
        F64x4(self.0.cos())
    }
    
    fn tan(&self) -> Self {
        F64x4(self.0.tan())
    }
}

// -----------------------------------------------------------------------------
// Реализации операторов (Add, Sub, Mul, Div, Rem, Neg)
// -----------------------------------------------------------------------------

impl Add for F32x4 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for F32x4 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl Mul for F32x4 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self(self.0 * rhs.0)
    }
}

impl Div for F32x4 {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Self(self.0 / rhs.0)
    }
}

impl Rem for F32x4 {
    type Output = Self;
    fn rem(self, rhs: Self) -> Self {
        let a: [f32; 4] = self.0.into();
        let b: [f32; 4] = rhs.0.into();
        let mut arr = [0.0f32; 4];
        for i in 0..4 {
            arr[i] = a[i] % b[i];
        }
        Self(f32x4::from(arr))
    }
}

impl Neg for F32x4 {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

// Аналогично для F32x8, F64x2, F64x4

impl Add for F32x8 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for F32x8 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl Mul for F32x8 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self(self.0 * rhs.0)
    }
}

impl Div for F32x8 {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Self(self.0 / rhs.0)
    }
}

impl Rem for F32x8 {
    type Output = Self;
    fn rem(self, rhs: Self) -> Self {
        let a: [f32; 8] = self.0.into();
        let b: [f32; 8] = rhs.0.into();
        let mut arr = [0.0f32; 8];
        for i in 0..8 {
            arr[i] = a[i] % b[i];
        }
        Self(f32x8::from(arr))
    }
}

impl Neg for F32x8 {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl Add for F64x2 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for F64x2 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl Mul for F64x2 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self(self.0 * rhs.0)
    }
}

impl Div for F64x2 {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Self(self.0 / rhs.0)
    }
}

impl Rem for F64x2 {
    type Output = Self;
    fn rem(self, rhs: Self) -> Self {
        let a: [f64; 2] = self.0.into();
        let b: [f64; 2] = rhs.0.into();
        let mut arr = [0.0f64; 2];
        for i in 0..2 {
            arr[i] = a[i] % b[i];
        }
        Self(f64x2::from(arr))
    }
}

impl Neg for F64x2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl Add for F64x4 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for F64x4 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

impl Mul for F64x4 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self(self.0 * rhs.0)
    }
}

impl Div for F64x4 {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Self(self.0 / rhs.0)
    }
}

impl Rem for F64x4 {
    type Output = Self;
    fn rem(self, rhs: Self) -> Self {
        let a: [f64; 4] = self.0.into();
        let b: [f64; 4] = rhs.0.into();
        let mut arr = [0.0f64; 4];
        for i in 0..4 {
            arr[i] = a[i] % b[i];
        }
        Self(f64x4::from(arr))
    }
}

impl Neg for F64x4 {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

// -----------------------------------------------------------------------------
// Unit tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_f32x4_basic() {
        let a = F32x4::load(&[1.0, 2.0, 3.0, 4.0]);
        let b = F32x4::load(&[5.0, 6.0, 7.0, 8.0]);
        
        let c = a + b;
        let mut arr = [0.0f32; 4];
        c.store(&mut arr);
        assert_eq!(arr, [6.0, 8.0, 10.0, 12.0]);
        
        let c = a * b;
        c.store(&mut arr);
        assert_eq!(arr, [5.0, 12.0, 21.0, 32.0]);
    }
    
    #[test]
    fn test_f32x4_math() {
        let a = F32x4::load(&[0.0, 0.5, 1.0, 2.0]);
        let sin_a = a.sin();
        let mut arr = [0.0f32; 4];
        sin_a.store(&mut arr);
        let expected = [0.0f32.sin(), 0.5f32.sin(), 1.0f32.sin(), 2.0f32.sin()];
        for i in 0..4 {
            assert!((arr[i] - expected[i]).abs() < 1e-5);
        }
    }
    
    #[test]
    fn test_f64x2_basic() {
        let a = F64x2::load(&[1.0, 2.0]);
        let b = F64x2::load(&[3.0, 4.0]);
        
        let c = a + b;
        let mut arr = [0.0f64; 2];
        c.store(&mut arr);
        assert_eq!(arr, [4.0, 6.0]);
    }
}