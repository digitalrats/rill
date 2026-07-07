// rill-core-dsp/src/complex_mat.rs
//! Complex-valued 2×2 and 3×3 matrices and filter design helpers.
//!
//! Closed-form determinant, inverse, and eigenvalues for small matrices.
//! Also provides bilinear transform and pole-to-coefficient conversion
//! utilities used in Butterworth/Chebyshev/elliptic filter design.
//!
//! # RT usage
//!
//! `ComplexMat2<T>` with `T = f32` is fully RT‑safe: all operations are
//! stack‑allocated and perform zero heap allocations. This enables:
//! - Complex‑coefficient biquad filters (Hilbert transformer, analytic signals)
//! - 2×2 rotation/scattering matrices in frequency‑domain processing
//! - Oversampled IIR stages with complex modulation
//!
//! # Batch processing via ComplexSoa
//!
//! For bulk complex arithmetic (e.g. per‑bin FFT convolution), the
//! `mul_complex_4` helper processes 4 complex multiplications at once
//! via the `ComplexSoa` eDSL from `rill‑core`:
//!
//! ```rust,no_run
//! use rill_core_dsp::complex_mat::mul_complex_4;
//!
//! let a_re = [1.0f32, 0.0, 0.5, -0.5];
//! let a_im = [0.0f32, 1.0, 0.5,  0.5];
//! let b_re = [2.0f32, 0.0, 1.0,  1.0];
//! let b_im = [3.0f32, 1.0, 1.0, -1.0];
//!
//! let mut out_re = [0.0f32; 4];
//! let mut out_im = [0.0f32; 4];
//! mul_complex_4(&a_re, &a_im, &b_re, &b_im, &mut out_re, &mut out_im);
//! // out_re[0] = 2.0, out_im[0] = 3.0  (1+0i)*(2+3i)
//! // out_re[1] = -1.0, out_im[1] = 0.0 (0+1i)*(0+1i)
//! ```

use num_complex::Complex;
use num_complex::Complex64;
use rill_core::prelude::{ComplexSoa, ScalarVector4};
use rill_core::Transcendental;

// ============================================================================
// Filter design helpers
// ============================================================================

/// Bilinear transform from s-domain to z-domain.
///
/// Maps a continuous-time pole/zero `s` to the discrete-time domain:
/// `z = (2 + s) / (2 - s)`
///
/// Used in Butterworth, Chebyshev, and elliptic filter design.
#[inline(always)]
pub fn bilinear_transform(s: Complex64) -> Complex64 {
    let two = Complex64::new(2.0, 0.0);
    (two + s) / (two - s)
}

/// Pre-warp an s-plane frequency for the bilinear transform.
///
/// Returns `2·tan(π·freq / sr)` — the warped cutoff frequency.
#[inline(always)]
pub fn prewarp_frequency(freq: f64, sample_rate: f64) -> f64 {
    2.0 * (std::f64::consts::PI * freq / sample_rate).tan()
}

/// Convert a complex-conjugate z-domain pole pair to biquad coefficients.
///
/// Given two poles `z1, z2` (which should be complex conjugates),
/// returns the denominator coefficients `(a1, a2)` for a Direct Form II biquad:
/// `H(z) = 1 / (1 + a1·z⁻¹ + a2·z⁻²)` where `a1 = -(z1+z2)`, `a2 = z1·z2`.
///
/// Since `z1` and `z2` are conjugates, both `a1` and `a2` are real.
#[inline(always)]
pub fn conjugate_pair_to_coeffs(z1: Complex64, z2: Complex64) -> (f64, f64) {
    let a1 = -(z1 + z2).re;
    let a2 = (z1 * z2).re;
    (a1, a2)
}

/// Convert a single real z-domain pole to biquad coefficients.
///
/// For odd-order filters, the unpaired real pole contributes `a1 = -z.re`, `a2 = 0`.
#[inline(always)]
pub fn single_pole_to_coeffs(z: Complex64) -> (f64, f64) {
    (-z.re, 0.0)
}

// ============================================================================
// Complex multiplication helpers (RT-safe, generic over T)
// ============================================================================

