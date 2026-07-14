use crate::CassetteDeck;
use rill_core::prelude::*;

/// Cassette deck processor with tape speed and bias control.
pub struct CassetteDeckProcessor<T: Transcendental, const BUF_SIZE: usize> {
    /// The cassette deck algorithm.
    pub algorithm: CassetteDeck,
    /// Tape speed in cm/s (1.19–19.05).
    pub tape_speed: f32,
    /// Bias level (0.0–1.0).
    pub bias_level: f32,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> CassetteDeckProcessor<T, BUF_SIZE> {
    /// Creates a cassette deck processor with default parameters.
    pub fn new(sample_rate: f32) -> Self {
        let mut deck = CassetteDeck::new(sample_rate as f64);
        deck.set_tape_speed(4.76);
        deck.set_bias_level(0.8);
        Self {
            algorithm: deck,
            tape_speed: 4.76,
            bias_level: 0.8,
            _phantom: std::marker::PhantomData,
        }
    }
    /// Sets tape speed in cm/s (clamped to 1.19–19.05).
    pub fn set_tape_speed(&mut self, speed: f32) {
        self.tape_speed = speed.clamp(1.19, 19.05);
        self.algorithm.set_tape_speed(self.tape_speed as f64);
    }
    /// Sets bias level (clamped to 0.0–1.0).
    pub fn set_bias_level(&mut self, bias: f32) {
        self.bias_level = bias.clamp(0.0, 1.0);
        self.algorithm.set_bias_level(self.bias_level as f64);
    }
}
