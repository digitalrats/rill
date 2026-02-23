//! Sawtooth wave oscillator

use super::AudioOscillator;
use kama_core::traits::{
    ParamMetadata, ParamType,
    AudioError, AudioNode, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
};

/// Sawtooth wave oscillator
///
/// Generates a sawtooth waveform with optional band-limiting
pub struct SawOsc {
    /// Current phase (0.0 to 1.0)
    phase: f32,
    /// Frequency in Hz
    frequency: f32,
    /// Sample rate in Hz
    sample_rate: f32,
    /// Output amplitude (0.0 - 1.0)
    amplitude: f32,
    /// Whether to apply band-limiting (anti-aliasing)
    bandlimited: bool,
}

impl SawOsc {
    /// Create a new sawtooth oscillator
    pub fn new(frequency: f32) -> Self {
        Self {
            phase: 0.0,
            frequency,
            sample_rate: 44100.0,
            amplitude: 1.0,
            bandlimited: true,
        }
    }

    /// Create with custom amplitude
    pub fn with_amplitude(mut self, amp: f32) -> Self {
        self.amplitude = amp.clamp(0.0, 1.0);
        self
    }

    /// Enable/disable band-limiting
    pub fn with_bandlimited(mut self, bl: bool) -> Self {
        self.bandlimited = bl;
        self
    }

    /// Generate next sample (non-bandlimited)
    fn generate_raw(&mut self) -> f32 {
        let sample = 2.0 * self.phase - 1.0;

        let phase_inc = self.frequency / self.sample_rate;
        self.phase += phase_inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        sample * self.amplitude
    }

    /// Generate next sample with band-limiting (BLEP)
    fn generate_bandlimited(&mut self) -> f32 {
        // Simple BLEP (Band-Limited Impulse) approximation
        // For a proper implementation, we'd need a more sophisticated algorithm
        let raw = 2.0 * self.phase - 1.0;

        // Check if we just passed a discontinuity
        let phase_inc = self.frequency / self.sample_rate;
        let next_phase = self.phase + phase_inc;

        let correction = if next_phase >= 1.0 {
            // We passed the discontinuity, apply BLEP
            let t = (1.0 - self.phase) / phase_inc; // fractional position of discontinuity
            -2.0 * (1.0 - t) // Simple linear BLEP
        } else {
            0.0
        };

        self.phase = if next_phase >= 1.0 {
            next_phase - 1.0
        } else {
            next_phase
        };

        (raw + correction) * self.amplitude
    }

    /// Generate a block of samples
    pub fn generate_block(&mut self, output: &mut [f32]) {
        if self.bandlimited {
            for out in output.iter_mut() {
                *out = self.generate_bandlimited();
            }
        } else {
            for out in output.iter_mut() {
                *out = self.generate_raw();
            }
        }
    }
}

impl AudioOscillator for SawOsc {
    fn set_frequency(&mut self, freq: f32) {
        self.frequency = freq.max(20.0).min(20000.0);
    }

    fn frequency(&self) -> f32 {
        self.frequency
    }

    fn set_amplitude(&mut self, amp: f32) {
        self.amplitude = amp.clamp(0.0, 1.0);
    }

    fn amplitude(&self) -> f32 {
        self.amplitude
    }

    fn reset_phase(&mut self) {
        self.phase = 0.0;
    }
}

impl AudioNode for SawOsc {
    fn process(
        &mut self,
        _inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
    ) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }

        let output = &mut outputs[0];
        self.generate_block(output);

        Ok(())
    }

    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "frequency" => Some(ParamValue::Float(self.frequency)),
            "amplitude" => Some(ParamValue::Float(self.amplitude)),
            "bandlimited" => Some(ParamValue::Bool(self.bandlimited)),
            _ => None,
        }
    }

    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("frequency", ParamValue::Float(f)) => {
                self.set_frequency(f);
                Ok(())
            }
            ("amplitude", ParamValue::Float(a)) => {
                self.set_amplitude(a);
                Ok(())
            }
            ("bandlimited", ParamValue::Bool(b)) => {
                self.bandlimited = b;
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
    }

    fn reset(&mut self) {
        self.phase = 0.0;
    }

    fn num_inputs(&self) -> usize {
        0
    }
    fn num_outputs(&self) -> usize {
        1
    }

    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }

    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Sawtooth Oscillator".to_string(),
            category: NodeCategory::Generator,
            description: "Sawtooth wave generator with optional band-limiting".to_string(),
            author: "Kama Oscillators".to_string(),
            version: "0.2.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "frequency".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(440.0),
                    min: Some(20.0),
                    max: Some(20000.0),
                    step: Some(1.0),
                    unit: Some("Hz".to_string()),
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
                    name: "bandlimited".to_string(),
                    typ: ParamType::Bool,
                    default: ParamValue::Bool(true),
                    min: None,
                    max: None,
                    step: None,
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
    fn test_saw_osc_generate() {
        let mut osc = SawOsc::new(440.0).with_amplitude(0.5);
        osc.init(44100.0);

        let sample = osc.generate_raw();
        assert!(sample >= -0.5 && sample <= 0.5);
    }

    #[test]
    fn test_saw_osc_block() {
        let mut osc = SawOsc::new(440.0);
        osc.init(44100.0);

        let mut output = vec![0.0; 1024];
        osc.generate_block(&mut output);

        assert!(output.iter().any(|&x| x != 0.0));
    }

    #[test]
    fn test_saw_osc_bandlimited() {
        let mut osc = SawOsc::new(440.0).with_bandlimited(true);
        osc.init(44100.0);

        let raw = osc.generate_raw();
        let bl = osc.generate_bandlimited();

        // Bandlimited version should be different
        assert!((raw - bl).abs() > 0.001);
    }
}
