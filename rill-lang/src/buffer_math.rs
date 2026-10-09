//! # Element-wise in-place buffer math
//!
//! Scalar-loop implementations of element-wise operations over mutable signal
//! slices (`&mut [T]`). The loops are deliberately plain scalar code: LLVM
//! auto-vectorizes predictable contiguous loops in release builds (manual
//! SIMD attempts in this workspace measured −36%/−34%/−120%).

use rill_core::math::Transcendental;

/// `out[i] = out[i] + rhs[i]` for every element.
pub fn add_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o += r;
    }
}

/// `out[i] = out[i] - rhs[i]` for every element.
pub fn sub_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o -= r;
    }
}

/// `out[i] = out[i] * rhs[i]` for every element.
pub fn mul_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o *= r;
    }
}

/// `out[i] = out[i] / rhs[i]` for every element.
pub fn div_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o /= r;
    }
}

/// `out[i] = out[i] % rhs[i]` for every element.
pub fn rem_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for (o, &r) in out.iter_mut().zip(rhs.iter()) {
        *o = *o % r;
    }
}

/// `out[i] = -out[i]` for every element.
pub fn neg_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = T::ZERO - *o;
    }
}

/// `out[i] = sin(out[i])` for every element.
pub fn sin_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.sin();
    }
}

/// `out[i] = cos(out[i])` for every element.
pub fn cos_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.cos();
    }
}

/// `out[i] = tan(out[i])` for every element.
pub fn tan_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.tan();
    }
}

/// `out[i] = sqrt(out[i])` for every element.
pub fn sqrt_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.sqrt();
    }
}

/// `out[i] = exp(out[i])` for every element.
pub fn exp_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.exp();
    }
}

/// `out[i] = ln(out[i])` for every element.
pub fn ln_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.ln();
    }
}

/// `out[i] = tanh(out[i])` for every element.
pub fn tanh_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.tanh();
    }
}

/// `out[i] = abs(out[i])` for every element.
pub fn abs_into<T: Transcendental>(out: &mut [T]) {
    for o in out.iter_mut() {
        *o = o.abs();
    }
}

/// `out[i] = min(out[i], rhs[i])` for every element.
pub fn min_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for i in 0..out.len() {
        out[i] = out[i].min(rhs[i]);
    }
}

/// `out[i] = max(out[i], rhs[i])` for every element.
pub fn max_into<T: Transcendental>(out: &mut [T], rhs: &[T]) {
    assert_eq!(out.len(), rhs.len());
    for i in 0..out.len() {
        out[i] = out[i].max(rhs[i]);
    }
}

/// Fill every element of `out` with the scalar `v`.
pub fn fill_into<T: Transcendental>(out: &mut [T], v: T) {
    out.fill(v);
}
