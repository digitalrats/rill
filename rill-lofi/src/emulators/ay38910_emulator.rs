use crate::config::LofiConfig;
use crate::lofi_processor::LofiProcessor;
use rill_core::prelude::*;

use super::ay38910_chip::Ay38910Chip;

#[deprecated = "Use Ay38910Backend + LofiInput instead"]
pub struct Ay38910Emulator<const BUF_SIZE: usize> {
    chip: Ay38910Chip,
    state: NodeState<f32, BUF_SIZE>,
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<f32, BUF_SIZE>>,
    lofi: LofiProcessor<BUF_SIZE>,
}

#[allow(deprecated)]
impl<const BUF_SIZE: usize> Ay38910Emulator<BUF_SIZE> {
    pub fn new(sample_rate: f32) -> Self {
        let chip = Ay38910Chip::new(1_750_000.0);
        let lofi_config = LofiConfig::for_system(crate::config::ClassicSystem::Custom {
            bit_depth: 8,
            sample_rate: 44100.0,
            nonlinear: false,
            noise_floor: -48.0,
        });
        let id = NodeId(0);
        let outputs = vec![Port::output(id, 0, "signal_out")];
        Self {
            chip,
            state: NodeState::new(sample_rate),
            id,
            metadata: NodeMetadata {
                name: "AY-3-8910".to_string(),
                type_name: None,
                category: NodeCategory::Source,
                description: "AY-3-8910 sound chip emulation [deprecated]".to_string(),
                author: "Rill Lo-Fi".to_string(),
                version: "1.0".to_string(),
                signal_inputs: 0,
                signal_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![ParamMetadata::new(
                    "chip_clock",
                    ParamType::Float,
                    ParamValue::Float(1_750_000.0),
                )
                .with_description("Chip master clock frequency")
                .with_range(1_000_000.0, 4_000_000.0, 100_000.0)
                .with_unit("Hz")],
            },
            outputs,
            lofi: LofiProcessor::new(lofi_config),
        }
    }

    pub fn write_register(&mut self, reg: usize, value: u8) {
        self.chip.write_register(reg, value);
    }

    pub fn read_register(&self, reg: usize) -> u8 {
        self.chip.read_register(reg)
    }

    fn generate_sample(&mut self) -> f32 {
        let raw = self.chip.generate_sample(self.state.sample_rate);
        self.lofi.process_sample(raw)
    }
}

#[allow(deprecated)]
impl<const BUF_SIZE: usize> Node<f32, BUF_SIZE> for Ay38910Emulator<BUF_SIZE> {
    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    fn init(&mut self, sr: f32) {
        self.state = NodeState::new(sr);
        self.lofi.init(sr);
    }
    fn reset(&mut self) {
        self.state.reset();
        self.chip.reset();
        self.lofi.reset();
    }
    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "chip_clock" => Some(ParamValue::Float(self.chip.chip_clock)),
            _ => None,
        }
    }
    fn set_parameter(&mut self, id: &ParameterId, v: ParamValue) -> ProcessResult<()> {
        match id.as_str() {
            "chip_clock" => {
                if let ParamValue::Float(f) = v {
                    self.chip.chip_clock = f.clamp(1_000_000.0, 4_000_000.0);
                    Ok(())
                } else {
                    Err(ProcessError::parameter("chip_clock must be a float"))
                }
            }
            _ => Err(ProcessError::parameter(format!("Unknown: {id}"))),
        }
    }
    fn id(&self) -> NodeId {
        self.id
    }
    fn set_id(&mut self, id: NodeId) {
        self.id = id;
    }
    fn input_port(&self, _: usize) -> Option<&Port<f32, BUF_SIZE>> {
        None
    }
    fn input_port_mut(&mut self, _: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        None
    }
    fn output_port(&self, i: usize) -> Option<&Port<f32, BUF_SIZE>> {
        self.outputs.get(i)
    }
    fn output_port_mut(&mut self, i: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        self.outputs.get_mut(i)
    }
    fn control_port(&self, _: usize) -> Option<&Port<f32, BUF_SIZE>> {
        None
    }
    fn control_port_mut(&mut self, _: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        None
    }
    fn state(&self) -> &NodeState<f32, BUF_SIZE> {
        &self.state
    }
    fn state_mut(&mut self) -> &mut NodeState<f32, BUF_SIZE> {
        &mut self.state
    }
    fn num_signal_inputs(&self) -> usize {
        0
    }
    fn num_signal_outputs(&self) -> usize {
        1
    }
}

#[allow(deprecated)]
impl<const BUF_SIZE: usize> Source<f32, BUF_SIZE> for Ay38910Emulator<BUF_SIZE> {
    fn generate(
        &mut self,
        _clock: &ClockTick,
        _ctrl: &[f32],
        _clk: &[ClockTick],
    ) -> ProcessResult<()> {
        for i in 0..BUF_SIZE {
            self.outputs[0].buffer.as_mut_array()[i] = self.generate_sample();
        }
        Ok(())
    }
}
