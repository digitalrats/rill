// rill-core/src/math/vector/complex.rs
//! Complex vector abstractions over the `Vector<T, 4>` eDSL.
//!
//! - `ComplexVector<T,V>` — 2 complex numbers in interleaved `[re0,im0,re1,im1]`
//! - `ComplexSoa<V>` — 4 complex numbers, separate re/im arrays. For FFT/convolution.

use core::marker::PhantomData;

use crate::math::vector::traits::{Vector, VectorMask};
use crate::Transcendental;

/// Two complex numbers: `[re0, im0, re1, im1]` in one 4‑lane vector.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ComplexVector<T: Transcendental, V: Vector<T, 4>> {
    data: V,
    _phantom: PhantomData<T>,
}

impl<T: Transcendental, V: Vector<T, 4>> ComplexVector<T, V> {
    /// Wrap a raw `Vector<T,4>` in `[re0, im0, re1, im1]` interleaved layout.
    pub fn from_raw(data: V) -> Self {
        Self {
            data,
            _phantom: PhantomData,
        }
    }

    /// Duplicate a single `(re, im)` pair into both lane pairs.
    pub fn splat_pair(re: T, im: T) -> Self {
        Self {
            data: V::load(&[re, im, re, im]),
            _phantom: PhantomData,
        }
    }

    /// Create from a single `(re, im)` pair, duplicated to both lane pairs.
    pub fn from_pair(c: (T, T)) -> Self {
        Self::splat_pair(c.0, c.1)
    }

    /// Create from two possibly-different complex pairs.
    /// Pairs go to lanes (0,1) and (2,3) respectively.
    pub fn from_two(c0: (T, T), c1: (T, T)) -> Self {
        Self {
            data: V::load(&[c0.0, c0.1, c1.0, c1.1]),
            _phantom: PhantomData,
        }
    }

    /// Return a reference to the raw `Vector<T,4>`.
    pub fn inner(&self) -> &V {
        &self.data
    }

    /// Extract the `i`-th lane from the underlying vector.
    pub fn extract(&self, i: usize) -> T {
        self.data.extract(i)
    }
    /// Real part of the first complex element (lane 0).
    pub fn re0(&self) -> T {
        self.data.extract(0)
    }
    /// Imaginary part of the first complex element (lane 1).
    pub fn im0(&self) -> T {
        self.data.extract(1)
    }

    /// Complex conjugate.
    pub fn conj(&self) -> Self {
        let v = V::load(&[
            self.data.extract(0),
            -self.data.extract(1),
            self.data.extract(2),
            -self.data.extract(3),
        ]);
        Self {
            data: v,
            _phantom: PhantomData,
        }
    }

    /// Complex multiplication: `self * other` (element-wise).
    pub fn cmul(&self, other: &Self) -> Self {
        let a_re = V::load(&[
            self.data.extract(0),
            self.data.extract(0),
            self.data.extract(2),
            self.data.extract(2),
        ]);
        let a_im = V::load(&[
            self.data.extract(1),
            self.data.extract(1),
            self.data.extract(3),
            self.data.extract(3),
        ]);
        let b_re = V::load(&[
            other.data.extract(0),
            other.data.extract(0),
            other.data.extract(2),
            other.data.extract(2),
        ]);
        let b_im = V::load(&[
            other.data.extract(1),
            other.data.extract(1),
            other.data.extract(3),
            other.data.extract(3),
        ]);
        let out_re = a_re * b_re - a_im * b_im;
        let out_im = a_re * b_im + a_im * b_re;
        Self {
            data: V::load(&[
                out_re.extract(0),
                out_im.extract(0),
                out_re.extract(2),
                out_im.extract(2),
            ]),
            _phantom: PhantomData,
        }
    }

    /// Complex addition: `self + other`.
    pub fn cadd(&self, other: &Self) -> Self {
        Self {
            data: self.data + other.data,
            _phantom: PhantomData,
        }
    }

    /// Complex subtraction: `self - other`.
    pub fn csub(&self, other: &Self) -> Self {
        Self {
            data: self.data - other.data,
            _phantom: PhantomData,
        }
    }

    /// Magnitude squared per complex element: `re² + im²` (lane‑pair summed).
    pub fn norm_sqr(&self) -> (T, T) {
        let r0 = self.data.extract(0);
        let i0 = self.data.extract(1);
        let r1 = self.data.extract(2);
        let i1 = self.data.extract(3);
        (r0 * r0 + i0 * i0, r1 * r1 + i1 * i1)
    }

