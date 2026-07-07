// rill-fft/src/real_fft.rs
//! Real-valued FFT using a complex FFT with packing/unpacking.
//!
//! Transforms `N` real samples into `N/2 + 1` complex frequency bins.
//! The inverse transform reconstructs `N` real samples from the complex bins.

use num_complex::Complex;
use rill_core::Transcendental;

use crate::complex_fft::ComplexFft;

/// Real-valued FFT.
///
/// Uses a half-size complex FFT internally via the two-for-one method.
/// Transforms `N` real samples into `N/2 + 1` complex bins (only the
/// non-redundant half of the spectrum). The Nyquist bin (`N/2`) and
/// DC bin (`0`) are purely real.
///
/// # Panics
///
/// Panics if `size` is not a power of two or less than 4.
pub struct RealFft<T: Transcendental> {
    size: usize,
    half_size: usize,
    complex_fft: ComplexFft<T>,
    scratch: Vec<Complex<T>>,
}

impl<T: Transcendental> RealFft<T> {
    /// Create a new real FFT for the given size.
    ///
    /// # Panics
    ///
    /// Panics if `size` is not a power of two or is less than 4.
    pub fn new(size: usize) -> Self {
        assert!(
            size.is_power_of_two(),
            "FFT size must be a power of two, got {size}"
        );
        assert!(size >= 4, "FFT size must be at least 4, got {size}");

        let half_size = size / 2;
        let complex_fft = ComplexFft::new(half_size);
        let scratch = vec![Complex::new(T::ZERO, T::ZERO); half_size];

        Self {
            size,
            half_size,
            complex_fft,
            scratch,
        }
    }

    /// Returns the FFT size (number of real input samples).
    pub fn size(&self) -> usize {
        self.size
    }

    /// Forward real FFT.
    ///
    /// Transforms `input` (N real samples) into `output` (N/2 + 1 complex bins).
    ///
    /// # Panics
    ///
    /// Panics if `input.len() != self.size()` or `output.len() != self.half_size + 1`.
    pub fn forward(&mut self, input: &[T], output: &mut [Complex<T>]) {
        assert_eq!(
            input.len(),
            self.size,
            "input length ({}) must match FFT size ({})",
            input.len(),
            self.size
        );
        assert_eq!(
            output.len(),
            self.half_size + 1,
            "output length ({}) must be half_size + 1 ({})",
            output.len(),
            self.half_size + 1
        );

        for i in 0..self.half_size {
            self.scratch[i] = Complex::new(input[2 * i], input[2 * i + 1]);
        }

        self.complex_fft.forward(&mut self.scratch);

        let z = &self.scratch;
        let z0 = z[0];
        output[0] = Complex::new(z0.re + z0.im, T::ZERO);
        output[self.half_size] = Complex::new(z0.re - z0.im, T::ZERO);

        let twopi = T::from_f64(2.0 * std::f64::consts::PI);
        let size_t = T::from_usize(self.size);
        for k in 1..self.half_size {
            let theta = twopi * T::from_usize(k) / size_t;
            let w_cos = theta.cos();
            let w_sin = theta.sin();

            let a = z[k];
            let b = Complex::new(z[self.half_size - k].re, -z[self.half_size - k].im);

            let c_re = a.re + b.re;
            let c_im = a.im + b.im;
            let d_re = a.re - b.re;
            let d_im = a.im - b.im;

            let half = T::from_f64(0.5);
            output[k].re = half * (c_re - w_sin * d_re + w_cos * d_im);
            output[k].im = half * (c_im - w_sin * d_im - w_cos * d_re);
        }
    }

