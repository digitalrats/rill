use rill_core::traits::Algorithm;
use rill_core::Transcendental;
use rill_core_dsp::filters::MoogLadder;

/// Processor wrapper for Moog ladder filter
pub struct MoogLadderProcessor<T: Transcendental, const BUF_SIZE: usize> {
    /// Filter cutoff frequency in Hz.
    pub cutoff: f32,
    /// Filter resonance amount (0.0–1.0).
    pub resonance: f32,
    /// The underlying Moog ladder algorithm.
    pub algorithm: MoogLadder<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> MoogLadderProcessor<T, BUF_SIZE> {
    /// Creates a Moog ladder processor with default parameters.
    pub fn new(sample_rate: f32) -> Self {
        let mut algorithm = MoogLadder::new(1000.0, 0.0);
        algorithm.init(sample_rate);

        Self {
            cutoff: 1000.0,
            resonance: 0.0,
            algorithm,
        }
    }

    /// Returns the current cutoff frequency in Hz.
    pub fn cutoff(&self) -> f32 {
        self.cutoff
    }

    /// Sets the cutoff frequency in Hz (clamped to 20–20000).
    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.clamp(20.0, 20000.0);
        self.update_algorithm();
    }

    /// Returns the current resonance amount.
    pub fn resonance(&self) -> f32 {
        self.resonance
    }

    /// Sets the resonance amount (clamped to 0.0–1.0).
    pub fn set_resonance(&mut self, resonance: f32) {
        self.resonance = resonance.clamp(0.0, 1.0);
        self.update_algorithm();
    }

    /// Returns a reference to the inner algorithm.
    pub fn algorithm(&self) -> &MoogLadder<T> {
        &self.algorithm
    }

    /// Returns a mutable reference to the inner algorithm.
    pub fn algorithm_mut(&mut self) -> &mut MoogLadder<T> {
        &mut self.algorithm
    }

    fn update_algorithm(&mut self) {
        self.algorithm.set_cutoff(self.cutoff);
        self.algorithm.set_resonance(self.resonance);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_moog_ladder_processor() {
        let processor = MoogLadderProcessor::<f32, 64>::new(44100.0);
        assert_eq!(processor.cutoff, 1000.0);
        assert_eq!(processor.resonance, 0.0);
    }
}