    /// Multiply both real and imaginary parts by a real scalar.
    pub fn scale_real(&self, scalar: T) -> Self {
        Self {
            data: self.data * V::splat(scalar),
            _phantom: PhantomData,
        }
    }

    /// Extract the first complex value as `(re, im)`.
    pub fn to_complex0(&self) -> (T, T) {
        (self.data.extract(0), self.data.extract(1))
    }

    /// Extract the second complex value as `(re, im)`.
    pub fn to_complex1(&self) -> (T, T) {
        (self.data.extract(2), self.data.extract(3))
    }

    /// Apply a closure to each of the two complex elements.
    pub fn map_complex<F>(&self, f: F) -> Self
    where
        F: Fn((T, T)) -> (T, T),
    {
        let c0 = f(self.to_complex0());
        let c1 = f(self.to_complex1());
        Self::from_two(c0, c1)
    }

    /// Iterate over the two complex elements as `(re, im)` pairs.
    pub fn iter_complex(&self) -> impl Iterator<Item = (T, T)> {
        [self.to_complex0(), self.to_complex1()].into_iter()
    }
}

// ============================================================================
// Operator overloads — natural `a + b`, `a - b`, `a * b`, `-a` syntax
// ============================================================================

impl<T: Transcendental, V: Vector<T, 4>> core::ops::Add for ComplexVector<T, V> {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        self.cadd(&rhs)
    }
}

impl<T: Transcendental, V: Vector<T, 4>> core::ops::Sub for ComplexVector<T, V> {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self.csub(&rhs)
    }
}

impl<T: Transcendental, V: Vector<T, 4>> core::ops::Mul for ComplexVector<T, V> {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        self.cmul(&rhs)
    }
}

impl<T: Transcendental, V: Vector<T, 4>> core::ops::Neg for ComplexVector<T, V> {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            data: -self.data,
            _phantom: PhantomData,
        }
    }
}

/// `cv * t` — scale both complex elements by a real scalar.
impl<T: Transcendental, V: Vector<T, 4>> core::ops::Mul<T> for ComplexVector<T, V> {
    type Output = Self;
    fn mul(self, rhs: T) -> Self {
        self.scale_real(rhs)
    }
}

impl<T: Transcendental, V: Vector<T, 4>> core::ops::AddAssign for ComplexVector<T, V> {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl<T: Transcendental, V: Vector<T, 4>> core::ops::SubAssign for ComplexVector<T, V> {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl<T: Transcendental, V: Vector<T, 4>> core::ops::MulAssign for ComplexVector<T, V> {
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

impl<T: Transcendental, V: Vector<T, 4>> core::ops::DivAssign for ComplexVector<T, V> {
    fn div_assign(&mut self, rhs: Self) {
        *self = *self / rhs;
    }
}

/// Complex division: `(a+bi)/(c+di) = (ac+bd)/(c²+d²) + i*(bc-ad)/(c²+d²)`.
impl<T: Transcendental, V: Vector<T, 4>> core::ops::Div for ComplexVector<T, V> {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        let a_re = self.data.extract(0);
        let a_im = self.data.extract(1);
        let a2_re = self.data.extract(2);
        let a2_im = self.data.extract(3);
        let b_re = rhs.data.extract(0);
        let b_im = rhs.data.extract(1);
        let b2_re = rhs.data.extract(2);
        let b2_im = rhs.data.extract(3);
        let denom0 = b_re * b_re + b_im * b_im;
        let denom1 = b2_re * b2_re + b2_im * b2_im;
        Self {
            data: V::load(&[
                (a_re * b_re + a_im * b_im) / denom0,
                (a_im * b_re - a_re * b_im) / denom0,
                (a2_re * b2_re + a2_im * b2_im) / denom1,
                (a2_im * b2_re - a2_re * b2_im) / denom1,
            ]),
            _phantom: PhantomData,
        }
    }
}

/// Four complex numbers, separate re/im arrays. For SIMD‑heavy operations.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ComplexSoa<T: Transcendental, V: Vector<T, 4>> {
    /// Real parts of four complex numbers (one per lane).
    pub re: V,
    /// Imaginary parts of four complex numbers (one per lane).
    pub im: V,
    _phantom: PhantomData<T>,
}

impl<T: Transcendental, V: Vector<T, 4> + VectorMask<T, 4>> ComplexSoa<T, V> {
    /// Load four complex numbers from separate re/im slices.
    pub fn load(re_slice: &[T], im_slice: &[T]) -> Self {
        Self {
            re: V::load(re_slice),
            im: V::load(im_slice),
            _phantom: PhantomData,
        }
    }

