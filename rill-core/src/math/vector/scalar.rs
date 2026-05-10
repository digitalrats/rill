//! # Scalar implementations of vector operations
//!
//! Fallback implementations for platforms without SIMD support or for debugging.

use super::traits::*;
use crate::{Scalar, Transcendental};

// -----------------------------------------------------------------------------
// Scalar vector types

/// Scalar vector of 1 element
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScalarVector1<T: Scalar>([T; 1]);

/// Scalar vector of 2 elements
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScalarVector2<T: Scalar>([T; 2]);

/// Scalar vector of 4 elements
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScalarVector4<T: Scalar>([T; 4]);

impl<T: Scalar> ScalarVector4<T> {
    /// Construct a vector by applying a function to each lane index.
    pub fn from_fn<F: FnMut(usize) -> T>(f: F) -> Self {
        Self(core::array::from_fn(f))
    }
}

/// Scalar vector of 8 elements
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScalarVector8<T: Scalar>([T; 8]);

// -----------------------------------------------------------------------------
// Vector implementation for ScalarVector4
// -----------------------------------------------------------------------------

impl<T: Scalar> Vector<T, 4> for ScalarVector4<T> {
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
        ScalarVector4(core::array::from_fn(|i| self.0[i] + other.0[i]))
    }

    fn sub(&self, other: &Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i] - other.0[i]))
    }

    fn mul(&self, other: &Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i] * other.0[i]))
    }

    fn div(&self, other: &Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i] / other.0[i]))
    }

    fn rem(&self, other: &Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i] % other.0[i]))
    }

    fn neg(&self) -> Self {
        ScalarVector4(core::array::from_fn(|i| -self.0[i]))
    }

    fn abs(&self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i].abs()))
    }

    fn min(&self, other: &Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i].min(other.0[i])))
    }

    fn max(&self, other: &Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i].max(other.0[i])))
    }

    fn clamp(&self, min: &Self, max: &Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| {
            self.0[i].clamp(min.0[i], max.0[i])
        }))
    }
}

impl<T: Transcendental> VectorTranscendental<T, 4> for ScalarVector4<T> {
    fn sqrt(&self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i].sqrt()))
    }
    fn exp(&self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i].exp()))
    }
    fn ln(&self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i].ln()))
    }
    fn sin(&self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i].sin()))
    }
    fn cos(&self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i].cos()))
    }
    fn tan(&self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i].tan()))
    }
}

impl<T: Scalar + PartialEq> VectorMask<T, 4> for ScalarVector4<T> {
    type Mask = ScalarVector4<T>;

    fn eq(&self, other: &Self) -> Self::Mask {
        ScalarVector4(core::array::from_fn(|i| {
            if self.0[i] == other.0[i] {
                T::ONE
            } else {
                T::ZERO
            }
        }))
    }
    fn ne(&self, other: &Self) -> Self::Mask {
        ScalarVector4(core::array::from_fn(|i| {
            if self.0[i] != other.0[i] {
                T::ONE
            } else {
                T::ZERO
            }
        }))
    }
    fn gt(&self, other: &Self) -> Self::Mask {
        ScalarVector4(core::array::from_fn(|i| {
            if self.0[i] > other.0[i] {
                T::ONE
            } else {
                T::ZERO
            }
        }))
    }
    fn ge(&self, other: &Self) -> Self::Mask {
        ScalarVector4(core::array::from_fn(|i| {
            if self.0[i] >= other.0[i] {
                T::ONE
            } else {
                T::ZERO
            }
        }))
    }
    fn lt(&self, other: &Self) -> Self::Mask {
        ScalarVector4(core::array::from_fn(|i| {
            if self.0[i] < other.0[i] {
                T::ONE
            } else {
                T::ZERO
            }
        }))
    }
    fn le(&self, other: &Self) -> Self::Mask {
        ScalarVector4(core::array::from_fn(|i| {
            if self.0[i] <= other.0[i] {
                T::ONE
            } else {
                T::ZERO
            }
        }))
    }
    fn select(&self, other: &Self, mask: Self::Mask) -> Self {
        ScalarVector4(core::array::from_fn(|i| {
            if mask.0[i] != T::ZERO {
                self.0[i]
            } else {
                other.0[i]
            }
        }))
    }
    fn all(mask: &Self::Mask) -> bool {
        mask.0.iter().all(|&v| v != T::ZERO)
    }
}

impl<T: Scalar> Default for ScalarVector4<T> {
    fn default() -> Self {
        ScalarVector4([T::ZERO; 4])
    }
}

