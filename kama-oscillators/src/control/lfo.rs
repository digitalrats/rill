//! Low-Frequency Oscillators for modulation
//!
//! Frequency range: 0.01Hz - 100Hz

use kama_core_traits::{
    param::{ParamMetadata, ParamType},
    AudioError, AudioNode, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
};

/// LFO waveform types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LfoWaveform {
    Sine,
    Triangle,
    Saw,
    Square,
    SampleAndHold,
    RandomWalk,
}

impl LfoWaveform {
    /// Get all available waveforms as strings
    pub fn names() -> Vec<&'static str> {
        vec!["sine", "triangle", "saw", "square", "s&h", "random_walk"]
    }

    /// Get waveform from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "sine" => Some(LfoWaveform::Sine),
            "triangle" => Some(LfoWaveform::Triangle),
            "saw" => Some(LfoWaveform::Saw),
            "square" => Some(LfoWaveform::Square),
            "s&h" | "sample_and_hold" => Some(LfoWaveform::SampleAndHold),
            "random_walk" => Some(LfoWaveform::RandomWalk),
            _ => None,
        }
    }
}

/// Low-Frequency Oscillator for modulation
///
/// Pure generator - no connection to parameters or automation.
/// Use with kama-automation to modulate parameters.
pub struct Lfo {
    /// Current phase (0.0 - 1.0)
    phase: f64,
    /// Frequency in Hz
    frequency: f64,
    /// Amplitude (0.0 - 1.0)
    amplitude: f64,
    /// Offset (-1.0 - 1.0)
    offset: f64,
    /// Waveform shape
    waveform: LfoWaveform,
    /// Sample rate
    sample_rate: f64,

    // State for Sample & Hold
    last_random: f64,
    hold_counter: usize,
    hold_time: usize,

    // State for Random Walk
    walk_value: f64,
    walk_step: f64,
}

impl Lfo {
    /// Create a new LFO
    pub fn new(frequency: f64, amplitude: f64, offset: f64) -> Self {
        Self {
            phase: 0.0,
            frequency: frequency.clamp(0.01, 100.0),
            amplitude: amplitude.clamp(0.0, 1.0),
            offset: offset.clamp(-1.0, 1.0),
            waveform: LfoWaveform::Sine,
            sample_rate: 44100.0,
            last_random: 0.0,
            hold_counter: 0,
            hold_time: (44100.0 / 10.0) as usize, // 0.1 seconds default hold
            walk_value: 0.0,
            walk_step: 0.01,
        }
    }

    /// Set waveform
    pub fn with_waveform(mut self, waveform: LfoWaveform) -> Self {
        self.waveform = waveform;
        self
    }

    /// Set hold time for Sample & Hold (in seconds)
    pub fn with_hold_time(mut self, seconds: f64) -> Self {
        self.hold_time = (seconds * self.sample_rate) as usize;
        self
    }

    /// Set step size for Random Walk
    pub fn with_walk_step(mut self, step: f64) -> Self {
        self.walk_step = step.abs();
        self
    }

    /// Generate next sample
    pub fn generate(&mut self) -> f64 {
        let phase_inc = self.frequency / self.sample_rate;
        self.phase += phase_inc;

        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        let raw = match self.waveform {
            LfoWaveform::Sine => (self.phase * 2.0 * std::f64::consts::PI).sin(),

            LfoWaveform::Triangle => {
                if self.phase < 0.5 {
                    4.0 * self.phase - 1.0
                } else {
                    3.0 - 4.0 * self.phase
                }
            }

            LfoWaveform::Saw => 2.0 * self.phase - 1.0,

            LfoWaveform::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }

            LfoWaveform::SampleAndHold => {
                self.hold_counter += 1;
                if self.hold_counter >= self.hold_time {
                    self.hold_counter = 0;
                    self.last_random = rand::random::<f64>() * 2.0 - 1.0;
                }
                self.last_random
            }

            LfoWaveform::RandomWalk => {
                self.walk_value += (rand::random::<f64>() - 0.5) * self.walk_step;
                self.walk_value = self.walk_value.clamp(-1.0, 1.0);
                self.walk_value
            }
        };

