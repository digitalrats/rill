//! Pulse generator for rhythmic patterns

use kama_core_traits::{
    param::{ParamMetadata, ParamType},
    AudioError, AudioNode, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
};

/// Pulse pattern generator
///
/// Generates rhythmic pulse patterns based on division and probability
pub struct PulseGenerator {
    /// Tempo in BPM
    tempo: f32,
    /// Division (1 = whole note, 4 = quarter, 8 = eighth, etc.)
    division: u32,
    /// Pulse width (0.0 - 1.0) as fraction of period
    width: f32,
    /// Probability of pulse occurring (0.0 - 1.0)
    probability: f32,
    /// Sample rate
    sample_rate: f32,

    /// Current phase (0.0 - 1.0)
    phase: f32,
    /// Current pulse state
    pulse: f32,
    /// Samples per pulse period
    samples_per_period: f32,
    /// Samples per pulse width
    samples_per_width: f32,

    /// Whether generator is running
    running: bool,
}

impl PulseGenerator {
    /// Create a new pulse generator
    pub fn new(tempo: f32) -> Self {
        let mut gen = Self {
            tempo: tempo.clamp(20.0, 300.0),
            division: 4, // quarter notes
            width: 0.5,
            probability: 1.0,
            sample_rate: 44100.0,
            phase: 0.0,
            pulse: 0.0,
            samples_per_period: 0.0,
            samples_per_width: 0.0,
            running: true,
        };
        gen.update_timing();
        gen
    }

    /// Set division (1 = whole note, 4 = quarter, 8 = eighth, etc.)
    pub fn with_division(mut self, div: u32) -> Self {
        self.division = div.max(1);
        self.update_timing();
        self
    }

    /// Set pulse width (0.0 - 1.0)
    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width.clamp(0.01, 0.99);
        self.update_timing();
        self
    }

    /// Set probability (0.0 - 1.0)
    pub fn with_probability(mut self, prob: f32) -> Self {
        self.probability = prob.clamp(0.0, 1.0);
        self
    }

    /// Update timing calculations
    fn update_timing(&mut self) {
        // Samples per whole note at current tempo
        let samples_per_whole = (60.0 / self.tempo) * 4.0 * self.sample_rate;

        // Samples per pulse period based on division
        self.samples_per_period = samples_per_whole / self.division as f32;

        // Samples per pulse width
        self.samples_per_width = self.samples_per_period * self.width;
    }

    /// Generate next sample
    pub fn generate(&mut self) -> f32 {
        if !self.running {
            return 0.0;
        }

        // Advance phase
        self.phase += 1.0 / self.samples_per_period;

        // Check for new period
        if self.phase >= 1.0 {
            self.phase -= 1.0;

            // Random chance for pulse this period
            if rand::random::<f32>() < self.probability {
                self.pulse = 1.0;
            } else {
                self.pulse = 0.0;
            }
        }

        // Turn off pulse after width
        if self.phase * self.samples_per_period > self.samples_per_width {
            self.pulse = 0.0;
        }

        self.pulse
    }

    /// Generate a block of samples
    pub fn generate_block(&mut self, output: &mut [f32]) {
        for out in output.iter_mut() {
            *out = self.generate();
        }
    }

    /// Set tempo
    pub fn set_tempo(&mut self, bpm: f32) {
        self.tempo = bpm.clamp(20.0, 300.0);
        self.update_timing();
    }

    /// Set division
    pub fn set_division(&mut self, div: u32) {
        self.division = div.max(1);
        self.update_timing();
    }

    /// Set pulse width
    pub fn set_width(&mut self, width: f32) {
        self.width = width.clamp(0.01, 0.99);
        self.update_timing();
    }

    /// Set probability
    pub fn set_probability(&mut self, prob: f32) {
        self.probability = prob.clamp(0.0, 1.0);
    }

    /// Start the generator
    pub fn start(&mut self) {
        self.running = true;
    }

    /// Stop the generator
    pub fn stop(&mut self) {
        self.running = false;
        self.pulse = 0.0;
    }

    /// Reset phase (does not generate pulse immediately)
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.pulse = 0.0;
    }

    /// Get current pulse value
    pub fn pulse(&self) -> f32 {
        self.pulse
    }
}

