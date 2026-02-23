//! Trigger generator for events and gates

use kama_core_traits::{
    param::{ParamMetadata, ParamType},
    AudioError, AudioNode, NodeCategory, NodeMetadata, NodeTypeId, ParamValue,
};

/// Trigger modes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TriggerMode {
    /// Single trigger on rising edge
    Rising,
    /// Single trigger on falling edge
    Falling,
    /// Gate mode (follows input)
    Gate,
    /// Toggle mode (alternates on each trigger)
    Toggle,
}

impl TriggerMode {
    /// Get all available modes as strings
    pub fn names() -> Vec<&'static str> {
        vec!["rising", "falling", "gate", "toggle"]
    }

    /// Get mode from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "rising" => Some(TriggerMode::Rising),
            "falling" => Some(TriggerMode::Falling),
            "gate" => Some(TriggerMode::Gate),
            "toggle" => Some(TriggerMode::Toggle),
            _ => None,
        }
    }
}

/// Trigger generator
///
/// Converts input signals to triggers and gates
pub struct Trigger {
    /// Trigger mode
    mode: TriggerMode,
    /// Current output value
    output: f32,
    /// Previous input value (for edge detection)
    last_input: f32,
    /// Trigger pulse duration in samples
    pulse_duration: usize,
    /// Current pulse counter
    pulse_counter: usize,
    /// Toggle state for toggle mode
    toggle_state: bool,
}

impl Trigger {
    /// Create a new trigger generator
    pub fn new() -> Self {
        Self {
            mode: TriggerMode::Rising,
            output: 0.0,
            last_input: 0.0,
            pulse_duration: 100, // ~2.3ms at 44.1kHz
            pulse_counter: 0,
            toggle_state: false,
        }
    }

    /// Set trigger mode
    pub fn with_mode(mut self, mode: TriggerMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set pulse duration in samples
    pub fn with_pulse_duration(mut self, samples: usize) -> Self {
        self.pulse_duration = samples.max(1);
        self
    }

    /// Set pulse duration in milliseconds
    pub fn with_pulse_ms(mut self, ms: f32, sample_rate: f32) -> Self {
        self.pulse_duration = ((ms / 1000.0) * sample_rate) as usize;
        self.pulse_duration = self.pulse_duration.max(1);
        self
    }

    /// Process one sample with input
    pub fn process(&mut self, input: f32) -> f32 {
        // Update pulse counter (decrement if active)
        if self.pulse_counter > 0 {
            self.pulse_counter -= 1;
            if self.pulse_counter == 0 {
                self.output = 0.0;
            }
        }

        // Edge detection and trigger generation
        match self.mode {
            TriggerMode::Rising => {
                if self.last_input <= 0.5 && input > 0.5 {
                    self.trigger();
                }
            }
            TriggerMode::Falling => {
                if self.last_input > 0.5 && input <= 0.5 {
                    self.trigger();
                }
            }
            TriggerMode::Gate => {
                self.output = if input > 0.5 { 1.0 } else { 0.0 };
            }
            TriggerMode::Toggle => {
                if (self.last_input <= 0.5 && input > 0.5) {
                    self.toggle_state = !self.toggle_state;
                    self.output = if self.toggle_state { 1.0 } else { 0.0 };
                }
            }
        }

        self.last_input = input;
        self.output
    }

    /// Process a block of samples
    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        let len = input.len().min(output.len());
        for i in 0..len {
            output[i] = self.process(input[i]);
        }
    }

    /// Generate a trigger pulse
    fn trigger(&mut self) {
        self.pulse_counter = self.pulse_duration;
        self.output = 1.0;
    }

    /// Check if currently triggered
    pub fn is_triggered(&self) -> bool {
        self.pulse_counter > 0
    }

    /// Get current output value
    pub fn output(&self) -> f32 {
        self.output
    }

    /// Reset state
    pub fn reset(&mut self) {
        self.output = 0.0;
        self.last_input = 0.0;
        self.pulse_counter = 0;
        self.toggle_state = false;
    }

    /// Set pulse duration in samples
    pub fn set_pulse_duration(&mut self, samples: usize) {
        self.pulse_duration = samples.max(1);
    }
}

impl Default for Trigger {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioNode for Trigger {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }

        let input = inputs[0];
        let output = &mut outputs[0];

