//! Delay effect with feedback

use kama_core::buffer::DelayLine;
use kama_core::traits::{
    ParamMetadata, ParamRange, ParamType, NodeCategory, NodeMetadata, NodeTypeId,
    ParameterId, ParamValue, Processor,
};
use kama_core::{ProcessResult, ProcessError};
use kama_core::math::AudioNum;

/// Maximum delay time in seconds
const MAX_DELAY_SECONDS: f32 = 0.5;
/// Maximum sample rate we support (48 kHz)
const MAX_SAMPLE_RATE: f32 = 48_000.0;
/// Maximum delay in samples (2 seconds at max sample rate)
const MAX_DELAY_SAMPLES: usize = (MAX_DELAY_SECONDS * MAX_SAMPLE_RATE) as usize;

/// Delay effect with feedback
///
/// Parameters:
/// - delay_time: delay time in seconds (0.01 - 2.0)
/// - feedback: feedback amount (0.0 - 0.99)
/// - mix: dry/wet mix (0.0 - 1.0)
pub struct Delay<const BUF_SIZE: usize> {
    /// Delay line
    delay_line: DelayLine<f32, MAX_DELAY_SAMPLES>,
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

impl<const BUF_SIZE: usize> Delay<BUF_SIZE> {
    /// Create a new delay effect with default parameters
    pub fn new() -> Self {
        let sample_rate = 44100.0;
        let delay_time = 0.5;
        let delay_samples = (delay_time * sample_rate) as usize;
        let mut delay_line = DelayLine::new(sample_rate);
        delay_line.set_delay_samples(delay_samples);

        Self {
            delay_line,
            delay_time,
            delay_samples,
            feedback: 0.3,
            mix: 0.5,
            sample_rate,
        }
    }

    /// Create a new delay effect with custom parameters
    pub fn with_params(delay_time: f32, feedback: f32, mix: f32) -> Self {
        let sample_rate = 44100.0;
        let delay_samples = (delay_time.clamp(0.01, MAX_DELAY_SECONDS) * sample_rate) as usize;
        let mut delay_line = DelayLine::new(sample_rate);
        delay_line.set_delay_samples(delay_samples);

        Self {
            delay_line,
            delay_time,
            delay_samples,
            feedback: feedback.clamp(0.0, 0.99),
            mix: mix.clamp(0.0, 1.0),
            sample_rate,
        }
    }

    /// Set delay time in seconds
    pub fn set_delay_time(&mut self, time: f32) {
        self.delay_time = time.clamp(0.01, MAX_DELAY_SECONDS);
        self.delay_samples = (self.delay_time * self.sample_rate) as usize;
        self.delay_line.set_delay_samples(self.delay_samples);
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
        // Read delayed sample
        let delayed = self.delay_line.read_delayed(self.delay_samples);
        // Output mix
        let output = input * (1.0 - self.mix) + delayed * self.mix;
        // Write input with feedback
        let write_sample = input + delayed * self.feedback;
        self.delay_line.write(write_sample);
        output
    }

    /// Process a block of samples (internal helper)
    fn process_block(&mut self, inputs: &[&[f32; BUF_SIZE]], outputs: &mut [&mut [f32; BUF_SIZE]]) {
        let input = inputs[0];
        let output = &mut outputs[0];
        for i in 0..BUF_SIZE {
            output[i] = self.process_sample(input[i]);
        }
    }
}

impl<const BUF_SIZE: usize> Processor<f32, BUF_SIZE> for Delay<BUF_SIZE> {
    fn process(
        &mut self,
        inputs: &[&[f32; BUF_SIZE]],
        outputs: &mut [&mut [f32; BUF_SIZE]],
        _control: &[f32],
    ) -> ProcessResult<()> {
        if inputs.len() < 1 || outputs.len() < 1 {
            return Err(ProcessError::processing("insufficient channels"));
        }
        self.process_block(inputs, outputs);
        Ok(())
    }

    fn num_audio_inputs(&self) -> usize {
        1
    }

    fn num_audio_outputs(&self) -> usize {
        1
    }

    fn num_control_inputs(&self) -> usize {
        0
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "delay_time" => Some(ParamValue::Float(self.delay_time)),
            "feedback" => Some(ParamValue::Float(self.feedback)),
            "mix" => Some(ParamValue::Float(self.mix)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match (id.as_str(), value) {
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
            _ => Err(ProcessError::parameter("unknown parameter")),
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.delay_samples = (self.delay_time * sample_rate) as usize;
        self.delay_line.set_delay_samples(self.delay_samples);
    }

    fn reset(&mut self) {
        self.delay_line.clear();
    }
}

// Implement NodeMetadata for compatibility
impl<const BUF_SIZE: usize> Delay<BUF_SIZE> {
    /// Returns metadata about this node
    pub fn metadata() -> NodeMetadata {
        NodeMetadata {
            name: "Delay".to_string(),
            category: NodeCategory::Processor,
            description: "Digital delay with feedback".to_string(),
            author: "Kama Digital Effects".to_string(),
            version: "0.3.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "delay_time".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.5),
                    range: ParamRange::new()
                        .with_min(0.01)
                        .with_max(MAX_DELAY_SECONDS)
                        .with_step(0.01),
                    unit: Some("s".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "feedback".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.3),
                    range: ParamRange::new()
                        .with_min(0.0)
                        .with_max(0.99)
                        .with_step(0.01),
                    unit: Some("gain".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "mix".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.5),
                    range: ParamRange::new()
                        .with_min(0.0)
                        .with_max(1.0)
                        .with_step(0.01),
                    unit: Some("mix".to_string()),
                    choices: None,
                },
            ],
        }
    }

    /// Returns the node type ID
    pub fn node_type_id() -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
}
