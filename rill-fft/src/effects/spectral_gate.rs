// rill-fft/src/effects/spectral_gate.rs
//! Spectral gate — frequency-domain noise gate.
//!
//! Transforms the signal into the frequency domain, silences bins whose
//! magnitude falls below a threshold, then transforms back. Useful for
//! noise reduction and creative spectral effects.

use num_complex::Complex;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;

use crate::real_fft::RealFft;

/// Spectral gate effect using overlap-add FFT processing.
///
/// # Type parameters
///
/// - `T` — sample type (`f32` or `f64`)
/// - `BUF_SIZE` — processing block size in samples
pub struct SpectralGate<T: Transcendental, const BUF_SIZE: usize> {
    fft_size: usize,
    half_plus_one: usize,
    fft: RealFft<T>,
    fft_in: Vec<T>,
    fft_out: Vec<Complex<T>>,
    ifft_out: Vec<T>,
    overlap: Vec<T>,
    threshold: T,
    ratio: f32,
}

impl<T: Transcendental, const BUF_SIZE: usize> SpectralGate<T, BUF_SIZE> {
    /// Create a new spectral gate.
    pub fn new() -> Self {
        let fft_size = rill_core::utils::next_power_of_two(2 * BUF_SIZE).max(4);
        let half_plus_one = fft_size / 2 + 1;
        let fft = RealFft::new(fft_size);
        let overlap_len = fft_size - BUF_SIZE;

        Self {
            fft_size,
            half_plus_one,
            fft,
            fft_in: vec![T::ZERO; fft_size],
            fft_out: vec![Complex::new(T::ZERO, T::ZERO); half_plus_one],
            ifft_out: vec![T::ZERO; fft_size],
            overlap: vec![T::ZERO; overlap_len],
            threshold: T::from_f32(0.01),
            ratio: 0.0,
        }
    }

    /// Set the gate threshold. Bins with magnitude below this are attenuated.
    pub fn set_threshold(&mut self, threshold: T) {
        self.threshold = threshold;
    }

    /// Set the gate ratio. 0.0 = hard gate (silence), 1.0 = no gate (passthrough).
    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio.clamp(0.0, 1.0);
    }

    /// Returns the FFT size.
    pub fn fft_size(&self) -> usize {
        self.fft_size
    }

    /// Process one block of samples.
    pub fn process(&mut self, input: &[T], output: &mut [T]) {
        assert_eq!(input.len(), BUF_SIZE, "input must have BUF_SIZE elements");
        assert_eq!(output.len(), BUF_SIZE, "output must have BUF_SIZE elements");

        self.fft_in.fill(T::ZERO);
        self.fft_in[..BUF_SIZE].copy_from_slice(input);
        self.fft.forward(&self.fft_in, &mut self.fft_out);

        let one_minus_ratio = T::from_f32(1.0 - self.ratio);
        for i in 0..self.half_plus_one {
            let c = self.fft_out[i];
            let mag_sq = c.re * c.re + c.im * c.im;
            let mag = mag_sq.to_f64().sqrt() as f32;
            if mag < self.threshold.to_f32() {
                let scale = if self.ratio < 0.001 {
                    T::ZERO
                } else {
                    T::from_f32((mag / self.threshold.to_f32()) * (1.0 - self.ratio))
                        / self.threshold
                };
                self.fft_out[i] = Complex::new(c.re * scale, c.im * scale);
            } else {
                // Expand above threshold: apply soft knee
                let above = mag - self.threshold.to_f32();
                let gain = T::from_f32(1.0) + one_minus_ratio * T::from_f32(above / (1.0 + above));
                self.fft_out[i] = Complex::new(c.re * gain, c.im * gain);
            }
        }

        self.fft.inverse(&self.fft_out, &mut self.ifft_out);

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

impl<T: Transcendental, const BUF_SIZE: usize> Default for SpectralGate<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T> for SpectralGate<T, BUF_SIZE> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(samples) => {
                assert_eq!(
                    samples.len(),
                    BUF_SIZE,
                    "SpectralGate expects BUF_SIZE={} input",
                    BUF_SIZE
                );
                assert_eq!(
                    output.len(),
                    BUF_SIZE,
                    "SpectralGate expects BUF_SIZE={} output",
                    BUF_SIZE
                );
                self.process(samples, output);
                Ok(())
            }
            None => {
                output.fill(T::ZERO);
                Ok(())
            }
        }
    }

    fn reset(&mut self) {
        self.fft_in.fill(T::ZERO);
        self.fft_out
            .fill(num_complex::Complex::new(T::ZERO, T::ZERO));
        self.ifft_out.fill(T::ZERO);
        self.overlap.fill(T::ZERO);
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "SpectralGate",
            category: AlgorithmCategory::Effect,
            description: "Frequency-domain noise gate via FFT",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_passthrough_with_high_ratio() {
        let mut gate = SpectralGate::<f32, 64>::new();
        gate.set_threshold(0.0);
        gate.set_ratio(1.0);

        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut output = vec![0.0f32; 64];
        gate.process(&input, &mut output);

        for (i, o) in input.iter().zip(output.iter()) {
            assert!((i - o).abs() < 0.01, "expected {i}, got {o}");
        }
    }

    #[test]
    fn test_silence_with_zero_threshold_ratio() {
        let mut gate = SpectralGate::<f32, 64>::new();
        gate.set_threshold(100.0);
        gate.set_ratio(0.0);

        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut output = vec![0.0f32; 64];
        gate.process(&input, &mut output);

        for o in output.iter() {
            assert!(o.abs() < 0.01);
        }
    }

    #[test]
    fn test_roundtrip_multiple_blocks() {
        let mut gate = SpectralGate::<f32, 64>::new();
        gate.set_threshold(0.0);
        gate.set_ratio(1.0);

        let block1: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
        let block2: Vec<f32> = (64..128).map(|i| (i as f32 * 0.1).sin()).collect();

        let mut out1 = vec![0.0f32; 64];
        let mut out2 = vec![0.0f32; 64];

        gate.process(&block1, &mut out1);
        gate.process(&block2, &mut out2);

        for (i, o) in block1.iter().zip(out1.iter()) {
            assert!((i - o).abs() < 0.05, "block1 idx: expected {i}, got {o}");
        }
        for (i, o) in block2.iter().zip(out2.iter()) {
            assert!((i - o).abs() < 0.05, "block2 idx: expected {i}, got {o}");
        }
    }
}
