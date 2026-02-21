//! ADSR Envelope generator

use kama_core_traits::{
    AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId,
    param::{ParamType, ParamMetadata}
};

/// ADSR envelope stage
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeStage {
    Attack,
    Decay,
    Sustain,
    Release,
    Off,
}

/// ADSR envelope generator for modulation
///
/// Generates control signals with Attack, Decay, Sustain, Release phases
pub struct Envelope {
    /// Attack time in seconds
    attack: f64,
    /// Decay time in seconds
    decay: f64,
    /// Sustain level (0.0 - 1.0)
    sustain: f64,
    /// Release time in seconds
    release: f64,
    
    /// Current stage
    stage: EnvelopeStage,
    /// Current level (0.0 - 1.0)
    level: f64,
    /// Sample rate
    sample_rate: f64,
    
    /// Whether envelope is triggered
    triggered: bool,
    /// Time in current stage (samples)
    stage_time: usize,
    
    /// Attack samples
    attack_samples: usize,
    /// Decay samples
    decay_samples: usize,
    /// Release samples
    release_samples: usize,
    
    /// Start level for current stage
    stage_start: f64,
    /// End level for current stage
    stage_end: f64,
}

impl Envelope {
    /// Create a new envelope
    pub fn new(attack: f64, decay: f64, sustain: f64, release: f64) -> Self {
        let mut env = Self {
            attack: attack.max(0.001),
            decay: decay.max(0.001),
            sustain: sustain.clamp(0.0, 1.0),
            release: release.max(0.001),
            stage: EnvelopeStage::Off,
            level: 0.0,
            sample_rate: 44100.0,
            triggered: false,
            stage_time: 0,
            attack_samples: 0,
            decay_samples: 0,
            release_samples: 0,
            stage_start: 0.0,
            stage_end: 0.0,
        };
        env.update_times();
        env
    }
    
    /// Update sample counts from times
    fn update_times(&mut self) {
        self.attack_samples = (self.attack * self.sample_rate) as usize;
        self.decay_samples = (self.decay * self.sample_rate) as usize;
        self.release_samples = (self.release * self.sample_rate) as usize;
    }
    
    /// Trigger the envelope (start attack)
    pub fn trigger(&mut self) {
        self.stage = EnvelopeStage::Attack;
        self.stage_time = 0;
        self.stage_start = self.level;
        self.stage_end = 1.0;
        self.triggered = true;
    }
    
    /// Release the envelope (start release)
    pub fn release(&mut self) {
        if self.triggered {
            self.stage = EnvelopeStage::Release;
            self.stage_time = 0;
            self.stage_start = self.level;
            self.stage_end = 0.0;
            self.triggered = false;
        }
    }
    
    /// Generate next sample
    pub fn generate(&mut self) -> f64 {
        match self.stage {
            EnvelopeStage::Attack => {
                self.stage_time += 1;
                let progress = self.stage_time as f64 / self.attack_samples as f64;
                self.level = self.stage_start + (self.stage_end - self.stage_start) * progress;
                
                if self.stage_time >= self.attack_samples {
                    self.stage = EnvelopeStage::Decay;
                    self.stage_time = 0;
                    self.stage_start = self.level;
                    self.stage_end = self.sustain;
                }
            }
            
            EnvelopeStage::Decay => {
                self.stage_time += 1;
                let progress = self.stage_time as f64 / self.decay_samples as f64;
                self.level = self.stage_start + (self.stage_end - self.stage_start) * progress;
                
                if self.stage_time >= self.decay_samples {
                    self.stage = EnvelopeStage::Sustain;
                    self.level = self.sustain;
                }
            }
            
            EnvelopeStage::Sustain => {
                self.level = self.sustain;
            }
            
            EnvelopeStage::Release => {
                self.stage_time += 1;
                let progress = self.stage_time as f64 / self.release_samples as f64;
                self.level = self.stage_start + (self.stage_end - self.stage_start) * progress;
                
                if self.stage_time >= self.release_samples {
                    self.stage = EnvelopeStage::Off;
                    self.level = 0.0;
                }
            }
            
            EnvelopeStage::Off => {
                self.level = 0.0;
            }
        }
        
        self.level
    }
    
    /// Generate a block of samples
    pub fn generate_block(&mut self, output: &mut [f64]) {
        for out in output.iter_mut() {
            *out = self.generate();
        }
    }
    
    /// Check if envelope is active (not off)
    pub fn is_active(&self) -> bool {
        !matches!(self.stage, EnvelopeStage::Off)
    }
    
