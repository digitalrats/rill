// rill-fft/src/spectrum.rs
//! FFT-based spectrum analyzer.
//!
//! Implements `SpectrumAnalyzer` from `rill-core-dsp` using `RealFft`.

use rill_core::prelude::Vector;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;
use rill_core_dsp::analyzer::{Analyzer, SpectrumAnalyzer};
use rill_core_dsp::complex_mat::soa_from_interleaved;

use crate::real_fft::RealFft;

/// FFT-based spectrum analyzer.
///
/// Transforms real input blocks into magnitude spectra via the real FFT.
/// The window function (Hann by default) is applied before the transform.
pub struct FftSpectrumAnalyzer<T: Transcendental> {
    fft: RealFft<T>,
    window: Vec<T>,
    scratch: Vec<num_complex::Complex<T>>,
    magnitude: Vec<f32>,
    block_buf: Vec<T>,
}

impl<T: Transcendental> FftSpectrumAnalyzer<T> {
    /// Create a new spectrum analyzer with FFT size `fft_size` and Hann window.
    ///
    /// # Panics
    ///
    /// Panics if `fft_size` is not a power of two.
    pub fn new(fft_size: usize) -> Self {
        let fft = RealFft::new(fft_size);
        let half_plus_one = fft_size / 2 + 1;

        let window = (0..fft_size)
            .map(|i| {
                let phase = T::from_f64(2.0 * std::f64::consts::PI * i as f64 / fft_size as f64);
                T::ONE - phase.cos()
            })
            .collect();

        Self {
            fft,
            window,
            scratch: vec![num_complex::Complex::new(T::ZERO, T::ZERO); half_plus_one],
            magnitude: vec![0.0f32; half_plus_one],
            block_buf: vec![T::ZERO; fft_size],
        }
    }

    /// Returns the FFT size.
    pub fn fft_size(&self) -> usize {
        self.fft.size()
    }

    /// Returns the magnitude spectrum (in f32).
    pub fn spectrum(&self) -> &[f32] {
        &self.magnitude
    }

    /// Compute the amplitude at a specific frequency.
    pub fn amplitude_at(&self, freq: f32, sample_rate: f32) -> f32 {
        let bin = (freq * self.fft.size() as f32 / sample_rate) as usize;
        self.magnitude.get(bin).copied().unwrap_or(0.0)
    }
}

impl<T: Transcendental> Algorithm<T> for FftSpectrumAnalyzer<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(samples) => {
                let len = samples.len().min(self.fft.size());
                self.block_buf.fill(T::ZERO);
                self.block_buf[..len].copy_from_slice(&samples[..len]);

                for i in 0..len {
                    self.block_buf[i] = self.block_buf[i] * self.window[i];
                }

                self.fft.forward(&self.block_buf, &mut self.scratch);

                // 4‑bin batch magnitude via ComplexSoa eDSL
                let len = self.scratch.len();
                let mut i = 0usize;
                while i + 3 < len {
                    let c = soa_from_interleaved(&self.scratch[i..i + 4]);
                    let mag_sq = c.norm_sqr();
                    self.magnitude[i] = mag_sq.extract(0).to_f64().sqrt() as f32;
                    self.magnitude[i + 1] = mag_sq.extract(1).to_f64().sqrt() as f32;
                    self.magnitude[i + 2] = mag_sq.extract(2).to_f64().sqrt() as f32;
                    self.magnitude[i + 3] = mag_sq.extract(3).to_f64().sqrt() as f32;
                    i += 4;
                }
                while i < len {
                    let c = self.scratch[i];
                    self.magnitude[i] = (c.re * c.re + c.im * c.im).to_f64().sqrt() as f32;
                    i += 1;
                }

                output.fill(T::ZERO);
                Ok(())
            }
            None => {
                output.fill(T::ZERO);
                Ok(())
            }
        }
    }

    fn reset(&mut self) {
        self.block_buf.fill(T::ZERO);
        self.scratch
            .fill(num_complex::Complex::new(T::ZERO, T::ZERO));
        self.magnitude.fill(0.0);
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "FftSpectrumAnalyzer",
            category: AlgorithmCategory::Analyzer,
            description: "FFT-based spectrum analyzer",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental> Analyzer<T> for FftSpectrumAnalyzer<T> {
    type Output = Vec<f32>;

    fn result(&self) -> &Self::Output {
        &self.magnitude
    }

    fn reset_analysis(&mut self) {
        self.magnitude.fill(0.0);
    }
}

impl<T: Transcendental> SpectrumAnalyzer<T> for FftSpectrumAnalyzer<T> {
    fn fft_size(&self) -> usize {
        self.fft.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dc_input_gives_dc_bin() {
        let mut analyzer = FftSpectrumAnalyzer::<f32>::new(16);
        let input = [2.5f32; 16];
        let mut output = [0.0f32; 16];
        analyzer.process(Some(&input), &mut output).unwrap();

        let dc_level = analyzer.spectrum()[0];
        assert!(dc_level > 5.0, "DC bin too low: {dc_level}");

        for i in 3..analyzer.spectrum().len() {
            assert!(
                analyzer.spectrum()[i] < 0.5,
                "bin {i} should be near zero, got {}",
                analyzer.spectrum()[i]
            );
        }
    }

    #[test]
    fn test_sine_input_gives_peak() {
        let fft_size = 128;
        let mut analyzer = FftSpectrumAnalyzer::<f32>::new(fft_size);
        let freq = 1000.0;
        let sr = 44100.0;

        let input: Vec<f32> = (0..fft_size)
            .map(|i| {
                let t = i as f32 / sr;
                (2.0 * std::f32::consts::PI * freq * t).sin()
            })
            .collect();

        let mut output = vec![0.0f32; fft_size];
        analyzer.process(Some(&input), &mut output).unwrap();

        let expected_bin = (freq * fft_size as f32 / sr) as usize;
        let peak = analyzer.spectrum()[expected_bin];
        let nearby = if expected_bin > 0 {
            analyzer.spectrum()[expected_bin - 1]
        } else {
            0.0
        };

        assert!(
            peak > nearby * 2.0,
            "peak at bin {expected_bin} should dominate"
        );
        assert!(peak > 0.5, "peak magnitude too low: {peak}");
    }

    #[test]
    fn test_amplitude_at() {
        let fft_size = 256;
        let mut analyzer = FftSpectrumAnalyzer::<f32>::new(fft_size);
        let sr = fft_size as f32;
        let freq = 2.0;

        let input: Vec<f32> = (0..fft_size)
            .map(|i| {
                let t = i as f32 / sr;
                (2.0 * std::f32::consts::PI * freq * t).sin()
            })
            .collect();

        let mut output = vec![0.0f32; fft_size];
        analyzer.process(Some(&input), &mut output).unwrap();

        let amp = analyzer.amplitude_at(freq, sr);
        assert!(amp > 0.5, "amplitude at {freq} Hz too low: {amp}");

        let amp_far = analyzer.amplitude_at(freq * 5.0, sr);
        assert!(
            amp_far < 0.15,
            "amplitude at 5x freq should be low: {amp_far}"
        );
    }

    #[test]
    fn test_reset_clears_spectrum() {
        let mut analyzer = FftSpectrumAnalyzer::<f32>::new(16);
        let input = [1.0f32; 16];
        let mut output = [0.0f32; 16];
        analyzer.process(Some(&input), &mut output).unwrap();

        analyzer.reset();
        assert!(analyzer.spectrum().iter().all(|&v| v == 0.0));
    }
}