impl AudioNode for PulseGenerator {
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
            "tempo" => Some(ParamValue::Float(self.tempo)),
            "division" => Some(ParamValue::Int(self.division as i32)),
            "width" => Some(ParamValue::Float(self.width)),
            "probability" => Some(ParamValue::Float(self.probability)),
            "pulse" => Some(ParamValue::Float(self.pulse)),
            "running" => Some(ParamValue::Bool(self.running)),
            _ => None,
        }
    }

    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("tempo", ParamValue::Float(t)) => {
                self.set_tempo(t);
                Ok(())
            }
            ("division", ParamValue::Int(d)) => {
                self.set_division(d as u32);
                Ok(())
            }
            ("width", ParamValue::Float(w)) => {
                self.set_width(w);
                Ok(())
            }
            ("probability", ParamValue::Float(p)) => {
                self.set_probability(p);
                Ok(())
            }
            ("running", ParamValue::Bool(r)) => {
                if r {
                    self.start();
                } else {
                    self.stop();
                }
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
        self.update_timing();
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
            name: "Pulse Generator".to_string(),
            category: NodeCategory::Generator,
            description: "Rhythmic pulse generator with probability".to_string(),
            author: "Kama Oscillators".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "tempo".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(120.0),
                    min: Some(20.0),
                    max: Some(300.0),
                    step: Some(1.0),
                    unit: Some("BPM".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "division".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(4),
                    min: Some(1.0),
                    max: Some(64.0),
                    step: Some(1.0),
                    unit: Some("notes".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "width".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.5),
                    min: Some(0.01),
                    max: Some(0.99),
                    step: Some(0.01),
                    unit: Some("%".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "probability".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("%".to_string()),
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
    fn test_pulse_generate() {
        let mut pulse = PulseGenerator::new(120.0)
            .with_division(64) // период ~1378 сэмплов
            .with_width(0.1)
            .with_probability(1.0);
        pulse.init(44100.0);

        // Ждём до 20000 сэмплов, должен появиться импульс
        let mut found = false;
        for i in 0..20000 {
            if pulse.generate() > 0.5 {
                found = true;
                println!("Pulse found at sample {}", i);
                break;
            }
        }
        assert!(found, "No pulse generated within 20000 samples");
    }

    #[test]
    fn test_pulse_division() {
        let mut pulse = PulseGenerator::new(120.0);
        pulse.init(44100.0);

        pulse.set_division(8);
        assert_eq!(pulse.division, 8);
    }

    #[test]
    fn test_pulse_probability() {
        let mut pulse = PulseGenerator::new(120.0)
            .with_division(64)
            .with_probability(0.0);
        pulse.init(44100.0);

        // Даже с probability 0, первый период может дать импульс? Нет, потому что при старте phase=0, pulse=0.
        // При достижении фазы 1.0, проверяется вероятность. С probability 0 никогда не будет импульса.
        let mut pulsed = false;
        for _ in 0..20000 {
            if pulse.generate() > 0.5 {
                pulsed = true;
                break;
            }
        }
        assert!(!pulsed, "Should not pulse with probability 0");
    }

    #[test]
    fn test_pulse_block() {
        let mut pulse = PulseGenerator::new(120.0)
            .with_division(64)
            .with_probability(1.0);
        pulse.init(44100.0);

        let mut output = vec![0.0; 20000];
        pulse.generate_block(&mut output);

        let pulse_count = output.iter().filter(|&&x| x > 0.5).count();
        println!("Pulse count: {} out of {}", pulse_count, output.len());
        assert!(pulse_count > 0, "Should have some pulses");
        assert!(
            pulse_count < output.len(),
            "Not all samples should be pulses"
        );
    }
}
