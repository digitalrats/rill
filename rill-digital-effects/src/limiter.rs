//! Limiter with lookahead using Delay + envelope detection

use crate::delay::Delay;
use rill_core::{
    buffer::DelayLine,
    math::Transcendental,
    traits::{Node, NodeCategory, NodeMetadata, NodeState, Processor},
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessError, ProcessResult,
};

/// Maximum lookahead time in seconds (10 ms)
const MAX_LOOKAHEAD_TIME: f32 = 0.01;
/// Maximum sample rate we support (192 kHz)
const MAX_SAMPLE_RATE: f32 = 192_000.0;
/// Maximum lookahead samples at max sample rate
const MAX_LOOKAHEAD_SAMPLES: usize = (MAX_LOOKAHEAD_TIME * MAX_SAMPLE_RATE) as usize;
/// Size of analysis buffer (double the max lookahead)
const ANALYSIS_BUF_SIZE: usize = MAX_LOOKAHEAD_SAMPLES * 2;

/// Limiter with lookahead using Delay + envelope detection
pub struct Limiter<T: Transcendental, const BUF_SIZE: usize> {
    /// Node identifier
    id: NodeId,
    /// Node metadata
    metadata: NodeMetadata,
    /// Input ports
    inputs: Vec<Port<T, BUF_SIZE>>,
    /// Output ports
    outputs: Vec<Port<T, BUF_SIZE>>,
    /// Control ports
    controls: Vec<Port<T, BUF_SIZE>>,
    /// Node state
    state: NodeState<T, BUF_SIZE>,
    /// Delay line for lookahead
    delay: Delay<T, BUF_SIZE>,
    /// Buffer for envelope detection
    analysis_buffer: DelayLine<T, ANALYSIS_BUF_SIZE>,
    /// Threshold in dB
    threshold_db: f32,
    /// Threshold in linear scale
    threshold_linear: T,
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
    init_buffer: Vec<T>,
    /// Whether we're in initialization phase
    initializing: bool,
    /// Whether we're in warmup phase after initialization
    warming_up: bool,
}

impl<T: Transcendental, const BUF_SIZE: usize> Limiter<T, BUF_SIZE> {
    /// Create a new limiter
    pub fn new(
        sample_rate: f32,
        threshold_db: f32,
        attack: f32,
        release: f32,
        output_gain: f32,
    ) -> Self {
        let threshold_db = threshold_db.clamp(-60.0, 0.0);
        let threshold_linear = T::from_f32(10.0_f32.powf(threshold_db / 20.0));

        let attack = attack.clamp(0.001, 0.1);
        let release = release.clamp(0.01, 1.0);

        let attack_coeff = (-1.0 / (attack * sample_rate)).exp();
        let release_coeff = (-1.0 / (release * sample_rate)).exp();

        let lookahead = 0.005; // 5ms default
        let lookahead_samples = (lookahead * sample_rate) as usize;

        // Delay with needed delay, feedback=0, mix=1.0 (100% wet)
        let delay = Delay::with_params(sample_rate, lookahead, 0.0, 1.0);

        // Buffer for analysis
        let analysis_buffer = DelayLine::new(sample_rate);

        // Buffer for temporary storage during initialization
        let init_buffer = Vec::with_capacity(lookahead_samples);

        let metadata = NodeMetadata::new("Limiter", NodeCategory::Processor);
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        inputs.push(Port::input(NodeId(0), 0, "signal_in"));
        outputs.push(Port::output(NodeId(0), 0, "signal_out"));

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            state: NodeState::new(sample_rate),
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
    pub fn process_sample(&mut self, input: T) -> T {
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

                // Debug
                // println!("Initialization complete, starting warmup...");
            }

            // During initialization output = input
            return input;
        }