    /// Create from four `(re, im)` pairs.
    pub fn from_pairs(p: [(T, T); 4]) -> Self {
        Self {
            re: V::load(&[p[0].0, p[1].0, p[2].0, p[3].0]),
            im: V::load(&[p[0].1, p[1].1, p[2].1, p[3].1]),
            _phantom: PhantomData,
        }
    }

    /// Store back to separate re/im slices.
    pub fn store(&self, re_slice: &mut [T], im_slice: &mut [T]) {
        self.re.store(re_slice);
        self.im.store(im_slice);
    }

    /// Extract a single complex value at lane `i`: returns `(re, im)`.
    pub fn extract_complex(&self, i: usize) -> (T, T) {
        (self.re.extract(i), self.im.extract(i))
    }

    /// Extract all four complex values as an array of `(re, im)` pairs.
    pub fn to_complexes(&self) -> [(T, T); 4] {
        [
            (self.re.extract(0), self.im.extract(0)),
            (self.re.extract(1), self.im.extract(1)),
            (self.re.extract(2), self.im.extract(2)),
            (self.re.extract(3), self.im.extract(3)),
        ]
    }

    /// Apply a closure to each of the four complex elements.
    pub fn map_complex<F>(&self, f: F) -> Self
    where
        F: Fn((T, T)) -> (T, T),
    {
        let c = self.to_complexes();
        Self::from_pairs([f(c[0]), f(c[1]), f(c[2]), f(c[3])])
    }

    /// Iterate over the four complex elements as `(re, im)` pairs.
    pub fn iter_complex(&self) -> impl Iterator<Item = (T, T)> {
        self.to_complexes().into_iter()
    }

    /// Complex multiplication: `self * other` on four numbers at once.
    pub fn cmul(&self, other: &Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
            _phantom: PhantomData,
        }
    }

    /// Complex multiply-accumulate: `self += a * b`.
    pub fn cmul_add(&mut self, a: &Self, b: &Self) {
        self.re = self.re + (a.re * b.re - a.im * b.im);
        self.im = self.im + (a.re * b.im + a.im * b.re);
    }

    /// Complex conjugate.
    pub fn conj(&self) -> Self {
        Self {
            re: self.re,
            im: -self.im,
            _phantom: PhantomData,
        }
    }

    /// Squared magnitude (re² + im²) per lane.
    pub fn norm_sqr(&self) -> V {
        self.re * self.re + self.im * self.im
    }

    /// True if all four norms are below `threshold_sq`.
    pub fn all_norm_sqr_lt(&self, threshold_sq: T) -> bool {
        let t = V::splat(threshold_sq);
        V::all(&self.norm_sqr().lt(&t))
    }

    /// Complex addition: `self + other`.
    pub fn cadd(&self, other: &Self) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
            _phantom: PhantomData,
        }
    }

    /// Complex subtraction: `self - other`.
    pub fn csub(&self, other: &Self) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
            _phantom: PhantomData,
        }
    }

    /// Multiply both re and im by a real scalar vector (per-lane).
    pub fn scale_real(&self, scalar: V) -> Self {
        Self {
            re: self.re * scalar,
            im: self.im * scalar,
            _phantom: PhantomData,
        }
    }
}

// ============================================================================
// Operator overloads for ComplexSoa — natural formula syntax
// ============================================================================

impl<T: Transcendental, V: Vector<T, 4> + VectorMask<T, 4>> core::ops::Add for ComplexSoa<T, V> {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        self.cadd(&rhs)
    }
}

impl<T: Transcendental, V: Vector<T, 4> + VectorMask<T, 4>> core::ops::Sub for ComplexSoa<T, V> {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self.csub(&rhs)
    }
}

impl<T: Transcendental, V: Vector<T, 4> + VectorMask<T, 4>> core::ops::Mul for ComplexSoa<T, V> {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        self.cmul(&rhs)
    }
}

impl<T: Transcendental, V: Vector<T, 4> + VectorMask<T, 4>> core::ops::Neg for ComplexSoa<T, V> {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            re: -self.re,
            im: -self.im,
            _phantom: PhantomData,
        }
    }
}

/// `csoa * sv` — scale all four complex numbers by a real vector (per‑lane).
impl<T: Transcendental, V: Vector<T, 4> + VectorMask<T, 4>> core::ops::Mul<V> for ComplexSoa<T, V> {
    type Output = Self;
    fn mul(self, rhs: V) -> Self {
        self.scale_real(rhs)
    }
}

