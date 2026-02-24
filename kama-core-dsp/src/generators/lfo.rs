//! Низкочастотные генераторы для модуляции

use crate::math::AudioNum;
use crate::algorithm::{Algorithm, AlgorithmMetadata, AlgorithmCategory};
use crate::generators::Generator;  // <-- Импортируем Generator
use super::basic::{BasicOscillator, Waveform};

/// LFO генератор (Low Frequency Oscillator)
pub struct LFO<T: AudioNum> {
    /// Внутренний осциллятор
    osc: BasicOscillator<T>,
    /// Биполярный режим (-1..1) или униполярный (0..1)
    bipolar: bool,
    /// Задержка фазы (для синхронизации)
    phase_offset: T,
}

impl<T: AudioNum> LFO<T> {
    /// Создать новый LFO
    pub fn new(frequency: f32, waveform: Waveform, bipolar: bool) -> Self {
        let one = T::from_f32(1.0);
        Self {
            osc: BasicOscillator::new(waveform, frequency, one),
            bipolar,
            phase_offset: T::ZERO,
        }
    }
    
    /// Установить биполярный режим
    pub fn set_bipolar(&mut self, bipolar: bool) {
        self.bipolar = bipolar;
    }
    
    /// Установить смещение фазы (0..1)
    pub fn set_phase_offset(&mut self, offset: T) {
        let one = T::from_f32(1.0);
        let zero = T::ZERO;
        self.phase_offset = if offset > one { one } else if offset < zero { zero } else { offset };
    }
    
    /// Синхронизировать с внешним clock
    pub fn sync(&mut self, reset: bool) {
        if reset {
            self.osc.set_phase(self.phase_offset);
        }
    }
    
    /// Получить значение для модуляции
    pub fn modulate(&mut self) -> T {
        // Используем process_sample из Algorithm
        let raw = self.osc.process_sample(T::ZERO);
        
        if self.bipolar {
            raw // уже -1..1
        } else {
            // Конвертируем в 0..1
            raw.mul(T::from_f32(0.5)).add(T::from_f32(0.5))
        }
    }
}

impl<T: AudioNum> Algorithm<T> for LFO<T> {
    fn init(&mut self, sample_rate: f32) {
        self.osc.init(sample_rate);
        self.osc.set_phase(self.phase_offset);
    }
    
    fn reset(&mut self) {
        self.osc.reset();
        self.osc.set_phase(self.phase_offset);
    }
    
    fn process_sample(&mut self, _input: T) -> T {
        self.modulate()
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "LFO",
            category: AlgorithmCategory::Generator,
            description: "Low Frequency Oscillator for modulation".to_string(),
            author: "Kama Audio",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
    
    fn as_any(&self) -> &dyn std::any::Any 
    where
        Self: 'static,
    {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any 
    where
        Self: 'static,
    {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_lfo_creation() {
        let mut lfo = LFO::<f32>::new(1.0, Waveform::Sine, false);
        lfo.init(44100.0);
        
        let val = lfo.process_sample(0.0);
        assert!(val >= -1.0 && val <= 1.0);
    }
}