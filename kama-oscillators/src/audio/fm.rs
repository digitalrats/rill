//! Frequency Modulation synthesis oscillator

use kama_core_traits::{
    AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId,
    param::{ParamType, ParamMetadata}
};
use super::{SineOsc, AudioOscillator};
use std::f32::consts::PI;

/// FM synthesis oscillator
///
/// Implements frequency modulation with a carrier and modulator oscillator
pub struct FmOsc {
    /// Carrier frequency in Hz
    carrier_freq: f32,
    /// Modulator frequency in Hz (ratio to carrier)
    modulator_ratio: f32,
    /// Modulation index (depth)
    modulation_index: f32,
    /// Sample rate
    sample_rate: f32,
    /// Output amplitude
    amplitude: f32,
    
    // Internal oscillators
    carrier_phase: f32,
    modulator_phase: f32,
    
    // Optional feedback
    feedback: f32,
    last_output: f32,
}

impl FmOsc {
    /// Create a new FM oscillator
    pub fn new(carrier_freq: f32) -> Self {
        Self {
            carrier_freq: carrier_freq.clamp(20.0, 20000.0),
            modulator_ratio: 1.0,
            modulation_index: 1.0,
            sample_rate: 44100.0,
            amplitude: 1.0,
            carrier_phase: 0.0,
            modulator_phase: 0.0,
            feedback: 0.0,
            last_output: 0.0,
        }
    }
    
    /// Set modulator frequency as ratio of carrier
    pub fn with_modulator_ratio(mut self, ratio: f32) -> Self {
        self.modulator_ratio = ratio.max(0.1).min(10.0);
        self
    }
    
    /// Set modulation index
    pub fn with_modulation_index(mut self, index: f32) -> Self {
        self.modulation_index = index.max(0.0).min(10.0);
        self
    }
    
    /// Set feedback amount (0.0 - 1.0)
    pub fn with_feedback(mut self, fb: f32) -> Self {
        self.feedback = fb.clamp(0.0, 1.0);
        self
    }
    
    /// Set amplitude
    pub fn with_amplitude(mut self, amp: f32) -> Self {
        self.amplitude = amp.clamp(0.0, 1.0);
        self
    }
    
    /// Generate next sample
    pub fn generate(&mut self) -> f32 {
        // Modulator frequency
        let mod_freq = self.carrier_freq * self.modulator_ratio;
        
        // Phase increments
        let carrier_inc = 2.0 * PI * self.carrier_freq / self.sample_rate;
        let modulator_inc = 2.0 * PI * mod_freq / self.sample_rate;
        
        // Modulator with optional feedback
        let modulator = self.modulator_phase.sin() + self.last_output * self.feedback;
        
        // Frequency modulation
        let modulated_phase = self.carrier_phase + modulator * self.modulation_index;
        let output = modulated_phase.sin() * self.amplitude;
        
        // Update phases
        self.carrier_phase += carrier_inc;
        self.modulator_phase += modulator_inc;
        
        // Wrap phases
        if self.carrier_phase > 2.0 * PI {
            self.carrier_phase -= 2.0 * PI;
        }
        if self.modulator_phase > 2.0 * PI {
            self.modulator_phase -= 2.0 * PI;
        }
        
        self.last_output = output;
        output
    }
    
    /// Generate a block of samples
    pub fn generate_block(&mut self, output: &mut [f32]) {
        for out in output.iter_mut() {
            *out = self.generate();
        }
    }
    
    /// Set carrier frequency
    pub fn set_carrier_freq(&mut self, freq: f32) {
        self.carrier_freq = freq.clamp(20.0, 20000.0);
    }
    
    /// Set modulator ratio
    pub fn set_modulator_ratio(&mut self, ratio: f32) {
        self.modulator_ratio = ratio.max(0.1).min(10.0);
    }
    
    /// Set modulation index
    pub fn set_modulation_index(&mut self, index: f32) {
        self.modulation_index = index.max(0.0).min(10.0);
    }
    
    /// Set feedback
    pub fn set_feedback(&mut self, fb: f32) {
        self.feedback = fb.clamp(0.0, 1.0);
    }
}

impl AudioNode for FmOsc {
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
            "carrier_freq" => Some(ParamValue::Float(self.carrier_freq)),
            "modulator_ratio" => Some(ParamValue::Float(self.modulator_ratio)),
            "modulation_index" => Some(ParamValue::Float(self.modulation_index)),
            "feedback" => Some(ParamValue::Float(self.feedback)),
            "amplitude" => Some(ParamValue::Float(self.amplitude)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("carrier_freq", ParamValue::Float(f)) => {
                self.set_carrier_freq(f);
                Ok(())
            }
            ("modulator_ratio", ParamValue::Float(r)) => {
                self.set_modulator_ratio(r);
                Ok(())
            }
            ("modulation_index", ParamValue::Float(i)) => {
                self.set_modulation_index(i);
                Ok(())
            }
            ("feedback", ParamValue::Float(f)) => {
                self.set_feedback(f);
                Ok(())
            }
            ("amplitude", ParamValue::Float(a)) => {
                self.amplitude = a.clamp(0.0, 1.0);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }
    
    fn reset(&mut self) {
        self.carrier_phase = 0.0;
        self.modulator_phase = 0.0;
        self.last_output = 0.0;
    }
    
    fn num_inputs(&self) -> usize { 0 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "FM Oscillator".to_string(),
            category: NodeCategory::Generator,
            description: "Frequency Modulation synthesizer".to_string(),
            author: "Kama Oscillators".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "carrier_freq".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(440.0),
                    min: Some(20.0),
                    max: Some(20000.0),
                    step: Some(1.0),
                    unit: Some("Hz".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "modulator_ratio".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.1),
                    max: Some(10.0),
                    step: Some(0.1),
                    unit: Some("ratio".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "modulation_index".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.0),
                    max: Some(10.0),
                    step: Some(0.1),
                    unit: Some("depth".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "feedback".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.0),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("amount".to_string()),
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
    fn test_fm_osc_generate() {
        let mut osc = FmOsc::new(440.0)
            .with_modulator_ratio(2.0)
            .with_modulation_index(0.5)
            .with_amplitude(0.5);
        osc.init(44100.0);
        
        let sample = osc.generate();
        assert!(sample >= -0.5 && sample <= 0.5);
    }
    
    #[test]
    fn test_fm_osc_block() {
        let mut osc = FmOsc::new(440.0);
        osc.init(44100.0);
        
        let mut output = vec![0.0; 1024];
        osc.generate_block(&mut output);
        
        assert!(output.iter().any(|&x| x != 0.0));
    }
    
    #[test]
    fn test_fm_osc_parameters() {
        let mut osc = FmOsc::new(440.0);
        
        osc.set_param("carrier_freq", ParamValue::Float(880.0)).unwrap();
        assert_eq!(osc.carrier_freq, 880.0);
        
        osc.set_param("modulator_ratio", ParamValue::Float(2.0)).unwrap();
        assert_eq!(osc.modulator_ratio, 2.0);
        
        osc.set_param("modulation_index", ParamValue::Float(1.5)).unwrap();
        assert_eq!(osc.modulation_index, 1.5);
    }
}