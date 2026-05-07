//! # Arithmetic operations for vectors
//!
//! Implementation of basic arithmetic operations for vector types.

use super::traits::*;
use crate::Transcendental;

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
