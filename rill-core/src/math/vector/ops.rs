//! # Arithmetic operations for vectors
//!
//! Implementation of basic arithmetic operations for vector types.
//!
//! ## Types
//!
//! | Type | Purpose |
//! |---|---|
//! | `add_slices` / `sub_slices` / … | Low-level element-wise ops (write to pre-allocated buffer) |
//! | [`SliceMut`] | Mutable slice wrapper — supports `+=`, `-=`, `*=`, `/=` operator syntax |
//! | [`SlicePair`] | Two-slice pair — supports `.add_into()` / `.sub_into()` / `.mul_into()` / `.div_into()` |
//!
//! ## Usage
//!
//! ```rust,no_run
//! use rill_core::math::vector::ops::{SliceMut, SlicePair};
//! use rill_core::prelude::ScalarVector4;
//!
//! let a = [1.0f32, 2.0, 3.0, 4.0];
//! let b = [5.0f32, 6.0, 7.0, 8.0];
//! let mut out = [0.0f32; 4];
//!
//! // Pair ops: compute a + b → out
//! SlicePair::new(&a, &b).add_into::<4, ScalarVector4<f32>>(&mut out);
//!
//! // Accumulation: out += a (element-wise via += operator)
//! let mut out_mut = SliceMut::new(&mut out);
//! out_mut += &a as &[f32];
//!
//! // Scalar broadcast: out *= 2.0
//! out_mut *= 2.0;
//! ```

use super::traits::*;
use crate::Transcendental;
use std::ops::{AddAssign, DivAssign, MulAssign, SubAssign};

// -----------------------------------------------------------------------------
// Helper functions

/// Element-wise addition of two slices, storing the result in a third
pub fn add_slices<T: Transcendental, const N: usize, V>(a: &[T], b: &[T], out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), out.len());

    let chunks = a.len() / N;
    let remainder = a.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let b_vec = V::load(&b[start..start + N]);
        let result = a_vec + b_vec;
        result.store(&mut out[start..start + N]);
    }

    // Handle remainder
    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] + b[start + i];
        }
    }
}

/// Element-wise subtraction of two slices
pub fn sub_slices<T: Transcendental, const N: usize, V>(a: &[T], b: &[T], out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), out.len());

    let chunks = a.len() / N;
    let remainder = a.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let b_vec = V::load(&b[start..start + N]);
        let result = a_vec - b_vec;
        result.store(&mut out[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] - b[start + i];
        }
    }
}

/// Element-wise multiplication of two slices
pub fn mul_slices<T: Transcendental, const N: usize, V>(a: &[T], b: &[T], out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), out.len());

    let chunks = a.len() / N;
    let remainder = a.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let b_vec = V::load(&b[start..start + N]);
        let result = a_vec * b_vec;
        result.store(&mut out[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] * b[start + i];
        }
    }
}

/// Element-wise division of two slices
pub fn div_slices<T: Transcendental, const N: usize, V>(a: &[T], b: &[T], out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), out.len());

    let chunks = a.len() / N;
    let remainder = a.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let b_vec = V::load(&b[start..start + N]);
        let result = a_vec / b_vec;
        result.store(&mut out[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] / b[start + i];
        }
    }
}

/// Multiply a slice by a scalar
pub fn mul_scalar_slice<T: Transcendental, const N: usize, V>(a: &[T], scalar: T, out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), out.len());

    let scalar_vec = V::splat(scalar);
    let chunks = a.len() / N;
    let remainder = a.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let result = a_vec * scalar_vec;
        result.store(&mut out[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] * scalar;
        }
    }
}

/// Add a scalar to a slice
pub fn add_scalar_slice<T: Transcendental, const N: usize, V>(a: &[T], scalar: T, out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), out.len());

    let scalar_vec = V::splat(scalar);
    let chunks = a.len() / N;
    let remainder = a.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let result = a_vec + scalar_vec;
        result.store(&mut out[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] + scalar;
        }
    }
}

// ============================================================================
// SliceMut — mutable slice with operator-assign syntax (out += a, out *= s)
// ============================================================================

/// A mutable slice that supports element-wise `+=`, `-=`, `*=`, `/=` with
/// another slice or scalar broadcast.
///
/// Operators use the scalar path (no SIMD). For SIMD-accelerated ops, use
/// the `add_slices` / `sub_slices` / … helpers or [`SlicePair`].
#[derive(Debug)]
pub struct SliceMut<'a, T>(pub &'a mut [T]);

impl<'a, T: Transcendental> SliceMut<'a, T> {
    /// Wrap a mutable slice.
    #[inline(always)]
    pub fn new(slice: &'a mut [T]) -> Self {
        Self(slice)
    }

    /// Unwrap, returning the underlying mutable slice.
    #[inline(always)]
    pub fn into_inner(self) -> &'a mut [T] {
        self.0
    }
}

