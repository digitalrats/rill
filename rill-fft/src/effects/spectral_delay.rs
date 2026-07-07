// rill-fft/src/effects/spectral_delay.rs
//! Spectral delay — frequency-dependent delay with feedback.
//!
//! Each frequency bin can be delayed independently, creating metallic
//! resonances, comb filtering, and spectral shimmer effects. The delay
//! is implemented as a circular buffer of FFT frames.

use num_complex::Complex;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;

use crate::real_fft::RealFft;

fn next_power_of_two(n: usize) -> usize {
    if n <= 2 {
        return 2;
    }
    let mut p = 1usize;
    while p < n {
        p <<= 1;
    }
    p
}

/// Spectral delay effect — applies different delay times to different frequency bins.
///
/// Stores a circular buffer of past FFT frames. Each bin's output is a mix
/// of the current bin and a delayed version with optional feedback.
///
/// # Type parameters
///
/// - `T` — sample type (`f32` or `f64`)
/// - `BUF_SIZE` — processing block size in samples
/// - `MAX_DELAY` — maximum delay in FFT frames
pub struct SpectralDelay<T: Transcendental, const BUF_SIZE: usize, const MAX_DELAY: usize> {
    fft_size: usize,
    half_plus_one: usize,
    fft: RealFft<T>,
    fft_in: Vec<T>,
    fft_out: Vec<Complex<T>>,
    ifft_out: Vec<T>,
    overlap: Vec<T>,
    delay_buffer: Vec<Vec<Complex<T>>>,
    write_head: usize,
    mix: T,
    feedback: T,
}

