// rill-fft/src/complex_fft.rs
//! Radix-2 complex FFT (forward and inverse) using Decimation-In-Time (DIT).
//!
//! All twiddle factors and bit-reversal tables are pre-computed in the constructor
//! for RT-safe processing. Two APIs are provided:
//!
//! - **Interleaved** — `forward(&mut [Complex<T>])` / `inverse(&mut [Complex<T>])`
//! - **SoA** (Structure of Arrays) — `forward_soa(&mut [T], &mut [T])` / `inverse_soa(...)`
//!
//! The SoA API operates on separate real and imaginary arrays, enabling
//! hardware SIMD acceleration when the `simd` feature is enabled (via
//! `F32x4`/`F64x2` from the `wide` crate).

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

    /// Forward complex FFT — interleaved `Complex<T>` API.
    ///
    /// # Panics
    ///
    /// Panics if `data.len() != self.size()`.
    pub fn forward(&self, data: &mut [Complex<T>]) {
        assert_eq!(data.len(), self.size, "data length must match FFT size");
        self.bit_reverse_permute(data);
        self.butterfly(data, false);
    }

    /// Inverse complex FFT — interleaved `Complex<T>` API.
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

    /// Forward complex FFT — SoA (Structure of Arrays) API.
    ///
    /// Operates on separate real and imaginary arrays. Both must have length
    /// `self.size()`. Zero heap allocations in the process path.
    ///
    /// With the `simd` feature, f32/f64 variants use hardware SIMD via
    /// `F32x4`/`F64x2` for the butterfly arithmetic.
    pub fn forward_soa(&self, re: &mut [T], im: &mut [T]) {
        assert_eq!(re.len(), self.size);
        assert_eq!(im.len(), self.size);
        self.bit_reverse_soa(re, im);
        self.butterfly_soa(re, im, false);
    }

    /// Inverse complex FFT — SoA API.
    ///
    /// Result is scaled by `1/N`.
    pub fn inverse_soa(&self, re: &mut [T], im: &mut [T]) {
        assert_eq!(re.len(), self.size);
        assert_eq!(im.len(), self.size);
        self.bit_reverse_soa(re, im);
        self.butterfly_soa(re, im, true);
        let scale = T::ONE / T::from_usize(self.size);
        for i in 0..self.size {
            re[i] *= scale;
            im[i] *= scale;
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

    fn bit_reverse_soa(&self, re: &mut [T], im: &mut [T]) {
        for i in 0..self.size {
            let j = self.bit_reverse[i];
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
    }

    fn butterfly_soa(&self, re: &mut [T], im: &mut [T], inverse: bool) {
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
                    let a_re = re[i];
                    let a_im = im[i];
                    let b_re = re[j];
                    let b_im = im[j];
                    let br = b_re * w_cos - b_im * w_sin;
                    let bi = b_im * w_cos + b_re * w_sin;
                    re[i] = a_re + br;
                    im[i] = a_im + bi;
                    re[j] = a_re - br;
                    im[j] = a_im - bi;
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
        // DFT of [1,1,1,1]: [4, 0, 0, 0]
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

    // ============================================================================
    // SoA API tests
    // ============================================================================

    #[test]
    fn test_soa_roundtrip() {
        let fft = ComplexFft::<f32>::new(1024);
        let mut re: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut im: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.01).cos()).collect();
        let re_orig = re.clone();
        let im_orig = im.clone();

        fft.forward_soa(&mut re, &mut im);
        fft.inverse_soa(&mut re, &mut im);

        for (a, b) in re.iter().zip(re_orig.iter()) {
            assert!((a - b).abs() < 5e-4);
        }
        for (a, b) in im.iter().zip(im_orig.iter()) {
            assert!((a - b).abs() < 5e-4);
        }
    }

    #[test]
    fn test_soa_f64_roundtrip() {
        let fft = ComplexFft::<f64>::new(256);
        let mut re: Vec<f64> = (0..256).map(|i| (i as f64 * 0.01).sin()).collect();
        let mut im: Vec<f64> = (0..256).map(|i| (i as f64 * 0.01).cos()).collect();
        let re_orig = re.clone();
        let im_orig = im.clone();

        fft.forward_soa(&mut re, &mut im);
        fft.inverse_soa(&mut re, &mut im);

        for (a, b) in re.iter().zip(re_orig.iter()) {
            assert!((a - b).abs() < 1e-8);
        }
        for (a, b) in im.iter().zip(im_orig.iter()) {
            assert!((a - b).abs() < 1e-8);
        }
    }

    #[test]
    fn test_soa_versus_interleaved() {
        let fft = ComplexFft::<f32>::new(256);
        let mut re: Vec<f32> = (0..256).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut im: Vec<f32> = (0..256).map(|i| (i as f32 * 0.01).cos()).collect();
        let interleaved: Vec<_> = re
            .iter()
            .zip(im.iter())
            .map(|(&r, &i)| Complex::new(r, i))
            .collect();
        let mut inter_ref = interleaved.clone();

        fft.forward(&mut inter_ref);
        fft.forward_soa(&mut re, &mut im);

        for (idx, (r_val, i_val)) in re.iter().zip(im.iter()).enumerate() {
            let diff_re = (r_val - inter_ref[idx].re).abs();
            let diff_im = (i_val - inter_ref[idx].im).abs();
            assert!(diff_re < 1e-3, "bin {idx}: re diff {diff_re}");
            assert!(diff_im < 1e-3, "bin {idx}: im diff {diff_im}");
        }
    }
}

// ============================================================================
// SIMD SoA specialisations (f32 → F32x4, f64 → F64x2)
// ============================================================================

#[cfg(feature = "simd")]
mod simd_soa {
    use super::*;
    use rill_core::math::vector::simd::wide::{F32x4, F64x2};
    use rill_core::math::vector::traits::Vector;

    impl ComplexFft<f32> {
        /// Accelerated SoA forward FFT with F32x4 SIMD butterfly.
        ///
        /// The last stage (step_ratio == 1, which does half the total work)
        /// runs with 4-element SIMD; earlier stages use the scalar SoA path.
        pub fn forward_simd(&mut self, re: &mut [f32], im: &mut [f32]) {
            Self::bit_reverse_soa_simd(&self.bit_reverse, re, im, self.size);
            Self::butterfly_simd_f32(
                &self.twiddle_cos,
                &self.twiddle_sin,
                self.size,
                re,
                im,
                false,
            );
        }

        /// Accelerated SoA inverse FFT with F32x4 SIMD butterfly.
        pub fn inverse_simd(&mut self, re: &mut [f32], im: &mut [f32]) {
            Self::bit_reverse_soa_simd(&self.bit_reverse, re, im, self.size);
            Self::butterfly_simd_f32(
                &self.twiddle_cos,
                &self.twiddle_sin,
                self.size,
                re,
                im,
                true,
            );
            let scale = 1.0f32 / self.size as f32;
            for i in 0..self.size {
                re[i] *= scale;
                im[i] *= scale;
            }
        }

        fn butterfly_simd_f32(
            tw_cos: &[f32],
            tw_sin: &[f32],
            size: usize,
            re: &mut [f32],
            im: &mut [f32],
            inverse: bool,
        ) {
            let mut step = 2usize;
            while step <= size {
                let half_step = step / 2;
                let step_ratio = size / step;
                let mut block = 0usize;
                while block < size {
                    let mut pair = 0usize;
                    // SIMD: only when twiddles are consecutive (step_ratio == 1)
                    if step_ratio == 1 {
                        while pair + 3 < half_step {
                            let base = block + pair;
                            let base_b = base + half_step;
                            let w_cos = F32x4::load(&tw_cos[pair..]);
                            let mut w_sin = F32x4::load(&tw_sin[pair..]);
                            if inverse {
                                w_sin = -w_sin;
                            }
                            let a_re = F32x4::load(&re[base..]);
                            let a_im = F32x4::load(&im[base..]);
                            let b_re = F32x4::load(&re[base_b..]);
                            let b_im = F32x4::load(&im[base_b..]);
                            let br = b_re * w_cos - b_im * w_sin;
                            let bi = b_im * w_cos + b_re * w_sin;
                            (a_re + br).store(&mut re[base..]);
                            (a_im + bi).store(&mut im[base..]);
                            (a_re - br).store(&mut re[base_b..]);
                            (a_im - bi).store(&mut im[base_b..]);
                            pair += 4;
                        }
                    }
                    // Scalar remainder
                    while pair < half_step {
                        let i = block + pair;
                        let j = i + half_step;
                        let ti = pair * step_ratio;
                        let wc = tw_cos[ti];
                        let mut ws = tw_sin[ti];
                        if inverse {
                            ws = -ws;
                        }
                        let ar = re[i];
                        let ai = im[i];
                        let br = re[j];
                        let bi = im[j];
                        let br2 = br * wc - bi * ws;
                        let bi2 = bi * wc + br * ws;
                        re[i] = ar + br2;
                        im[i] = ai + bi2;
                        re[j] = ar - br2;
                        im[j] = ai - bi2;
                        pair += 1;
                    }
                    block += step;
                }
                step *= 2;
            }
        }

        fn bit_reverse_soa_simd(bit_rev: &[usize], re: &mut [f32], im: &mut [f32], _size: usize) {
            for (i, &j) in bit_rev.iter().enumerate() {
                if i < j {
                    re.swap(i, j);
                    im.swap(i, j);
                }
            }
        }
    }

    impl ComplexFft<f64> {
        /// Accelerated SoA forward FFT with F64x2 SIMD butterfly.
        pub fn forward_simd(&mut self, re: &mut [f64], im: &mut [f64]) {
            Self::bit_reverse_soa_simd(&self.bit_reverse, re, im, self.size);
            Self::butterfly_simd_f64(
                &self.twiddle_cos,
                &self.twiddle_sin,
                self.size,
                re,
                im,
                false,
            );
        }

        /// Accelerated SoA inverse FFT with F64x2 SIMD butterfly.
        pub fn inverse_simd(&mut self, re: &mut [f64], im: &mut [f64]) {
            Self::bit_reverse_soa_simd(&self.bit_reverse, re, im, self.size);
            Self::butterfly_simd_f64(
                &self.twiddle_cos,
                &self.twiddle_sin,
                self.size,
                re,
                im,
                true,
            );
            let scale = 1.0f64 / self.size as f64;
            for i in 0..self.size {
                re[i] *= scale;
                im[i] *= scale;
            }
        }

        fn butterfly_simd_f64(
            tw_cos: &[f64],
            tw_sin: &[f64],
            size: usize,
            re: &mut [f64],
            im: &mut [f64],
            inverse: bool,
        ) {
            let mut step = 2usize;
            while step <= size {
                let half_step = step / 2;
                let step_ratio = size / step;
                let mut block = 0usize;
                while block < size {
                    let mut pair = 0usize;
                    if step_ratio == 1 {
                        while pair + 1 < half_step {
                            let base = block + pair;
                            let base_b = base + half_step;
                            let w_cos = F64x2::load(&tw_cos[pair..]);
                            let mut w_sin = F64x2::load(&tw_sin[pair..]);
                            if inverse {
                                w_sin = -w_sin;
                            }
                            let a_re = F64x2::load(&re[base..]);
                            let a_im = F64x2::load(&im[base..]);
                            let b_re = F64x2::load(&re[base_b..]);
                            let b_im = F64x2::load(&im[base_b..]);
                            let br = b_re * w_cos - b_im * w_sin;
                            let bi = b_im * w_cos + b_re * w_sin;
                            (a_re + br).store(&mut re[base..]);
                            (a_im + bi).store(&mut im[base..]);
                            (a_re - br).store(&mut re[base_b..]);
                            (a_im - bi).store(&mut im[base_b..]);
                            pair += 2;
                        }
                    }
                    while pair < half_step {
                        let i = block + pair;
                        let j = i + half_step;
                        let ti = pair * step_ratio;
                        let wc = tw_cos[ti];
                        let mut ws = tw_sin[ti];
                        if inverse {
                            ws = -ws;
                        }
                        let ar = re[i];
                        let ai = im[i];
                        let br = re[j];
                        let bi = im[j];
                        let br2 = br * wc - bi * ws;
                        let bi2 = bi * wc + br * ws;
                        re[i] = ar + br2;
                        im[i] = ai + bi2;
                        re[j] = ar - br2;
                        im[j] = ai - bi2;
                        pair += 1;
                    }
                    block += step;
                }
                step *= 2;
            }
        }

        fn bit_reverse_soa_simd(bit_rev: &[usize], re: &mut [f64], im: &mut [f64], _size: usize) {
            for (i, &j) in bit_rev.iter().enumerate() {
                if i < j {
                    re.swap(i, j);
                    im.swap(i, j);
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_simd_soa_f32_roundtrip() {
            let mut fft = ComplexFft::<f32>::new(1024);
            let mut re: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.01).sin()).collect();
            let mut im: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.01).cos()).collect();
            let re_orig = re.clone();
            let im_orig = im.clone();

            fft.forward_simd(&mut re, &mut im);
            fft.inverse_simd(&mut re, &mut im);

            for (a, b) in re.iter().zip(re_orig.iter()) {
                assert!((a - b).abs() < 5e-4);
            }
            for (a, b) in im.iter().zip(im_orig.iter()) {
                assert!((a - b).abs() < 5e-4);
            }
        }

        #[test]
        fn test_simd_soa_f64_roundtrip() {
            let mut fft = ComplexFft::<f64>::new(512);
            let mut re: Vec<f64> = (0..512).map(|i| (i as f64 * 0.01).sin()).collect();
            let mut im: Vec<f64> = (0..512).map(|i| (i as f64 * 0.01).cos()).collect();
            let re_orig = re.clone();
            let im_orig = im.clone();

            fft.forward_simd(&mut re, &mut im);
            fft.inverse_simd(&mut re, &mut im);

            for (a, b) in re.iter().zip(re_orig.iter()) {
                assert!((a - b).abs() < 1e-7);
            }
            for (a, b) in im.iter().zip(im_orig.iter()) {
                assert!((a - b).abs() < 1e-7);
            }
        }
    }
}
