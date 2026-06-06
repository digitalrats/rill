//! Sample-rate converter built on [`InterpolatedReader`].
//!
//! Provides [`Resampler`] for converting audio between different sample rates
//! using linear or cubic interpolation. Accepts a pre-loaded buffer at
//! `source_rate` and outputs at `target_rate` (set via [`Algorithm::init`]).
//!
//! # Use case
//!
//! A WAV loaded at 44100 Hz must play back through a JACK backend that
//! negotiated 48000 Hz. `Resampler` computes the ratio `44100/48000` and
//! reads the buffer with interpolation, producing correct-speed output.
//!
//! # RT safety
//!
//! All heap allocation happens at construction time. The `process` method
//! performs only reads and math — no allocation, no locking.

use crate::generators::InterpolatedReader;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;

/// Sample-rate converter wrapping [`InterpolatedReader`].
///
/// Accepts a pre-loaded buffer with a known source sample rate and outputs
/// at a target rate that can differ from the source rate.
///
/// # Examples
///
/// ```
/// use rill_core::traits::Algorithm;
/// use rill_core_dsp::generators::Resampler;
///
/// let source_rate = 44100.0;
/// let samples = vec![0.0f32; 1024];
/// let mut rs = Resampler::new(samples, source_rate);
/// rs.init(48000.0);
/// let mut out = vec![0.0f32; 512];
/// rs.process(None, &mut out).unwrap();
/// assert!((rs.position() - 512.0 * 44100.0 / 48000.0).abs() < 1.0);
/// ```
pub struct Resampler<T: Transcendental> {
    reader: InterpolatedReader<T>,
    source_rate: f64,
    target_rate: f64,
}

impl<T: Transcendental> Resampler<T> {
    /// Create a new resampler from a sample buffer and its source sample rate.
    ///
    /// The resampler starts with `target_rate = source_rate` (passthrough).
    /// Call [`Algorithm::init`] or [`set_target_rate`] to convert to a
    /// different rate.
    pub fn new(buffer: Vec<T>, source_rate: f64) -> Self {
        let mut reader = InterpolatedReader::new(buffer);
        reader.set_rate(1.0);
        Self {
            reader,
            source_rate,
            target_rate: source_rate,
        }
    }

    /// Create a resampler from a pre-allocated boxed slice.
    pub fn from_boxed(buffer: Box<[T]>, source_rate: f64) -> Self {
        let mut reader = InterpolatedReader::from_boxed(buffer);
        reader.set_rate(1.0);
        Self {
            reader,
            source_rate,
            target_rate: source_rate,
        }
    }

    /// Number of samples in the source buffer.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.reader.len()
    }

    /// Returns `true` if the source buffer is empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.reader.is_empty()
    }

    /// Source sample rate in Hz.
    #[inline(always)]
    pub fn source_rate(&self) -> f64 {
        self.source_rate
    }

    /// Set the source sample rate and recompute the interpolation ratio.
    #[inline(always)]
    pub fn set_source_rate(&mut self, hz: f64) {
        self.source_rate = hz;
        self.update_ratio();
    }

    /// Target sample rate in Hz.
    #[inline(always)]
    pub fn target_rate(&self) -> f64 {
        self.target_rate
    }

    /// Set the target sample rate and recompute the interpolation ratio.
    #[inline(always)]
    pub fn set_target_rate(&mut self, hz: f64) {
        self.target_rate = hz;
        self.update_ratio();
    }

    /// Enable (`true`) or disable (`false`) cubic Hermite interpolation.
    ///
    /// Linear interpolation (default) is faster; cubic gives higher quality
    /// at the cost of more computation.
    #[inline(always)]
    pub fn set_cubic(&mut self, cubic: bool) {
        self.reader.set_cubic(cubic);
    }

    /// Returns `true` if cubic interpolation is enabled.
    #[inline(always)]
    pub fn is_cubic(&self) -> bool {
        self.reader.is_cubic()
    }

    /// Current read position in the source buffer (in source samples).
    #[inline(always)]
    pub fn position(&self) -> f64 {
        self.reader.position()
    }

    /// Set the read position in the source buffer.
    #[inline(always)]
    pub fn set_position(&mut self, pos: f64) {
        self.reader.set_position(pos);
    }

    /// Replace the source buffer and reset position to 0.
    ///
    /// The new buffer is assumed to have the same source rate.
    pub fn set_buffer(&mut self, buffer: Vec<T>) {
        self.reader.set_buffer(buffer);
    }

    /// Return the internal buffer as an immutable slice.
    #[inline(always)]
    pub fn as_slice(&self) -> &[T] {
        self.reader.as_slice()
    }

    /// The computed interpolation ratio (`source_rate / target_rate`).
    ///
    /// When `ratio > 1.0` the source runs faster → downsampling.
    /// When `ratio < 1.0` the source runs slower → upsampling.
    #[inline(always)]
    pub fn ratio(&self) -> f64 {
        self.reader.rate()
    }

    /// Recompute the reader's rate from source and target rates.
    fn update_ratio(&mut self) {
        let ratio = if self.target_rate > 0.0 {
            self.source_rate / self.target_rate
        } else {
            1.0
        };
        self.reader.set_rate(ratio);
    }
}