    /// Get current stage
    pub fn stage(&self) -> EnvelopeStage {
        self.stage
    }
    
    /// Set attack time
    pub fn set_attack(&mut self, attack: f64) {
        self.attack = attack.max(0.001);
        self.update_times();
    }
    
    /// Set decay time
    pub fn set_decay(&mut self, decay: f64) {
        self.decay = decay.max(0.001);
        self.update_times();
    }
    
    /// Set sustain level
    pub fn set_sustain(&mut self, sustain: f64) {
        self.sustain = sustain.clamp(0.0, 1.0);
    }
    
    /// Set release time
    pub fn set_release(&mut self, release: f64) {
        self.release = release.max(0.001);
        self.update_times();
    }
    
    /// Reset envelope
    pub fn reset(&mut self) {
        self.stage = EnvelopeStage::Off;
        self.level = 0.0;
        self.triggered = false;
        self.stage_time = 0;
    }
}

impl AudioNode for Envelope {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }
        
        // Check for trigger input
        if !inputs.is_empty() && inputs[0].len() > 0 {
            if inputs[0][0] > 0.5 && !self.triggered {
                self.trigger();
            } else if inputs[0][0] <= 0.5 && self.triggered {
                self.release();
            }
        }
        
        let output = &mut outputs[0];
        for out in output.iter_mut() {
            *out = self.generate() as f32;
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "attack" => Some(ParamValue::Float(self.attack as f32)),
            "decay" => Some(ParamValue::Float(self.decay as f32)),
            "sustain" => Some(ParamValue::Float(self.sustain as f32)),
            "release" => Some(ParamValue::Float(self.release as f32)),
            "stage" => {
                let stage_str = match self.stage {
                    EnvelopeStage::Attack => "attack",
                    EnvelopeStage::Decay => "decay",
                    EnvelopeStage::Sustain => "sustain",
                    EnvelopeStage::Release => "release",
                    EnvelopeStage::Off => "off",
                };
                Some(ParamValue::Choice(stage_str.to_string()))
            }
            "level" => Some(ParamValue::Float(self.level as f32)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("attack", ParamValue::Float(a)) => {
                self.set_attack(a as f64);
                Ok(())
            }
            ("decay", ParamValue::Float(d)) => {
                self.set_decay(d as f64);
                Ok(())
            }
            ("sustain", ParamValue::Float(s)) => {
                self.set_sustain(s as f64);
                Ok(())
            }
            ("release", ParamValue::Float(r)) => {
                self.set_release(r as f64);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate as f64;
        self.update_times();
    }
    
    fn reset(&mut self) {
        self.reset();
    }
    
    fn num_inputs(&self) -> usize { 1 } // Trigger input
    fn num_outputs(&self) -> usize { 1 }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "ADSR Envelope".to_string(),
            category: NodeCategory::Generator,
            description: "Attack-Decay-Sustain-Release envelope generator".to_string(),
            author: "Kama Oscillators".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "attack".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.01),
                    min: Some(0.001),
                    max: Some(10.0),
                    step: Some(0.001),
                    unit: Some("s".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "decay".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.1),
                    min: Some(0.001),
                    max: Some(10.0),
                    step: Some(0.001),
                    unit: Some("s".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "sustain".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.7),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("level".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "release".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.2),
                    min: Some(0.001),
                    max: Some(10.0),
                    step: Some(0.001),
                    unit: Some("s".to_string()),
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
    fn test_envelope_trigger() {
        let mut env = Envelope::new(0.01, 0.1, 0.5, 0.2);
        env.init(44100.0);
        
        env.trigger();
        assert!(matches!(env.stage(), EnvelopeStage::Attack));
        
        let val = env.generate();
        assert!(val > 0.0);
    }
    
    #[test]
    fn test_envelope_release() {
        let mut env = Envelope::new(0.01, 0.1, 0.5, 0.2);
        env.init(44100.0);
        
        env.trigger();
        for _ in 0..1000 {
            env.generate();
        }
        
        env.release();
        assert!(matches!(env.stage(), EnvelopeStage::Release));
    }
    
    #[test]
    fn test_envelope_block() {
        let mut env = Envelope::new(0.01, 0.1, 0.5, 0.2);
        env.init(44100.0);
        
        env.trigger();
        
        let mut output = vec![0.0; 1024];
        env.generate_block(&mut output);
        
        assert!(output.iter().any(|&x| x > 0.0));
        assert!(output.iter().all(|&x| x <= 1.0));
    }
}