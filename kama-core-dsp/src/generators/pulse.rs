//! Pulse wave генератор с PWM (Pulse Width Modulation)

use kama_core::AudioNum;
use crate::algorithm::{Algorithm, AlgorithmMetadata, AlgorithmCategory};
use super::Generator;

/// Pulse wave генератор с PWM
pub struct PulseOscillator<T: AudioNum> {
    /// Базовая частота
    frequency: f32,
    /// Амплитуда
    amplitude: T,
    /// Ширина импульса (0..1)
    pulse_width: T,
    /// Модуляция ширины (PWM)
    pwm_amount: T,
    /// Текущая фаза
    phase: T,
    /// Инкремент фазы
    phase_inc: T,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum> PulseOscillator<T> {
    /// Создать новый pulse генератор
    pub fn new(frequency: f32, pulse_width: T) -> Self {
        let mut osc = Self {
            frequency,
            amplitude: T::from_f32(1.0),
            pulse_width: pulse_width.clamp(T::ZERO, T::from_f32(1.0)),
            pwm_amount: T::ZERO,
            phase: T::ZERO,
            phase_inc: T::ZERO,
            sample_rate: 44100.0,
        };
        osc.update_phase_inc();
        osc
    }
    
    fn update_phase_inc(&mut self) {
        self.phase_inc = T::from_f32(self.frequency / self.sample_rate);
    }
    
    /// Установить ширину импульса
    pub fn set_pulse_width(&mut self, width: T) {
        self.pulse_width = width.clamp(T::from_f32(0.01), T::from_f32(0.99));
    }
    
    /// Установить глубину PWM
    pub fn set_pwm_amount(&mut self, amount: T) {
        self.pwm_amount = amount.clamp(T::ZERO, T::from_f32(1.0));
    }
    
    /// Применить внешнюю модуляцию к ширине импульса
    pub fn modulate_pulse_width(&mut self, modulation: T) -> T {
        let modulated = self.pulse_width.add(modulation.mul(self.pwm_amount));
        modulated.clamp(T::from_f32(0.01), T::from_f32(0.99))
    }
    
    /// Anti-aliased pulse wave
    fn generate_pulse(&mut self, width: T) -> T {
        let raw = if self.phase.to_f32() < width.to_f32() {
            self.amplitude
        } else {
            self.amplitude.neg()
        };
        
        // Blep коррекция для обоих фронтов
        let inc = self.phase_inc;
        let next_phase = self.phase.add(inc);
        
        let mut blep = T::ZERO;
        
        // Восходящий фронт
        if self.phase < width && next_phase >= width {
            let t = width.sub(self.phase).div(inc);
            blep = blep.add(T::from_f32(2.0).mul(t).sub(T::from_f32(1.0)));
        }
        
        // Нисходящий фронт (при переполнении фазы)
        if next_phase.to_f32() >= 1.0 {
            let t = T::from_f32(1.0).sub(self.phase).div(inc);
            blep = blep.sub(T::from_f32(2.0).mul(t).sub(T::from_f32(1.0)));
        }
        
        raw.add(blep.mul(self.amplitude))
    }
}

impl<T: AudioNum> Algorithm<T> for PulseOscillator<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_phase_inc();
        self.phase = T::ZERO;
    }
    
    fn reset(&mut self) {
        self.phase = T::ZERO;
    }
    
    fn process_sample(&mut self, modulation: T) -> T {
        let width = self.modulate_pulse_width(modulation);
        let output = self.generate_pulse(width);
        
        self.phase = self.phase.add(self.phase_inc);
        if self.phase.to_f32() >= 1.0 {
            self.phase = self.phase.sub(T::from_f32(1.0));
        }
        
        output
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Pulse Oscillator",
            category: AlgorithmCategory::Generator,
            description: "Pulse wave oscillator with PWM".to_string(),
            author: "Kama Audio",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: AudioNum> Generator<T> for PulseOscillator<T> {
    fn phase(&self) -> T { self.phase }
    fn set_phase(&mut self, phase: T) { self.phase = phase; }
    fn frequency(&self) -> f32 { self.frequency }
    fn set_frequency(&mut self, freq: f32) { 
        self.frequency = freq;
        self.update_phase_inc();
    }
    fn amplitude(&self) -> T { self.amplitude }
    fn set_amplitude(&mut self, amp: T) { self.amplitude = amp; }
}