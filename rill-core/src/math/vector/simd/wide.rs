//! Cross-platform SIMD implementations via the `wide` crate
//!
//! This module provides vector types using the `wide` library,
//! which provides portable SIMD operations with fallback to scalar implementations.
//!
//! Types:
//! - `F32x4`, `F32x8` for `f32`
//! - `F64x2`, `F64x4` for `f64`

use crate::Transcendental;
use std::ops::{Add, Div, Mul, Neg, Rem, Sub};
use wide::{f32x4, f32x8, f64x2, f64x4, CmpEq, CmpGe, CmpGt, CmpLe, CmpLt, CmpNe};

use crate::math::vector::traits::{Vector, VectorMask, VectorTranscendental};

// -----------------------------------------------------------------------------
// Wrappers around wide types for implementing the Vector trait
// -----------------------------------------------------------------------------

/// SIMD vector of 4 `f32` elements
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct F32x4(f32x4);

/// SIMD vector of 8 `f32` elements
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct F32x8(f32x8);

/// SIMD vector of 2 `f64` elements
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct F64x2(f64x2);

/// SIMD vector of 4 `f64` elements
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct F64x4(f64x4);

// -----------------------------------------------------------------------------
// Default implementations
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
// Vector implementation for F32x4
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
        // wide does not provide a remainder operation, implement component-wise
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
}

impl VectorTranscendental<f32, 4> for F32x4 {
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
// Vector implementation for F32x8
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
}

impl VectorTranscendental<f32, 8> for F32x8 {
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
// Vector implementation for F64x2
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
}

impl VectorTranscendental<f64, 2> for F64x2 {
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
// Vector implementation for F64x4
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
}

impl VectorTranscendental<f64, 4> for F64x4 {
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
// VectorMask implementation
// -----------------------------------------------------------------------------

impl VectorMask<f64, 4> for F64x4 {
    // In wide 0.7, comparison masks are the same type as the vector,
    // where -1.0 = true and 0.0 = false.
    type Mask = F64x4;

    fn eq(&self, other: &Self) -> F64x4 {
        F64x4(self.0.cmp_eq(other.0))
    }

    fn ne(&self, other: &Self) -> F64x4 {
        F64x4(self.0.cmp_ne(other.0))
    }

    fn gt(&self, other: &Self) -> F64x4 {
        F64x4(self.0.cmp_gt(other.0))
    }

    fn ge(&self, other: &Self) -> F64x4 {
        F64x4(self.0.cmp_ge(other.0))
    }

    fn lt(&self, other: &Self) -> F64x4 {
        F64x4(self.0.cmp_lt(other.0))
    }

    fn le(&self, other: &Self) -> F64x4 {
        F64x4(self.0.cmp_le(other.0))
    }

    fn select(&self, other: &Self, mask: F64x4) -> Self {
        // f64x4::blend(self=mask, t=true_vals, f=false_vals)
        // returns t where self != 0, f where self == 0
        F64x4(mask.0.blend(self.0, other.0))
    }

    fn all(mask: &F64x4) -> bool {
        // move_mask returns bit i = sign bit of lane i
        // For -1.0 (true), sign bit is 1; for 0.0 (false), sign bit is 0.
        mask.0.move_mask() == 0b1111
    }
}

// -----------------------------------------------------------------------------
// Operator implementations (Add, Sub, Mul, Div, Rem, Neg)
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

// Similarly for F32x8, F64x2, F64x4

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
    use crate::math::vector::traits::VectorMask;

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

    #[test]
    fn test_f64x4_basic() {
        let a = F64x4::load(&[1.0, 2.0, 3.0, 4.0]);
        let b = F64x4::load(&[5.0, 6.0, 7.0, 8.0]);

        let c = a + b;
        let mut arr = [0.0f64; 4];
        c.store(&mut arr);
        assert_eq!(arr, [6.0, 8.0, 10.0, 12.0]);

        let c = a * b;
        c.store(&mut arr);
        assert_eq!(arr, [5.0, 12.0, 21.0, 32.0]);
    }

