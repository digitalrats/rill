//! Sine wave oscillator

use kama_core_traits::{
    AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId,
    param::{ParamType, ParamMetadata}
};
use crate::audio::AudioOscillator;  // <-- добавляем импорт
use std::f32::consts::PI;

/// Sine wave oscillator for audio frequencies
pub struct SineOsc {
    /// Current phase in radians (0 to 2*PI)
    phase: f32,
    /// Frequency in Hz
    frequency: f32,
    /// Sample rate in Hz
    sample_rate: f32,
    /// Output amplitude (0.0 - 1.0)
    amplitude: f32,
}

impl SineOsc {
    /// Create a new sine oscillator
    pub fn new(frequency: f32) -> Self {
        Self {
            phase: 0.0,
            frequency,
            sample_rate: 44100.0,
            amplitude: 1.0,
        }
    }
    
    /// Create with custom amplitude
    pub fn with_amplitude(mut self, amp: f32) -> Self {
        self.amplitude = amp.clamp(0.0, 1.0);
        self
    }
    
    /// Generate next sample and advance phase
    pub fn generate(&mut self) -> f32 {
        let sample = self.phase.sin() * self.amplitude;
        
        let phase_inc = 2.0 * PI * self.frequency / self.sample_rate;
        self.phase += phase_inc;
        if self.phase > 2.0 * PI {
            self.phase -= 2.0 * PI;
        }
        
        sample
    }
    
    /// Generate a block of samples
    pub fn generate_block(&mut self, output: &mut [f32]) {
        let phase_inc = 2.0 * PI * self.frequency / self.sample_rate;
        
        for out in output.iter_mut() {
            *out = self.phase.sin() * self.amplitude;
            self.phase += phase_inc;
            if self.phase > 2.0 * PI {
                self.phase -= 2.0 * PI;
            }
        }
    }
}

impl AudioOscillator for SineOsc {
    fn set_frequency(&mut self, freq: f32) {
        self.frequency = freq.max(20.0).min(20000.0);
    }
    
    fn frequency(&self) -> f32 {
        self.frequency
    }
    
    fn set_amplitude(&mut self, amp: f32) {
        self.amplitude = amp.clamp(0.0, 1.0);
    }
    
    fn amplitude(&self) -> f32 {
        self.amplitude
    }
    
    fn reset_phase(&mut self) {
        self.phase = 0.0;
    }
}

impl AudioNode for SineOsc {
    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }
        
        let output = &mut outputs[0];
        self.generate_block(output);
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "frequency" => Some(ParamValue::Float(self.frequency)),
            "amplitude" => Some(ParamValue::Float(self.amplitude)),
            "phase" => Some(ParamValue::Float(self.phase / (2.0 * PI))),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("frequency", ParamValue::Float(f)) => {
                self.set_frequency(f);
                Ok(())
            }
            ("amplitude", ParamValue::Float(a)) => {
                self.set_amplitude(a);
                Ok(())
            }
            ("phase", ParamValue::Float(p)) => {
                self.phase = (p * 2.0 * PI).clamp(0.0, 2.0 * PI);
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
    
    fn num_inputs(&self) -> usize { 0 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Sine Oscillator".to_string(),
            category: NodeCategory::Generator,
            description: "Pure sine wave generator".to_string(),
            author: "Kama Oscillators".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "frequency".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(440.0),
                    min: Some(20.0),
                    max: Some(20000.0),
                    step: Some(1.0),
                    unit: Some("Hz".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "amplitude".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("gain".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "phase".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.0),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("cycles".to_string()),
                    choices: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;
    
    #[test]
    fn test_sine_osc_generate() {
        let mut osc = SineOsc::new(440.0).with_amplitude(0.5);
        osc.init(44100.0);
        
        let sample = osc.generate();
        assert!(approx_eq!(f32, sample, 0.0, epsilon = 0.001));
        
        let sample2 = osc.generate();
        assert!(sample2 != 0.0);
    }
    
    #[test]
    fn test_sine_osc_block() {
        let mut osc = SineOsc::new(440.0);
        osc.init(44100.0);
        
        let mut output = vec![0.0; 1024];
        osc.generate_block(&mut output);
        
        assert!(output.iter().any(|&x| x != 0.0));
    }
    
    #[test]
    fn test_sine_osc_parameters() {
        let mut osc = SineOsc::new(440.0);
        osc.set_param("frequency", ParamValue::Float(880.0)).unwrap();
        assert_eq!(osc.frequency(), 880.0);
        
        osc.set_param("amplitude", ParamValue::Float(0.5)).unwrap();
        assert_eq!(osc.amplitude(), 0.5);
    }
}