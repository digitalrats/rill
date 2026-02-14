//! Sine wave generator processor

use std::sync::Arc;
use parking_lot::RwLock;

use kama_core::dsp::SineOscillator;
use kama_core::AudioNode;

use crate::engine::AudioProcessor;

/// Процессор, генерирующий синусоидальную волну
pub struct SineProcessor {
    oscillator: SineOscillator,
    sample_rate: Arc<RwLock<f32>>,
    position: usize,
}

impl SineProcessor {
    /// Создать новый процессор с заданной частотой
    pub fn new(frequency: f32, sample_rate: f32) -> Self {
        Self {
            oscillator: SineOscillator::new(frequency),
            sample_rate: Arc::new(RwLock::new(sample_rate)),
            position: 0,
        }
    }
    
    /// Изменить частоту
    pub fn set_frequency(&mut self, frequency: f32) {
        self.oscillator.set_param("frequency", kama_core::param::ParamValue::Float(frequency))
            .unwrap_or(());
    }
}

impl AudioProcessor for SineProcessor {
    fn process(&mut self, _input: &[f32], output: &mut [f32]) {
        let sample_rate = *self.sample_rate.read();
        let mut temp = vec![0.0f32; output.len() / 2];
        
        let mut temp_slice = [temp.as_mut_slice()];
        self.oscillator.process(&[], &mut temp_slice).unwrap();
        
        // Копируем на левый и правый каналы
        for i in 0..temp.len() {
            output[i * 2] = temp[i] * 0.5;
            output[i * 2 + 1] = temp[i] * 0.5;
        }
        
        self.position += temp.len();
    }
    
    fn reset(&mut self) {
        self.position = 0;
        self.oscillator.reset();
    }
    
    fn set_sample_rate(&mut self, sample_rate: f32) {
        *self.sample_rate.write() = sample_rate;
        self.oscillator.init(sample_rate);
    }
}