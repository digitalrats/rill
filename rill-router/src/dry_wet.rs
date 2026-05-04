use rill_core::{
    math::Transcendental,
    traits::{NodeCategory, NodeMetadata, NodeState, Processor, SignalNode},
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessError, ProcessResult,
};

/// Processor that blends a dry and a wet signal into a stereo output.
///
/// Each sample is computed as `(dry_in * dry_gain + wet_in * wet_gain) * master_gain`
/// and sent to both left and right outputs.
///
/// # Parameters
/// - `dry`    (0.0 – 1.0)
/// - `wet`    (0.0 – 1.0)
/// - `master` (0.0 – 2.0)
pub struct DryWetMix<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,

    dry: f32,
    wet: f32,
    master: f32,
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for DryWetMix<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> DryWetMix<T, BUF_SIZE> {
    /// Create a new `DryWetMix` with unity dry gain, 0.5 wet gain, and unity master.
    pub fn new() -> Self {
        let mut metadata = NodeMetadata::new("DryWetMix", NodeCategory::Processor);
        metadata.parameters = vec![
            rill_core::ParamMetadata::new(
                "dry",
                rill_core::ParamType::Float,
                ParamValue::Float(1.0),
            )
            .with_range(0.0, 1.0, 0.01),
            rill_core::ParamMetadata::new(
                "wet",
                rill_core::ParamType::Float,
                ParamValue::Float(0.5),
            )
            .with_range(0.0, 1.0, 0.01),
            rill_core::ParamMetadata::new(
                "master",
                rill_core::ParamType::Float,
                ParamValue::Float(1.0),
            )
            .with_range(0.0, 2.0, 0.01),
        ];

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        inputs.push(Port::input(NodeId(0), 0, "dry_in"));
        inputs.push(Port::input(NodeId(0), 1, "wet_in"));
        outputs.push(Port::output(NodeId(0), 0, "out_L"));
        outputs.push(Port::output(NodeId(0), 1, "out_R"));

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            state: NodeState::new(44100.0),
            dry: 1.0,
            wet: 0.5,
            master: 1.0,
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE> for DryWetMix<T, BUF_SIZE> {
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

    fn init(&mut self, _sample_rate: f32) {}

    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "dry" => Some(ParamValue::Float(self.dry)),
            "wet" => Some(ParamValue::Float(self.wet)),
            "master" => Some(ParamValue::Float(self.master)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        if let Some(v) = value.as_f32() {
            match name {
                "dry" => {
                    self.dry = v.clamp(0.0, 1.0);
                    Ok(())
                }
                "wet" => {
                    self.wet = v.clamp(0.0, 1.0);
                    Ok(())
                }
                "master" => {
                    self.master = v.clamp(0.0, 2.0);
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
    fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
        None
    }
    fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        None
    }

    fn num_signal_inputs(&self) -> usize {
        2
    }
    fn num_signal_outputs(&self) -> usize {
        2
    }

    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }
    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE> for DryWetMix<T, BUF_SIZE> {
    fn process(
        &mut self,
        _clock: &ClockTick,
        _signal_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        let dry_buf = *self.inputs[0].buffer.as_array();
        let wet_buf = *self.inputs[1].buffer.as_array();
        let (out_l, out_r) = self.outputs.split_at_mut(1);
        let (out_l, out_r) = (
            out_l[0].buffer.as_mut_array(),
            out_r[0].buffer.as_mut_array(),
        );

        let dry_gain = T::from_f32(self.dry);
        let wet_gain = T::from_f32(self.wet);
        let master_gain = T::from_f32(self.master);

        for (((&dry, &wet), out_l), out_r) in dry_buf
            .iter()
            .zip(wet_buf.iter())
            .zip(out_l.iter_mut())
            .zip(out_r.iter_mut())
        {
            let sig = dry * dry_gain + wet * wet_gain;
            *out_l = sig * master_gain;
            *out_r = sig * master_gain;
        }

        self.state.advance();
        Ok(())
    }

    fn latency(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dry_wet_mix_creation() {
        let m = DryWetMix::<f32, 64>::new();
        assert!((m.dry - 1.0).abs() < 1e-6);
        assert!((m.wet - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_dry_wet_params() {
        let mut m = DryWetMix::<f32, 64>::new();
        let id = ParameterId::new("wet").unwrap();
        m.set_parameter(&id, ParamValue::Float(0.75)).unwrap();
        assert!((m.wet - 0.75).abs() < 1e-6);
    }
}
