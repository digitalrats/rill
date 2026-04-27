use rill_core::{
    AudioNode, AudioNum, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue, ParameterId,
    Port, ProcessError, ProcessResult, Processor,
};
use rill_core::traits::{ActionContext, Algorithm};
use rill_core_dsp::filters::MoogLadder;

/// Processor wrapper for Moog ladder filter
pub struct MoogLadderProcessor<T: AudioNum, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    outputs: Vec<Port<T, BUF_SIZE>>,
    controls: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    /// Cutoff frequency (Hz)
    pub cutoff: f32,
    /// Resonance (0.0 – 1.0)
    pub resonance: f32,
    /// Inner MoogLadder algorithm
    pub algorithm: MoogLadder<T>,
}

impl<T: AudioNum, const BUF_SIZE: usize> MoogLadderProcessor<T, BUF_SIZE> {
    /// Create a new Moog ladder processor
    pub fn new(sample_rate: f32) -> Self {
        let metadata = NodeMetadata::new("MoogLadderProcessor", NodeCategory::Processor);

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();

        inputs.push(Port::input(NodeId(0), 0, "audio_in"));
        outputs.push(Port::output(NodeId(0), 0, "audio_out"));

        let mut algorithm = MoogLadder::new(1000.0, 0.0);
        algorithm.init(sample_rate);

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            state: NodeState::new(sample_rate),
            cutoff: 1000.0,
            resonance: 0.0,
            algorithm,
        }
    }

    fn update_algorithm(&mut self) {
        self.algorithm.set_cutoff(self.cutoff);
        self.algorithm.set_resonance(self.resonance);
    }
}

impl<T: AudioNum, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE>
    for MoogLadderProcessor<T, BUF_SIZE>
{
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
        self.state.sample_rate = sample_rate;
        self.algorithm.init(sample_rate);
    }

    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
        self.algorithm.reset();
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        let name = id.as_str();
        match name {
            "cutoff" => Some(ParamValue::Float(self.cutoff)),
            "resonance" => Some(ParamValue::Float(self.resonance)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        if let Some(v) = value.as_f32() {
            match name {
                "cutoff" => {
                    self.cutoff = v.clamp(20.0, 20000.0);
                    self.update_algorithm();
                    Ok(())
                }
                "resonance" => {
                    self.resonance = v.clamp(0.0, 1.0);
                    self.update_algorithm();
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

impl<T: AudioNum, const BUF_SIZE: usize> Processor<T, BUF_SIZE>
    for MoogLadderProcessor<T, BUF_SIZE>
{
    fn process(
        &mut self,
        _clock: &rill_core::ClockTick,
        _audio_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[rill_core::ClockTick],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        let input_buf = *self.inputs[0].buffer.as_array();
        let output_buf = self.outputs[0].buffer.as_mut_array();
        let ctx = ActionContext::new(_clock);
        self.algorithm
            .process(Some(&input_buf[..]), &mut output_buf[..], &ctx)?;
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
    fn test_moog_ladder_processor() {
        let processor = MoogLadderProcessor::<f32, 64>::new(44100.0);
        assert_eq!(processor.cutoff, 1000.0);
        assert_eq!(processor.resonance, 0.0);
    }
}