/// Complex multiplication: `a * b`.
///
/// Scalar primitive. For batch processing (4+ elements), prefer
/// `mul_complex_4` or `soa_from_interleaved` which use `ComplexSoa`.
#[inline(always)]
pub fn mul_complex<T>(a: Complex<T>, b: Complex<T>) -> Complex<T>
where
    T: Copy + std::ops::Add<Output = T> + std::ops::Sub<Output = T> + std::ops::Mul<Output = T>,
{
    Complex::new(a.re * b.re - a.im * b.im, a.re * b.im + a.im * b.re)
}

/// Complex multiply-accumulate: `acc += a * b`.
///
/// Scalar primitive. For batch processing, prefer `mul_complex_add_4`.
#[inline(always)]
pub fn mul_complex_add<T>(acc: &mut Complex<T>, a: Complex<T>, b: Complex<T>)
where
    T: Copy + std::ops::Add<Output = T> + std::ops::Sub<Output = T> + std::ops::Mul<Output = T>,
{
    acc.re = acc.re + (a.re * b.re - a.im * b.im);
    acc.im = acc.im + (a.re * b.im + a.im * b.re);
}

/// Batch complex multiply: processes 4 consecutive complex numbers at once.
///
/// Uses `ComplexSoa` for 4‑wide SIMD‑friendly arithmetic.
/// `re`/`im` slices must each have at least 4 elements.
/// `out_re`/`out_im` each receive the 4 results.
///
/// This is the vectorised equivalent of calling `mul_complex()` 4 times.
#[inline(always)]
pub fn mul_complex_4<T>(
    a_re: &[T],
    a_im: &[T],
    b_re: &[T],
    b_im: &[T],
    out_re: &mut [T],
    out_im: &mut [T],
) where
    T: Transcendental + 'static,
{
    use rill_core::prelude::{ComplexSoa, ScalarVector4};
    let a = ComplexSoa::<T, ScalarVector4<T>>::load(a_re, a_im);
    let b = ComplexSoa::<T, ScalarVector4<T>>::load(b_re, b_im);
    let prod = a * b;
    prod.store(out_re, out_im);
}

/// Batch complex multiply-accumulate: `acc += a * b` for 4 elements at once.
///
/// The four accumulator slots in `acc_re`/`acc_im` are read, incremented by
/// the product, and written back.
#[inline(always)]
pub fn mul_complex_add_4<T>(
    acc_re: &mut [T],
    acc_im: &mut [T],
    a_re: &[T],
    a_im: &[T],
    b_re: &[T],
    b_im: &[T],
) where
    T: Transcendental + 'static,
{
    use rill_core::prelude::{ComplexSoa, ScalarVector4};
    let mut acc = ComplexSoa::<T, ScalarVector4<T>>::load(acc_re, acc_im);
    let a = ComplexSoa::<T, ScalarVector4<T>>::load(a_re, a_im);
    let b = ComplexSoa::<T, ScalarVector4<T>>::load(b_re, b_im);
    acc.cmul_add(&a, &b);
    acc.store(acc_re, acc_im);
}

/// Load 4 consecutive complex numbers from an interleaved slice into `ComplexSoa`.
///
/// This is the canonical way to batch-load `Complex<T>` data for SIMD processing.
/// Equivalent to calling `from_pairs` with individually extracted fields.
pub fn soa_from_interleaved<T: Transcendental + 'static>(
    slice: &[Complex<T>],
) -> rill_core::prelude::ComplexSoa<T, rill_core::prelude::ScalarVector4<T>> {
    use rill_core::prelude::ComplexSoa;
    ComplexSoa::from_pairs([
        (slice[0].re, slice[0].im),
        (slice[1].re, slice[1].im),
        (slice[2].re, slice[2].im),
        (slice[3].re, slice[3].im),
    ])
}

/// Store a `ComplexSoa` result back to 4 consecutive interleaved `Complex<T>` entries.
pub fn soa_to_interleaved<T: Transcendental + 'static>(
    soa: &rill_core::prelude::ComplexSoa<T, rill_core::prelude::ScalarVector4<T>>,
    slice: &mut [Complex<T>],
) {
    let c = soa.to_complexes();
    slice[0] = Complex::new(c[0].0, c[0].1);
    slice[1] = Complex::new(c[1].0, c[1].1);
    slice[2] = Complex::new(c[2].0, c[2].1);
    slice[3] = Complex::new(c[3].0, c[3].1);
}

