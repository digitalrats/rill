//! Noise generators

use kama_core::traits::processor::{Processor, ProcessResult};
use kama_core::traits::{ParameterId, ParamValue};
use rand::Rng;

/// Types of noise
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoiseType {
    /// White noise (uniform spectrum)
    White,
    
    /// Pink noise (1/f spectrum)
    Pink,
    
    /// Brown noise (1/f² spectrum)
    Brown,
}

/// Noise oscillator
///
/// Generates various types of noise.
///
/// # Parameters
/// - `type`: Noise type (white/pink/brown)
/// - `amplitude`: Output amplitude (0.0 to 1.0)
///
/// # Outputs
/// - Port 0: Noise output
pub struct NoiseOsc<const BUF_SIZE: usize> {
    /// Noise type
    noise_type: NoiseType,
    
    /// Output amplitude
    amplitude: f32,
    
    /// Sample rate
    sample_rate: f32,
    
    // State for pink noise (Paul Kellett's method)
    pink_b0: f32,
    pink_b1: f32,
    pink_b2: f32,
    pink_b3: f32,
    pink_b4: f32,
    pink_b5: f32,
    pink_b6: f32,
    
    // State for brown noise
    brown_value: f32,
}

impl<const BUF_SIZE: usize> NoiseOsc<BUF_SIZE> {
    /// Create new noise oscillator
    pub fn new() -> Self {
        Self {
            noise_type: NoiseType::White,
            amplitude: 0.5,
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

    /// Generate white noise
    fn generate_white(&mut self) -> f32 {
        let mut rng = rand::thread_rng();
        (rng.gen::<f32>() * 2.0 - 1.0) * self.amplitude
    }

    /// Generate pink noise (1/f)
    fn generate_pink(&mut self) -> f32 {
        let mut rng = rand::thread_rng();
        let white = rng.gen::<f32>() * 2.0 - 1.0;

        self.pink_b0 = 0.99886 * self.pink_b0 + white * 0.0555179;
        self.pink_b1 = 0.99332 * self.pink_b1 + white * 0.0750759;
        self.pink_b2 = 0.96900 * self.pink_b2 + white * 0.1538520;
        self.pink_b3 = 0.86650 * self.pink_b3 + white * 0.3104856;
        self.pink_b4 = 0.55000 * self.pink_b4 + white * 0.5329522;
        self.pink_b5 = -0.7616 * self.pink_b5 - white * 0.0168980;

        let pink = self.pink_b0
            + self.pink_b1
            + self.pink_b2
            + self.pink_b3
            + self.pink_b4
            + self.pink_b5
            + self.pink_b6
            + white * 0.5362;
        
        self.pink_b6 = white * 0.115926;
        
        (pink * 0.11) * self.amplitude // Scale to approx [-1,1]
    }

    /// Generate brown noise (1/f²)
    fn generate_brown(&mut self) -> f32 {
        let mut rng = rand::thread_rng();
        let white = rng.gen::<f32>() * 2.0 - 1.0;

        self.brown_value = 0.997 * self.brown_value + white * 0.03;
        self.brown_value.clamp(-1.0, 1.0) * self.amplitude
    }

    /// Generate a block of samples
    fn generate_block(&mut self, output: &mut [f32; BUF_SIZE]) {
        match self.noise_type {
            NoiseType::White => {
                for sample in output.iter_mut() {
                    *sample = self.generate_white();
                }
            }
            NoiseType::Pink => {
                for sample in output.iter_mut() {
                    *sample = self.generate_pink();
                }
            }
            NoiseType::Brown => {
                for sample in output.iter_mut() {
                    *sample = self.generate_brown();
                }
            }
        }
    }
}

impl<const BUF_SIZE: usize> Default for NoiseOsc<BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const BUF_SIZE: usize> Processor<BUF_SIZE> for NoiseOsc<BUF_SIZE> {
    fn process(
        &mut self,
        _inputs: &[&[f32; BUF_SIZE]],
        outputs: &mut [&mut [f32; BUF_SIZE]],
    ) -> ProcessResult<()> {
        if outputs.is_empty() {
            return Ok(());
        }

        self.generate_block(outputs[0]);
        Ok(())
    }

    fn num_inputs(&self) -> usize {
        0
    }

    fn num_outputs(&self) -> usize {
        1
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
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

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match (id.as_str(), value) {
            ("type", ParamValue::Choice(t)) => {
                self.noise_type = match t.as_str() {
                    "white" => NoiseType::White,
                    "pink" => NoiseType::Pink,
                    "brown" => NoiseType::Brown,
                    _ => return Err(ProcessError::Parameter(format!("Unknown noise type: {}", t))),
                };
                self.reset();
                Ok(())
            }
            ("amplitude", ParamValue::Float(a)) => {
                self.amplitude = a.clamp(0.0, 1.0);
                Ok(())
            }
            _ => Err(ProcessError::Parameter(format!("Unknown parameter: {}", id))),
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.reset();
    }

    fn reset(&mut self) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noise_creation() {
        let noise = NoiseOsc::<64>::new()
            .with_type(NoiseType::Pink)
            .with_amplitude(0.3);
        
        assert!(matches!(noise.noise_type, NoiseType::Pink));
        assert_eq!(noise.amplitude, 0.3);
    }

    #[test]
    fn test_noise_generation() {
        let mut noise = NoiseOsc::<64>::new();
        noise.init(44100.0);
        
        let mut output = [0.0; 64];
        let mut outputs = [&mut output];
        
        noise.process(&[], &mut outputs).unwrap();
        
        // Should have some non-zero samples
        assert!(output.iter().any(|&x| x != 0.0));
        
        // All samples should be within amplitude range
        for &sample in &output {
            assert!(sample >= -1.0 && sample <= 1.0);
        }
    }

    #[test]
    fn test_noise_types() {
        let types = [NoiseType::White, NoiseType::Pink, NoiseType::Brown];
        
        for &noise_type in &types {
            let mut noise = NoiseOsc::<64>::new()
                .with_type(noise_type);
            
            noise.init(44100.0);
            
            let mut output = [0.0; 64];
            let mut outputs = [&mut output];
            
            noise.process(&[], &mut outputs).unwrap();
            
            // All types should produce valid output
            assert!(output.iter().any(|&x| x != 0.0));
        }
    }
}