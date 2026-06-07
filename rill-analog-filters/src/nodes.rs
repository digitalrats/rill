use rill_core::prelude::*;
use rill_core_model::wdf::MoogLadder;

/// WDF-based Moog Ladder filter graph node.
///
/// A 4-pole low-pass filter using Wave Digital Filter modeling.
pub struct WdfMoogLadderProcessor<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    /// The underlying WDF Moog Ladder algorithm.
    pub algorithm: MoogLadder<f64>,
    /// Cutoff frequency in Hz.
    pub cutoff: f32,
    /// Resonance amount (0.0–1.0).
    pub resonance: f32,
}

impl<T: Transcendental, const BUF_SIZE: usize> WdfMoogLadderProcessor<T, BUF_SIZE> {
    /// Create a new WDF Moog Ladder processor at the given sample rate.
    pub fn new(sample_rate: f32) -> Self {
        let mut metadata = NodeMetadata::new("WdfMoogLadder", NodeCategory::Processor);
        metadata.parameters = vec![
            ParamMetadata::new("cutoff", ParamType::Float, ParamValue::Float(1000.0))
                .with_description("Cutoff frequency (Hz)")
                .with_range(20.0, 20000.0, 1.0),
            ParamMetadata::new("resonance", ParamType::Float, ParamValue::Float(0.0))
                .with_description("Resonance (0-1)")
                .with_range(0.0, 1.0, 0.01),
        ];

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        inputs.push(Port::input(NodeId(0), 0, "signal_in"));
        outputs.push(Port::output(NodeId(0), 0, "signal_out"));

        let pole = rill_core_model::wdf::RcPole::new(0.0);
        let mut algorithm = MoogLadder::new(pole, 1000.0, 0.0, sample_rate as f64);
        algorithm.update_coeffs();
        algorithm.set_cutoff(1000.0);

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            state: NodeState::new(sample_rate),
            algorithm,
            cutoff: 1000.0,
            resonance: 0.0,
        }
    }

    fn update_algorithm(&mut self) {
        self.algorithm.set_cutoff(self.cutoff as f64);
        self.algorithm.set_resonance(self.resonance as f64);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE>
    for WdfMoogLadderProcessor<T, BUF_SIZE>
{
    fn node_type_id(&self) -> NodeTypeId
    where
        Self: 'static + Sized,
    {
        NodeTypeId::of::<Self>()
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
        let pole = rill_core_model::wdf::RcPole::new(0.0);
        self.algorithm = MoogLadder::new(pole, 1000.0, 0.0, sample_rate as f64);
        self.algorithm.update_coeffs();
        self.update_algorithm();
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

    fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
        None
    }

    fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        None
    }

    fn num_signal_inputs(&self) -> usize {
        1
    }

    fn num_signal_outputs(&self) -> usize {
        1
    }

    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }

    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE>
    for WdfMoogLadderProcessor<T, BUF_SIZE>
{
    fn process(
        &mut self,
        _ctx: &RenderContext,
        _signal_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        let input_buf = *self.inputs[0].buffer.as_array();
        let output_buf = self.outputs[0].buffer.as_mut_array();
        for i in 0..BUF_SIZE {
            let x = input_buf[i].to_f64();
            let y = self.algorithm.process_sample(x);
            output_buf[i] = T::from_f32(y as f32);
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
    fn test_wdf_moog_ladder_processor_creation() {
        let p = WdfMoogLadderProcessor::<f32, 64>::new(44100.0);
        assert_eq!(p.cutoff, 1000.0);
        assert_eq!(p.resonance, 0.0);
    }

    #[test]
    fn test_wdf_moog_ladder_processor_params() {
        let mut p = WdfMoogLadderProcessor::<f32, 64>::new(44100.0);
        let cutoff_id = ParameterId::new("cutoff").unwrap();
        let res_id = ParameterId::new("resonance").unwrap();

        p.set_parameter(&cutoff_id, ParamValue::Float(5000.0))
            .unwrap();
        assert!((p.cutoff - 5000.0).abs() < 1.0);

        p.set_parameter(&res_id, ParamValue::Float(0.7)).unwrap();
        assert!((p.resonance - 0.7).abs() < 1e-6);
    }
}
