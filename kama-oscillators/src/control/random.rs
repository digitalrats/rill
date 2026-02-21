//! Random walk and chaos generators

use kama_core_traits::{
    AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId,
    param::{ParamType, ParamMetadata}
};
use rand::Rng;

/// Random walk generator
///
/// Generates a smooth random signal by taking small steps
pub struct RandomWalk {
    /// Current value (-1.0 to 1.0)
    value: f64,
    /// Maximum step size per sample
    step_size: f64,
    /// Sample rate
    sample_rate: f64,
    /// Output amplitude
    amplitude: f64,
    /// Offset
    offset: f64,
}

impl RandomWalk {
    /// Create a new random walk generator
    pub fn new() -> Self {
        Self {
            value: 0.0,
            step_size: 0.01,
            sample_rate: 44100.0,
            amplitude: 1.0,
            offset: 0.0,
        }
    }
    
    /// Set step size (0.0 - 1.0)
    pub fn with_step_size(mut self, step: f64) -> Self {
        self.step_size = step.clamp(0.0, 1.0);
        self
    }
    
    /// Set amplitude
    pub fn with_amplitude(mut self, amp: f64) -> Self {
        self.amplitude = amp.clamp(0.0, 1.0);
        self
    }
    
    /// Set offset
    pub fn with_offset(mut self, offset: f64) -> Self {
        self.offset = offset.clamp(-1.0, 1.0);
        self
    }
    
    /// Generate next sample
    pub fn generate(&mut self) -> f64 {
        let mut rng = rand::thread_rng();
        let step = (rng.gen::<f64>() - 0.5) * 2.0 * self.step_size;
        
        self.value += step;
        self.value = self.value.clamp(-1.0, 1.0);
        
        self.value * self.amplitude + self.offset
    }
    
    /// Generate a block of samples
    pub fn generate_block(&mut self, output: &mut [f64]) {
        for out in output.iter_mut() {
            *out = self.generate();
        }
    }
    
    /// Reset to zero
    pub fn reset(&mut self) {
        self.value = 0.0;
    }
    
    /// Set step size
    pub fn set_step_size(&mut self, step: f64) {
        self.step_size = step.clamp(0.0, 1.0);
    }
}

impl Default for RandomWalk {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioNode for RandomWalk {
    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }
        
        let output = &mut outputs[0];
        for out in output.iter_mut() {
            *out = self.generate() as f32;
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "step_size" => Some(ParamValue::Float(self.step_size as f32)),
            "amplitude" => Some(ParamValue::Float(self.amplitude as f32)),
            "offset" => Some(ParamValue::Float(self.offset as f32)),
            "value" => Some(ParamValue::Float(self.value as f32)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("step_size", ParamValue::Float(s)) => {
                self.set_step_size(s as f64);
                Ok(())
            }
            ("amplitude", ParamValue::Float(a)) => {
                self.amplitude = a.clamp(0.0, 1.0) as f64;
                Ok(())
            }
            ("offset", ParamValue::Float(o)) => {
                self.offset = o.clamp(-1.0, 1.0) as f64;
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate as f64;
    }
    
    fn reset(&mut self) {
        self.reset();
    }
    
    fn num_inputs(&self) -> usize { 0 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Random Walk".to_string(),
            category: NodeCategory::Generator,
            description: "Smooth random signal generator".to_string(),
            author: "Kama Oscillators".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "step_size".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.01),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.001),
                    unit: Some("size".to_string()),
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
                    name: "offset".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.0),
                    min: Some(-1.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: None,
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
    fn test_random_walk_generate() {
        let mut rw = RandomWalk::new()
            .with_step_size(0.1)
            .with_amplitude(0.5);
        rw.init(44100.0);
        
        let val = rw.generate();
        assert!(val >= -0.5 && val <= 0.5);
    }
    
    #[test]
    fn test_random_walk_block() {
        let mut rw = RandomWalk::new();
        rw.init(44100.0);
        
        let mut output = vec![0.0; 1024];
        rw.generate_block(&mut output);
        
        assert!(output.iter().any(|&x| x != 0.0));
    }
}