    #[test]
    fn test_f64x4_math() {
        let a = F64x4::load(&[0.0, 0.5, 1.0, 2.0]);
        let sqrt_a = a.sqrt();
        let mut arr = [0.0f64; 4];
        sqrt_a.store(&mut arr);
        let expected = [0.0f64.sqrt(), 0.5f64.sqrt(), 1.0f64.sqrt(), 2.0f64.sqrt()];
        for i in 0..4 {
            assert!((arr[i] - expected[i]).abs() < 1e-12);
        }

        let exp_a = a.exp();
        exp_a.store(&mut arr);
        let expected = [0.0f64.exp(), 0.5f64.exp(), 1.0f64.exp(), 2.0f64.exp()];
        for i in 0..4 {
            assert!((arr[i] - expected[i]).abs() < 1e-12);
        }
    }

    #[test]
    fn test_f64x4_vector_mask_lt() {
        // wide 0.7 returns mask with from_bits(u64::MAX) = NaN for true, 0.0 for false
        // Use move_mask to check bits
        let a = F64x4::load(&[1.0, 2.0, 3.0, 4.0]);
        let b = F64x4::load(&[3.0, 3.0, 3.0, 3.0]);
        let mask = <F64x4 as VectorMask<f64, 4>>::lt(&a, &b);
        // move_mask extracts sign bit of each lane
        assert_eq!(mask.0.move_mask() & 0b1111, 0b0011); // lanes 0,1 true
    }

    #[test]
    fn test_f64x4_vector_mask_gt() {
        let a = F64x4::load(&[1.0, 2.0, 3.0, 4.0]);
        let b = F64x4::load(&[2.0, 2.0, 2.0, 2.0]);
        let mask = <F64x4 as VectorMask<f64, 4>>::gt(&a, &b);
        assert_eq!(mask.0.move_mask() & 0b1111, 0b1100); // lanes 2,3 true
    }

    #[test]
    fn test_f64x4_vector_mask_eq() {
        let a = F64x4::load(&[1.0, 2.0, 3.0, 4.0]);
        let b = F64x4::load(&[1.0, 0.0, 3.0, 5.0]);
        let mask = <F64x4 as VectorMask<f64, 4>>::eq(&a, &b);
        assert_eq!(mask.0.move_mask() & 0b1111, 0b0101); // lanes 0,2 true
    }

    #[test]
    fn test_f64x4_vector_mask_all() {
        let all_true = <F64x4 as VectorMask<f64, 4>>::lt(&F64x4::splat(1.0), &F64x4::splat(2.0));
        assert!(<F64x4 as VectorMask<f64, 4>>::all(&all_true));

        let partial_true = <F64x4 as VectorMask<f64, 4>>::lt(
            &F64x4::load(&[1.0, 2.0, 3.0, 4.0]),
            &F64x4::splat(3.0),
        );
        assert!(!<F64x4 as VectorMask<f64, 4>>::all(&partial_true));
    }

    #[test]
    fn test_f64x4_vector_mask_select() {
        let true_vals = F64x4::load(&[10.0, 20.0, 30.0, 40.0]);
        let false_vals = F64x4::load(&[1.0, 2.0, 3.0, 4.0]);
        // mask: true where true_vals < 25
        let threshold = F64x4::load(&[5.0, 25.0, 25.0, 25.0]);
        let mask = <F64x4 as VectorMask<f64, 4>>::lt(&true_vals, &threshold);
        let selected = <F64x4 as VectorMask<f64, 4>>::select(&true_vals, &false_vals, mask);
        // lanes 0 true (10 < 5? No — 10 < 5 false, so lane 0 is false)

        // Actually: a = [10, 20, 30, 40], threshold = [5, 25, 25, 25]
        // a < threshold: [false, true, false, false]
        assert_eq!(mask.0.move_mask() & 0b1111, 0b0010);
        // select: only lane 1 takes from true_vals (20)
        let mut arr = [0.0; 4];
        selected.store(&mut arr);
        assert!((arr[0] - 1.0).abs() < 1e-15);
        assert!((arr[1] - 20.0).abs() < 1e-15);
        assert!((arr[2] - 3.0).abs() < 1e-15);
        assert!((arr[3] - 4.0).abs() < 1e-15);
    }
}
