//! Sine wave generator processor

use std::sync::Arc;
use parking_lot::RwLock;

use crate::engine::AudioProcessor;

const TWO_PI: f32 = std::f32::consts::TAU;

/// Процессор, генерирующий синусоидальную волну
pub struct SineProcessor {
    frequency: f32,
    sample_rate: Arc<RwLock<f32>>,
    phase: f32,
    position: usize,
}

impl SineProcessor {
    /// Создать новый процессор с заданной частотой
    pub fn new(frequency: f32, sample_rate: f32) -> Self {
        Self {
            frequency,
            sample_rate: Arc::new(RwLock::new(sample_rate)),
            phase: 0.0,
            position: 0,
        }
    }

    /// Изменить частоту
    pub fn set_frequency(&mut self, frequency: f32) {
        self.frequency = frequency;
    }
}

impl AudioProcessor for SineProcessor {
    fn process(&mut self, _input: &[f32], output: &mut [f32]) {
        let sample_rate = *self.sample_rate.read();
        let n = output.len() / 2;
        for i in 0..n {
            let sample = (self.phase * TWO_PI).sin() * 0.5;
            output[i * 2] = sample;
            output[i * 2 + 1] = sample;
            self.phase += self.frequency / sample_rate;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
        self.position += n;
    }

    fn reset(&mut self) {
        self.position = 0;
        self.phase = 0.0;
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        *self.sample_rate.write() = sample_rate;
    }
}
