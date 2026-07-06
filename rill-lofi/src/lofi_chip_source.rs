//! LofiChipSource — Source node wrapping an audio chip emulator + lofi processing.
//!
//! Follows the `SineOsc` pattern: owns a DSP engine (`Algorithm<f32>`),
//! generates audio via `process()`, applies lofi post-processing.

use std::marker::PhantomData;

use rill_core::{
    time::{ClockTick, RenderContext},
    traits::{
        algorithm::Algorithm, parameter_write::ParameterWrite, Node, NodeCategory, NodeId,
        NodeMetadata, NodeState, ParamValue, ParameterId, Port, ProcessResult, Source,
    },
};

use crate::chip_emulator::ChipEmulator;
use crate::config::LofiConfig;
use crate::lofi_processor::LofiProcessor;

/// Source node wrapping a chip emulator with lofi processing.
///
/// `C` implements both `Algorithm<f32>` (audio generation) and
/// `ChipEmulator` (register writes).  `LofiProcessor` applies
/// bitcrushing, noise, and DAC coloring after the chip output.
pub struct LofiChipSource<C: Algorithm<f32> + ChipEmulator + ParameterWrite, const BUF_SIZE: usize>
{
    id: NodeId,
    metadata: NodeMetadata,
    chip: C,
    lofi: LofiProcessor<BUF_SIZE>,
    outputs: Vec<Port<f32, BUF_SIZE>>,
    state: NodeState<f32, BUF_SIZE>,
    _phantom: PhantomData<[f32; BUF_SIZE]>,
}

impl<C: Algorithm<f32> + ChipEmulator + ParameterWrite, const BUF_SIZE: usize>
    LofiChipSource<C, BUF_SIZE>
{
    /// Create a new chip source with the given emulator and lofi configuration.
    pub fn new(chip: C, lofi_config: LofiConfig, num_channels: usize) -> Self {
        let mut metadata = NodeMetadata::new("LofiChip", NodeCategory::Source);
        metadata.signal_inputs = 0;
        metadata.signal_outputs = num_channels;
        let outputs = (0..num_channels)
            .map(|i| {
                Port::output(
                    NodeId(0),
                    i as u16,
                    &if num_channels == 1 {
                        "out".into()
                    } else {
                        format!("ch_{i}")
                    },
                )
            })
            .collect();
        Self {
            id: NodeId(0),
            metadata,
            chip,
            lofi: LofiProcessor::new(lofi_config),
            outputs,
            state: NodeState::new(44100.0),
            _phantom: PhantomData,
        }
    }
}

impl<C: Algorithm<f32> + ChipEmulator + ParameterWrite, const BUF_SIZE: usize> Node<f32, BUF_SIZE>
    for LofiChipSource<C, BUF_SIZE>
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
        self.chip.init(sample_rate);
        self.lofi.init(sample_rate);
        self.state.sample_rate = sample_rate;
    }

    fn reset(&mut self) {
        self.chip.reset();
        self.lofi.reset();
        self.state.reset();
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        self.lofi.get_parameter(id)
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        // Try chip-specific parameters first (via ParameterWrite)
        if self
            .chip
            .write_parameter(id.as_str(), value.clone())
            .is_ok()
        {
            return Ok(());
        }
        // Delegate lofi parameters (bit_depth, dry_wet, etc.) to LofiProcessor
        Node::<f32, BUF_SIZE>::set_parameter(&mut self.lofi, id, value)
    }

    fn input_port(&self, _index: usize) -> Option<&Port<f32, BUF_SIZE>> {
        None
    }

    fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        None
    }

    fn output_port(&self, index: usize) -> Option<&Port<f32, BUF_SIZE>> {
        self.outputs.get(index)
    }

    fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        self.outputs.get_mut(index)
    }

    fn control_port(&self, _index: usize) -> Option<&Port<f32, BUF_SIZE>> {
        None
    }

    fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        None
    }

    fn num_signal_inputs(&self) -> usize {
        0
    }

    fn num_signal_outputs(&self) -> usize {
        self.outputs.len()
    }

    fn state(&self) -> &NodeState<f32, BUF_SIZE> {
        &self.state
    }

    fn state_mut(&mut self) -> &mut NodeState<f32, BUF_SIZE> {
        &mut self.state
    }
}

impl<C: Algorithm<f32> + ChipEmulator + ParameterWrite, const BUF_SIZE: usize> Source<f32, BUF_SIZE>
    for LofiChipSource<C, BUF_SIZE>
{
    fn generate(
        &mut self,
        _ctx: &RenderContext,
        _control_inputs: &[f32],
        _clock_inputs: &[RenderContext],
        _tick: &ClockTick,
    ) -> ProcessResult<()> {
        // Generate raw chip audio into a temp buffer
        let mut raw = [0.0f32; BUF_SIZE];
        self.chip.process(None, &mut raw)?;

        // Apply lofi processing and write to output ports
        let out0 = self.outputs[0].write();
        for (j, s) in out0.iter_mut().enumerate() {
            *s = self.lofi.process_sample(raw[j]);
        }
        // Copy channel 0 to additional output channels
        let out0_copy = *self.outputs[0].read();
        for port in self.outputs.iter_mut().skip(1) {
            port.write().copy_from_slice(&out0_copy);
        }

        self.state.advance();
        Ok(())
    }
}
