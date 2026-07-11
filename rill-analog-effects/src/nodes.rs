use crate::CassetteDeck;
use rill_core::prelude::*;

pub struct CassetteDeckProcessor<T: Transcendental, const BUF_SIZE: usize> {
    pub algorithm: CassetteDeck,
    pub tape_speed: f32,
    pub bias_level: f32,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> CassetteDeckProcessor<T, BUF_SIZE> {
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
    pub fn set_tape_speed(&mut self, speed: f32) {
        self.tape_speed = speed.clamp(1.19, 19.05);
        self.algorithm.set_tape_speed(self.tape_speed as f64);
    }
    pub fn set_bias_level(&mut self, bias: f32) {
        self.bias_level = bias.clamp(0.0, 1.0);
        self.algorithm.set_bias_level(self.bias_level as f64);
    }
}
