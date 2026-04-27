use core::fmt;
use core::ops::{Add, Div, Mul, Neg, Rem, Sub};

use crate::Scalar;
use crate::Transcendental;

/// Основной трейт для векторных типов (базовые операции).
///
/// Параметризован типом элемента `T: Scalar` и шириной `N`.
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
    fn splat(value: T) -> Self;
    fn load(slice: &[T]) -> Self;
    fn store(&self, slice: &mut [T]);
    fn extract(&self, index: usize) -> T;
    fn insert(&self, index: usize, value: T) -> Self;

    fn add(&self, other: &Self) -> Self;
    fn sub(&self, other: &Self) -> Self;
    fn mul(&self, other: &Self) -> Self;
    fn div(&self, other: &Self) -> Self;
    fn rem(&self, other: &Self) -> Self;
    fn neg(&self) -> Self;

    fn abs(&self) -> Self;
    fn min(&self, other: &Self) -> Self;
    fn max(&self, other: &Self) -> Self;
    fn clamp(&self, min: &Self, max: &Self) -> Self;
}

/// Трейт для векторных типов с трансцендентными операциями.
///
/// Доступен только для `T: Transcendental` (f32, f64).
pub trait VectorTranscendental<T: Transcendental, const N: usize>: Vector<T, N> {
    fn sqrt(&self) -> Self;
    fn exp(&self) -> Self;
    fn ln(&self) -> Self;
    fn sin(&self) -> Self;
    fn cos(&self) -> Self;
    fn tan(&self) -> Self;
}

pub trait VectorScalarOps<T: Scalar, const N: usize> {
    fn add_scalar(&self, scalar: T) -> Self;
    fn sub_scalar(&self, scalar: T) -> Self;
    fn mul_scalar(&self, scalar: T) -> Self;
    fn div_scalar(&self, scalar: T) -> Self;
    fn rem_scalar(&self, scalar: T) -> Self;
}

pub trait VectorReduce<T: Scalar, const N: usize> {
    fn horizontal_sum(&self) -> T;
    fn horizontal_product(&self) -> T;
    fn horizontal_min(&self) -> T;
    fn horizontal_max(&self) -> T;
    fn horizontal_mean(&self) -> T;
}

pub trait VectorMask<T: Scalar, const N: usize> {
    type Mask;

    fn eq(&self, other: &Self) -> Self::Mask;
    fn ne(&self, other: &Self) -> Self::Mask;
    fn gt(&self, other: &Self) -> Self::Mask;
    fn ge(&self, other: &Self) -> Self::Mask;
    fn lt(&self, other: &Self) -> Self::Mask;
    fn le(&self, other: &Self) -> Self::Mask;
    fn select(&self, other: &Self, mask: Self::Mask) -> Self;
    fn all(mask: &Self::Mask) -> bool;
}
