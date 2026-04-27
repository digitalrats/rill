//! Delay effect with feedback

use rill_core::{
    buffer::DelayLine,
    math::Transcendental,
    traits::{AudioNode, NodeCategory, NodeMetadata, NodeState, Processor},
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessError, ProcessResult,
};

/// Maximum delay time in seconds
const MAX_DELAY_SECONDS: f32 = 0.5;
/// Maximum sample rate we support (48 kHz)
const MAX_SAMPLE_RATE: f32 = 48_000.0;
/// Maximum delay in samples (2 seconds at max sample rate)
const MAX_DELAY_SAMPLES: usize = (MAX_DELAY_SECONDS * MAX_SAMPLE_RATE) as usize;

/// Delay effect with feedback
///
/// Parameters:
/// - delay_time: delay time in seconds (0.01 - 2.0)
/// - feedback: feedback amount (0.0 - 0.99)
/// - mix: dry/wet mix (0.0 - 1.0)
pub struct Delay<T: Transcendental, const BUF_SIZE: usize> {
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
    /// Delay time in seconds
    pub delay_time: f32,
    /// Delay time in samples
    delay_samples: usize,
    /// Feedback amount (0.0 - 0.99)
    pub feedback: f32,
    /// Dry/wet mix (0.0 = dry, 1.0 = wet)
    pub mix: f32,
    /// Delay line
    delay_line: DelayLine<T, MAX_DELAY_SAMPLES>,
    /// Sample rate (cached)
    sample_rate: f32,
}

impl<T: Transcendental, const BUF_SIZE: usize> Delay<T, BUF_SIZE> {
    /// Create a new delay effect with default parameters
    pub fn new(sample_rate: f32) -> Self {
        let metadata = NodeMetadata::new("Delay", NodeCategory::Processor);

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();

        // Create one audio input and one audio output
        inputs.push(Port::input(NodeId(0), 0, "audio_in"));
        outputs.push(Port::output(NodeId(0), 0, "audio_out"));

        let delay_time = 0.5;
        let delay_samples = (delay_time * sample_rate) as usize;
        let mut delay_line = DelayLine::new(sample_rate);
        delay_line.set_delay_samples(delay_samples);

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            state: NodeState::new(sample_rate),
            delay_time,
            delay_samples,
            feedback: 0.3,
            mix: 0.5,
            delay_line,
            sample_rate,
        }
    }

    /// Create a new delay effect with custom parameters
    pub fn with_params(sample_rate: f32, delay_time: f32, feedback: f32, mix: f32) -> Self {
        let mut instance = Self::new(sample_rate);
        instance.set_delay_time(delay_time);
        instance.set_feedback(feedback);
        instance.set_mix(mix);
        instance
    }

    /// Set delay time in seconds
    pub fn set_delay_time(&mut self, time: f32) {
        self.delay_time = time.clamp(0.01, MAX_DELAY_SECONDS);
        self.update_delay_samples();
    }

    /// Set feedback amount
    pub fn set_feedback(&mut self, fb: f32) {
        self.feedback = fb.clamp(0.0, 0.99);
    }

    /// Set dry/wet mix
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    /// Update delay samples based on current sample rate
    fn update_delay_samples(&mut self) {
        self.delay_samples = (self.delay_time * self.sample_rate) as usize;
        if self.delay_samples >= MAX_DELAY_SAMPLES {
            self.delay_samples = MAX_DELAY_SAMPLES - 1;
        }
        self.delay_line.set_delay_samples(self.delay_samples);
    }

    /// Process a single sample (internal helper)
    pub fn process_sample(&mut self, input: T) -> T {
        // Read delayed sample
        let delayed = self.delay_line.read_delayed(self.delay_samples);
        // Output mix
        let dry = input;
        let wet = delayed;
        let mix = T::from_f32(self.mix);
        let one_minus_mix = T::ONE - mix;
        let output = dry.mul(one_minus_mix).add(wet.mul(mix));
        // Write input with feedback
        let feedback = T::from_f32(self.feedback);
        let write_sample = input.add(delayed.mul(feedback));
        self.delay_line.write(write_sample);
        output
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE> for Delay<T, BUF_SIZE> {
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
        // Update port IDs? For simplicity, we ignore for now.
    }

    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_delay_samples();
        self.delay_line.clear();
    }

    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
        self.delay_line.clear();
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        let name = id.as_str();
        match name {
            "delay_time" => Some(ParamValue::Float(self.delay_time)),
            "feedback" => Some(ParamValue::Float(self.feedback)),
            "mix" => Some(ParamValue::Float(self.mix)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        if let Some(v) = value.as_f32() {
            match name {
                "delay_time" => {
                    self.set_delay_time(v);
                    Ok(())
                }
                "feedback" => {
                    self.set_feedback(v);
                    Ok(())
                }
                "mix" => {
                    self.set_mix(v);
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

    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }

    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE> for Delay<T, BUF_SIZE> {
    fn process(
        &mut self,
        _clock: &ClockTick,
        _audio_inputs: &[&[T; BUF_SIZE]],
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