impl<T: Scalar> Vector<T, 1> for ScalarVector1<T> {
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
}

impl<T: Transcendental> VectorTranscendental<T, 1> for ScalarVector1<T> {
    fn sqrt(&self) -> Self {
        let mut a = [T::ZERO; 1];
        a[0] = self.0[0].sqrt();
        ScalarVector1(a)
    }
    fn exp(&self) -> Self {
        let mut a = [T::ZERO; 1];
        a[0] = self.0[0].exp();
        ScalarVector1(a)
    }
    fn ln(&self) -> Self {
        let mut a = [T::ZERO; 1];
        a[0] = self.0[0].ln();
        ScalarVector1(a)
    }
    fn sin(&self) -> Self {
        let mut a = [T::ZERO; 1];
        a[0] = self.0[0].sin();
        ScalarVector1(a)
    }
    fn cos(&self) -> Self {
        let mut a = [T::ZERO; 1];
        a[0] = self.0[0].cos();
        ScalarVector1(a)
    }
    fn tan(&self) -> Self {
        let mut a = [T::ZERO; 1];
        a[0] = self.0[0].tan();
        ScalarVector1(a)
    }
}

impl<T: Scalar> Default for ScalarVector1<T> {
    fn default() -> Self {
        ScalarVector1([T::ZERO; 1])
    }
}

// Implementations for ScalarVector2 and ScalarVector8 are similar (omitted for brevity)

impl<T: Scalar> Vector<T, 2> for ScalarVector2<T> {
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
        ScalarVector2(core::array::from_fn(|i| self.0[i] + other.0[i]))
    }

    fn sub(&self, other: &Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i] - other.0[i]))
    }

    fn mul(&self, other: &Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i] * other.0[i]))
    }

    fn div(&self, other: &Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i] / other.0[i]))
    }

    fn rem(&self, other: &Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i] % other.0[i]))
    }

    fn neg(&self) -> Self {
        ScalarVector2(core::array::from_fn(|i| -self.0[i]))
    }

    fn abs(&self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i].abs()))
    }

    fn min(&self, other: &Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i].min(other.0[i])))
    }

    fn max(&self, other: &Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i].max(other.0[i])))
    }

    fn clamp(&self, min: &Self, max: &Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| {
            self.0[i].clamp(min.0[i], max.0[i])
        }))
    }
}

impl<T: Transcendental> VectorTranscendental<T, 2> for ScalarVector2<T> {
    fn sqrt(&self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i].sqrt()))
    }
    fn exp(&self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i].exp()))
    }
    fn ln(&self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i].ln()))
    }
    fn sin(&self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i].sin()))
    }
    fn cos(&self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i].cos()))
    }
    fn tan(&self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i].tan()))
    }
}

impl<T: Scalar> Default for ScalarVector2<T> {
    fn default() -> Self {
        ScalarVector2([T::ZERO; 2])
    }
}

// -----------------------------------------------------------------------------
// Operator implementations
// -----------------------------------------------------------------------------

use std::ops::{Add, Div, Mul, Neg, Rem, Sub};

impl<T: Scalar> Add for ScalarVector4<T> {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i] + rhs.0[i]))
    }
}

impl<T: Scalar> Sub for ScalarVector4<T> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i] - rhs.0[i]))
    }
}

impl<T: Scalar> Mul for ScalarVector4<T> {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i] * rhs.0[i]))
    }
}

impl<T: Scalar> Div for ScalarVector4<T> {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i] / rhs.0[i]))
    }
}

impl<T: Scalar> Rem for ScalarVector4<T> {
    type Output = Self;

    fn rem(self, rhs: Self) -> Self {
        ScalarVector4(core::array::from_fn(|i| self.0[i] % rhs.0[i]))
    }
}

impl<T: Scalar> Neg for ScalarVector4<T> {
    type Output = Self;

    fn neg(self) -> Self {
        ScalarVector4(core::array::from_fn(|i| -self.0[i]))
    }
}

impl<T: Scalar> Add for ScalarVector2<T> {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i] + rhs.0[i]))
    }
}

impl<T: Scalar> Sub for ScalarVector2<T> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i] - rhs.0[i]))
    }
}

impl<T: Scalar> Mul for ScalarVector2<T> {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i] * rhs.0[i]))
    }
}

impl<T: Scalar> Div for ScalarVector2<T> {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i] / rhs.0[i]))
    }
}

