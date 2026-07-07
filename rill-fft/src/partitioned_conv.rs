// rill-fft/src/partitioned_conv.rs
//! Partitioned frequency-domain convolution for long impulse responses.
//!
//! Splits the IR into equal-sized partitions and processes each partition
//! independently using overlap-add FFT convolution. Scales to very long IRs
//! (hundreds of thousands of samples) without excessive latency.
//!
//! # Algorithm
//!
//! Uniform partitioned convolution with overlap-add:
//! 1. Split IR into partitions of size `BUF_SIZE`
//! 2. Each partition is zero-padded to FFT size, then FFT'd → spectrum
//! 3. Input blocks are FFT'd and stored in a circular buffer
//! 4. Output = IFFT(Σ input_fft_circ[k] · ir_spectrum[k])
//! 5. Overlap-add to produce the output block

use num_complex::Complex;
use rill_core::Transcendental;

use crate::real_fft::RealFft;

/// Uniform partitioned convolver using overlap-add.
///
/// # Type parameters
///
/// - `T` — sample type (`f32` or `f64`)
/// - `BUF_SIZE` — processing block size (also the partition size)
pub struct PartitionedConvolver<T: Transcendental, const BUF_SIZE: usize> {
    fft_size: usize,
    half_plus_one: usize,
    num_partitions: usize,
    fft: RealFft<T>,
    ir_spectra: Vec<Vec<Complex<T>>>,
    input_fft_ring: Vec<Vec<Complex<T>>>,
    ring_head: usize,
    fft_in: Vec<T>,
    product: Vec<Complex<T>>,
    ifft_out: Vec<T>,
    overlap: Vec<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> PartitionedConvolver<T, BUF_SIZE> {
    /// Create a new partitioned convolver.
    ///
    /// `ir_len` is the total length of the impulse response in samples.
    ///
    /// # Panics
    ///
    /// Panics if `fft_size < 4` or if `ir_len` is such that `fft_size < BUF_SIZE`.
    pub fn new(ir_len: usize) -> Self {
        let fft_size = rill_core::utils::next_power_of_two(2 * BUF_SIZE).max(4);
        assert!(fft_size >= 4, "FFT size must be at least 4");
        assert!(fft_size >= BUF_SIZE, "FFT size must be >= BUF_SIZE");

        let half_plus_one = fft_size / 2 + 1;
        let num_partitions = ir_len.div_ceil(BUF_SIZE);
        let fft = RealFft::new(fft_size);
        let overlap_len = fft_size - BUF_SIZE;

        let ir_spectra = vec![vec![Complex::new(T::ZERO, T::ZERO); half_plus_one]; num_partitions];
        let input_fft_ring =
            vec![vec![Complex::new(T::ZERO, T::ZERO); half_plus_one]; num_partitions];

        Self {
            fft_size,
            half_plus_one,
            num_partitions,
            fft,
            ir_spectra,
            input_fft_ring,
            ring_head: 0,
            fft_in: vec![T::ZERO; fft_size],
            product: vec![Complex::new(T::ZERO, T::ZERO); half_plus_one],
            ifft_out: vec![T::ZERO; fft_size],
            overlap: vec![T::ZERO; overlap_len],
        }
    }

    /// Set the impulse response.
    ///
    /// Splits the IR into partitions, zero-pads each to FFT size,
    /// and computes the FFT of each partition.
    pub fn set_ir(&mut self, ir: &[T]) {
        let new_num = ir.len().div_ceil(BUF_SIZE);
        if new_num != self.num_partitions {
            self.num_partitions = new_num;
            self.ir_spectra =
                vec![vec![Complex::new(T::ZERO, T::ZERO); self.half_plus_one]; new_num];
            self.input_fft_ring =
                vec![vec![Complex::new(T::ZERO, T::ZERO); self.half_plus_one]; new_num];
            self.ring_head = 0;
        }

        for (p, spectrum) in self.ir_spectra.iter_mut().enumerate() {
            let start = p * BUF_SIZE;
            let end = ((p + 1) * BUF_SIZE).min(ir.len());
            let chunk_len = end - start;

            self.fft_in.fill(T::ZERO);
            self.fft_in[..chunk_len].copy_from_slice(&ir[start..end]);

            self.fft.forward(&self.fft_in, spectrum);
        }
    }

    /// Returns the number of partitions.
    pub fn num_partitions(&self) -> usize {
        self.num_partitions
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

        self.fft_in.fill(T::ZERO);
        self.fft_in[..BUF_SIZE].copy_from_slice(input);
        self.fft
            .forward(&self.fft_in, &mut self.input_fft_ring[self.ring_head]);

        self.product.fill(Complex::new(T::ZERO, T::ZERO));

        for k in 0..self.num_partitions {
            let ir_idx = k;
            let history_idx = if self.ring_head >= k {
                self.ring_head - k
            } else {
                self.num_partitions + self.ring_head - k
            };

            let ir_spec = &self.ir_spectra[ir_idx];
            let inp_spec = &self.input_fft_ring[history_idx];

            for i in 0..self.half_plus_one {
                let s = ir_spec[i];
                let f = inp_spec[i];
                self.product[i].re += s.re * f.re - s.im * f.im;
                self.product[i].im += s.re * f.im + s.im * f.re;
            }
        }

        self.fft.inverse(&self.product, &mut self.ifft_out);

        let overlap_len = self.fft_size - BUF_SIZE;
        for (out, (ifft_val, overlap_val)) in output
            .iter_mut()
            .zip(self.ifft_out.iter().zip(self.overlap.iter()))
        {
            *out = *ifft_val + *overlap_val;
        }
        for i in 0..overlap_len {
            self.overlap[i] = self.ifft_out[BUF_SIZE + i];
        }

        self.ring_head = (self.ring_head + 1) % self.num_partitions;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unit_impulse_is_passthrough() {
        let mut conv = PartitionedConvolver::<f32, 4>::new(4);
        conv.set_ir(&[1.0, 0.0, 0.0, 0.0]);

        let input = [0.5f32, 0.3, -0.2, 0.8];
        let mut output = [0.0f32; 4];
        conv.process(&input, &mut output);

        for (i, o) in input.iter().zip(output.iter()) {
            assert!((i - o).abs() < 1e-3, "expected {i}, got {o}");
        }
    }

    #[test]
    fn test_roundtrip_with_direct_conv() {
        let ir = [0.3f32, 0.5, 0.2, 0.1, 0.05, 0.02, 0.01, 0.0];

        let mut conv = PartitionedConvolver::<f32, 4>::new(ir.len());
        conv.set_ir(&ir);

        let input = [1.0f32, 2.0, 3.0, 4.0];
        let mut output = [0.0f32; 4];
        conv.process(&input, &mut output);

        let mut ref_out = [0.0f32; 4];
        for n in 0..4 {
            let mut acc = 0.0;
            for k in 0..ir.len() {
                if k <= n {
                    acc += ir[k] * input[n - k];
                }
            }
            ref_out[n] = acc;
        }

        for (o, r) in output.iter().zip(ref_out.iter()) {
            assert!((o - r).abs() < 1e-3, "part: {o}, ref: {r}");
        }
    }

    #[test]
    fn test_multiple_blocks() {
        let ir = [0.3f32, 0.5, 0.2, 0.1];

        let mut conv = PartitionedConvolver::<f32, 4>::new(ir.len());
        conv.set_ir(&ir);

        let blocks = [[1.0f32, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0]];

        let mut all_output = Vec::new();
        for block in &blocks {
            let mut out = [0.0f32; 4];
            conv.process(block, &mut out);
            all_output.extend_from_slice(&out);
        }

        let full_input: Vec<f32> = blocks.iter().flatten().copied().collect();
        let mut ref_out = vec![0.0f32; full_input.len()];
        for n in 0..full_input.len() {
            let mut acc = 0.0;
            for k in 0..ir.len() {
                if k <= n {
                    acc += ir[k] * full_input[n - k];
                }
            }
            ref_out[n] = acc;
        }

        for (i, (o, r)) in all_output.iter().zip(ref_out.iter()).enumerate() {
            assert!((o - r).abs() < 2e-3, "idx {i}: part: {o}, ref: {r}");
        }
    }

    #[test]
    fn test_long_ir_multiple_partitions() {
        let ir: Vec<f32> = (0..32).map(|i| 0.9f32.powi(i as i32)).collect();

        let mut conv = PartitionedConvolver::<f32, 4>::new(ir.len());
        conv.set_ir(&ir);

        assert_eq!(conv.num_partitions(), 8);

        let input: Vec<f32> = (0..20).map(|i| (i as f32 * 0.5).sin()).collect();

        let mut all_output = Vec::new();
        for chunk in input.chunks(4) {
            let mut block = [0.0f32; 4];
            block[..chunk.len()].copy_from_slice(chunk);
            let mut out = [0.0f32; 4];
            conv.process(&block, &mut out);
            all_output.extend_from_slice(&out);
        }

        let mut ref_out = vec![0.0f32; input.len()];
        for n in 0..input.len() {
            let mut acc = 0.0;
            for k in 0..ir.len() {
                if k <= n {
                    acc += ir[k] * input[n - k];
                }
            }
            ref_out[n] = acc;
        }

        for (i, (o, r)) in all_output
            .iter()
            .take(input.len())
            .zip(ref_out.iter())
            .enumerate()
        {
            assert!((o - r).abs() < 5e-3, "idx {i}: part: {o}, ref: {r}");
        }
    }
}