impl<'a, T: Transcendental> AddAssign<&[T]> for SliceMut<'a, T> {
    fn add_assign(&mut self, rhs: &[T]) {
        for (a, &b) in self.0.iter_mut().zip(rhs.iter()) {
            *a += b;
        }
    }
}

impl<'a, T: Transcendental> SubAssign<&[T]> for SliceMut<'a, T> {
    fn sub_assign(&mut self, rhs: &[T]) {
        for (a, &b) in self.0.iter_mut().zip(rhs.iter()) {
            *a -= b;
        }
    }
}

impl<'a, T: Transcendental> MulAssign<&[T]> for SliceMut<'a, T> {
    fn mul_assign(&mut self, rhs: &[T]) {
        for (a, &b) in self.0.iter_mut().zip(rhs.iter()) {
            *a *= b;
        }
    }
}

impl<'a, T: Transcendental> DivAssign<&[T]> for SliceMut<'a, T> {
    fn div_assign(&mut self, rhs: &[T]) {
        for (a, &b) in self.0.iter_mut().zip(rhs.iter()) {
            *a /= b;
        }
    }
}

impl<'a, T: Transcendental> AddAssign<T> for SliceMut<'a, T> {
    fn add_assign(&mut self, rhs: T) {
        for v in self.0.iter_mut() {
            *v = *v + rhs;
        }
    }
}

impl<'a, T: Transcendental> SubAssign<T> for SliceMut<'a, T> {
    fn sub_assign(&mut self, rhs: T) {
        for v in self.0.iter_mut() {
            *v = *v - rhs;
        }
    }
}

impl<'a, T: Transcendental> MulAssign<T> for SliceMut<'a, T> {
    fn mul_assign(&mut self, rhs: T) {
        for v in self.0.iter_mut() {
            *v = *v * rhs;
        }
    }
}

impl<'a, T: Transcendental> DivAssign<T> for SliceMut<'a, T> {
    fn div_assign(&mut self, rhs: T) {
        for v in self.0.iter_mut() {
            *v = *v / rhs;
        }
    }
}

// ============================================================================
// SlicePair — two-slice pair with SIMD-accelerated element-wise ops
// ============================================================================

/// A pair of immutable slices, providing SIMD-accelerated element-wise
/// binary operations that write the result into a pre-allocated output buffer.
///
/// # Example
///
/// ```rust,no_run
/// use rill_core::math::vector::ops::SlicePair;
/// use rill_core::prelude::ScalarVector4;
///
/// let a = [1.0f32, 2.0, 3.0, 4.0];
/// let b = [5.0f32, 6.0, 7.0, 8.0];
/// let mut out = [0.0f32; 4];
///
/// SlicePair::new(&a, &b).add_into::<4, ScalarVector4<f32>>(&mut out);
/// assert_eq!(out, [6.0, 8.0, 10.0, 12.0]);
/// ```
pub struct SlicePair<'a, T>(pub &'a [T], pub &'a [T]);

impl<'a, T: Transcendental> SlicePair<'a, T> {
    /// Create a new pair from two slices.
    #[inline(always)]
    pub fn new(a: &'a [T], b: &'a [T]) -> Self {
        Self(a, b)
    }

    /// Element-wise addition: `out[i] = a[i] + b[i]`.
    #[inline(always)]
    pub fn add_into<const N: usize, V: Vector<T, N>>(self, out: &mut [T]) {
        add_slices::<T, N, V>(self.0, self.1, out)
    }

    /// Element-wise subtraction: `out[i] = a[i] - b[i]`.
    #[inline(always)]
    pub fn sub_into<const N: usize, V: Vector<T, N>>(self, out: &mut [T]) {
        sub_slices::<T, N, V>(self.0, self.1, out)
    }

    /// Element-wise multiplication: `out[i] = a[i] * b[i]`.
    #[inline(always)]
    pub fn mul_into<const N: usize, V: Vector<T, N>>(self, out: &mut [T]) {
        mul_slices::<T, N, V>(self.0, self.1, out)
    }

    /// Element-wise division: `out[i] = a[i] / b[i]`.
    #[inline(always)]
    pub fn div_into<const N: usize, V: Vector<T, N>>(self, out: &mut [T]) {
        div_slices::<T, N, V>(self.0, self.1, out)
    }

    /// Element-wise remainder: `out[i] = a[i] % b[i]`.
    pub fn rem_into<const N: usize, V: Vector<T, N>>(self, out: &mut [T]) {
        for (o, (&a, &b)) in out.iter_mut().zip(self.0.iter().zip(self.1.iter())) {
            *o = a % b;
        }
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Test implementations for scalar vectors will be added later
    // #[test]
    // fn test_add_slices() {
    // }
}
