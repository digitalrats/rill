// rill-fft/src/overlap_add.rs
//! Overlap-add convolution using real FFT.
//!
//! Efficient for medium-length impulse responses (up to ~16384 samples).
//! For very long IRs, use `PartitionedConvolver`.

use num_complex::Complex;
use rill_core::Transcendental;

use crate::real_fft::RealFft;

/// Overlap-add frequency-domain convolver.
///
/// Uses a real FFT for efficient frequency-domain multiplication.
/// All scratch buffers are pre-allocated at construction time.
///
/// # Parameters
///
/// - `T` — sample type (`f32` or `f64`)
/// - `BUF_SIZE` — processing block size in samples
pub struct OverlapAddConvolver<T: Transcendental, const BUF_SIZE: usize> {
    fft_size: usize,
    fft: RealFft<T>,
    ir_spectrum: Vec<Complex<T>>,
    input_buf: Vec<T>,
    fft_in: Vec<T>,
    fft_out: Vec<Complex<T>>,
    product: Vec<Complex<T>>,
    ifft_out: Vec<T>,
    overlap: Vec<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> OverlapAddConvolver<T, BUF_SIZE> {
    /// Create a new overlap-add convolver.
    ///
    /// `ir_len` is the expected length of the impulse response. The FFT size
    /// is chosen as the next power of two >= `BUF_SIZE + ir_len - 1`.
    ///
    /// # Panics
    ///
    /// Panics if the resulting FFT size is less than 4.
    pub fn new(ir_len: usize) -> Self {
        let fft_size = rill_core::utils::next_power_of_two(BUF_SIZE + ir_len - 1).max(4);
        assert!(fft_size >= 4, "FFT size must be at least 4");

        let fft = RealFft::new(fft_size);
        let half_plus_one = fft_size / 2 + 1;
        let overlap_len = fft_size - BUF_SIZE;

        Self {
            fft_size,
            fft,
            ir_spectrum: vec![Complex::new(T::ZERO, T::ZERO); half_plus_one],
            input_buf: vec![T::ZERO; BUF_SIZE],
            fft_in: vec![T::ZERO; fft_size],
            fft_out: vec![Complex::new(T::ZERO, T::ZERO); half_plus_one],
            product: vec![Complex::new(T::ZERO, T::ZERO); half_plus_one],
            ifft_out: vec![T::ZERO; fft_size],
            overlap: vec![T::ZERO; overlap_len],
        }
    }

    /// Set the impulse response.
    ///
    /// Computes and stores the FFT of the zero-padded IR.
    pub fn set_ir(&mut self, ir: &[T]) {
        let mut padded = vec![T::ZERO; self.fft_size];
        let len = ir.len().min(self.fft_size);
        padded[..len].copy_from_slice(&ir[..len]);

        self.fft.forward(&padded, &mut self.ir_spectrum);
    }

    /// Returns the FFT size.
    pub fn fft_size(&self) -> usize {
        self.fft_size
    }

    /// Process one block of samples.
    ///
    /// `input` must have exactly `BUF_SIZE` elements.
    /// `output` must have exactly `BUF_SIZE` elements.
    pub fn process(&mut self, input: &[T], output: &mut [T]) {
        assert_eq!(input.len(), BUF_SIZE, "input must have BUF_SIZE elements");
        assert_eq!(output.len(), BUF_SIZE, "output must have BUF_SIZE elements");

        self.input_buf.copy_from_slice(input);

        self.fft_in.fill(T::ZERO);
        self.fft_in[..BUF_SIZE].copy_from_slice(&self.input_buf);

        self.fft.forward(&self.fft_in, &mut self.fft_out);

        for i in 0..self.fft_out.len() {
            let s = self.ir_spectrum[i];
            let f = self.fft_out[i];
            self.product[i] = Complex::new(s.re * f.re - s.im * f.im, s.re * f.im + s.im * f.re);
        }

        self.fft.inverse(&self.product, &mut self.ifft_out);

        for (out, (ifft_val, overlap_val)) in output
            .iter_mut()
            .zip(self.ifft_out.iter().zip(self.overlap.iter()))
        {
            *out = *ifft_val + *overlap_val;
        }

        let overlap_len = self.fft_size - BUF_SIZE;
        for i in 0..overlap_len {
            self.overlap[i] = self.ifft_out[BUF_SIZE + i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unit_impulse_is_passthrough() {
        let mut conv = OverlapAddConvolver::<f32, 8>::new(4);
        conv.set_ir(&[1.0, 0.0, 0.0, 0.0]);

        let input = [0.5f32, 0.3, -0.2, 0.8, 0.1, -0.5, 0.4, 0.0];
        let mut output = [0.0f32; 8];
        conv.process(&input, &mut output);

        for (i, o) in input.iter().zip(output.iter()) {
            assert!((i - o).abs() < 1e-3, "expected {i}, got {o}");
        }
    }

    #[test]
    fn test_delayed_impulse_is_delay() {
        let mut conv = OverlapAddConvolver::<f32, 8>::new(4);
        conv.set_ir(&[0.0, 0.0, 1.0, 0.0]);

        let input = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut output = [0.0f32; 8];
        conv.process(&input, &mut output);

        assert!((output[0] - 0.0).abs() < 1e-3);
        assert!((output[1] - 0.1).abs() < 0.5);
        assert!((output[2] - 1.0).abs() < 0.5);
        assert!((output[3] - 2.0).abs() < 0.5);
    }

    #[test]
    fn test_roundtrip_with_direct_conv() {
        let ir = [0.3f32, 0.5, 0.2, 0.1];

        let mut ola = OverlapAddConvolver::<f32, 8>::new(ir.len());
        ola.set_ir(&ir);

        let input = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut ola_out = [0.0f32; 8];
        ola.process(&input, &mut ola_out);

        // Compute reference via direct convolution
        let mut ref_out = [0.0f32; 8];
        for n in 0..8 {
            let mut acc = 0.0;
            for k in 0..ir.len() {
                if k <= n {
                    acc += ir[k] * input[n - k];
                }
            }
            ref_out[n] = acc;
        }

        for (o, r) in ola_out.iter().zip(ref_out.iter()) {
            assert!((o - r).abs() < 1e-3, "OLA: {o}, ref: {r}");
        }
    }

    #[test]
    fn test_roundtrip_two_blocks() {
        let ir = [0.3f32, 0.5, 0.2, 0.1];

        let mut conv = OverlapAddConvolver::<f32, 4>::new(ir.len());
        conv.set_ir(&ir);

        let block1 = [1.0f32, 2.0, 3.0, 4.0];
        let block2 = [5.0f32, 6.0, 7.0, 8.0];

        let mut out1 = [0.0f32; 4];
        let mut out2 = [0.0f32; 4];

        conv.process(&block1, &mut out1);
        conv.process(&block2, &mut out2);

        let full_input = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut ref_out = [0.0f32; 8];
        for n in 0..8 {
            let mut acc = 0.0;
            for k in 0..ir.len() {
                if k <= n {
                    acc += ir[k] * full_input[n - k];
                }
            }
            ref_out[n] = acc;
        }

        for (i, (o, r)) in out1
            .iter()
            .chain(out2.iter())
            .zip(ref_out.iter())
            .enumerate()
        {
            assert!((o - r).abs() < 1e-3, "idx {i}: OLA: {o}, ref: {r}");
        }
    }
}
