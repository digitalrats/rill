use crate::config::LofiConfig;
use crate::lofi_processor::LofiProcessor;
use rill_core::prelude::*;

use super::nes_chip::NesChip;

#[deprecated = "Use NesBackend + LofiInput instead"]
pub struct NesEmulator<const BUF_SIZE: usize> {
    chip: NesChip,
    state: NodeState<f32, BUF_SIZE>,
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<f32, BUF_SIZE>>,
    lofi: LofiProcessor<BUF_SIZE>,
    regs: [u8; 22],
}

#[allow(deprecated)]
impl<const BUF_SIZE: usize> NesEmulator<BUF_SIZE> {
    pub fn new(sample_rate: f32) -> Self {
        let lofi_config = LofiConfig::for_system(crate::config::ClassicSystem::Nes);
        let id = NodeId(0);
        Self {
            chip: NesChip::new(),
            state: NodeState::new(sample_rate),
            id,
            metadata: NodeMetadata {
                name: "NES Sound Chip".to_string(),
                type_name: None,
                category: NodeCategory::Source,
                description: "Nintendo Entertainment System 2A03 sound chip emulation [deprecated]"
                    .to_string(),
                author: "Rill Lo-Fi".to_string(),
                version: "1.0".to_string(),
                signal_inputs: 0,
                signal_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            },
            outputs: vec![Port::output(id, 0, "signal_out")],
            lofi: LofiProcessor::new(lofi_config),
            regs: [0; 22],
        }
    }

    pub fn write_register(&mut self, addr: usize, value: u8) {
        if addr < 22 {
            self.regs[addr] = value;
            self.chip.write_registers(&self.regs);
        }
    }
}

#[allow(deprecated)]
impl<const BUF_SIZE: usize> Node<f32, BUF_SIZE> for NesEmulator<BUF_SIZE> {
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
    fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
        None
    }
    fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
        Ok(())
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
impl<const BUF_SIZE: usize> Source<f32, BUF_SIZE> for NesEmulator<BUF_SIZE> {
    fn generate(
        &mut self,
        _clock: &ClockTick,
        _ctrl: &[f32],
        _clk: &[ClockTick],
    ) -> ProcessResult<()> {
        self.chip.write_registers(&self.regs);
        for i in 0..BUF_SIZE {
            let raw = self.chip.generate_sample(self.state.sample_rate);
            self.outputs[0].buffer.as_mut_array()[i] = self.lofi.process_sample(raw);
        }
        Ok(())
    }
}
