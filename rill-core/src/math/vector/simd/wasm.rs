//! # WebAssembly SIMD implementations
//!
//! Uses SIMD128 instructions for vector operations in WebAssembly.

#![cfg(target_arch = "wasm32")]
#![allow(unused_imports)]
#![allow(dead_code)]

use super::super::traits::*;
use crate::math::vector::traits::VectorTranscendental;
use crate::Transcendental;

// -----------------------------------------------------------------------------
// SIMD types

/// Vector of 4 f32 elements (wasm SIMD128)
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct F32x4([f32; 4]);

/// Vector of 2 f64 elements (wasm SIMD128)
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct F64x2([f64; 2]);

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
        let mut arr = [0.0; 4];
        for i in 0..4 {
            arr[i] = self.0[i] + other.0[i];
        }
        F32x4(arr)
    }

    fn sub(&self, other: &Self) -> Self {
        let mut arr = [0.0; 4];
        for i in 0..4 {
            arr[i] = self.0[i] - other.0[i];
        }
        F32x4(arr)
    }

    fn mul(&self, other: &Self) -> Self {
        let mut arr = [0.0; 4];
        for i in 0..4 {
            arr[i] = self.0[i] * other.0[i];
        }
        F32x4(arr)
    }

    fn div(&self, other: &Self) -> Self {
        let mut arr = [0.0; 4];
        for i in 0..4 {
            arr[i] = self.0[i] / other.0[i];
        }
        F32x4(arr)
    }

    fn rem(&self, other: &Self) -> Self {
        let mut arr = [0.0; 4];
        for i in 0..4 {
            arr[i] = self.0[i] % other.0[i];
        }
        F32x4(arr)
    }

    fn neg(&self) -> Self {
        let mut arr = [0.0; 4];
        for i in 0..4 {
            arr[i] = -self.0[i];
        }
        F32x4(arr)
    }

    fn abs(&self) -> Self {
        let mut arr = [0.0; 4];
        for i in 0..4 {
            arr[i] = self.0[i].abs();
        }
        F32x4(arr)
    }

    fn min(&self, other: &Self) -> Self {
        let mut arr = [0.0; 4];
        for i in 0..4 {
            arr[i] = self.0[i].min(other.0[i]);
        }
        F32x4(arr)
    }

    fn max(&self, other: &Self) -> Self {
        let mut arr = [0.0; 4];
        for i in 0..4 {
            arr[i] = self.0[i].max(other.0[i]);
        }
        F32x4(arr)
    }

    fn clamp(&self, min: &Self, max: &Self) -> Self {
        let mut arr = [0.0; 4];
        for i in 0..4 {
            arr[i] = self.0[i].clamp(min.0[i], max.0[i]);
        }
        F32x4(arr)
    }
}

impl VectorTranscendental<f32, 4> for F32x4 {
    fn sqrt(&self) -> Self {
        let mut a = [0.0; 4];
        for i in 0..4 {
            a[i] = self.0[i].sqrt();
        }
        F32x4(a)
    }
    fn exp(&self) -> Self {
        let mut a = [0.0; 4];
        for i in 0..4 {
            a[i] = self.0[i].exp();
        }
        F32x4(a)
    }
    fn ln(&self) -> Self {
        let mut a = [0.0; 4];
        for i in 0..4 {
            a[i] = self.0[i].ln();
        }
        F32x4(a)
    }
    fn sin(&self) -> Self {
        let mut a = [0.0; 4];
        for i in 0..4 {
            a[i] = self.0[i].sin();
        }
        F32x4(a)
    }
    fn cos(&self) -> Self {
        let mut a = [0.0; 4];
        for i in 0..4 {
            a[i] = self.0[i].cos();
        }
        F32x4(a)
    }
    fn tan(&self) -> Self {
        let mut a = [0.0; 4];
        for i in 0..4 {
            a[i] = self.0[i].tan();
        }
        F32x4(a)
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
    fn test_f32x4_basic() {
        let a = F32x4::splat(2.0);
        let b = F32x4::splat(3.0);
        let c = a.add(&b);
        assert_eq!(c.extract(0), 5.0);
    }
}
