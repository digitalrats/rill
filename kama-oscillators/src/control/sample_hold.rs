//! Sample & Hold generator

use kama_core::traits::{
    ParamMetadata, ParamType,
    AudioError, AudioNode, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
};
use rand::Rng;

/// Sample & Hold generator
///
/// Samples a random value at regular intervals and holds it
pub struct SampleAndHold {
    /// Current held value
    value: f32,
    /// Hold time in seconds
    hold_time: f64,
    /// Counter for current hold
    counter: usize,
    /// Hold time in samples
    hold_samples: usize,
    /// Sample rate
    sample_rate: f64,
    /// Output amplitude
    amplitude: f32,
    /// Offset
    offset: f32,
}

impl SampleAndHold {
    /// Create a new Sample & Hold generator
    pub fn new(hold_time: f64) -> Self {
        Self {
            value: 0.0,
            hold_time: hold_time.max(0.001),
            counter: 0,
            hold_samples: 0,
            sample_rate: 44100.0,
            amplitude: 1.0,
            offset: 0.0,
        }
    }

    /// Set amplitude
    pub fn with_amplitude(mut self, amp: f32) -> Self {
        self.amplitude = amp.clamp(0.0, 1.0);
        self
    }

    /// Set offset
    pub fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset.clamp(-1.0, 1.0);
        self
    }

    /// Update hold samples from time
    fn update_hold_samples(&mut self) {
        self.hold_samples = (self.hold_time * self.sample_rate) as usize;
    }

    /// Generate next sample
    pub fn generate(&mut self) -> f32 {
        self.counter += 1;

        if self.counter >= self.hold_samples {
            self.counter = 0;
            let mut rng = rand::thread_rng();
            self.value = rng.gen::<f32>() * 2.0 - 1.0;
        }

        self.value * self.amplitude + self.offset
    }

    /// Generate a block of samples
    pub fn generate_block(&mut self, output: &mut [f32]) {
        for out in output.iter_mut() {
            *out = self.generate();
        }
    }

    /// Set hold time
    pub fn set_hold_time(&mut self, time: f64) {
        self.hold_time = time.max(0.001);
        self.update_hold_samples();
    }

    /// Get current held value
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Reset counter
    pub fn reset(&mut self) {
        self.counter = 0;
        self.value = 0.0;
    }
}

impl AudioNode for SampleAndHold {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }

        // Optional: use input as trigger
        let trigger = if !inputs.is_empty() && inputs[0].len() > 0 {
            inputs[0][0] > 0.5
        } else {
            true // free-running if no trigger
        };

        let output = &mut outputs[0];

        if trigger {
            // Free-running mode
            for out in output.iter_mut() {
                *out = self.generate();
            }
        } else {
            // Hold current value
            for out in output.iter_mut() {
                *out = self.value * self.amplitude + self.offset;
            }
        }

        Ok(())
    }

    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "hold_time" => Some(ParamValue::Float(self.hold_time as f32)),
            "amplitude" => Some(ParamValue::Float(self.amplitude)),
            "offset" => Some(ParamValue::Float(self.offset)),
            "value" => Some(ParamValue::Float(self.value)),
            _ => None,
        }
    }

    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("hold_time", ParamValue::Float(t)) => {
                self.set_hold_time(t as f64);
                Ok(())
            }
            ("amplitude", ParamValue::Float(a)) => {
                self.amplitude = a.clamp(0.0, 1.0);
                Ok(())
            }
            ("offset", ParamValue::Float(o)) => {
                self.offset = o.clamp(-1.0, 1.0);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!(
                "Unknown parameter: {}",
                name
            ))),
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate as f64;
        self.update_hold_samples();
    }

    fn reset(&mut self) {
        self.reset();
    }

    fn num_inputs(&self) -> usize {
        1
    } // Optional trigger input
    fn num_outputs(&self) -> usize {
        1
    }

    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }

    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Sample & Hold".to_string(),
            category: NodeCategory::Generator,
            description: "Random sample and hold generator".to_string(),
            author: "Kama Oscillators".to_string(),
            version: "0.2.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "hold_time".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.1),
                    min: Some(0.001),
                    max: Some(10.0),
                    step: Some(0.001),
                    unit: Some("s".to_string()),
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
    fn test_sample_hold_generate() {
        let mut sh = SampleAndHold::new(0.1).with_amplitude(0.5);
        sh.init(44100.0);

        let val = sh.generate();
        assert!(val >= -0.5 && val <= 0.5);
    }

    #[test]
    fn test_sample_hold_hold_time() {
        let mut sh = SampleAndHold::new(0.1);
        sh.init(44100.0);

        let first = sh.generate();
        let second = sh.generate();

        // Should be same value (holding)
        assert_eq!(first, second);
    }

    #[test]
    fn test_sample_hold_block() {
        let mut sh = SampleAndHold::new(0.01);
        sh.init(44100.0);

        let mut output = vec![0.0; 1024];
        sh.generate_block(&mut output);

        assert!(output.iter().any(|&x| x != 0.0));
    }
}
