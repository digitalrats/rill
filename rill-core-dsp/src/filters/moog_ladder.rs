//! Moog ladder filter — classic 4-pole lowpass with resonance
//!
//! Uses four `OnePole` filters in series with one-sample-delayed
//! resonance feedback.  Resonance is clamped to `[-1, 1]` to prevent
//! runaway oscillation.

use rill_core::traits::{ActionContext, ProcessResult};
use rill_core::AudioNum;
use super::{Filter, FilterParams, OnePole};
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};

/// Moog ladder 4-pole lowpass filter
pub struct MoogLadder<T: AudioNum> {
    stages: [OnePole<T>; 4],
    cutoff: f32,
    resonance: f32,
    sample_rate: f32,
    feedback_prev: T,
}

impl<T: AudioNum> MoogLadder<T> {
    /// Create a new Moog ladder filter
    pub fn new(cutoff: f32, resonance: f32) -> Self {
        let params = FilterParams {
            filter_type: super::FilterType::LowPass,
            cutoff,
            q: 0.0,
            gain_db: 0.0,
        };

        Self {
            stages: [
                OnePole::new(params.clone()),
                OnePole::new(params.clone()),
                OnePole::new(params.clone()),
                OnePole::new(params),
            ],
            cutoff,
            resonance,
            sample_rate: 44100.0,
            feedback_prev: T::ZERO,
        }
    }

    /// Get cutoff frequency (Hz)
    pub fn cutoff(&self) -> f32 {
        self.cutoff
    }

    /// Set cutoff frequency (Hz)
    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.clamp(20.0, 20000.0);
        for stage in &mut self.stages {
            stage.set_cutoff(self.cutoff);
        }
    }

    /// Get resonance (0.0 – 1.0)
    pub fn resonance(&self) -> f32 {
        self.resonance
    }

    /// Set resonance (0.0 – 1.0, higher = more feedback)
    pub fn set_resonance(&mut self, resonance: f32) {
        self.resonance = resonance.clamp(0.0, 1.0);
    }

    /// Process a single sample
    pub fn process_sample(&mut self, input: T) -> T {
        let feedback = self.feedback_prev * T::from_f32(self.resonance * 4.0);
        let clamped = feedback.clamp(T::from_f32(-1.0), T::from_f32(1.0));

        let x = input - clamped;

        let mut out = x;
        for stage in &mut self.stages {
            out = stage.process_sample(out);
        }

        self.feedback_prev = out;
        out
    }
}

impl<T: AudioNum> Algorithm<T> for MoogLadder<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for stage in &mut self.stages {
            stage.init(sample_rate);
        }
        self.feedback_prev = T::ZERO;
    }

    fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.reset();
        }
        self.feedback_prev = T::ZERO;
    }

    fn process(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());
        for i in 0..len {
            output[i] = self.process_sample(input[i]);
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Moog Ladder Filter",
            category: AlgorithmCategory::Filter,
            description: "Classic 4-pole Moog transistor ladder VCF",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_moog_ladder_creation() {
        let mut filter = MoogLadder::<f32>::new(1000.0, 0.5);
        filter.init(44100.0);
        assert_eq!(filter.cutoff, 1000.0);
        assert!((filter.resonance - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_moog_ladder_process_sample() {
        let mut filter = MoogLadder::<f32>::new(1000.0, 0.0);
        filter.init(44100.0);

        // DC should pass through (filter is lowpass at 1000Hz)
        let mut out = 0.0_f32;
        for _ in 0..100 {
            out = filter.process_sample(0.5);
        }

        // With resonance = 0, output should be near 0.5 for DC through 4 one-poles
        assert!((out - 0.5).abs() < 0.1);
    }

    #[test]
    fn test_moog_ladder_resonance() {
        let mut filter = MoogLadder::<f32>::new(1000.0, 0.8);
        filter.init(44100.0);

        // Resonance should boost the signal
        let mut out = 0.0_f32;
        for i in 0..1000 {
            let t = i as f32 / 44100.0;
            let input = (2.0 * std::f32::consts::PI * 1000.0 * t).sin() * 0.1;
            out = filter.process_sample(input);
        }

        // At high resonance, output should be non-zero
        assert!(out.abs() > 0.0);
    }
}