impl<T: Transcendental, V: Vector<T, 4> + VectorMask<T, 4>> core::ops::AddAssign
    for ComplexSoa<T, V>
{
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl<T: Transcendental, V: Vector<T, 4> + VectorMask<T, 4>> core::ops::SubAssign
    for ComplexSoa<T, V>
{
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl<T: Transcendental, V: Vector<T, 4> + VectorMask<T, 4>> core::ops::MulAssign
    for ComplexSoa<T, V>
{
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}

impl<T: Transcendental, V: Vector<T, 4> + VectorMask<T, 4>> core::ops::DivAssign
    for ComplexSoa<T, V>
{
    fn div_assign(&mut self, rhs: Self) {
        *self = *self / rhs;
    }
}

/// Complex division (four at once): `(re+im·i)/(rre+rim·i)` per lane.
///
/// `denom = rre² + rim²`, then `re_out = (re·rre + im·rim)/denom`,
/// `im_out = (im·rre − re·rim)/denom`.
impl<T: Transcendental, V: Vector<T, 4> + VectorMask<T, 4>> core::ops::Div for ComplexSoa<T, V> {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        let denom = rhs.re * rhs.re + rhs.im * rhs.im;
        Self {
            re: (self.re * rhs.re + self.im * rhs.im) / denom,
            im: (self.im * rhs.re - self.re * rhs.im) / denom,
            _phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::vector::scalar::ScalarVector4;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn test_complex_vector_conj() {
        type CV = ComplexVector<f32, ScalarVector4<f32>>;
        let cv = CV::splat_pair(1.0, 2.0);
        assert!(approx_eq(cv.re0(), 1.0));
        assert!(approx_eq(cv.im0(), 2.0));
        let conj = cv.conj();
        assert!(approx_eq(conj.re0(), 1.0));
        assert!(approx_eq(conj.im0(), -2.0));
    }

    #[test]
    fn test_complex_vector_cmul() {
        type CV = ComplexVector<f32, ScalarVector4<f32>>;
        let a = CV::splat_pair(1.0, 2.0);
        let b = CV::splat_pair(3.0, 4.0);
        let prod = a.cmul(&b);
        assert!(approx_eq(prod.re0(), -5.0));
        assert!(approx_eq(prod.im0(), 10.0));
    }

    #[test]
    fn test_complex_soa_cmul() {
        type CS = ComplexSoa<f32, ScalarVector4<f32>>;
        let a = CS::load(&[1.0, 0.0, 0.5, -0.5], &[0.0, 1.0, 0.5, 0.5]);
        let b = CS::load(&[2.0, 0.0, 1.0, 1.0], &[3.0, 1.0, 1.0, -1.0]);
        let prod = a.cmul(&b);
        assert!(approx_eq(prod.re.extract(0), 2.0));
        assert!(approx_eq(prod.im.extract(0), 3.0));
        assert!(approx_eq(prod.re.extract(1), -1.0));
        assert!(approx_eq(prod.im.extract(1), 0.0));
    }

    #[test]
    fn test_complex_soa_conj_norm() {
        type CS = ComplexSoa<f32, ScalarVector4<f32>>;
        let a = CS::load(&[3.0, 1.0, 0.0, 2.0], &[4.0, 1.0, 1.0, 3.0]);
        let conj = a.conj();
        assert!(approx_eq(conj.im.extract(0), -4.0));
        let mag2 = a.norm_sqr();
        assert!(approx_eq(mag2.extract(0), 25.0));
        assert!(approx_eq(mag2.extract(1), 2.0));
        assert!(approx_eq(mag2.extract(2), 1.0));
        assert!(approx_eq(mag2.extract(3), 13.0));
    }

    #[test]
    fn test_complex_soa_cmul_add() {
        type CS = ComplexSoa<f32, ScalarVector4<f32>>;
        let a = CS::load(&[1.0, 2.0, 0.0, 4.0], &[0.0, 0.0, 1.0, 0.0]);
        let b = CS::load(&[2.0, 3.0, 0.0, 0.0], &[3.0, 0.0, 1.0, 0.0]);
        let mut acc = CS::load(&[0.0; 4], &[0.0; 4]);
        acc.cmul_add(&a, &b);
        assert!(approx_eq(acc.re.extract(0), 2.0));
        assert!(approx_eq(acc.im.extract(0), 3.0));
        assert!(approx_eq(acc.re.extract(1), 6.0));
        assert!(approx_eq(acc.im.extract(1), 0.0));
    }

    #[test]
    fn test_from_pair() {
        type CV = ComplexVector<f32, ScalarVector4<f32>>;
        let cv = CV::from_pair((1.0, 2.0));
        assert!(approx_eq(cv.to_complex0().0, 1.0));
        assert!(approx_eq(cv.to_complex0().1, 2.0));
        assert!(approx_eq(cv.to_complex1().0, 1.0));
        assert!(approx_eq(cv.to_complex1().1, 2.0));
    }

    #[test]
    fn test_from_pairs() {
        type CS = ComplexSoa<f32, ScalarVector4<f32>>;
        let soa = CS::from_pairs([(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)]);
        let c = soa.to_complexes();
        assert!(approx_eq(c[0].0, 1.0));
        assert!(approx_eq(c[0].1, 0.0));
        assert!(approx_eq(c[1].0, 0.0));
        assert!(approx_eq(c[1].1, 1.0));
        assert!(approx_eq(c[2].0, -1.0));
        assert!(approx_eq(c[2].1, 0.0));
        assert!(approx_eq(c[3].0, 0.0));
        assert!(approx_eq(c[3].1, -1.0));
    }

    #[test]
    fn test_map_complex_soa() {
        type CS = ComplexSoa<f32, ScalarVector4<f32>>;
        let soa = CS::from_pairs([(1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0)]);
        // Scale each element by 2
        let scaled = soa.map_complex(|(re, im)| (re * 2.0, im * 2.0));
        let c = scaled.to_complexes();
        assert!(approx_eq(c[0].0, 2.0));
        assert!(approx_eq(c[1].0, 4.0));
        assert!(approx_eq(c[2].0, 6.0));
        assert!(approx_eq(c[3].0, 8.0));
    }

    #[test]
    fn test_map_complex_vector() {
        type CV = ComplexVector<f32, ScalarVector4<f32>>;
        let cv = CV::from_two((1.0, 2.0), (3.0, 4.0));
        // Negate both
        let neg = cv.map_complex(|(re, im)| (-re, -im));
        assert!(approx_eq(neg.to_complex0().0, -1.0));
        assert!(approx_eq(neg.to_complex0().1, -2.0));
        assert!(approx_eq(neg.to_complex1().0, -3.0));
        assert!(approx_eq(neg.to_complex1().1, -4.0));
    }

    #[test]
    fn test_complex_vector_csub() {
        type CV = ComplexVector<f32, ScalarVector4<f32>>;
        let a = CV::from_two((3.0, 5.0), (2.0, 1.0));
        let b = CV::from_two((1.0, 2.0), (1.0, 0.0));
        let diff = a.csub(&b);
        assert!(approx_eq(diff.to_complex0().0, 2.0));
        assert!(approx_eq(diff.to_complex0().1, 3.0));
        assert!(approx_eq(diff.to_complex1().0, 1.0));
        assert!(approx_eq(diff.to_complex1().1, 1.0));
    }

    #[test]
    fn test_complex_vector_norm_sqr() {
        type CV = ComplexVector<f32, ScalarVector4<f32>>;
        let cv = CV::from_two((3.0, 4.0), (1.0, 1.0));
        let (n0, n1) = cv.norm_sqr();
        assert!(approx_eq(n0, 25.0)); // 3²+4²
        assert!(approx_eq(n1, 2.0)); // 1²+1²
    }

    #[test]
    fn test_operators_complex_vector() {
        type CV = ComplexVector<f32, ScalarVector4<f32>>;
        let a = CV::from_two((1.0, 2.0), (3.0, 4.0));
        let b = CV::from_two((0.5, 1.0), (1.0, 0.0));
        let sum = a + b;
        assert!(approx_eq(sum.to_complex0().0, 1.5));
        assert!(approx_eq(sum.to_complex0().1, 3.0));
        let diff = a - b;
        assert!(approx_eq(diff.to_complex0().0, 0.5));
        assert!(approx_eq(diff.to_complex0().1, 1.0));
        let neg = -a;
        assert!(approx_eq(neg.to_complex0().0, -1.0));
        assert!(approx_eq(neg.to_complex0().1, -2.0));
    }

    #[test]
    fn test_operators_complex_soa() {
        type CS = ComplexSoa<f32, ScalarVector4<f32>>;
        let a = CS::load(&[1.0, 2.0, 3.0, 4.0], &[0.0; 4]);
        let b = CS::load(&[5.0, 6.0, 7.0, 8.0], &[0.0; 4]);
        let sum = a + b;
        assert!(approx_eq(sum.re.extract(0), 6.0));
        let neg = -a;
        assert!(approx_eq(neg.re.extract(0), -1.0));
    }
}
