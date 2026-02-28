//! LFO автомат — генерирует периодические сигналы

use crate::automaton::{Automaton, SourceAutomaton, SignalValue};
use crate::core::{WorldTime, WorldSignal};
use std::f32::consts::PI;

/// Форма волны LFO
#[derive(Debug, Clone, Copy)]
pub enum LfoWaveform {
    Sine,
    Triangle,
    Saw,
    Square,
    SampleAndHold,
}

/// LFO автомат
pub struct LfoAutomaton {
    name: String,
    frequency: f32,
    amplitude: f32,
    offset: f32,
    waveform: LfoWaveform,
    phase: f32,
    last_value: f32,
    sample_rate: f32,  // Тиков в секунду
}

impl LfoAutomaton {
    pub fn new(name: impl Into<String>, sample_rate: f32) -> Self {
        Self {
            name: name.into(),
            frequency: 1.0,
            amplitude: 1.0,
            offset: 0.0,
            waveform: LfoWaveform::Sine,
            phase: 0.0,
            last_value: 0.0,
            sample_rate,
        }
    }
    
    pub fn with_frequency(mut self, freq: f32) -> Self {
        self.frequency = freq.max(0.01).min(100.0);
        self
    }
    
    pub fn with_waveform(mut self, waveform: LfoWaveform) -> Self {
        self.waveform = waveform;
        self
    }
    
    pub fn with_range(mut self, min: f32, max: f32) -> Self {
        self.offset = (min + max) * 0.5;
        self.amplitude = (max - min) * 0.5;
        self
    }
    
    fn generate(&mut self, delta: f32) -> f32 {
        self.phase += self.frequency * delta;
        self.phase = self.phase.fract();
        
        let raw = match self.waveform {
            LfoWaveform::Sine => (self.phase * 2.0 * PI).sin(),
            LfoWaveform::Triangle => {
                if self.phase < 0.5 {
                    4.0 * self.phase - 1.0
                } else {
                    3.0 - 4.0 * self.phase
                }
            }
            LfoWaveform::Saw => 2.0 * self.phase - 1.0,
            LfoWaveform::Square => {
                if self.phase < 0.5 { 1.0 } else { -1.0 }
            }
            LfoWaveform::SampleAndHold => {
                if self.phase < self.frequency * delta {
                    rand::random::<f32>() * 2.0 - 1.0
                } else {
                    self.last_value
                }
            }
        };
        
        self.last_value = raw * self.amplitude + self.offset;
        self.last_value
    }
}

impl Automaton for LfoAutomaton {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn process(&mut self, time: WorldTime, _inputs: &[WorldSignal]) -> Vec<WorldSignal> {
        let value = self.generate(time.delta as f32);
        
        vec![WorldSignal::new(
            crate::core::SignalOrigin::Automaton(self.name.clone()),
            SignalValue::continuous((value + 1.0) * 0.5), // -1..1 → 0..1
        )]
    }
    
    fn peek(&self) -> f32 {
        self.last_value
    }
    
    fn reset(&mut self) {
        self.phase = 0.0;
        self.last_value = 0.0;
    }
}

impl SourceAutomaton for LfoAutomaton {
    fn set_parameter(&mut self, name: &str, value: f32) {
        match name {
            "frequency" => self.frequency = value.max(0.01).min(100.0),
            "amplitude" => self.amplitude = value.clamp(0.0, 1.0),
            "offset" => self.offset = value.clamp(-1.0, 1.0),
            _ => {}
        }
    }
}