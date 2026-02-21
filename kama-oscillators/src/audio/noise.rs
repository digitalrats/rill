//! Noise generators

use kama_core_traits::{
    AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId,
    param::{ParamType, ParamMetadata}
};
use rand::Rng;
use super::AudioOscillator;

/// Types of noise
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoiseType {
    White,
    Pink,
    Brown,
}

impl NoiseType {
    /// Get all available types as strings
    pub fn names() -> Vec<&'static str> {
        vec!["white", "pink", "brown"]
    }
    
    /// Get type from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "white" => Some(NoiseType::White),
            "pink" => Some(NoiseType::Pink),
            "brown" => Some(NoiseType::Brown),
            _ => None,
        }
    }
}

/// Noise generator
pub struct NoiseOsc {
    /// Noise type
    noise_type: NoiseType,
    /// Output amplitude (0.0 - 1.0)
    amplitude: f32,
    /// Sample rate
    sample_rate: f32,
    
    // State for colored noise
    pink_b0: f32,
    pink_b1: f32,
    pink_b2: f32,
    pink_b3: f32,
    pink_b4: f32,
    pink_b5: f32,
    pink_b6: f32,
    
    brown_value: f32,
}

impl NoiseOsc {
    /// Create a new noise generator
    pub fn new() -> Self {
        Self {
            noise_type: NoiseType::White,
            amplitude: 1.0,
            sample_rate: 44100.0,
            pink_b0: 0.0,
            pink_b1: 0.0,
            pink_b2: 0.0,
            pink_b3: 0.0,
            pink_b4: 0.0,
            pink_b5: 0.0,
            pink_b6: 0.0,
            brown_value: 0.0,
        }
    }
    
    /// Set noise type
    pub fn with_type(mut self, noise_type: NoiseType) -> Self {
        self.noise_type = noise_type;
        self
    }
    
    /// Set amplitude
    pub fn with_amplitude(mut self, amp: f32) -> Self {
        self.amplitude = amp.clamp(0.0, 1.0);
        self
    }
    
    /// Generate next sample
    pub fn generate(&mut self) -> f32 {
        let sample = match self.noise_type {
            NoiseType::White => self.generate_white(),
            NoiseType::Pink => self.generate_pink(),
            NoiseType::Brown => self.generate_brown(),
        };
        
        sample * self.amplitude
    }
    
    /// Generate white noise
    fn generate_white(&mut self) -> f32 {
        let mut rng = rand::thread_rng();
        rng.gen::<f32>() * 2.0 - 1.0
    }
    
    /// Generate pink noise (1/f noise) using Paul Kellett's method
    fn generate_pink(&mut self) -> f32 {
        let mut rng = rand::thread_rng();
        let white = rng.gen::<f32>() * 2.0 - 1.0;
        
        self.pink_b0 = 0.99886 * self.pink_b0 + white * 0.0555179;
        self.pink_b1 = 0.99332 * self.pink_b1 + white * 0.0750759;
        self.pink_b2 = 0.96900 * self.pink_b2 + white * 0.1538520;
        self.pink_b3 = 0.86650 * self.pink_b3 + white * 0.3104856;
        self.pink_b4 = 0.55000 * self.pink_b4 + white * 0.5329522;
        self.pink_b5 = -0.7616 * self.pink_b5 - white * 0.0168980;
        
        let pink = self.pink_b0 + self.pink_b1 + self.pink_b2 + 
                   self.pink_b3 + self.pink_b4 + self.pink_b5 + 
                   self.pink_b6 + white * 0.5362;
        self.pink_b6 = white * 0.115926;
        
        pink * 0.11 // Scale to approximately [-1.0, 1.0]
    }
    
    /// Generate Brownian noise (1/f^2 noise)
    fn generate_brown(&mut self) -> f32 {
        let mut rng = rand::thread_rng();
        let white = rng.gen::<f32>() * 2.0 - 1.0;
        
        self.brown_value = 0.997 * self.brown_value + white * 0.03;
        self.brown_value.clamp(-1.0, 1.0)
    }
    
    /// Generate a block of samples
    pub fn generate_block(&mut self, output: &mut [f32]) {
        for out in output.iter_mut() {
            *out = self.generate();
        }
    }
    
    /// Set noise type
    pub fn set_noise_type(&mut self, noise_type: NoiseType) {
        self.noise_type = noise_type;
        self.reset();
    }
    
    /// Reset internal state
    pub fn reset(&mut self) {
        self.pink_b0 = 0.0;
        self.pink_b1 = 0.0;
        self.pink_b2 = 0.0;
        self.pink_b3 = 0.0;
        self.pink_b4 = 0.0;
        self.pink_b5 = 0.0;
        self.pink_b6 = 0.0;
        self.brown_value = 0.0;
    }
}

impl Default for NoiseOsc {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioOscillator for NoiseOsc {
    fn set_frequency(&mut self, _freq: f32) {
        // Noise doesn't have frequency
    }
    
    fn frequency(&self) -> f32 {
        0.0
    }
    
    fn set_amplitude(&mut self, amp: f32) {
        self.amplitude = amp.clamp(0.0, 1.0);
    }
    
    fn amplitude(&self) -> f32 {
        self.amplitude
    }
    
    fn reset_phase(&mut self) {
        self.reset();
    }
}

impl AudioNode for NoiseOsc {
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
            "type" => {
                let type_str = match self.noise_type {
                    NoiseType::White => "white",
                    NoiseType::Pink => "pink",
                    NoiseType::Brown => "brown",
                };
                Some(ParamValue::Choice(type_str.to_string()))
            }
            "amplitude" => Some(ParamValue::Float(self.amplitude)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("type", ParamValue::Choice(t)) => {
                self.noise_type = NoiseType::from_str(&t)
                    .ok_or_else(|| AudioError::Parameter(format!("Unknown noise type: {}", t)))?;
                self.reset();
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
        self.reset_phase();
    }
    
    fn num_inputs(&self) -> usize { 0 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Noise Generator".to_string(),
            category: NodeCategory::Generator,
            description: "White, pink, and brown noise generator".to_string(),
            author: "Kama Oscillators".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "type".to_string(),
                    typ: ParamType::Choice,
                    default: ParamValue::Choice("white".to_string()),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: Some(NoiseType::names().iter()
                        .enumerate()
                        .map(|(i, &name)| (name.to_string(), i as f32))
                        .collect()),
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
    fn test_noise_generate() {
        let mut noise = NoiseOsc::new().with_amplitude(0.5);
        noise.init(44100.0);
        
        let sample = noise.generate();
        assert!(sample >= -0.5 && sample <= 0.5);
    }
    
    #[test]
    fn test_noise_types() {
        let types = [NoiseType::White, NoiseType::Pink, NoiseType::Brown];
        
        for &t in &types {
            let mut noise = NoiseOsc::new().with_type(t);
            noise.init(44100.0);
            
            let sample = noise.generate();
            assert!(sample >= -1.0 && sample <= 1.0);
        }
    }
    
    #[test]
    fn test_noise_block() {
        let mut noise = NoiseOsc::new();
        noise.init(44100.0);
        
        let mut output = vec![0.0; 1024];
        noise.generate_block(&mut output);
        
        assert!(output.iter().any(|&x| x != 0.0));
    }
}