        // 4. During warmup phase (first lookahead_samples after initialization)
        if self.warming_up {
            // Still use input as output while Delay fills with real data
            if self.position < self.lookahead_samples * 2 {
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
                    // println!("Warmup complete at pos {}", self.position);
                }

                return input;
            }
        }

        // 5. Analyze signal in analysis_buffer
        // Look for maximum amplitude within lookahead window
        let mut max_amp = T::ZERO;
        for offset in 0..self.lookahead_samples {
            let sample = self.analysis_buffer.read_delayed(offset);
            let abs_sample = sample.abs();
            if abs_sample > max_amp {
                max_amp = abs_sample;
            }
        }

        // 6. Compute target gain
        let target_gain = if max_amp > self.threshold_linear {
            self.threshold_linear.div(max_amp).to_f32()
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
        let output = delayed.mul(T::from_f32(self.current_gain * self.output_gain));

        // Debug for high signal
        // if input > T::ONE && self.position > self.lookahead_samples * 2 {
        //     println!("PROC: pos={}, in={:.3}, max={:.3}, target={:.3}, gain={:.3}, delay={:.3}, out={:.3}",
        //              self.position, input.to_f32(), max_amp.to_f32(), target_gain, self.current_gain, delayed.to_f32(), output.to_f32());
        // }

        output.clamp(T::from_f32(-2.0), T::from_f32(2.0))
    }

    /// Process a block of samples
    pub fn process_block(&mut self, input: &[T], output: &mut [T]) {
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
        self.threshold_linear = T::from_f32(10.0_f32.powf(self.threshold_db / 20.0));
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
                let test_val = T::from_f32(0.1);
                self.analysis_buffer.write(test_val);
                let _ = self.delay.process_sample(test_val);
            }
            self.initializing = false;
            self.warming_up = false;
            self.position = self.lookahead_samples * 2;
            // println!("Force ready completed");
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for Limiter<T, BUF_SIZE> {
    fn node_type_id(&self) -> rill_core::NodeTypeId
    where
        Self: 'static + Sized,
    {
        rill_core::NodeTypeId::of::<Self>()
    }

    fn id(&self) -> NodeId {
        self.id
    }

    fn set_id(&mut self, id: NodeId) {
        self.id = id;
    }

    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
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
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
        Limiter::reset(self);
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        let name = id.as_str();
        match name {
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
        let name = id.as_str();
        if let Some(v) = value.as_f32() {
            match name {
                "threshold" => {
                    self.set_threshold(v);
                    Ok(())
                }
                "attack" => {
                    self.set_attack(v);
                    Ok(())
                }
                "release" => {
                    self.set_release(v);
                    Ok(())
                }
                "output_gain" => {
                    self.output_gain = v.clamp(0.0, 2.0);
                    Ok(())
                }
                "lookahead" => {
                    self.set_lookahead(v);
                    Ok(())
                }
                _ => Err(ProcessError::parameter(format!(
                    "Unknown parameter: {}",
                    name
                ))),
            }
        } else {
            Err(ProcessError::parameter("Expected float value"))
        }
    }

    fn input_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.inputs.get(index)
    }

    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.inputs.get_mut(index)
    }

    fn output_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.outputs.get(index)
    }

    fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.outputs.get_mut(index)
    }

    fn control_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.controls.get(index)
    }

    fn control_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.controls.get_mut(index)
    }

    fn num_inputs(&self) -> usize {
        self.inputs.len()
    }

    fn num_outputs(&self) -> usize {
        self.outputs.len()
    }

    fn num_signal_inputs(&self) -> usize {
        self.inputs.len()
    }

    fn num_signal_outputs(&self) -> usize {
        self.outputs.len()
    }

    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }

    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE> for Limiter<T, BUF_SIZE> {
    fn process(
        &mut self,
        _clock: &ClockTick,
        _signal_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        let input_buf = *self.inputs[0].buffer.as_array();
        let mut temp = [T::ZERO; BUF_SIZE];
        for i in 0..BUF_SIZE {
            temp[i] = self.process_sample(input_buf[i]);
        }
        *self.outputs[0].buffer.as_mut_array() = temp;
        Ok(())
    }

    fn latency(&self) -> usize {
        0
    }
}
