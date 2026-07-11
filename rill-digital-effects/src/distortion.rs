//! Distortion effect with waveshaping

use rill_core::{
    math::Transcendental,
    traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata},
    traits::ProcessResult,
};
use std::marker::PhantomData;

/// Distortion type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistortionType {
    HardClip,
    SoftClip,
    Tube,
    Fuzz,
}

impl DistortionType {
    pub fn names() -> Vec<&'static str> {
        vec!["hard_clip", "soft_clip", "tube", "fuzz"]
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "hard_clip" => Some(DistortionType::HardClip),
            "soft_clip" => Some(DistortionType::SoftClip),
            "tube" => Some(DistortionType::Tube),
            "fuzz" => Some(DistortionType::Fuzz),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            DistortionType::HardClip => "hard_clip",
            DistortionType::SoftClip => "soft_clip",
            DistortionType::Tube => "tube",
            DistortionType::Fuzz => "fuzz",
        }
    }
}

/// Distortion effect
///
/// Parameters:
/// - drive: input gain (1.0 - 100.0)
/// - type: distortion type
/// - output_gain: output level (0.0 - 2.0)
pub struct Distortion<T: Transcendental, const BUF_SIZE: usize> {
    pub distortion_type: DistortionType,
    pub drive: f32,
    pub output_gain: f32,
    _phantom: PhantomData<(T, [T; BUF_SIZE])>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for Distortion<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Distortion<T, BUF_SIZE> {
    pub fn new() -> Self {
        Self {
            distortion_type: DistortionType::SoftClip,
            drive: 1.0,
            output_gain: 1.0,
            _phantom: PhantomData,
        }
    }

    pub fn with_params(distortion_type: DistortionType, drive: f32, output_gain: f32) -> Self {
        let mut instance = Self::new();
        instance.set_type(distortion_type);
        instance.set_drive(drive);
        instance.set_output_gain(output_gain);
        instance
    }

    pub fn set_type(&mut self, distortion_type: DistortionType) {
        self.distortion_type = distortion_type;
    }

    pub fn set_drive(&mut self, drive: f32) {
        self.drive = drive.clamp(1.0, 100.0);
    }

    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain = gain.clamp(0.0, 2.0);
    }

    pub fn process_sample(&self, input: T) -> T {
        let driven = input.mul(T::from_f32(self.drive));

        let distorted = match self.distortion_type {
            DistortionType::HardClip => driven.clamp(T::MIN, T::MAX),
            DistortionType::SoftClip => T::from_f32(driven.to_f32().tanh()),
            DistortionType::Tube => {
                if driven > T::ZERO {
                    T::ONE - (-driven).exp()
                } else {
                    -T::ONE + driven.exp()
                }
            }
            DistortionType::Fuzz => {
                if driven > T::ZERO {
                    T::ONE - T::ONE.div(T::ONE + driven)
                } else {
                    driven
                }
            }
        };

        distorted.mul(T::from_f32(self.output_gain))
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T> for Distortion<T, BUF_SIZE> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());
        for i in 0..len {
            output[i] = self.process_sample(input[i]);
        }
        Ok(())
    }

    fn reset(&mut self) {}

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Distortion",
            category: AlgorithmCategory::Effect,
            description: "Distortion effect with multiple types (hard clip, soft clip, tube, fuzz)",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}