        raw * self.amplitude + self.offset
    }

    /// Generate a block of samples
    pub fn generate_block(&mut self, output: &mut [f64]) {
        for out in output.iter_mut() {
            *out = self.generate();
        }
    }

    /// Peek current value without advancing phase
    pub fn peek(&self) -> f64 {
        let raw = match self.waveform {
            LfoWaveform::Sine => (self.phase * 2.0 * std::f64::consts::PI).sin(),
            LfoWaveform::Triangle => {
                if self.phase < 0.5 {
                    4.0 * self.phase - 1.0
                } else {
                    3.0 - 4.0 * self.phase
                }
            }
            LfoWaveform::Saw => 2.0 * self.phase - 1.0,
            LfoWaveform::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoWaveform::SampleAndHold => self.last_random,
            LfoWaveform::RandomWalk => self.walk_value,
        };
        raw * self.amplitude + self.offset
    }

    /// Set frequency
    pub fn set_frequency(&mut self, freq: f64) {
        self.frequency = freq.clamp(0.01, 100.0);
    }

    /// Set amplitude
    pub fn set_amplitude(&mut self, amp: f64) {
        self.amplitude = amp.clamp(0.0, 1.0);
    }

    /// Set offset
    pub fn set_offset(&mut self, offset: f64) {
        self.offset = offset.clamp(-1.0, 1.0);
    }

    /// Reset phase and internal state
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.last_random = 0.0;
        self.walk_value = 0.0;
        self.hold_counter = 0;
    }

    /// Get current phase (0.0 - 1.0)
    pub fn phase(&self) -> f64 {
        self.phase
    }
}

impl AudioNode for Lfo {
    fn process(
        &mut self,
        _inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
    ) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }

        let output = &mut outputs[0];
        for out in output.iter_mut() {
            *out = self.generate() as f32;
        }

        Ok(())
    }

    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "frequency" => Some(ParamValue::Float(self.frequency as f32)),
            "amplitude" => Some(ParamValue::Float(self.amplitude as f32)),
            "offset" => Some(ParamValue::Float(self.offset as f32)),
            "phase" => Some(ParamValue::Float(self.phase as f32)),
            "waveform" => {
                let w = match self.waveform {
                    LfoWaveform::Sine => "sine",
                    LfoWaveform::Triangle => "triangle",
                    LfoWaveform::Saw => "saw",
                    LfoWaveform::Square => "square",
                    LfoWaveform::SampleAndHold => "s&h",
                    LfoWaveform::RandomWalk => "random_walk",
                };
                Some(ParamValue::Choice(w.to_string()))
            }
            _ => None,
        }
    }

    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("frequency", ParamValue::Float(f)) => {
                self.set_frequency(f as f64);
                Ok(())
            }
            ("amplitude", ParamValue::Float(a)) => {
                self.set_amplitude(a as f64);
                Ok(())
            }
            ("offset", ParamValue::Float(o)) => {
                self.set_offset(o as f64);
                Ok(())
            }
            ("waveform", ParamValue::Choice(s)) => {
                self.waveform = LfoWaveform::from_str(&s)
                    .ok_or_else(|| AudioError::Parameter(format!("Unknown waveform: {}", s)))?;
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
        self.hold_time = (0.1 * self.sample_rate) as usize; // Recalculate hold time
    }

    fn reset(&mut self) {
        self.reset();
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
            name: "LFO".to_string(),
            category: NodeCategory::Generator,
            description: "Low-Frequency Oscillator for modulation".to_string(),
            author: "Kama Oscillators".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "frequency".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.01),
                    max: Some(100.0),
                    step: Some(0.01),
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
                    unit: None,
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
                ParamMetadata {
                    name: "waveform".to_string(),
                    typ: ParamType::Choice,
                    default: ParamValue::Choice("sine".to_string()),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: Some(
                        LfoWaveform::names()
                            .iter()
                            .enumerate()
                            .map(|(i, &name)| (name.to_string(), i as f32))
                            .collect(),
                    ),
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;

    #[test]
    fn test_lfo_generate() {
        let mut lfo = Lfo::new(1.0, 1.0, 0.0);
        lfo.init(44100.0);

        let val = lfo.generate();
        assert!(val >= -1.0 && val <= 1.0);
    }

    #[test]
    fn test_lfo_waveforms() {
        let waveforms = [
            LfoWaveform::Sine,
            LfoWaveform::Triangle,
            LfoWaveform::Saw,
            LfoWaveform::Square,
        ];

        for w in waveforms {
            let mut lfo = Lfo::new(1.0, 1.0, 0.0).with_waveform(w);
            lfo.init(44100.0);

            let val = lfo.generate();
            assert!(val >= -1.0 && val <= 1.0);
        }
    }

    #[test]
    fn test_lfo_parameters() {
        let mut lfo = Lfo::new(1.0, 1.0, 0.0);

        lfo.set_param("frequency", ParamValue::Float(2.0)).unwrap();
        assert!(approx_eq!(f64, lfo.frequency, 2.0, epsilon = 0.001));

        lfo.set_param("amplitude", ParamValue::Float(0.5)).unwrap();
        assert!(approx_eq!(f64, lfo.amplitude, 0.5, epsilon = 0.001));

        lfo.set_param("offset", ParamValue::Float(0.2)).unwrap();
        assert!(approx_eq!(f64, lfo.offset, 0.2, epsilon = 0.001));
    }
}
