use crate::CassetteDeckModel;
use rill_core::prelude::*;

pub struct CassetteDeckProcessor<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    pub algorithm: CassetteDeckModel,
    pub tape_speed: f32,
    pub bias_level: f32,
    pub noise_floor: f32,
    pub wow_flutter: f32,
}

impl<T: Transcendental, const BUF_SIZE: usize> CassetteDeckProcessor<T, BUF_SIZE> {
    pub fn new(sample_rate: f32) -> Self {
        let mut metadata = NodeMetadata::new("CassetteDeck", NodeCategory::Processor);
        metadata.parameters = vec![
            ParamMetadata::new("tape_speed", ParamType::Float, ParamValue::Float(4.76))
                .with_description("Tape speed (cm/s)")
                .with_range(1.19, 19.05, 0.01),
            ParamMetadata::new("bias_level", ParamType::Float, ParamValue::Float(0.8))
                .with_description("Bias level")
                .with_range(0.0, 1.0, 0.01),
            ParamMetadata::new("noise_floor", ParamType::Float, ParamValue::Float(0.0001))
                .with_description("Noise floor")
                .with_range(0.0, 0.01, 0.00001),
            ParamMetadata::new("wow_flutter", ParamType::Float, ParamValue::Float(0.002))
                .with_description("Wow & flutter intensity")
                .with_range(0.0, 0.01, 0.0001),
        ];

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        inputs.push(Port::input(NodeId(0), 0, "audio_in"));
        outputs.push(Port::output(NodeId(0), 0, "audio_out"));

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            state: NodeState::new(sample_rate),
            algorithm: CassetteDeckModel::new(sample_rate as f64),
            tape_speed: 4.76,
            bias_level: 0.8,
            noise_floor: 0.0001,
            wow_flutter: 0.002,
        }
    }

    fn update_algorithm(&mut self) {
        self.algorithm.set_tape_speed(self.tape_speed as f64);
        self.algorithm.set_bias_level(self.bias_level as f64);
        self.algorithm.noise_floor = self.noise_floor as f64;
        self.algorithm.wow_flutter = self.wow_flutter as f64;
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE>
    for CassetteDeckProcessor<T, BUF_SIZE>
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
        self.algorithm = CassetteDeckModel::new(sample_rate as f64);
        self.update_algorithm();
    }

    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
        self.algorithm = CassetteDeckModel::new(self.state.sample_rate as f64);
        self.update_algorithm();
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        let name = id.as_str();
        match name {
            "tape_speed" => Some(ParamValue::Float(self.tape_speed)),
            "bias_level" => Some(ParamValue::Float(self.bias_level)),
            "noise_floor" => Some(ParamValue::Float(self.noise_floor)),
            "wow_flutter" => Some(ParamValue::Float(self.wow_flutter)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        if let Some(v) = value.as_f32() {
            match name {
                "tape_speed" => {
                    self.tape_speed = v.clamp(1.19, 19.05);
                    self.update_algorithm();
                    Ok(())
                }
                "bias_level" => {
                    self.bias_level = v.clamp(0.0, 1.0);
                    self.update_algorithm();
                    Ok(())
                }
                "noise_floor" => {
                    self.noise_floor = v.max(0.0);
                    self.update_algorithm();
                    Ok(())
                }
                "wow_flutter" => {
                    self.wow_flutter = v.max(0.0);
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

    fn num_audio_inputs(&self) -> usize {
        1
    }

    fn num_audio_outputs(&self) -> usize {
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
    for CassetteDeckProcessor<T, BUF_SIZE>
{
    fn process(
        &mut self,
        _clock: &ClockTick,
        _audio_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        let input_buf = *self.inputs[0].buffer.as_array();
        let output_buf = self.outputs[0].buffer.as_mut_array();
        for i in 0..BUF_SIZE {
            let x = input_buf[i].to_f64();
            let y = self.algorithm.process(x);
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
    fn test_cassette_deck_processor_creation() {
        let p = CassetteDeckProcessor::<f32, 64>::new(44100.0);
        assert!((p.tape_speed - 4.76).abs() < 1e-6);
        assert!((p.bias_level - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_cassette_deck_processor_params() {
        let mut p = CassetteDeckProcessor::<f32, 64>::new(44100.0);
        let speed_id = ParameterId::new("tape_speed").unwrap();
        p.set_parameter(&speed_id, ParamValue::Float(9.52)).unwrap();
        assert!((p.tape_speed - 9.52).abs() < 1e-6);
    }
}
