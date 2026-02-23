//! Clock generator for tempo synchronization

use kama_core_traits::{
    param::{ParamMetadata, ParamType},
    AudioError, AudioNode, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
};

/// Clock division ratios
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClockDivision {
    Whole = 1,
    Half = 2,
    Quarter = 4,
    Eighth = 8,
    Sixteenth = 16,
    ThirtySecond = 32,
}

impl ClockDivision {
    /// Get all available divisions as strings
    pub fn names() -> Vec<&'static str> {
        vec!["1/1", "1/2", "1/4", "1/8", "1/16", "1/32"]
    }

    /// Get division from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "1/1" => Some(ClockDivision::Whole),
            "1/2" => Some(ClockDivision::Half),
            "1/4" => Some(ClockDivision::Quarter),
            "1/8" => Some(ClockDivision::Eighth),
            "1/16" => Some(ClockDivision::Sixteenth),
            "1/32" => Some(ClockDivision::ThirtySecond),
            _ => None,
        }
    }

    /// Get division as multiplier (1.0 = quarter note)
    pub fn as_multiplier(&self) -> f32 {
        match self {
            ClockDivision::Whole => 4.0,
            ClockDivision::Half => 2.0,
            ClockDivision::Quarter => 1.0,
            ClockDivision::Eighth => 0.5,
            ClockDivision::Sixteenth => 0.25,
            ClockDivision::ThirtySecond => 0.125,
        }
    }
}

/// Clock generator for tempo synchronization
///
/// Generates a continuous clock signal with configurable tempo and division
pub struct Clock {
    /// Tempo in BPM
    tempo: f32,
    /// Current division
    division: ClockDivision,
    /// Sample rate
    sample_rate: f32,

    /// Current phase (0.0 - 1.0)
    phase: f32,
    /// Current pulse state (0.0 or 1.0)
    pulse: f32,
    /// Whether a trigger occurred this sample
    triggered: bool,

    /// Samples per quarter note
    samples_per_quarter: f32,
    /// Samples per pulse based on division
    samples_per_pulse: f32,

    /// Whether clock is running
    running: bool,
}

impl Clock {
    /// Create a new clock generator
    pub fn new(tempo: f32) -> Self {
        let mut clock = Self {
            tempo: tempo.clamp(20.0, 300.0),
            division: ClockDivision::Quarter,
            sample_rate: 44100.0,
            phase: 0.0,
            pulse: 0.0,
            triggered: false,
            samples_per_quarter: 0.0,
            samples_per_pulse: 0.0,
            running: true,
        };
        clock.update_timing();
        clock
    }

    /// Set clock division
    pub fn with_division(mut self, division: ClockDivision) -> Self {
        self.division = division;
        self.update_timing();
        self
    }

    /// Update timing calculations
    fn update_timing(&mut self) {
        self.samples_per_quarter = (60.0 / self.tempo) * self.sample_rate;
        self.samples_per_pulse = self.samples_per_quarter * self.division.as_multiplier();
    }

    /// Generate next sample
    pub fn generate(&mut self) -> f32 {
        if !self.running {
            return 0.0;
        }

        // Current pulse value for this sample
        let current_pulse = self.pulse;
        self.triggered = current_pulse > 0.5;

        // Advance phase
        self.phase += 1.0 / self.samples_per_pulse;

        // Set next pulse if needed
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.pulse = 1.0;
        } else {
            self.pulse = 0.0;
        }

        current_pulse
    }

    /// Generate a block of samples
    pub fn generate_block(&mut self, output: &mut [f32]) {
        for out in output.iter_mut() {
            *out = self.generate();
        }
    }

    /// Get trigger state for current sample
    pub fn triggered(&self) -> bool {
        self.triggered
    }

    /// Set tempo in BPM
    pub fn set_tempo(&mut self, bpm: f32) {
        self.tempo = bpm.clamp(20.0, 300.0);
        self.update_timing();
    }

    /// Set division
    pub fn set_division(&mut self, division: ClockDivision) {
        self.division = division;
        self.update_timing();
    }

    /// Start the clock
    pub fn start(&mut self) {
        self.running = true;
    }

    /// Stop the clock
    pub fn stop(&mut self) {
        self.running = false;
        self.pulse = 0.0;
        self.triggered = false;
    }

    /// Reset phase (does not generate pulse immediately)
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.pulse = 0.0;
        self.triggered = false;
    }

    /// Get current phase (0.0 - 1.0)
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Get current pulse value
    pub fn pulse(&self) -> f32 {
        self.pulse
    }
}

impl AudioNode for Clock {
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
            "division" => {
                let div_str = match self.division {
                    ClockDivision::Whole => "1/1",
                    ClockDivision::Half => "1/2",
                    ClockDivision::Quarter => "1/4",
                    ClockDivision::Eighth => "1/8",
                    ClockDivision::Sixteenth => "1/16",
                    ClockDivision::ThirtySecond => "1/32",
                };
                Some(ParamValue::Choice(div_str.to_string()))
            }
            "phase" => Some(ParamValue::Float(self.phase)),
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
            ("division", ParamValue::Choice(d)) => {
                if let Some(div) = ClockDivision::from_str(&d) {
                    self.set_division(div);
                    Ok(())
                } else {
                    Err(AudioError::Parameter(format!("Unknown division: {}", d)))
                }
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
            name: "Clock Generator".to_string(),
            category: NodeCategory::Generator,
            description: "Tempo-synchronized clock generator".to_string(),
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
                    typ: ParamType::Choice,
                    default: ParamValue::Choice("1/4".to_string()),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: Some(
                        ClockDivision::names()
                            .iter()
                            .enumerate()
                            .map(|(i, &name)| (name.to_string(), i as f32))
                            .collect(),
                    ),
                },
                ParamMetadata {
                    name: "running".to_string(),
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
    fn test_clock_generate() {
        let mut clock = Clock::new(120.0).with_division(ClockDivision::ThirtySecond); // 32-е ноты — чаще
        clock.init(44100.0);

        let mut found = false;
        for i in 0..10000 {
            if clock.generate() > 0.5 {
                found = true;
                println!("Clock pulse at sample {}", i);
                break;
            }
        }
        assert!(found, "No clock pulse detected within 10000 samples");
    }

    #[test]
    fn test_clock_trigger() {
        let mut clock = Clock::new(120.0).with_division(ClockDivision::ThirtySecond);
        clock.init(44100.0);

        let mut triggered = false;
        for _ in 0..10000 {
            clock.generate();
            if clock.triggered() {
                triggered = true;
                break;
            }
        }
        assert!(triggered, "Clock should trigger within 10000 samples");
    }

    #[test]
    fn test_clock_division() {
        let mut clock = Clock::new(120.0);
        clock.init(44100.0);

        clock.set_division(ClockDivision::Eighth);
        assert_eq!(clock.division.as_multiplier(), 0.5);
    }

    #[test]
    fn test_clock_block() {
        let mut clock = Clock::new(120.0).with_division(ClockDivision::ThirtySecond);
        clock.init(44100.0);

        let mut output = vec![0.0; 10000];
        clock.generate_block(&mut output);

        let pulse_count = output.iter().filter(|&&x| x == 1.0).count();
        println!("Clock pulses: {}", pulse_count);
        assert!(pulse_count > 0, "Should have pulses");
        assert!(
            pulse_count < output.len(),
            "Not all samples should be pulses"
        );
    }
}
