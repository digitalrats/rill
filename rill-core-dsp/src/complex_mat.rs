// rill-core-dsp/src/complex_mat.rs
//! Complex-valued 2×2 and 3×3 matrices for filter analysis.
//!
//! Closed-form determinant, inverse, and eigenvalues for small matrices
//! — used in Butterworth/Chebyshev pole design. All operations are
//! stack-allocated and allocation-free.

use num_complex::Complex;

/// Complex-valued 2×2 matrix stored on the stack.
///
/// Row-major layout: `[[m00, m01], [m10, m11]]`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ComplexMat2<T> {
    /// Row 0, col 0
    pub m00: Complex<T>,
    /// Row 0, col 1
    pub m01: Complex<T>,
    /// Row 1, col 0
    pub m10: Complex<T>,
    /// Row 1, col 1
    pub m11: Complex<T>,
}

impl<T> ComplexMat2<T>
where
    T: Copy
        + std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Mul<Output = T>
        + std::ops::Div<Output = T>
        + std::ops::Neg<Output = T>
        + num_traits::Float,
{
    /// Create a new 2×2 matrix from four elements.
    pub fn new(m00: Complex<T>, m01: Complex<T>, m10: Complex<T>, m11: Complex<T>) -> Self {
        Self { m00, m01, m10, m11 }
    }

    /// Zero matrix.
    pub fn zero() -> Self {
        Self {
            m00: Complex::new(T::zero(), T::zero()),
            m01: Complex::new(T::zero(), T::zero()),
            m10: Complex::new(T::zero(), T::zero()),
            m11: Complex::new(T::zero(), T::zero()),
        }
    }

    /// Identity matrix.
    pub fn identity() -> Self {
        let zero = Complex::new(T::zero(), T::zero());
        let one = Complex::new(T::one(), T::zero());
        Self {
            m00: one,
            m01: zero,
            m10: zero,
            m11: one,
        }
    }

    /// Determinant: m00*m11 − m01*m10.
    pub fn det(&self) -> Complex<T> {
        self.m00 * self.m11 - self.m01 * self.m10
    }

    /// Inverse (closed form for 2×2).
    /// Returns `None` if the determinant is zero.
    pub fn inv(&self) -> Option<Self> {
        let d = self.det();
        if d.norm_sqr() <= T::epsilon() {
            return None;
        }
        Some(Self {
            m00: self.m11 / d,
            m01: -self.m01 / d,
            m10: -self.m10 / d,
            m11: self.m00 / d,
        })
    }

    /// Trace: m00 + m11.
    pub fn trace(&self) -> Complex<T> {
        self.m00 + self.m11
    }

    /// Eigenvalues — solutions to λ² − trace·λ + det = 0.
    /// Returns `None` for the degenerate single-root case.
    pub fn eigenvalues(&self) -> Option<[Complex<T>; 2]> {
        let tr = self.trace();
        let det = self.det();
        let half = T::from(0.5).unwrap();
        let disc = tr * tr - Complex::new(T::from(4.0).unwrap(), T::zero()) * det;
        if disc.norm_sqr() <= T::epsilon() {
            return None;
        }
        let sqrt_disc = disc.sqrt();
        Some([
            Complex::new(half, T::zero()) * (tr + sqrt_disc),
            Complex::new(half, T::zero()) * (tr - sqrt_disc),
        ])
    }

    /// Multiply matrix by a column vector [x, y]ᵀ.
    pub fn mul_vec(&self, x: Complex<T>, y: Complex<T>) -> [Complex<T>; 2] {
        [self.m00 * x + self.m01 * y, self.m10 * x + self.m11 * y]
    }

    /// Scale all elements by a scalar.
    pub fn scale(&self, s: T) -> Self {
        let cs = Complex::new(s, T::zero());
        Self {
            m00: self.m00 * cs,
            m01: self.m01 * cs,
            m10: self.m10 * cs,
            m11: self.m11 * cs,
        }
    }
}

impl<T> std::ops::Add for ComplexMat2<T>
where
    T: Copy
        + std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Mul<Output = T>
        + std::ops::Div<Output = T>
        + std::ops::Neg<Output = T>
        + num_traits::Float,
{
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            m00: self.m00 + rhs.m00,
            m01: self.m01 + rhs.m01,
            m10: self.m10 + rhs.m10,
            m11: self.m11 + rhs.m11,
        }
    }
}

