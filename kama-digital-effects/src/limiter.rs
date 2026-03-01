// kama-digital-effects/src/limiter.rs

use kama_core::buffer::DelayLine;
use kama_core::traits::{
    ParamMetadata, ParamRange, ParamType, NodeCategory, NodeMetadata, NodeTypeId,
    ParameterId, ParamValue, Processor,
};
use kama_core::{ProcessResult, ProcessError};
use kama_core::math::AudioNum;
use crate::delay::Delay;

/// Maximum lookahead time in seconds (10 ms)
const MAX_LOOKAHEAD_TIME: f32 = 0.01;
/// Maximum sample rate we support (192 kHz)
const MAX_SAMPLE_RATE: f32 = 192_000.0;
/// Maximum lookahead samples at max sample rate
const MAX_LOOKAHEAD_SAMPLES: usize = (MAX_LOOKAHEAD_TIME * MAX_SAMPLE_RATE) as usize;
/// Size of analysis buffer (double the max lookahead)
const ANALYSIS_BUF_SIZE: usize = MAX_LOOKAHEAD_SAMPLES * 2;

/// Limiter with lookahead using Delay + envelope detection
pub struct Limiter<const BUF_SIZE: usize> {
    /// Delay line for lookahead
    delay: Delay<BUF_SIZE>,
    /// Buffer for envelope detection
    analysis_buffer: DelayLine<f32, ANALYSIS_BUF_SIZE>,
    /// Threshold in dB
    threshold_db: f32,
    /// Threshold in linear scale
    threshold_linear: f32,
    /// Output gain after limiting
    output_gain: f32,
    /// Attack time in seconds
    attack: f32,
    /// Release time in seconds
    release: f32,
    /// Lookahead time in seconds
    lookahead: f32,
    /// Lookahead in samples
    lookahead_samples: usize,
    /// Current gain reduction
    current_gain: f32,
    /// Attack coefficient
    attack_coeff: f32,
    /// Release coefficient
    release_coeff: f32,
    /// Sample rate
    sample_rate: f32,
    /// Current write position
    position: usize,
    /// Buffer for direct passthrough during initialization
    init_buffer: Vec<f32>,
    /// Whether we're in initialization phase
    initializing: bool,
    /// Whether we're in warmup phase after initialization
    warming_up: bool,
}

impl<const BUF_SIZE: usize> Limiter<BUF_SIZE> {
    /// Create a new limiter
    pub fn new(threshold_db: f32, attack: f32, release: f32, output_gain: f32) -> Self {
        let threshold_db = threshold_db.clamp(-60.0, 0.0);
        let threshold_linear = 10.0_f32.powf(threshold_db / 20.0);

        let sample_rate = 44100.0;
        let attack = attack.clamp(0.001, 0.1);
        let release = release.clamp(0.01, 1.0);

        let attack_coeff = (-1.0 / (attack * sample_rate)).exp();
        let release_coeff = (-1.0 / (release * sample_rate)).exp();

        let lookahead = 0.005; // 5ms default
        let lookahead_samples = (lookahead * sample_rate) as usize;

        // Delay with needed delay, feedback=0, mix=1.0 (100% wet)
        let delay = Delay::with_params(lookahead, 0.0, 1.0);

        // Buffer for analysis
        let analysis_buffer = DelayLine::new(sample_rate);

        // Buffer for temporary storage during initialization
        let init_buffer = Vec::with_capacity(lookahead_samples);

        Self {
            delay,
            analysis_buffer,
            threshold_db,
            threshold_linear,
            output_gain: output_gain.clamp(0.0, 2.0),
            attack,
            release,
            lookahead,
            lookahead_samples,
            current_gain: 1.0,
            attack_coeff,
            release_coeff,
            sample_rate,
            position: 0,
            init_buffer,
            initializing: true,
            warming_up: false,
        }
    }

