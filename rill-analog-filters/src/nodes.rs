use rill_core::prelude::*;
use rill_core_model::wdf::MoogLadder;

/// WDF-based Moog ladder filter processor for signal graphs.
pub struct WdfMoogLadderProcessor<T: Transcendental, const BUF_SIZE: usize> {
    /// The WDF Moog ladder algorithm implementation.
    pub algorithm: MoogLadder<f64>,
    /// Filter cutoff frequency in Hz.
    pub cutoff: f32,
    /// Filter resonance amount (0.0–1.0).
    pub resonance: f32,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> WdfMoogLadderProcessor<T, BUF_SIZE> {
    /// Creates a WDF Moog ladder processor with default parameters.
    pub fn new(sample_rate: f32) -> Self {
        let pole = rill_core_model::wdf::RcPole::new(0.0);
        let mut algorithm = MoogLadder::new(pole, 1000.0, 0.0, sample_rate as f64);
        algorithm.update_coeffs();
        algorithm.set_cutoff(1000.0);
        Self {
            algorithm,
            cutoff: 1000.0,
            resonance: 0.0,
            _phantom: std::marker::PhantomData,
        }
    }
    /// Sets the cutoff frequency in Hz (clamped to 20–20000).
    pub fn set_cutoff(&mut self, cutoff: f32) {
        self.cutoff = cutoff.clamp(20.0, 20000.0);
        self.algorithm.set_cutoff(self.cutoff as f64);
    }
    /// Sets the resonance amount (clamped to 0.0–1.0).
    pub fn set_resonance(&mut self, resonance: f32) {
        self.resonance = resonance.clamp(0.0, 1.0);
        self.algorithm.set_resonance(self.resonance as f64);
    }
}