    /// Inverse real FFT.
    ///
    /// Reconstructs `output` (N real samples) from `input` (N/2 + 1 complex bins).
    /// This is the exact inverse of `forward()`.
    ///
    /// # Panics
    ///
    /// Panics if `input.len() != self.half_size + 1` or `output.len() != self.size()`.
    pub fn inverse(&mut self, input: &[Complex<T>], output: &mut [T]) {
        assert_eq!(
            input.len(),
            self.half_size + 1,
            "input length ({}) must be half_size + 1 ({})",
            input.len(),
            self.half_size + 1
        );
        assert_eq!(
            output.len(),
            self.size,
            "output length ({}) must match FFT size ({})",
            output.len(),
            self.size
        );

        let x = input;

        let half = T::from_f64(0.5);
        self.scratch[0] = Complex::new(
            half * (x[0].re + x[self.half_size].re),
            half * (x[0].re - x[self.half_size].re),
        );

        let twopi = T::from_f64(2.0 * std::f64::consts::PI);
        let size_t = T::from_usize(self.size);
        for k in 1..self.half_size {
            let theta = twopi * T::from_usize(k) / size_t;
            let w_cos = theta.cos();
            let w_sin = theta.sin();

            let a = x[k];
            let b = Complex::new(x[self.half_size - k].re, -x[self.half_size - k].im);

            let c_re = a.re + b.re;
            let c_im = a.im + b.im;
            let d_re = a.re - b.re;
            let d_im = a.im - b.im;

            let half = T::from_f64(0.5);
            self.scratch[k].re = half * (c_re - w_sin * d_re - w_cos * d_im);
            self.scratch[k].im = half * (c_im - w_sin * d_im + w_cos * d_re);
        }

        self.complex_fft.inverse(&mut self.scratch);

        for i in 0..self.half_size {
            output[2 * i] = self.scratch[i].re;
            output[2 * i + 1] = self.scratch[i].im;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_real_fft_size_4_roundtrip() {
        let mut fft = RealFft::<f32>::new(4);
        let input = [1.0f32, 2.0, 3.0, 4.0];
        let mut spectrum = vec![Complex::new(0.0, 0.0); 3];
        let mut output = [0.0f32; 4];

        fft.forward(&input, &mut spectrum);
        fft.inverse(&spectrum, &mut output);

        for (a, b) in input.iter().zip(output.iter()) {
            assert!((a - b).abs() < 1e-3, "expected {a}, got {b}");
        }
    }

    #[test]
    fn test_real_fft_size_8_roundtrip() {
        let mut fft = RealFft::<f32>::new(8);
        let input: Vec<f32> = (0..8).map(|i| (i as f32 * 0.7).sin()).collect();
        let mut spectrum = vec![Complex::new(0.0, 0.0); 5];
        let mut output = vec![0.0f32; 8];

        fft.forward(&input, &mut spectrum);
        fft.inverse(&spectrum, &mut output);

        for (a, b) in input.iter().zip(output.iter()) {
            assert!((a - b).abs() < 1e-3, "expected {a}, got {b}");
        }
    }

    #[test]
    fn test_real_fft_size_1024_roundtrip() {
        let mut fft = RealFft::<f32>::new(1024);
        let input: Vec<f32> = (0..1024)
            .map(|i| {
                let x = i as f32 * 0.05;
                x.sin() + 0.5 * (x * 2.3).cos()
            })
            .collect();
        let mut spectrum = vec![Complex::new(0.0, 0.0); 513];
        let mut output = vec![0.0f32; 1024];

        fft.forward(&input, &mut spectrum);
        fft.inverse(&spectrum, &mut output);

        for (a, b) in input.iter().zip(output.iter()) {
            assert!((a - b).abs() < 5e-4, "at index: expected {a}, got {b}");
        }
    }

    #[test]
    fn test_real_fft_size_16_roundtrip() {
        let mut fft = RealFft::<f32>::new(16);
        let input: Vec<f32> = (0..16).map(|i| i as f32).collect();
        let mut spectrum = vec![Complex::new(0.0, 0.0); 9];
        let mut output = vec![0.0f32; 16];

        fft.forward(&input, &mut spectrum);
        fft.inverse(&spectrum, &mut output);

        for (a, b) in input.iter().zip(output.iter()) {
            assert!((a - b).abs() < 5e-4, "expected {a}, got {b}");
        }
    }

    #[test]
    fn test_real_fft_f64_roundtrip() {
        let mut fft = RealFft::<f64>::new(8);
        let input: Vec<f64> = (0..8).map(|i| (i as f64 * 0.5).sin()).collect();
        let mut spectrum = vec![Complex::new(0.0, 0.0); 5];
        let mut output = vec![0.0f64; 8];

        fft.forward(&input, &mut spectrum);
        fft.inverse(&spectrum, &mut output);

        for (a, b) in input.iter().zip(output.iter()) {
            assert!((a - b).abs() < 1e-10, "expected {a}, got {b}");
        }
    }

    #[test]
    fn test_real_fft_dc_input() {
        let mut fft = RealFft::<f32>::new(16);
        let input = [2.5f32; 16];
        let mut spectrum = vec![Complex::new(0.0, 0.0); 9];

        fft.forward(&input, &mut spectrum);

        assert!((spectrum[0].re - 40.0).abs() < 1e-3);
        assert!(spectrum[0].im.abs() < 1e-3);
    }

    #[test]
    fn test_real_fft_nyquist_bin() {
        let mut fft = RealFft::<f32>::new(8);
        let input = [1.0f32, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
        let mut spectrum = vec![Complex::new(0.0, 0.0); 5];

        fft.forward(&input, &mut spectrum);

        assert!(spectrum[4].re.abs() > 0.1);
    }
}
