//! Distortion effect with waveshaping

use rill_core::{
    math::Transcendental,
    traits::{SignalNode, NodeCategory, NodeMetadata, NodeState, Processor},
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessError, ProcessResult,
};

/// Distortion type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistortionType {
    /// Hard clipping
    HardClip,
    /// Soft clipping (tanh)
    SoftClip,
    /// Tube-like saturation
    Tube,
    /// Fuzz (asymmetric)
    Fuzz,
}

impl DistortionType {
    /// Get all available types as strings
    pub fn names() -> Vec<&'static str> {
        vec!["hard_clip", "soft_clip", "tube", "fuzz"]
    }

    /// Get type from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "hard_clip" => Some(DistortionType::HardClip),
            "soft_clip" => Some(DistortionType::SoftClip),
            "tube" => Some(DistortionType::Tube),
            "fuzz" => Some(DistortionType::Fuzz),
            _ => None,
        }
    }

    /// Convert to string
    pub fn as_str(&self) -> &'static str {
        match self {
            DistortionType::HardClip => "hard_clip",
            DistortionType::SoftClip => "soft_clip",
            DistortionType::Tube => "tube",
            DistortionType::Fuzz => "fuzz",
        }
    }
}

/// Distortion effect
///
/// Parameters:
/// - drive: input gain (1.0 - 100.0)
/// - type: distortion type
/// - output_gain: output level (0.0 - 2.0)
pub struct Distortion<T: Transcendental, const BUF_SIZE: usize> {
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
    /// Distortion type
    pub distortion_type: DistortionType,
    /// Drive (input gain)
    pub drive: f32,
    /// Output gain
    pub output_gain: f32,
    /// Sample rate (unused but required for Processor)
    sample_rate: f32,
}

impl<T: Transcendental, const BUF_SIZE: usize> Distortion<T, BUF_SIZE> {
    /// Create a new distortion effect with default parameters
    pub fn new(sample_rate: f32) -> Self {
        let metadata = NodeMetadata::new("Distortion", NodeCategory::Processor);

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();

        // Create one audio input and one audio output
        inputs.push(Port::input(NodeId(0), 0, "signal_in"));
        outputs.push(Port::output(NodeId(0), 0, "signal_out"));

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            state: NodeState::new(sample_rate),
            distortion_type: DistortionType::SoftClip,
            drive: 1.0,
            output_gain: 1.0,
            sample_rate,
        }
    }

    /// Create a new distortion effect with custom parameters
    pub fn with_params(
        sample_rate: f32,
        distortion_type: DistortionType,
        drive: f32,
        output_gain: f32,
    ) -> Self {
        let mut instance = Self::new(sample_rate);
        instance.set_type(distortion_type);
        instance.set_drive(drive);
        instance.set_output_gain(output_gain);
        instance
    }

    /// Set distortion type
    pub fn set_type(&mut self, distortion_type: DistortionType) {
        self.distortion_type = distortion_type;
    }

    /// Set drive
    pub fn set_drive(&mut self, drive: f32) {
        self.drive = drive.max(1.0).min(100.0);
    }

    /// Set output gain
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain = gain.clamp(0.0, 2.0);
    }

    /// Process a single sample
    pub fn process_sample(&self, input: T) -> T {
        let driven = input.mul(T::from_f32(self.drive));

        let distorted = match self.distortion_type {
            DistortionType::HardClip => driven.clamp(T::MIN, T::MAX),
            DistortionType::SoftClip => T::from_f32(driven.to_f32().tanh()),
            DistortionType::Tube => {
                // Tube-like saturation
                if driven > T::ZERO {
                    T::ONE - (-driven).exp()
                } else {
                    -T::ONE + driven.exp()
                }
            }
            DistortionType::Fuzz => {
                // Asymmetric fuzz
                if driven > T::ZERO {
                    T::ONE - T::ONE.div(T::ONE + driven)
                } else {
                    driven
                }
            }
        };

        distorted.mul(T::from_f32(self.output_gain))
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE> for Distortion<T, BUF_SIZE> {
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
    }

    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        let name = id.as_str();
        match name {
            "type" => Some(ParamValue::Choice(
                self.distortion_type.as_str().to_string(),
            )),
            "drive" => Some(ParamValue::Float(self.drive)),
            "output_gain" => Some(ParamValue::Float(self.output_gain)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        match name {
            "type" => {
                if let ParamValue::Choice(t) = value {
                    if let Some(dt) = DistortionType::from_str(&t) {
                        self.set_type(dt);
                        Ok(())
                    } else {
                        Err(ProcessError::parameter("unknown distortion type"))
                    }
                } else {
                    Err(ProcessError::parameter("expected Choice value"))
                }
            }
            "drive" => {
                if let Some(v) = value.as_f32() {
                    self.set_drive(v);
                    Ok(())
                } else {
                    Err(ProcessError::parameter("expected float value"))
                }
            }
            "output_gain" => {
                if let Some(v) = value.as_f32() {
                    self.set_output_gain(v);
                    Ok(())
                } else {
                    Err(ProcessError::parameter("expected float value"))
                }
            }
            _ => Err(ProcessError::parameter("unknown parameter")),
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

impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE> for Distortion<T, BUF_SIZE> {
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
