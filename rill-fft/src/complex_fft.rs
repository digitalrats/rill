// rill-fft/src/complex_fft.rs
//! Radix-2 complex FFT (forward and inverse) using Decimation-In-Time (DIT).
//!
//! All twiddle factors and bit-reversal tables are pre-computed in the constructor
//! for RT-safe processing. The FFT operates in-place on a mutable slice of `Complex<T>`.
//!
//! ## SIMD potential
//!
//! The current interleaved `Complex<T>` layout is not SIMD‑friendly. A future
//! `simd`‑enabled path would internally convert to a SoA (Structure of Arrays)
//! layout — separate `re` and `im` arrays — then process 4 butterflies at once
//! using `F32x4` / `F64x2` from the `wide` crate. The conversion cost (~2·N copies
//! per transform) is amortised over the O(N log N) butterfly work for N ≥ 256.

use num_complex::Complex;
use rill_core::Transcendental;

/// Radix-2 Decimation-In-Time (DIT) complex FFT.
///
/// Supports sizes that are powers of two. All scratch buffers are pre-allocated
/// at construction time — `process()` performs zero heap allocations.
pub struct ComplexFft<T: Transcendental> {
    size: usize,
    bit_reverse: Box<[usize]>,
    twiddle_cos: Box<[T]>,
    twiddle_sin: Box<[T]>,
}

impl<T: Transcendental> ComplexFft<T> {
    /// Create a new complex FFT for the given size.
    ///
    /// # Panics
    ///
    /// Panics if `size` is not a power of two.
    pub fn new(size: usize) -> Self {
        assert!(
            size.is_power_of_two(),
            "FFT size must be a power of two, got {size}"
        );
        assert!(size >= 2, "FFT size must be at least 2, got {size}");

        let half_size = size / 2;
        let log2_size = size.trailing_zeros();

        let mut bit_reverse = vec![0usize; size].into_boxed_slice();
        for i in 0..size {
            bit_reverse[i] = i.reverse_bits() >> (usize::BITS - log2_size);
        }

        let twopi = T::from_f64(2.0 * std::f64::consts::PI);
        let size_t = T::from_usize(size);
        let mut twiddle_cos = vec![T::ZERO; half_size].into_boxed_slice();
        let mut twiddle_sin = vec![T::ZERO; half_size].into_boxed_slice();
        for k in 0..half_size {
            let angle = twopi * T::from_usize(k) / size_t;
            twiddle_cos[k] = angle.cos();
            twiddle_sin[k] = -angle.sin();
        }

        Self {
            size,
            bit_reverse,
            twiddle_cos,
            twiddle_sin,
        }
    }

    /// Returns the FFT size.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Forward complex FFT (in-place).
    ///
    /// # Panics
    ///
    /// Panics if `data.len() != self.size()`.
    pub fn forward(&self, data: &mut [Complex<T>]) {
        assert_eq!(data.len(), self.size, "data length must match FFT size");
        self.bit_reverse_permute(data);
        self.butterfly(data, false);
    }

    /// Inverse complex FFT (in-place).
    ///
    /// Result is scaled by `1/N`.
    ///
    /// # Panics
    ///
    /// Panics if `data.len() != self.size()`.
    pub fn inverse(&self, data: &mut [Complex<T>]) {
        assert_eq!(data.len(), self.size, "data length must match FFT size");
        self.bit_reverse_permute(data);
        self.butterfly(data, true);
        let scale = T::ONE / T::from_usize(self.size);
        for val in data.iter_mut() {
            *val = Complex::new(val.re * scale, val.im * scale);
        }
    }

    fn bit_reverse_permute(&self, data: &mut [Complex<T>]) {
        for i in 0..self.size {
            let j = self.bit_reverse[i];
            if i < j {
                data.swap(i, j);
            }
        }
    }

