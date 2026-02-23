//! Delay effect with feedback

use kama_buffers::RingBuffer;
use kama_core_traits::{
    param::{ParamMetadata, ParamType},
    AudioError, AudioNode, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
};
use std::f32::consts::PI;

/// Delay effect with feedback
///
/// Parameters:
/// - delay_time: delay time in seconds (0.01 - 2.0)
/// - feedback: feedback amount (0.0 - 0.99)
/// - mix: dry/wet mix (0.0 - 1.0)
pub struct Delay {
    /// Delay buffer
    buffer: RingBuffer,
    /// Delay time in seconds
    delay_time: f32,
    /// Delay time in samples
    delay_samples: usize,
    /// Feedback amount (0.0 - 0.99)
    feedback: f32,
    /// Dry/wet mix (0.0 = dry, 1.0 = wet)
    mix: f32,
    /// Sample rate
    sample_rate: f32,
}

impl Delay {
    /// Create a new delay effect
    pub fn new(delay_time: f32, feedback: f32, mix: f32) -> Self {
        let sample_rate = 44100.0;
        let max_delay_samples = (2.0 * sample_rate) as usize; // 2 seconds max
        let buffer = RingBuffer::new(max_delay_samples);

        let delay_samples = (delay_time * sample_rate) as usize;

        Self {
            buffer,
            delay_time,
            delay_samples,
            feedback: feedback.clamp(0.0, 0.99),
            mix: mix.clamp(0.0, 1.0),
            sample_rate,
        }
    }

    /// Set delay time in seconds
    pub fn set_delay_time(&mut self, time: f32) {
        self.delay_time = time.clamp(0.01, 2.0);
        self.delay_samples = (self.delay_time * self.sample_rate) as usize;
    }

    /// Set feedback amount
    pub fn set_feedback(&mut self, fb: f32) {
        self.feedback = fb.clamp(0.0, 0.99);
    }

    /// Set dry/wet mix
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    /// Process a single sample
    pub fn process_sample(&mut self, input: f32) -> f32 {
        // Read from delay buffer
        let mut delayed = 0.0;
        self.buffer
            .read(self.delay_samples, std::slice::from_mut(&mut delayed));

        // Calculate output
        let output = input * (1.0 - self.mix) + delayed * self.mix;

        // Write to buffer with feedback
        let write_sample = input + delayed * self.feedback;
        self.buffer.write(&[write_sample]);

        output
    }
}

impl AudioNode for Delay {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }

        let input = inputs[0];
        let output = &mut outputs[0];
        let len = input.len().min(output.len());

        for i in 0..len {
            output[i] = self.process_sample(input[i]);
        }

        Ok(())
    }

    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "delay_time" => Some(ParamValue::Float(self.delay_time)),
            "feedback" => Some(ParamValue::Float(self.feedback)),
            "mix" => Some(ParamValue::Float(self.mix)),
            _ => None,
        }
    }

    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("delay_time", ParamValue::Float(t)) => {
                self.set_delay_time(t);
                Ok(())
            }
            ("feedback", ParamValue::Float(f)) => {
                self.set_feedback(f);
                Ok(())
            }
            ("mix", ParamValue::Float(m)) => {
                self.set_mix(m);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!(
                "Unknown parameter: {}",
                name
            ))),
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.delay_samples = (self.delay_time * sample_rate) as usize;
        // Reset buffer?
        self.buffer.reset();
    }

    fn reset(&mut self) {
        self.buffer.reset();
    }

    fn num_inputs(&self) -> usize {
        1
    }
    fn num_outputs(&self) -> usize {
        1
    }

    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }

    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Delay".to_string(),
            category: NodeCategory::Effect,
            description: "Digital delay with feedback".to_string(),
            author: "Kama Digital Effects".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "delay_time".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.5),
                    min: Some(0.01),
                    max: Some(2.0),
                    step: Some(0.01),
                    unit: Some("s".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "feedback".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.3),
                    min: Some(0.0),
                    max: Some(0.99),
                    step: Some(0.01),
                    unit: Some("gain".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "mix".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.5),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("mix".to_string()),
                    choices: None,
                },
            ],
        }
    }
}