        self.process_block(input, output);

        Ok(())
    }

    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "mode" => {
                let mode_str = match self.mode {
                    TriggerMode::Rising => "rising",
                    TriggerMode::Falling => "falling",
                    TriggerMode::Gate => "gate",
                    TriggerMode::Toggle => "toggle",
                };
                Some(ParamValue::Choice(mode_str.to_string()))
            }
            "pulse_duration" => Some(ParamValue::Int(self.pulse_duration as i32)),
            "output" => Some(ParamValue::Float(self.output)),
            "triggered" => Some(ParamValue::Bool(self.is_triggered())),
            _ => None,
        }
    }

    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("mode", ParamValue::Choice(m)) => {
                if let Some(mode) = TriggerMode::from_str(&m) {
                    self.mode = mode;
                    Ok(())
                } else {
                    Err(AudioError::Parameter(format!("Unknown mode: {}", m)))
                }
            }
            ("pulse_duration", ParamValue::Int(d)) => {
                self.set_pulse_duration(d as usize);
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
        self.reset();
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
            name: "Trigger".to_string(),
            category: NodeCategory::Utility,
            description: "Trigger and gate generator".to_string(),
            author: "Kama Oscillators".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "mode".to_string(),
                    typ: ParamType::Choice,
                    default: ParamValue::Choice("rising".to_string()),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: Some(
                        TriggerMode::names()
                            .iter()
                            .enumerate()
                            .map(|(i, &name)| (name.to_string(), i as f32))
                            .collect(),
                    ),
                },
                ParamMetadata {
                    name: "pulse_duration".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(100),
                    min: Some(1.0),
                    max: Some(44100.0),
                    step: Some(1.0),
                    unit: Some("samples".to_string()),
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
    fn test_trigger_rising() {
        let mut trigger = Trigger::new()
            .with_mode(TriggerMode::Rising)
            .with_pulse_duration(5);

        trigger.reset();

        assert_eq!(trigger.process(0.0), 0.0);
        assert_eq!(trigger.process(0.3), 0.0);
        assert_eq!(trigger.process(0.7), 1.0); // rising edge triggers immediately
        assert_eq!(trigger.process(0.8), 1.0); // still in pulse
                                               // after 5 samples, pulse ends
        for _ in 0..4 {
            trigger.process(0.9);
        }
        assert_eq!(trigger.process(1.0), 0.0);
    }

    #[test]
    fn test_trigger_gate() {
        let mut trigger = Trigger::new().with_mode(TriggerMode::Gate);

        trigger.reset();

        assert_eq!(trigger.process(0.0), 0.0);
        assert_eq!(trigger.process(0.3), 0.0);
        assert_eq!(trigger.process(0.7), 1.0); // follows input
        assert_eq!(trigger.process(0.8), 1.0);
        assert_eq!(trigger.process(0.4), 0.0);
        assert_eq!(trigger.process(0.3), 0.0);
    }

    #[test]
    fn test_trigger_toggle() {
        let mut trigger = Trigger::new().with_mode(TriggerMode::Toggle);

        trigger.reset();

        assert_eq!(trigger.process(0.0), 0.0);
        assert_eq!(trigger.process(0.3), 0.0);
        assert_eq!(trigger.process(0.7), 1.0); // first rising toggles on
        assert_eq!(trigger.process(0.8), 1.0);
        assert_eq!(trigger.process(0.3), 1.0); // still on
        assert_eq!(trigger.process(0.7), 0.0); // second rising toggles off
        assert_eq!(trigger.process(0.8), 0.0);
    }

    #[test]
    fn test_trigger_block() {
        let mut trigger = Trigger::new().with_pulse_duration(3);

        let input = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        let mut output = vec![0.0; 8];

        trigger.process_block(&input, &mut output);

        assert_eq!(output[0], 0.0);
        assert_eq!(output[1], 0.0);
        assert_eq!(output[2], 0.0);
        assert_eq!(output[3], 1.0, "Index 3 should be pulse");
        assert_eq!(output[4], 1.0, "Index 4 should be pulse");
        assert_eq!(output[5], 1.0, "Index 5 should be pulse");
        assert_eq!(output[6], 0.0, "Index 6 should be 0");
        assert_eq!(output[7], 0.0, "Index 7 should be 0");
    }
}
