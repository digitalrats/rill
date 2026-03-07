//! Distortion effect with waveshaping

use kama_core::traits::{
    ParamMetadata, ParamRange, ParamType, NodeCategory, NodeMetadata, NodeTypeId,
    ParameterId, ParamValue, Processor,
};
use kama_core::{ProcessResult, ProcessError};

/// Distortion type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistortionType {
    /// Hard clipping
    HardClip,
    /// Soft clipping (tanh)
    SoftClip,
    /// Tube-like saturation
    Tube,
    /// Fuzz (asymmetric)
    Fuzz,
}

impl DistortionType {
    /// Get all available types as strings
    pub fn names() -> Vec<&'static str> {
        vec!["hard_clip", "soft_clip", "tube", "fuzz"]
    }

    /// Get type from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "hard_clip" => Some(DistortionType::HardClip),
            "soft_clip" => Some(DistortionType::SoftClip),
            "tube" => Some(DistortionType::Tube),
            "fuzz" => Some(DistortionType::Fuzz),
            _ => None,
        }
    }
}

/// Distortion effect
///
/// Parameters:
/// - drive: input gain (1.0 - 100.0)
/// - type: distortion type
/// - output_gain: output level (0.0 - 2.0)
pub struct Distortion<const BUF_SIZE: usize> {
    /// Distortion type
    distortion_type: DistortionType,
    /// Drive (input gain)
    drive: f32,
    /// Output gain
    output_gain: f32,
    /// Sample rate (unused but required for Processor)
    sample_rate: f32,
}

impl<const BUF_SIZE: usize> Distortion<BUF_SIZE> {
    /// Create a new distortion effect with default parameters
    pub fn new() -> Self {
        Self {
            distortion_type: DistortionType::SoftClip,
            drive: 1.0,
            output_gain: 1.0,
            sample_rate: 44100.0,
        }
    }

    /// Create a new distortion effect with custom parameters
    pub fn with_params(distortion_type: DistortionType, drive: f32, output_gain: f32) -> Self {
        Self {
            distortion_type,
            drive: drive.max(1.0).min(100.0),
            output_gain: output_gain.clamp(0.0, 2.0),
            sample_rate: 44100.0,
        }
    }

    /// Process a single sample
    pub fn process_sample(&self, input: f32) -> f32 {
        let driven = input * self.drive;

        let distorted = match self.distortion_type {
            DistortionType::HardClip => driven.clamp(-1.0, 1.0),
            DistortionType::SoftClip => driven.tanh(),
            DistortionType::Tube => {
                // Tube-like saturation
                if driven > 0.0 {
                    1.0 - (-driven).exp()
                } else {
                    -1.0 + driven.exp()
                }
            }
            DistortionType::Fuzz => {
                // Asymmetric fuzz
                if driven > 0.0 {
                    1.0 - 1.0 / (1.0 + driven)
                } else {
                    driven
                }
            }
        };

        distorted * self.output_gain
    }

    /// Set distortion type
    pub fn set_type(&mut self, distortion_type: DistortionType) {
        self.distortion_type = distortion_type;
    }

    /// Set drive
    pub fn set_drive(&mut self, drive: f32) {
        self.drive = drive.max(1.0).min(100.0);
    }

    /// Set output gain
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain = gain.clamp(0.0, 2.0);
    }

    /// Process a block of samples (internal helper)
    fn process_block(&self, inputs: &[&[f32; BUF_SIZE]], outputs: &mut [&mut [f32; BUF_SIZE]]) {
        let input = inputs[0];
        let output = &mut outputs[0];
        for i in 0..BUF_SIZE {
            output[i] = self.process_sample(input[i]);
        }
    }
}

impl<const BUF_SIZE: usize> Processor<f32, BUF_SIZE> for Distortion<BUF_SIZE> {
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
            "type" => {
                let type_str = match self.distortion_type {
                    DistortionType::HardClip => "hard_clip",
                    DistortionType::SoftClip => "soft_clip",
                    DistortionType::Tube => "tube",
                    DistortionType::Fuzz => "fuzz",
                };
                Some(ParamValue::Choice(type_str.to_string()))
            }
            "drive" => Some(ParamValue::Float(self.drive)),
            "output_gain" => Some(ParamValue::Float(self.output_gain)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match (id.as_str(), value) {
            ("type", ParamValue::Choice(t)) => {
                if let Some(dt) = DistortionType::from_str(&t) {
                    self.set_type(dt);
                    Ok(())
                } else {
                    Err(ProcessError::parameter("unknown distortion type"))
                }
            }
            ("drive", ParamValue::Float(d)) => {
                self.set_drive(d);
                Ok(())
            }
            ("output_gain", ParamValue::Float(g)) => {
                self.set_output_gain(g);
                Ok(())
            }
            _ => Err(ProcessError::parameter("unknown parameter")),
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        // Nothing else to initialize
    }

    fn reset(&mut self) {
        // Nothing to reset
    }
}

// Implement NodeMetadata for compatibility
impl<const BUF_SIZE: usize> Distortion<BUF_SIZE> {
    /// Returns metadata about this node
    pub fn metadata() -> NodeMetadata {
        NodeMetadata {
            name: "Distortion".to_string(),
            category: NodeCategory::Processor,
            description: "Distortion with multiple waveshaping types".to_string(),
            author: "Kama Digital Effects".to_string(),
            version: "0.3.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "type".to_string(),
                    typ: ParamType::Choice,
                    default: ParamValue::Choice("soft_clip".to_string()),
                    range: ParamRange::new(),
                    unit: None,
                    choices: Some(
                        DistortionType::names()
                            .iter()
                            .enumerate()
                            .map(|(i, &name)| (name.to_string(), i as f32))
                            .collect(),
                    ),
                },
                ParamMetadata {
                    name: "drive".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    range: ParamRange::new()
                        .with_min(1.0)
                        .with_max(100.0)
                        .with_step(1.0),
                    unit: Some("gain".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "output_gain".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    range: ParamRange::new()
                        .with_min(0.0)
                        .with_max(2.0)
                        .with_step(0.1),
                    unit: Some("gain".to_string()),
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