    fn butterfly(&self, data: &mut [Complex<T>], inverse: bool) {
        let mut step = 2usize;
        while step <= self.size {
            let half_step = step / 2;
            let step_ratio = self.size / step;
            let mut block = 0usize;
            while block < self.size {
                for pair in 0..half_step {
                    let i = block + pair;
                    let j = i + half_step;
                    let twiddle_idx = pair * step_ratio;
                    let w_cos = self.twiddle_cos[twiddle_idx];
                    let mut w_sin = self.twiddle_sin[twiddle_idx];
                    if inverse {
                        w_sin = -w_sin;
                    }
                    let a = data[i];
                    let b = data[j];
                    let b_re = b.re * w_cos - b.im * w_sin;
                    let b_im = b.im * w_cos + b.re * w_sin;
                    data[i] = Complex::new(a.re + b_re, a.im + b_im);
                    data[j] = Complex::new(a.re - b_re, a.im - b_im);
                }
                block += step;
            }
            step *= 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: Complex<f32>, b: Complex<f32>, eps: f32) -> bool {
        (a.re - b.re).abs() < eps && (a.im - b.im).abs() < eps
    }

    #[test]
    fn test_fft_size_4_forward() {
        let fft = ComplexFft::<f32>::new(4);
        let mut data = [
            Complex::new(1.0, 0.0),
            Complex::new(1.0, 0.0),
            Complex::new(1.0, 0.0),
            Complex::new(1.0, 0.0),
        ];
        fft.forward(&mut data);
        assert!((data[0].re - 4.0).abs() < 1e-4);
        assert!((data[1].re - 0.0).abs() < 1e-4);
        assert!((data[2].re - 0.0).abs() < 1e-4);
        assert!((data[3].re - 0.0).abs() < 1e-4);
    }

    #[test]
    fn test_fft_size_4_roundtrip() {
        let fft = ComplexFft::<f32>::new(4);
        let original = [
            Complex::new(1.0, 2.0),
            Complex::new(3.0, 4.0),
            Complex::new(5.0, 6.0),
            Complex::new(7.0, 8.0),
        ];
        let mut data = original;
        fft.forward(&mut data);
        fft.inverse(&mut data);
        for i in 0..4 {
            assert!(approx_eq(data[i], original[i], 1e-4));
        }
    }

    #[test]
    fn test_fft_size_8_roundtrip() {
        let fft = ComplexFft::<f32>::new(8);
        let original: Vec<_> = (0..8)
            .map(|i| Complex::new(i as f32, -(i as f32)))
            .collect();
        let mut data = original.clone();
        fft.forward(&mut data);
        fft.inverse(&mut data);
        for (a, b) in data.iter().zip(original.iter()) {
            assert!(approx_eq(*a, *b, 1e-3));
        }
    }

    #[test]
    fn test_fft_size_16_roundtrip() {
        let fft = ComplexFft::<f32>::new(16);
        let original: Vec<_> = (0..16)
            .map(|i| Complex::new((i as f32).sin(), (i as f32).cos()))
            .collect();
        let mut data = original.clone();
        fft.forward(&mut data);
        fft.inverse(&mut data);
        for (a, b) in data.iter().zip(original.iter()) {
            assert!(approx_eq(*a, *b, 1e-3));
        }
    }

    #[test]
    fn test_fft_impulse_is_constant() {
        let fft = ComplexFft::<f32>::new(8);
        let mut data = [Complex::new(0.0, 0.0); 8];
        data[0] = Complex::new(1.0, 0.0);
        fft.forward(&mut data);
        let expected = 1.0;
        for val in &data {
            assert!((val.re - expected).abs() < 1e-4);
            assert!(val.im.abs() < 1e-4);
        }
    }

    #[test]
    fn test_fft_f64_roundtrip() {
        let fft = ComplexFft::<f64>::new(8);
        let original: Vec<_> = (0..8)
            .map(|i| Complex::new((i as f64).sin(), (i as f64).cos()))
            .collect();
        let mut data = original.clone();
        fft.forward(&mut data);
        fft.inverse(&mut data);
        for (a, b) in data.iter().zip(original.iter()) {
            assert!((a.re - b.re).abs() < 1e-10);
            assert!((a.im - b.im).abs() < 1e-10);
        }
    }

    #[test]
    fn test_fft_size_1024_roundtrip() {
        let fft = ComplexFft::<f32>::new(1024);
        let original: Vec<_> = (0..1024)
            .map(|i| {
                let x = i as f32 * 0.01;
                Complex::new(x.sin(), x.cos())
            })
            .collect();
        let mut data = original.clone();
        fft.forward(&mut data);
        fft.inverse(&mut data);
        for (a, b) in data.iter().zip(original.iter()) {
            assert!(approx_eq(*a, *b, 5e-4));
        }
    }

    #[test]
    fn test_fft_dc_offset() {
        let fft = ComplexFft::<f32>::new(16);
        let mut data = [Complex::new(3.0, 0.0); 16];
        fft.forward(&mut data);
        assert!((data[0].re - 48.0).abs() < 1e-3);
        for i in 1..16 {
            assert!((data[i].re).abs() < 1e-3);
            assert!((data[i].im).abs() < 1e-3);
        }
    }

    #[test]
    #[should_panic(expected = "power of two")]
    fn test_fft_non_power_of_two_panics() {
        ComplexFft::<f32>::new(10);
    }

    #[test]
    #[should_panic(expected = "at least 2")]
    fn test_fft_size_one_panics() {
        ComplexFft::<f32>::new(1);
    }

    #[test]
    fn test_fft_size_2_basic() {
        let fft = ComplexFft::<f32>::new(2);
        let mut data = [Complex::new(1.0, 0.0), Complex::new(-1.0, 0.0)];
        fft.forward(&mut data);
        assert!((data[0].re - 0.0).abs() < 1e-4);
        assert!((data[1].re - 2.0).abs() < 1e-4);
    }
}
