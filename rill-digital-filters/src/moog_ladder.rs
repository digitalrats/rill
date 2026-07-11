use rill_core::traits::Algorithm;
use rill_core::Transcendental;
use rill_core_dsp::filters::MoogLadder;

/// Processor wrapper for Moog ladder filter
pub struct MoogLadderProcessor<T: Transcendental, const BUF_SIZE: usize> {
    pub cutoff: f32,
    pub resonance: f32,
    pub algorithm: MoogLadder<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> MoogLadderProcessor<T, BUF_SIZE> {
    pub fn new(sample_rate: f32) -> Self {
        let mut algorithm = MoogLadder::new(1000.0, 0.0);
        algorithm.init(sample_rate);

        Self {
            cutoff: 1000.0,
            resonance: 0.0,
            algorithm,
        }
    }

    pub fn cutoff(&self) -> f32 {
        self.cutoff
    }

    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.clamp(20.0, 20000.0);
        self.update_algorithm();
    }

    pub fn resonance(&self) -> f32 {
        self.resonance
    }

    pub fn set_resonance(&mut self, resonance: f32) {
        self.resonance = resonance.clamp(0.0, 1.0);
        self.update_algorithm();
    }

    pub fn algorithm(&self) -> &MoogLadder<T> {
        &self.algorithm
    }

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
