//! Triangle wave oscillator

use kama_core_traits::{
    AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId,
    param::{ParamType, ParamMetadata}
};
use super::AudioOscillator;

/// Triangle wave oscillator
pub struct TriangleOsc {
    /// Current phase (0.0 to 1.0)
    phase: f32,
    /// Frequency in Hz
    frequency: f32,
    /// Sample rate in Hz
    sample_rate: f32,
    /// Output amplitude (0.0 - 1.0)
    amplitude: f32,
}

impl TriangleOsc {
    /// Create a new triangle oscillator
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
    
    /// Generate next sample
    pub fn generate(&mut self) -> f32 {
        // Triangle wave: 4 * |phase - 0.5| - 1
        let sample = 4.0 * (self.phase - 0.5).abs() - 1.0;
        
        let phase_inc = self.frequency / self.sample_rate;
        self.phase += phase_inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        
        sample * self.amplitude
    }
    
    /// Generate a block of samples
    pub fn generate_block(&mut self, output: &mut [f32]) {
        for out in output.iter_mut() {
            *out = self.generate();
        }
    }
}

impl AudioOscillator for TriangleOsc {
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

impl AudioNode for TriangleOsc {
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
            name: "Triangle Oscillator".to_string(),
            category: NodeCategory::Generator,
            description: "Triangle wave generator".to_string(),
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
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_triangle_osc_generate() {
        let mut osc = TriangleOsc::new(440.0).with_amplitude(0.5);
        osc.init(44100.0);
        
        let sample = osc.generate();
        assert!(sample >= -0.5 && sample <= 0.5);
    }
    
    #[test]
    fn test_triangle_osc_block() {
        let mut osc = TriangleOsc::new(440.0);
        osc.init(44100.0);
        
        let mut output = vec![0.0; 1024];
        osc.generate_block(&mut output);
        
        assert!(output.iter().any(|&x| x != 0.0));
    }
}