    /// Process a single sample
    pub fn process_sample(&mut self, input: f32) -> f32 {
        self.position += 1;

        // 1. Write input to analysis_buffer
        self.analysis_buffer.write(input);

        // 2. Get delayed signal from Delay
        let delayed = self.delay.process_sample(input);

        // 3. During initialization phase
        if self.initializing {
            // Save input to init_buffer
            self.init_buffer.push(input);

            // Check if initialization is complete
            if self.position >= self.lookahead_samples {
                self.initializing = false;
                self.warming_up = true;

                // Clear Delay
                self.delay.reset();

                println!("Initialization complete, starting warmup...");
            }

            // During initialization output = input
            if self.position <= 5 {
                println!("Init {}: in={:.3}, out={:.3}", self.position, input, input);
            }

            return input;
        }

        // 4. During warmup phase (first lookahead_samples after initialization)
        if self.warming_up {
            // Still use input as output while Delay fills with real data
            if self.position < self.lookahead_samples * 2 {
                if self.position == self.lookahead_samples + 1 {
                    println!("Warmup started at pos {}", self.position);
                }

                // Fill Delay with real values
                if self.position - self.lookahead_samples <= self.init_buffer.len() {
                    let idx = self.position - self.lookahead_samples - 1;
                    if idx < self.init_buffer.len() {
                        let sample = self.init_buffer[idx];
                        let _ = self.delay.process_sample(sample);
                    }
                }

                // Check if warmup is complete
                if self.position >= self.lookahead_samples * 2 - 1 {
                    self.warming_up = false;
                    println!("Warmup complete at pos {}", self.position);
                }

                return input;
            }
        }

        // 5. Analyze signal in analysis_buffer
        // Look for maximum amplitude within lookahead window
        let mut max_amp = 0.0f32;
        for offset in 0..self.lookahead_samples {
            let sample = self.analysis_buffer.read_delayed(offset);
            max_amp = max_amp.max(sample.abs());
        }

        // 6. Compute target gain
        let target_gain = if max_amp > self.threshold_linear {
            self.threshold_linear / max_amp
        } else {
            1.0
        };

        // 7. Smooth gain
        if target_gain < self.current_gain {
            self.current_gain =
                self.current_gain * self.attack_coeff + target_gain * (1.0 - self.attack_coeff);
        } else {
            self.current_gain =
                self.current_gain * self.release_coeff + target_gain * (1.0 - self.release_coeff);
        }

        // 8. Apply gain to delayed signal
        let output = delayed * self.current_gain * self.output_gain;

        // Debug for high signal
        if input > 1.0 && self.position > self.lookahead_samples * 2 {
            println!("PROC: pos={}, in={:.3}, max={:.3}, target={:.3}, gain={:.3}, delay={:.3}, out={:.3}", 
                     self.position, input, max_amp, target_gain, self.current_gain, delayed, output);
        }

        output.clamp(-2.0, 2.0)
    }

    /// Process a block of samples
    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        for i in 0..input.len().min(output.len()) {
            output[i] = self.process_sample(input[i]);
        }
    }

    /// Get current gain reduction
    pub fn current_gain(&self) -> f32 {
        self.current_gain
    }

    /// Get lookahead samples count
    pub fn lookahead_samples(&self) -> usize {
        self.lookahead_samples
    }

    /// Set threshold in dB
    pub fn set_threshold(&mut self, db: f32) {
        self.threshold_db = db.clamp(-60.0, 0.0);
        self.threshold_linear = 10.0_f32.powf(self.threshold_db / 20.0);
    }

    /// Set attack time
    pub fn set_attack(&mut self, attack: f32) {
        self.attack = attack.clamp(0.001, 0.1);
        self.attack_coeff = (-1.0 / (self.attack * self.sample_rate)).exp();
    }

    /// Set release time
    pub fn set_release(&mut self, release: f32) {
        self.release = release.clamp(0.01, 1.0);
        self.release_coeff = (-1.0 / (self.release * self.sample_rate)).exp();
    }

    /// Set lookahead time
    pub fn set_lookahead(&mut self, lookahead: f32) {
        self.lookahead = lookahead.clamp(0.0, 0.01);
        self.lookahead_samples = (self.lookahead * self.sample_rate) as usize;
        self.delay.set_delay_time(lookahead);
        self.analysis_buffer.clear();
        self.current_gain = 1.0;
        self.position = 0;
        self.init_buffer.clear();
        self.initializing = true;
        self.warming_up = false;
    }

    /// Reset internal state - now with forced buffer filling
    pub fn reset(&mut self) {
        self.current_gain = 1.0;
        self.position = 0;
        self.init_buffer.clear();
        self.initializing = true;
        self.warming_up = false;
        self.delay.reset();
        self.analysis_buffer.clear();
    }

    /// Force finish initialization and warmup (for tests)
    pub fn force_ready(&mut self) {
        if self.initializing || self.warming_up {
            // Fill buffers with test values
            for _ in 0..self.lookahead_samples * 2 {
                let test_val = 0.1;
                self.analysis_buffer.write(test_val);
                let _ = self.delay.process_sample(test_val);
            }
            self.initializing = false;
            self.warming_up = false;
            self.position = self.lookahead_samples * 2;
            println!("Force ready completed");
        }
    }
}