impl<T: Transcendental> Algorithm<T> for Resampler<T> {
    fn init(&mut self, sample_rate: f32) {
        self.set_target_rate(sample_rate as f64);
    }

    fn reset(&mut self) {
        self.reader.set_position(0.0);
    }

    fn process(&mut self, _input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.reader.render_block(output);
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Resampler",
            category: AlgorithmCategory::Utility,
            description: "Sample-rate converter using linear or cubic interpolation",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(rs: &mut Resampler<f64>, out: &mut [f64]) {
        rs.process(None, out).unwrap();
    }

    #[test]
    fn test_passthrough() {
        let buf = vec![1.0f64, 2.0, 3.0, 4.0];
        let mut rs = Resampler::new(buf, 44100.0);
        rs.init(44100.0);

        let mut out = [0.0f64; 4];
        process(&mut rs, &mut out);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_upsample_2x() {
        let buf = vec![0.0f64, 10.0];
        let mut rs = Resampler::new(buf, 22050.0);
        rs.init(44100.0);

        // ratio = 0.5 → each source sample spans 2 output samples
        let mut out = [0.0f64; 4];
        process(&mut rs, &mut out);
        // position: 0.0 → 0.5 → 1.0 → 1.5
        assert!((out[0] - 0.0).abs() < 1e-10, "pos 0.0, got {}", out[0]);
        assert!((out[1] - 5.0).abs() < 1e-10, "pos 0.5, got {}", out[1]);
        assert!((out[2] - 10.0).abs() < 1e-10, "pos 1.0, got {}", out[2]);
        assert!(
            (out[3] - 10.0).abs() < 1e-10,
            "pos 1.5 clamped, got {}",
            out[3]
        );
    }

    #[test]
    fn test_downsample_2x() {
        let buf: Vec<f64> = (0..8).map(|i| i as f64 * 100.0).collect();
        let mut rs = Resampler::new(buf, 88200.0);
        rs.init(44100.0);

        // ratio = 2.0 → skip every other source sample
        let mut out = [0.0f64; 4];
        process(&mut rs, &mut out);
        assert!((out[0] - 0.0).abs() < 1e-10);
        assert!((out[1] - 200.0).abs() < 1e-10);
        assert!((out[2] - 400.0).abs() < 1e-10);
        assert!((out[3] - 600.0).abs() < 1e-10);
    }

    #[test]
    fn test_44k1_to_48k_non_integer_ratio() {
        let buf: Vec<f64> = (0..441).map(|i| i as f64 * 0.01).collect();
        let mut rs = Resampler::new(buf, 44100.0);
        rs.init(48000.0);
        rs.set_cubic(true);

        // ratio = 44100/48000 ≈ 0.91875
        let abs_diff = (rs.ratio() - 44100.0 / 48000.0).abs();
        assert!(abs_diff < 1e-10, "ratio mismatch: {}", rs.ratio());

        let mut out = [0.0f64; 480];
        process(&mut rs, &mut out);
        // position after 480 output samples ≈ 480 * 0.91875 = 441
        let expected_pos = 480.0 * 44100.0 / 48000.0;
        let pos_diff = (rs.position() - expected_pos).abs();
        assert!(pos_diff < 1e-9, "position mismatch: {}", rs.position());
    }

    #[test]
    fn test_empty_buffer() {
        let buf: Vec<f64> = vec![];
        let mut rs = Resampler::new(buf, 44100.0);
        rs.init(48000.0);
        let mut out = [1.0f64; 4];
        process(&mut rs, &mut out);
        assert_eq!(out, [0.0; 4]);
    }

    #[test]
    fn test_set_source_rate_dynamic() {
        let buf = vec![0.0f64, 10.0];
        let mut rs = Resampler::new(buf, 44100.0);
        rs.init(44100.0);
        assert!((rs.ratio() - 1.0).abs() < 1e-10);

        rs.set_source_rate(22050.0);
        assert!((rs.ratio() - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_reset() {
        let buf: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let mut rs = Resampler::new(buf, 44100.0);
        rs.init(44100.0);

        let mut out = [0.0f64; 3];
        process(&mut rs, &mut out);
        assert!((rs.position() - 3.0).abs() < 1e-10);

        rs.reset();
        assert!((rs.position() - 0.0).abs() < 1e-10);
    }
}
