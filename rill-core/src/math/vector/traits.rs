use core::fmt;
use core::ops::{Add, Div, Mul, Neg, Rem, Sub};

use crate::Scalar;
use crate::Transcendental;

/// Core trait for vector types (basic operations).
///
/// Parameterised by element type `T: Scalar` and lane width `N`.
pub trait Vector<T: Scalar, const N: usize>:
    Copy
    + Clone
    + Send
    + Sync
    + 'static
    + Default
    + PartialEq
    + fmt::Debug
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Rem<Output = Self>
    + Neg<Output = Self>
{
    /// Construct a vector with all lanes set to the same value.
    fn splat(value: T) -> Self;
    /// Load a vector from a slice (panics if slice is too short).
    fn load(slice: &[T]) -> Self;
    /// Store the vector lanes into a slice.
    fn store(&self, slice: &mut [T]);
    /// Extract the value at the given lane index.
    fn extract(&self, index: usize) -> T;
    /// Return a new vector with the value at the given lane replaced.
    fn insert(&self, index: usize, value: T) -> Self;

    /// Lane-wise addition.
    fn add(&self, other: &Self) -> Self;
    /// Lane-wise subtraction.
    fn sub(&self, other: &Self) -> Self;
    /// Lane-wise multiplication.
    fn mul(&self, other: &Self) -> Self;
    /// Lane-wise division.
    fn div(&self, other: &Self) -> Self;
    /// Lane-wise remainder.
    fn rem(&self, other: &Self) -> Self;
    /// Lane-wise negation.
    fn neg(&self) -> Self;

    /// Lane-wise absolute value.
    fn abs(&self) -> Self;
    /// Lane-wise minimum.
    fn min(&self, other: &Self) -> Self;
    /// Lane-wise maximum.
    fn max(&self, other: &Self) -> Self;
    /// Lane-wise clamp to the inclusive range `[min, max]`.
    fn clamp(&self, min: &Self, max: &Self) -> Self;
}

/// Trait for vector types with transcendental operations.
///
/// Only available for `T: Transcendental` (f32, f64).
pub trait VectorTranscendental<T: Transcendental, const N: usize>: Vector<T, N> {
    /// Lane-wise square root.
    fn sqrt(&self) -> Self;
    /// Lane-wise exponential (e^x).
    fn exp(&self) -> Self;
    /// Lane-wise natural logarithm.
    fn ln(&self) -> Self;
    /// Lane-wise sine (input in radians).
    fn sin(&self) -> Self;
    /// Lane-wise cosine (input in radians).
    fn cos(&self) -> Self;
    /// Lane-wise tangent (input in radians).
    fn tan(&self) -> Self;
}

/// Scalar-vector arithmetic operations.
///
/// Each method broadcasts the scalar across all lanes.
pub trait VectorScalarOps<T: Scalar, const N: usize> {
    /// Add a scalar to every lane.
    fn add_scalar(&self, scalar: T) -> Self;
    /// Subtract a scalar from every lane.
    fn sub_scalar(&self, scalar: T) -> Self;
    /// Multiply every lane by a scalar.
    fn mul_scalar(&self, scalar: T) -> Self;
    /// Divide every lane by a scalar.
    fn div_scalar(&self, scalar: T) -> Self;
    /// Compute the remainder of every lane divided by a scalar.
    fn rem_scalar(&self, scalar: T) -> Self;
}

/// Horizontal reduction operations (vector → scalar).
pub trait VectorReduce<T: Scalar, const N: usize> {
    /// Sum of all lanes.
    fn horizontal_sum(&self) -> T;
    /// Product of all lanes.
    fn horizontal_product(&self) -> T;
    /// Minimum value across all lanes.
    fn horizontal_min(&self) -> T;
    /// Maximum value across all lanes.
    fn horizontal_max(&self) -> T;
    /// Arithmetic mean of all lanes.
    fn horizontal_mean(&self) -> T;
}

/// Vector comparison and masking operations.
///
/// Produces a bitmask (or SIMD mask) from lane-wise comparisons,
/// and allows selecting between two vectors based on a mask.
pub trait VectorMask<T: Scalar, const N: usize> {
    /// The mask type (e.g. `i32` bitmask or SIMD mask register).
    type Mask;

    /// Lane-wise equality comparison.
    fn eq(&self, other: &Self) -> Self::Mask;
    /// Lane-wise inequality comparison.
    fn ne(&self, other: &Self) -> Self::Mask;
    /// Lane-wise greater-than comparison.
    fn gt(&self, other: &Self) -> Self::Mask;
    /// Lane-wise greater-or-equal comparison.
    fn ge(&self, other: &Self) -> Self::Mask;
    /// Lane-wise less-than comparison.
    fn lt(&self, other: &Self) -> Self::Mask;
    /// Lane-wise less-or-equal comparison.
    fn le(&self, other: &Self) -> Self::Mask;
    /// Select lanes from `self` (where mask is truthy) or `other`.
    fn select(&self, other: &Self, mask: Self::Mask) -> Self;
    /// Returns true if all mask lanes are set.
    fn all(mask: &Self::Mask) -> bool;
}
