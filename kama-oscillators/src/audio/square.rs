//! Square wave oscillator

use super::AudioOscillator;
use kama_core_traits::{
    param::{ParamMetadata, ParamType},
    AudioError, AudioNode, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
};

/// Square wave oscillator with variable pulse width
pub struct SquareOsc {
    /// Current phase (0.0 to 1.0)
    phase: f32,
    /// Frequency in Hz
    frequency: f32,
    /// Sample rate in Hz
    sample_rate: f32,
    /// Output amplitude (0.0 - 1.0)
    amplitude: f32,
    /// Pulse width (0.0 - 1.0), 0.5 = square
    pulse_width: f32,
    /// Whether to apply band-limiting
    bandlimited: bool,
}

impl SquareOsc {
    /// Create a new square oscillator
    pub fn new(frequency: f32) -> Self {
        Self {
            phase: 0.0,
            frequency,
            sample_rate: 44100.0,
            amplitude: 1.0,
            pulse_width: 0.5,
            bandlimited: true,
        }
    }

    /// Create with custom amplitude
    pub fn with_amplitude(mut self, amp: f32) -> Self {
        self.amplitude = amp.clamp(0.0, 1.0);
        self
    }

    /// Set pulse width (0.0 - 1.0)
    pub fn with_pulse_width(mut self, pw: f32) -> Self {
        self.pulse_width = pw.clamp(0.01, 0.99);
        self
    }

    /// Enable/disable band-limiting
    pub fn with_bandlimited(mut self, bl: bool) -> Self {
        self.bandlimited = bl;
        self
    }

    /// Generate next sample (non-bandlimited)
    fn generate_raw(&mut self) -> f32 {
        let sample = if self.phase < self.pulse_width {
            1.0
        } else {
            -1.0
        };

        let phase_inc = self.frequency / self.sample_rate;
        self.phase += phase_inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        sample * self.amplitude
    }

    /// Generate next sample with band-limiting
    fn generate_bandlimited(&mut self) -> f32 {
        let phase_inc = self.frequency / self.sample_rate;
        let next_phase = self.phase + phase_inc;

        let mut correction = 0.0;

        // Check for rising edge
        if self.phase < self.pulse_width && next_phase >= self.pulse_width {
            let t = (self.pulse_width - self.phase) / phase_inc;
            correction += 2.0 * (1.0 - t); // BLEP for rising edge
        }

        // Check for falling edge (wrap around)
        if next_phase >= 1.0 {
            let t = (1.0 - self.phase) / phase_inc;
            correction -= 2.0 * (1.0 - t); // BLEP for falling edge
        }

        let sample = if self.phase < self.pulse_width {
            1.0
        } else {
            -1.0
        };

        self.phase = if next_phase >= 1.0 {
            next_phase - 1.0
        } else {
            next_phase
        };

        (sample + correction) * self.amplitude
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

    /// Set pulse width
    pub fn set_pulse_width(&mut self, pw: f32) {
        self.pulse_width = pw.clamp(0.01, 0.99);
    }

    /// Get pulse width
    pub fn pulse_width(&self) -> f32 {
        self.pulse_width
    }
}

impl AudioOscillator for SquareOsc {
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

impl AudioNode for SquareOsc {
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
            "pulse_width" => Some(ParamValue::Float(self.pulse_width)),
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
            ("pulse_width", ParamValue::Float(p)) => {
                self.set_pulse_width(p);
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
            name: "Square Oscillator".to_string(),
            category: NodeCategory::Generator,
            description: "Square wave generator with variable pulse width".to_string(),
            author: "Kama Oscillators".to_string(),
            version: "0.1.0".to_string(),
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
                    name: "pulse_width".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.5),
                    min: Some(0.01),
                    max: Some(0.99),
                    step: Some(0.01),
                    unit: Some("%".to_string()),
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
    fn test_square_osc_generate() {
        let mut osc = SquareOsc::new(440.0).with_amplitude(0.5);
        osc.init(44100.0);

        let sample = osc.generate_raw();
        assert!(sample == 0.5 || sample == -0.5);
    }

    #[test]
    fn test_square_osc_pulse_width() {
        let mut osc = SquareOsc::new(440.0).with_pulse_width(0.25);
        osc.init(44100.0);

        assert_eq!(osc.pulse_width(), 0.25);
    }

    #[test]
    fn test_square_osc_block() {
        let mut osc = SquareOsc::new(440.0);
        osc.init(44100.0);

        let mut output = vec![0.0; 1024];
        osc.generate_block(&mut output);

        assert!(output.iter().any(|&x| x != 0.0));
    }
}