/// Complex-valued 2×2 matrix stored on the stack.
///
/// Row-major layout: `[[m00, m01], [m10, m11]]`.
///
/// # RT example — complex biquad step
///
/// ```rust,no_run
/// use rill_core_dsp::complex_mat::ComplexMat2;
/// use num_complex::Complex;
///
/// // Complex biquad denominator: y[n] = x[n] - a1*y[n-1] - a2*y[n-2]
/// let a1 = Complex::new(0.3f32, 0.5f32);
/// let a2 = Complex::new(-0.2f32, -0.1f32);
/// let mut y_prev = [Complex::new(0.0, 0.0); 2];
///
/// for n in 0..128 {
///     let x = Complex::new((n as f32 * 0.1).sin(), (n as f32 * 0.07).cos());
///     let y_n = x - a1 * y_prev[0] - a2 * y_prev[1];
///     y_prev[1] = y_prev[0];
///     y_prev[0] = y_n;
/// }
/// ```
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
        + num_traits::Float
        + Transcendental
        + 'static,
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

    /// Pack into `ComplexSoa` for vectorised operations.
    fn pack_soa(&self) -> rill_core::prelude::ComplexSoa<T, rill_core::prelude::ScalarVector4<T>>
    where
        T: Transcendental + 'static,
    {
        rill_core::prelude::ComplexSoa::from_pairs([
            (self.m00.re, self.m00.im),
            (self.m01.re, self.m01.im),
            (self.m10.re, self.m10.im),
            (self.m11.re, self.m11.im),
        ])
    }

    /// Unpack from `ComplexSoa` after vectorised operations.
    fn unpack_soa(
        soa: &rill_core::prelude::ComplexSoa<T, rill_core::prelude::ScalarVector4<T>>,
    ) -> Self
    where
        T: Transcendental + 'static,
    {
        let c = soa.to_complexes();
        Self {
            m00: Complex::new(c[0].0, c[0].1),
            m01: Complex::new(c[1].0, c[1].1),
            m10: Complex::new(c[2].0, c[2].1),
            m11: Complex::new(c[3].0, c[3].1),
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

    /// Matrix × vector via ComplexSoa.
    ///
    /// Computes `[m00*x + m01*y, m10*x + m11*y]` in one SoA pass.
    pub fn mul_vec(&self, x: Complex<T>, y: Complex<T>) -> [Complex<T>; 2] {
        let m = self.pack_soa();
        // x at lanes 0,2; y at lanes 1,3
        let vec = rill_core::prelude::ComplexSoa::<T, rill_core::prelude::ScalarVector4<T>>::load(
            &[x.re, y.re, x.re, y.re],
            &[x.im, y.im, x.im, y.im],
        );
        let prod = m * vec;
        let c0 = prod.extract_complex(0);
        let c1 = prod.extract_complex(1);
        let c2 = prod.extract_complex(2);
        let c3 = prod.extract_complex(3);
        [
            Complex::new(c0.0 + c1.0, c0.1 + c1.1),
            Complex::new(c2.0 + c3.0, c2.1 + c3.1),
        ]
    }

    /// Scale all elements by a scalar via ComplexSoa::scale_real.
    pub fn scale(&self, s: T) -> Self {
        use rill_core::prelude::Vector;
        let soa = self.pack_soa();
        let s_soa = rill_core::prelude::ScalarVector4::<T>::splat(s);
        let scaled = soa.scale_real(s_soa);
        Self::unpack_soa(&scaled)
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
        + num_traits::Float
        + Transcendental
        + 'static,
{
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        let a = self.pack_soa();
        let b = rhs.pack_soa();
        let sum = a + b;
        Self::unpack_soa(&sum)
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
        + num_traits::Float
        + Transcendental
        + 'static,
{
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        let a = self.pack_soa();
        let b = rhs.pack_soa();
        let diff = a - b;
        Self::unpack_soa(&diff)
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
        + num_traits::Float
        + Transcendental
        + 'static,
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

    // ============================================================================
    // RT-path tests (Complex<f32> — oversampling / Hilbert / complex filters)
    // ============================================================================

    #[test]
    fn test_complex_mat2_f32_rt_steps() {
        // Complex biquad denominator step:
        // y[n] = x[n] - a1*y[n-1] - a2*y[n-2]  (complex-valued)
        let a1 = Complex::new(0.3, 0.5);
        let a2 = Complex::new(-0.2, -0.1);

        // Simulate 100 RT samples — zero allocations
        let mut y = [Complex::new(0.0f32, 0.0f32); 2];
        let mut all_finite = true;
        for n in 0..100 {
            let x = Complex::new((n as f32 * 0.1).sin(), (n as f32 * 0.07).cos());
            let y_n = x - a1 * y[0] - a2 * y[1];
            y[1] = y[0];
            y[0] = y_n;
            all_finite &= y_n.re.is_finite() && y_n.im.is_finite();
        }
        assert!(all_finite, "complex biquad produced NaN/Inf in RT path");
    }

    #[test]
    fn test_complex_mat2_f32_rotation() {
        // Frequency-shift rotation matrix: exp(j*2π*f/fs) applied per sample.
        // Used in analytic signal generation and oversampling.
        let freq = 1000.0f32;
        let sr = 44100.0f32;
        let phase_step = 2.0 * std::f32::consts::PI * freq / sr;

        let rot = ComplexMat2::<f32>::new(
            Complex::new(phase_step.cos(), phase_step.sin()), // m00 = exp(jθ)
            Complex::new(0.0, 0.0),                           // m01
            Complex::new(0.0, 0.0),                           // m10
            Complex::new(phase_step.cos(), phase_step.sin()), // m11 = exp(jθ)
        );

        let z = Complex::new(1.0f32, 0.0);
        let [re, _im] = rot.mul_vec(z, Complex::new(0.0, 0.0));

        assert!((re.re - phase_step.cos()).abs() < 1e-4);
        assert!((re.im - phase_step.sin()).abs() < 1e-4);
    }

    #[test]
    fn test_complex_mat2_f64_rt_stability() {
        // f64 version — same test, higher precision
        let a1 = Complex::new(0.3f64, 0.5f64);
        let a2 = Complex::new(-0.2f64, -0.1f64);

        let mut y = [Complex::new(0.0f64, 0.0f64); 2];
        let mut all_finite = true;
        for n in 0..1000 {
            let x = Complex::new((n as f64 * 0.1).sin(), (n as f64 * 0.07).cos());
            let y_n = x - a1 * y[0] - a2 * y[1];
            y[1] = y[0];
            y[0] = y_n;
            all_finite &= y_n.re.is_finite() && y_n.im.is_finite();
        }
        assert!(all_finite, "complex biquad f64 produced NaN/Inf");
    }

    #[test]
    fn test_mul_complex_4() {
        let a_re = [1.0f32, 0.0, 0.5, -0.5];
        let a_im = [0.0f32, 1.0, 0.5, 0.5];
        let b_re = [2.0f32, 0.0, 1.0, 1.0];
        let b_im = [3.0f32, 1.0, 1.0, -1.0];
        let mut out_re = [0.0f32; 4];
        let mut out_im = [0.0f32; 4];

        mul_complex_4(&a_re, &a_im, &b_re, &b_im, &mut out_re, &mut out_im);

        // (1+0i)*(2+3i) = 2+3i
        assert!((out_re[0] - 2.0).abs() < 1e-4);
        assert!((out_im[0] - 3.0).abs() < 1e-4);
        // (0+1i)*(0+1i) = -1+0i
        assert!((out_re[1] + 1.0).abs() < 1e-4);
        assert!((out_im[1] - 0.0).abs() < 1e-4);
    }

    #[test]
    fn test_mul_complex_add_4() {
        let mut acc_re = [1.0f32; 4];
        let mut acc_im = [0.0f32; 4];
        let a_re = [1.0f32, 0.0, 2.0, 0.0];
        let a_im = [0.0f32, 1.0, 0.0, 0.0];
        let b_re = [2.0f32, 0.0, 3.0, 0.0];
        let b_im = [3.0f32, 1.0, 0.0, 0.0];

        mul_complex_add_4(&mut acc_re, &mut acc_im, &a_re, &a_im, &b_re, &b_im);

        // [1+0i] + (1+0i)*(2+3i) = 1 + (2+3i) = 3+3i
        assert!((acc_re[0] - 3.0).abs() < 1e-4);
        assert!((acc_im[0] - 3.0).abs() < 1e-4);
        // [1+0i] + (0+1i)*(0+1i) = 1 + (-1+0i) = 0+0i
        assert!((acc_re[1] - 0.0).abs() < 1e-4);
        assert!((acc_im[1] - 0.0).abs() < 1e-4);
    }
}