impl<T> std::ops::Sub for ComplexMat2<T>
where
    T: Copy
        + std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Mul<Output = T>
        + std::ops::Div<Output = T>
        + std::ops::Neg<Output = T>
        + num_traits::Float,
{
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            m00: self.m00 - rhs.m00,
            m01: self.m01 - rhs.m01,
            m10: self.m10 - rhs.m10,
            m11: self.m11 - rhs.m11,
        }
    }
}

impl<T> std::ops::Mul for ComplexMat2<T>
where
    T: Copy
        + std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Mul<Output = T>
        + std::ops::Div<Output = T>
        + std::ops::Neg<Output = T>
        + num_traits::Float,
{
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            m00: self.m00 * rhs.m00 + self.m01 * rhs.m10,
            m01: self.m00 * rhs.m01 + self.m01 * rhs.m11,
            m10: self.m10 * rhs.m00 + self.m11 * rhs.m10,
            m11: self.m10 * rhs.m01 + self.m11 * rhs.m11,
        }
    }
}

/// Complex-valued 3×3 matrix. Useful for eigenvalue analysis of
/// cascaded biquad sections.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ComplexMat3<T> {
    /// Row-major elements
    pub m: [[Complex<T>; 3]; 3],
}

impl<T> ComplexMat3<T>
where
    T: Copy
        + std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Mul<Output = T>
        + std::ops::Div<Output = T>
        + std::ops::Neg<Output = T>
        + num_traits::Float,
{
    /// Create from row-major array.
    pub fn from_rows(rows: [[Complex<T>; 3]; 3]) -> Self {
        Self { m: rows }
    }

    /// Zero matrix.
    pub fn zero() -> Self {
        Self {
            m: [[Complex::new(T::zero(), T::zero()); 3]; 3],
        }
    }

    /// Identity matrix.
    pub fn identity() -> Self {
        let zero = Complex::new(T::zero(), T::zero());
        let one = Complex::new(T::one(), T::zero());
        let mut m = [[zero; 3]; 3];
        m[0][0] = one;
        m[1][1] = one;
        m[2][2] = one;
        Self { m }
    }

    /// Determinant (Sarrus' rule, closed form).
    pub fn det(&self) -> Complex<T> {
        let a = self.m[0][0];
        let b = self.m[0][1];
        let c = self.m[0][2];
        let d = self.m[1][0];
        let e = self.m[1][1];
        let f = self.m[1][2];
        let g = self.m[2][0];
        let h = self.m[2][1];
        let i = self.m[2][2];

        a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    }

    /// Trace.
    pub fn trace(&self) -> Complex<T> {
        self.m[0][0] + self.m[1][1] + self.m[2][2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_det_is_one() {
        let m = ComplexMat2::<f32>::identity();
        let d = m.det();
        assert!((d.re - 1.0).abs() < 1e-6);
        assert!(d.im.abs() < 1e-6);
    }

    #[test]
    fn test_inv_times_mat_is_identity() {
        let m = ComplexMat2::<f32>::new(
            Complex::new(2.0, 0.0),
            Complex::new(1.0, 0.0),
            Complex::new(1.0, 0.0),
            Complex::new(3.0, 0.0),
        );
        let inv = m.inv().unwrap();
        let prod = m * inv;
        assert!((prod.m00.re - 1.0).abs() < 1e-4);
        assert!((prod.m11.re - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_singular_inv_returns_none() {
        let m = ComplexMat2::<f32>::new(
            Complex::new(1.0, 0.0),
            Complex::new(2.0, 0.0),
            Complex::new(2.0, 0.0),
            Complex::new(4.0, 0.0),
        );
        assert!(m.inv().is_none());
    }

    #[test]
    fn test_eigenvalues() {
        let m = ComplexMat2::<f32>::new(
            Complex::new(0.0, 0.0),
            Complex::new(1.0, 0.0),
            Complex::new(-2.0, 0.0),
            Complex::new(-3.0, 0.0),
        );
        let ev = m.eigenvalues().unwrap();
        // characteristic: λ² + 3λ + 2 = 0 → λ = -1, -2
        assert!((ev[0].re + 1.0).abs() < 1e-4 || (ev[0].re + 2.0).abs() < 1e-4);
        assert!((ev[1].re + 1.0).abs() < 1e-4 || (ev[1].re + 2.0).abs() < 1e-4);
    }

    #[test]
    fn test_mat3_det() {
        let m = ComplexMat3::<f32>::identity();
        let d = m.det();
        assert!((d.re - 1.0).abs() < 1e-6);
    }
}
