//! Distortion effect with waveshaping

use kama_core::traits::{
    ParamMetadata, ParamType,
    AudioError, AudioNode, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
};

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
pub struct Distortion {
    /// Distortion type
    distortion_type: DistortionType,
    /// Drive (input gain)
    drive: f32,
    /// Output gain
    output_gain: f32,
}

impl Distortion {
    /// Create a new distortion effect
    pub fn new(distortion_type: DistortionType, drive: f32, output_gain: f32) -> Self {
        Self {
            distortion_type,
            drive: drive.max(1.0).min(100.0),
            output_gain: output_gain.clamp(0.0, 2.0),
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
}

impl AudioNode for Distortion {
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

    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("type", ParamValue::Choice(t)) => {
                if let Some(dt) = DistortionType::from_str(&t) {
                    self.set_type(dt);
                    Ok(())
                } else {
                    Err(AudioError::Parameter(format!(
                        "Unknown distortion type: {}",
                        t
                    )))
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
            _ => Err(AudioError::Parameter(format!(
                "Unknown parameter: {}",
                name
            ))),
        }
    }

    fn init(&mut self, _sample_rate: f32) {
        // Nothing to initialize
    }

    fn reset(&mut self) {
        // Nothing to reset
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
            name: "Distortion".to_string(),
            category: NodeCategory::Effect,
            description: "Distortion with multiple waveshaping types".to_string(),
            author: "Kama Digital Effects".to_string(),
            version: "0.2.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "type".to_string(),
                    typ: ParamType::Choice,
                    default: ParamValue::Choice("soft_clip".to_string()),
                    min: None,
                    max: None,
                    step: None,
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
                    min: Some(1.0),
                    max: Some(100.0),
                    step: Some(1.0),
                    unit: Some("gain".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "output_gain".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.0),
                    max: Some(2.0),
                    step: Some(0.1),
                    unit: Some("gain".to_string()),
                    choices: None,
                },
            ],
        }
    }
}