impl<const BUF_SIZE: usize> Processor<f32, BUF_SIZE> for Limiter<BUF_SIZE> {
    fn process(
        &mut self,
        inputs: &[&[f32; BUF_SIZE]],
        outputs: &mut [&mut [f32; BUF_SIZE]],
        _control: &[f32],
    ) -> ProcessResult<()> {
        if inputs.len() < 1 || outputs.len() < 1 {
            return Err(ProcessError::processing("insufficient channels"));
        }
        let input = inputs[0];
        let output = &mut outputs[0];
        for i in 0..BUF_SIZE {
            output[i] = self.process_sample(input[i]);
        }
        Ok(())
    }

    fn num_audio_inputs(&self) -> usize {
        1
    }

    fn num_audio_outputs(&self) -> usize {
        1
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "threshold" => Some(ParamValue::Float(self.threshold_db)),
            "attack" => Some(ParamValue::Float(self.attack)),
            "release" => Some(ParamValue::Float(self.release)),
            "output_gain" => Some(ParamValue::Float(self.output_gain)),
            "lookahead" => Some(ParamValue::Float(self.lookahead)),
            "current_gain" => Some(ParamValue::Float(self.current_gain)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match (id.as_str(), value) {
            ("threshold", ParamValue::Float(t)) => {
                self.set_threshold(t);
                Ok(())
            }
            ("attack", ParamValue::Float(a)) => {
                self.set_attack(a);
                Ok(())
            }
            ("release", ParamValue::Float(r)) => {
                self.set_release(r);
                Ok(())
            }
            ("output_gain", ParamValue::Float(g)) => {
                self.output_gain = g.clamp(0.0, 2.0);
                Ok(())
            }
            ("lookahead", ParamValue::Float(l)) => {
                self.set_lookahead(l);
                Ok(())
            }
            _ => Err(ProcessError::parameter("unknown parameter")),
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.attack_coeff = (-1.0 / (self.attack * sample_rate)).exp();
        self.release_coeff = (-1.0 / (self.release * sample_rate)).exp();

        self.lookahead_samples = (self.lookahead * sample_rate) as usize;
        self.analysis_buffer = DelayLine::new(sample_rate);
        self.current_gain = 1.0;
        self.position = 0;
        self.init_buffer.clear();
        self.initializing = true;
        self.warming_up = false;

        self.delay.init(sample_rate);
        self.delay.set_delay_time(self.lookahead);
    }

    fn reset(&mut self) {
        Limiter::reset(self);
    }
}

// Implement NodeMetadata for compatibility
impl<const BUF_SIZE: usize> Limiter<BUF_SIZE> {
    /// Returns metadata about this node
    pub fn metadata() -> NodeMetadata {
        NodeMetadata {
            name: "Limiter".to_string(),
            category: NodeCategory::Processor,
            description: "Lookahead limiter using Delay".to_string(),
            author: "Kama Digital Effects".to_string(),
            version: "0.2.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "threshold".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.0),
                    range: ParamRange::new()
                        .with_min(-60.0)
                        .with_max(0.0)
                        .with_step(1.0),
                    unit: Some("dB".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "attack".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.005),
                    range: ParamRange::new()
                        .with_min(0.001)
                        .with_max(0.1)
                        .with_step(0.001),
                    unit: Some("s".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "release".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.1),
                    range: ParamRange::new()
                        .with_min(0.01)
                        .with_max(1.0)
                        .with_step(0.01),
                    unit: Some("s".to_string()),
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
                ParamMetadata {
                    name: "lookahead".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.005),
                    range: ParamRange::new()
                        .with_min(0.0)
                        .with_max(0.01)
                        .with_step(0.0001),
                    unit: Some("s".to_string()),
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
