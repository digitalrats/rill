use crate::node::{AudioNode, NodeMetadata, NodeCategory};
use crate::param::{ParamValue, ParamType};
use crate::AudioError;

/// Синусоидальный осциллятор
pub struct SineOscillator {
    frequency: f32,
    phase: f32,
    sample_rate: f32,
    amplitude: f32,
}

impl SineOscillator {
    pub fn new(frequency: f32) -> Self {
        Self {
            frequency,
            phase: 0.0,
            sample_rate: 44100.0,
            amplitude: 1.0,
        }
    }
    
    pub fn with_amplitude(mut self, amplitude: f32) -> Self {
        self.amplitude = amplitude.max(0.0).min(1.0);
        self
    }
}

impl AudioNode for SineOscillator {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }
        
        let output = &mut outputs[0];
        let phase_increment = self.frequency / self.sample_rate * 2.0 * std::f32::consts::PI;
        
        for sample in output.iter_mut() {
            *sample = self.phase.sin() * self.amplitude;
            self.phase += phase_increment;
            
            // Нормализовать фазу для сохранения точности
            if self.phase >= 2.0 * std::f32::consts::PI {
                self.phase -= 2.0 * std::f32::consts::PI;
            }
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "frequency" => Some(ParamValue::Float(self.frequency)),
            "amplitude" => Some(ParamValue::Float(self.amplitude)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("frequency", ParamValue::Float(f)) => {
                self.frequency = f.max(0.0).min(self.sample_rate / 2.0);
                Ok(())
            }
            ("amplitude", ParamValue::Float(a)) => {
                self.amplitude = a.max(0.0).min(1.0);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }
    
    fn reset(&mut self) {
        self.phase = 0.0;
    }
    
    fn num_inputs(&self) -> usize { 0 } // Осциллятор не имеет входов
    fn num_outputs(&self) -> usize { 1 }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Sine Oscillator".to_string(),
            category: NodeCategory::Generator,
            description: "Simple sine wave oscillator".to_string(),
            author: "Kama Core".to_string(),
            version: "1.0".to_string(),
            parameters: vec![
                crate::node::ParamMetadata {
                    name: "frequency".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(440.0),
                    min: Some(20.0),
                    max: Some(20000.0),
                    step: Some(1.0),
                    unit: Some("Hz".to_string()),
                    choices: None,
                },
                crate::node::ParamMetadata {
                    name: "amplitude".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("linear".to_string()),
                    choices: None,
                },
            ],
        }
    }
}