impl<T: Transcendental, const BUF_SIZE: usize, const MAX_DELAY: usize>
    SpectralDelay<T, BUF_SIZE, MAX_DELAY>
{
    /// Create a new spectral delay.
    ///
    /// # Panics
    ///
    /// Panics if `MAX_DELAY` is 0.
    pub fn new() -> Self {
        assert!(MAX_DELAY > 0, "MAX_DELAY must be at least 1");

        let fft_size = next_power_of_two(2 * BUF_SIZE);
        let half_plus_one = fft_size / 2 + 1;
        let fft = RealFft::new(fft_size);
        let overlap_len = fft_size - BUF_SIZE;

        let delay_buffer = vec![vec![Complex::new(T::ZERO, T::ZERO); half_plus_one]; MAX_DELAY];

        Self {
            fft_size,
            half_plus_one,
            fft,
            fft_in: vec![T::ZERO; fft_size],
            fft_out: vec![Complex::new(T::ZERO, T::ZERO); half_plus_one],
            ifft_out: vec![T::ZERO; fft_size],
            overlap: vec![T::ZERO; overlap_len],
            delay_buffer,
            write_head: 0,
            mix: T::from_f32(0.5),
            feedback: T::from_f32(0.3),
        }
    }

    /// Set wet/dry mix (0.0 = dry, 1.0 = wet).
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = T::from_f32(mix.clamp(0.0, 1.0));
    }

    /// Set feedback amount (0.0 = no feedback, 0.99 = near infinite).
    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = T::from_f32(feedback.clamp(0.0, 0.99));
    }

    /// Returns the FFT size.
    pub fn fft_size(&self) -> usize {
        self.fft_size
    }

    /// Process one block of samples.
    ///
    /// Combines the current spectrum with a delayed spectrum bin-by-bin.
    /// Lower-frequency bins can have longer delays than higher ones,
    /// creating unique spatial effects.
    pub fn process(&mut self, input: &[T], output: &mut [T]) {
        assert_eq!(input.len(), BUF_SIZE, "input must have BUF_SIZE elements");
        assert_eq!(output.len(), BUF_SIZE, "output must have BUF_SIZE elements");

        self.fft_in.fill(T::ZERO);
        self.fft_in[..BUF_SIZE].copy_from_slice(input);
        self.fft.forward(&self.fft_in, &mut self.fft_out);

        let current = self.fft_out.clone();

        let one = T::ONE;
        let one_minus_mix = one - self.mix;

        for i in 0..self.half_plus_one {
            let freq_ratio = T::from_usize(i) / T::from_usize(self.half_plus_one - 1);

            let delay_frames = (one - freq_ratio) * T::from_usize(MAX_DELAY - 1);
            let delay_int = delay_frames.to_f32() as usize;
            let delay_frac = delay_frames - T::from_usize(delay_int);

            let read_idx = if self.write_head >= delay_int {
                self.write_head - delay_int
            } else {
                MAX_DELAY + self.write_head - delay_int
            };

            let delayed = self.delay_buffer[read_idx][i];

            let delayed_mix = if delay_int + 1 < MAX_DELAY {
                let next_idx = if read_idx > 0 {
                    read_idx - 1
                } else {
                    MAX_DELAY - 1
                };
                let next = self.delay_buffer[next_idx][i];
                Complex::new(
                    delayed.re * (one - delay_frac) + next.re * delay_frac,
                    delayed.im * (one - delay_frac) + next.im * delay_frac,
                )
            } else {
                delayed
            };

            self.fft_out[i] = Complex::new(
                current[i].re * one_minus_mix + delayed_mix.re * self.mix,
                current[i].im * one_minus_mix + delayed_mix.im * self.mix,
            );

            // Store in delay buffer (current + feedback from delayed)
            self.delay_buffer[self.write_head][i] = Complex::new(
                current[i].re + delayed_mix.re * self.feedback,
                current[i].im + delayed_mix.im * self.feedback,
            );
        }

        self.write_head = (self.write_head + 1) % MAX_DELAY;

        self.fft.inverse(&self.fft_out, &mut self.ifft_out);

        for (out, (ifft_val, overlap_val)) in output
            .iter_mut()
            .zip(self.ifft_out.iter().zip(self.overlap.iter()))
        {
            *out = *ifft_val + *overlap_val;
        }

        let overlap_len = self.fft_size - BUF_SIZE;
        self.overlap
            .copy_from_slice(&self.ifft_out[BUF_SIZE..BUF_SIZE + overlap_len]);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize, const MAX_DELAY: usize> Default
    for SpectralDelay<T, BUF_SIZE, MAX_DELAY>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize, const MAX_DELAY: usize> Algorithm<T>
    for SpectralDelay<T, BUF_SIZE, MAX_DELAY>
{
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(samples) => {
                assert_eq!(
                    samples.len(),
                    BUF_SIZE,
                    "SpectralDelay expects BUF_SIZE={} input",
                    BUF_SIZE
                );
                assert_eq!(
                    output.len(),
                    BUF_SIZE,
                    "SpectralDelay expects BUF_SIZE={} output",
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
        self.delay_buffer.iter_mut().for_each(|buf| {
            buf.fill(num_complex::Complex::new(T::ZERO, T::ZERO));
        });
        self.write_head = 0;
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "SpectralDelay",
            category: AlgorithmCategory::Effect,
            description: "Frequency-dependent delay via FFT",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_passthrough_no_delay() {
        let mut delay = SpectralDelay::<f32, 64, 8>::new();
        delay.set_mix(0.0);
        delay.set_feedback(0.0);

        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut output = vec![0.0f32; 64];
        delay.process(&input, &mut output);

        for (i, o) in input.iter().zip(output.iter()) {
            assert!((i - o).abs() < 0.05, "expected {i}, got {o}");
        }
    }

    #[test]
    fn test_process_does_not_panic() {
        let mut delay = SpectralDelay::<f32, 64, 8>::new();
        delay.set_mix(0.5);
        delay.set_feedback(0.3);

        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.15).sin()).collect();
        let mut output = vec![0.0f32; 64];

        // Process multiple blocks to ensure stability
        for _ in 0..10 {
            delay.process(&input, &mut output);
            // Output should be finite (no NaN or infinity)
            for o in output.iter() {
                assert!(o.is_finite());
            }
        }
    }

    #[test]
    fn test_zero_feedback_is_passthrough() {
        let mut delay = SpectralDelay::<f32, 64, 4>::new();
        delay.set_mix(0.0);
        delay.set_feedback(0.0);

        let block1: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
        let block2: Vec<f32> = (64..128).map(|i| (i as f32 * 0.1).sin()).collect();

        let mut out1 = vec![0.0f32; 64];
        let mut out2 = vec![0.0f32; 64];

        delay.process(&block1, &mut out1);
        delay.process(&block2, &mut out2);

        for (i, o) in block1.iter().zip(out1.iter()) {
            assert!((i - o).abs() < 0.05, "block1: expected {i}, got {o}");
        }
        for (i, o) in block2.iter().zip(out2.iter()) {
            assert!((i - o).abs() < 0.05, "block2: expected {i}, got {o}");
        }
    }
}
