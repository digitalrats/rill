//! # x86/x86_64 SIMD implementations
//!
//! Uses SSE2, SSE4.1, AVX, AVX2 and AVX512 instructions for vector operations.

#![cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#![allow(unused_imports)]
#![allow(dead_code)]

use super::super::traits::*;
use crate::Transcendental;

// -----------------------------------------------------------------------------
// SIMD types

/// Vector of 4 f32 elements (SSE)
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct F32x4([f32; 4]);

/// Vector of 8 f32 elements (AVX)
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct F32x8([f32; 8]);

/// Vector of 16 f32 elements (AVX512)
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct F32x16([f32; 16]);

/// Vector of 2 f64 elements (SSE2)
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct F64x2([f64; 2]);

/// Vector of 4 f64 elements (AVX)
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct F64x4([f64; 4]);

/// Vector of 8 f64 elements (AVX512)
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct F64x8([f64; 8]);

// -----------------------------------------------------------------------------
// Vector implementation for F32x4
// -----------------------------------------------------------------------------

impl Vector<f32, 4> for F32x4 {
    fn splat(value: f32) -> Self {
        F32x4([value; 4])
    }

    fn load(slice: &[f32]) -> Self {
        let mut arr = [0.0; 4];
        arr.copy_from_slice(&slice[0..4]);
        F32x4(arr)
    }

    fn store(&self, slice: &mut [f32]) {
        slice[0..4].copy_from_slice(&self.0);
    }

    fn extract(&self, index: usize) -> f32 {
        self.0[index]
    }

    fn insert(&self, index: usize, value: f32) -> Self {
        let mut arr = self.0;
        arr[index] = value;
        F32x4(arr)
    }

    fn add(&self, other: &Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i] + other.0[i]))
    }

    fn sub(&self, other: &Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i] - other.0[i]))
    }

    fn mul(&self, other: &Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i] * other.0[i]))
    }

    fn div(&self, other: &Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i] / other.0[i]))
    }

    fn rem(&self, other: &Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i] % other.0[i]))
    }

    fn neg(&self) -> Self {
        F32x4(core::array::from_fn(|i| -self.0[i]))
    }

    fn abs(&self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i].abs()))
    }

    fn min(&self, other: &Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i].min(other.0[i])))
    }

    fn max(&self, other: &Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i].max(other.0[i])))
    }

    fn clamp(&self, min: &Self, max: &Self) -> Self {
        F32x4(core::array::from_fn(|i| {
            self.0[i].clamp(min.0[i], max.0[i])
        }))
    }
}

impl VectorTranscendental<f32, 4> for F32x4 {
    fn sqrt(&self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i].sqrt()))
    }
    fn exp(&self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i].exp()))
    }
    fn ln(&self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i].ln()))
    }
    fn sin(&self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i].sin()))
    }
    fn cos(&self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i].cos()))
    }
    fn tan(&self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i].tan()))
    }
}

// For now, implement remaining types as stubs (scalar versions)
// In the future, real SIMD instructions via core::arch::x86_64 will go here

use std::ops::{Add, Div, Mul, Neg, Rem, Sub};

impl Add for F32x4 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i] + rhs.0[i]))
    }
}

impl Sub for F32x4 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i] - rhs.0[i]))
    }
}

impl Mul for F32x4 {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i] * rhs.0[i]))
    }
}

impl Div for F32x4 {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i] / rhs.0[i]))
    }
}

impl Rem for F32x4 {
    type Output = Self;

    fn rem(self, rhs: Self) -> Self {
        F32x4(core::array::from_fn(|i| self.0[i] % rhs.0[i]))
    }
}

impl Neg for F32x4 {
    type Output = Self;

    fn neg(self) -> Self {
        F32x4(core::array::from_fn(|i| -self.0[i]))
    }
}

impl Default for F32x4 {
    fn default() -> Self {
        F32x4([0.0; 4])
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_f32x4_splat() {
        let v = F32x4::splat(2.5);
        assert_eq!(v.extract(0), 2.5);
        assert_eq!(v.extract(3), 2.5);
    }

    #[test]
    fn test_f32x4_add() {
        let a = F32x4::splat(1.0);
        let b = F32x4::splat(2.0);
        let c = a + b;
        assert_eq!(c.extract(0), 3.0);
    }

    #[test]
    fn test_f32x4_mul() {
        let a = F32x4::splat(3.0);
        let b = F32x4::splat(4.0);
        let c = a * b;
        assert_eq!(c.extract(0), 12.0);
    }

    #[test]
    fn test_f32x4_sin() {
        let a = F32x4::splat(0.0);
        let b = a.sin();
        assert_eq!(b.extract(0), 0.0);
    }
}