impl<T: Scalar> Rem for ScalarVector2<T> {
    type Output = Self;

    fn rem(self, rhs: Self) -> Self {
        ScalarVector2(core::array::from_fn(|i| self.0[i] % rhs.0[i]))
    }
}

impl<T: Scalar> Neg for ScalarVector2<T> {
    type Output = Self;

    fn neg(self) -> Self {
        ScalarVector2(core::array::from_fn(|i| -self.0[i]))
    }
}

impl<T: Scalar> Add for ScalarVector1<T> {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] + rhs.0[0];
        ScalarVector1(arr)
    }
}

impl<T: Scalar> Sub for ScalarVector1<T> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] - rhs.0[0];
        ScalarVector1(arr)
    }
}

impl<T: Scalar> Mul for ScalarVector1<T> {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] * rhs.0[0];
        ScalarVector1(arr)
    }
}

impl<T: Scalar> Div for ScalarVector1<T> {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] / rhs.0[0];
        ScalarVector1(arr)
    }
}

impl<T: Scalar> Rem for ScalarVector1<T> {
    type Output = Self;

    fn rem(self, rhs: Self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = self.0[0] % rhs.0[0];
        ScalarVector1(arr)
    }
}

impl<T: Scalar> Neg for ScalarVector1<T> {
    type Output = Self;

    fn neg(self) -> Self {
        let mut arr = [T::ZERO; 1];
        arr[0] = -self.0[0];
        ScalarVector1(arr)
    }
}

// -----------------------------------------------------------------------------
// Scalar operations
// -----------------------------------------------------------------------------

impl<T: Scalar> Mul<T> for ScalarVector4<T> {
    type Output = Self;

    fn mul(self, rhs: T) -> Self {
        self * Self::splat(rhs)
    }
}

impl<T: Scalar> Div<T> for ScalarVector4<T> {
    type Output = Self;

    fn div(self, rhs: T) -> Self {
        self / Self::splat(rhs)
    }
}

impl<T: Scalar> Add<T> for ScalarVector4<T> {
    type Output = Self;

    fn add(self, rhs: T) -> Self {
        self + Self::splat(rhs)
    }
}

impl<T: Scalar> Sub<T> for ScalarVector4<T> {
    type Output = Self;

    fn sub(self, rhs: T) -> Self {
        self - Self::splat(rhs)
    }
}

impl<T: Scalar> Rem<T> for ScalarVector4<T> {
    type Output = Self;

    fn rem(self, rhs: T) -> Self {
        self % Self::splat(rhs)
    }
}

impl<T: Scalar> Mul<T> for ScalarVector2<T> {
    type Output = Self;

    fn mul(self, rhs: T) -> Self {
        self * Self::splat(rhs)
    }
}

impl<T: Scalar> Div<T> for ScalarVector2<T> {
    type Output = Self;

    fn div(self, rhs: T) -> Self {
        self / Self::splat(rhs)
    }
}

impl<T: Scalar> Add<T> for ScalarVector2<T> {
    type Output = Self;

    fn add(self, rhs: T) -> Self {
        self + Self::splat(rhs)
    }
}

impl<T: Scalar> Sub<T> for ScalarVector2<T> {
    type Output = Self;

    fn sub(self, rhs: T) -> Self {
        self - Self::splat(rhs)
    }
}

impl<T: Scalar> Rem<T> for ScalarVector2<T> {
    type Output = Self;

    fn rem(self, rhs: T) -> Self {
        self % Self::splat(rhs)
    }
}

impl<T: Scalar> Mul<T> for ScalarVector1<T> {
    type Output = Self;

    fn mul(self, rhs: T) -> Self {
        self * Self::splat(rhs)
    }
}

impl<T: Scalar> Div<T> for ScalarVector1<T> {
    type Output = Self;

    fn div(self, rhs: T) -> Self {
        self / Self::splat(rhs)
    }
}

impl<T: Scalar> Add<T> for ScalarVector1<T> {
    type Output = Self;

    fn add(self, rhs: T) -> Self {
        self + Self::splat(rhs)
    }
}

impl<T: Scalar> Sub<T> for ScalarVector1<T> {
    type Output = Self;

    fn sub(self, rhs: T) -> Self {
        self - Self::splat(rhs)
    }
}

impl<T: Scalar> Rem<T> for ScalarVector1<T> {
    type Output = Self;

    fn rem(self, rhs: T) -> Self {
        self % Self::splat(rhs)
    }
}

// -----------------------------------------------------------------------------
// Tests
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
