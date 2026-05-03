//! # AudioOutput — Stereo Sink Node
//!
//! Registered as `"rill/output"` with `NodeVariant::Sink`.

use rill_core::{
    math::Transcendental,
    traits::{NodeCategory, NodeMetadata, NodeState, SignalNode, Sink},
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessResult,
};

use crate::audio_io::AudioIoPtr;

/// Stereo audio output sink. Writes to backend's output buffer in `consume()`.
pub struct AudioOutput<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    backend: AudioIoPtr,
}

impl<T: Transcendental, const BUF_SIZE: usize> AudioOutput<T, BUF_SIZE> {
    pub fn new() -> Self {
        let mut metadata = NodeMetadata::new("AudioOutput", NodeCategory::Sink);
        metadata.signal_inputs = 2;
        metadata.signal_outputs = 0;

        let mut inputs = Vec::new();
        inputs.push(Port::input(NodeId(0), 0, "left"));
        inputs.push(Port::input(NodeId(0), 1, "right"));

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            state: NodeState::new(44100.0),
            backend: AudioIoPtr::null(),
        }
    }

    pub fn set_backend(&mut self, backend: AudioIoPtr) {
        self.backend = backend;
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE>
    for AudioOutput<T, BUF_SIZE>
{
    fn node_type_id(&self) -> rill_core::NodeTypeId
    where Self: 'static + Sized { rill_core::NodeTypeId::of::<Self>() }

    fn id(&self) -> NodeId { self.id }
    fn set_id(&mut self, id: NodeId) { self.id = id; }
    fn metadata(&self) -> NodeMetadata { self.metadata.clone() }
    fn init(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) { self.state.sample_pos = 0; self.state.blocks_processed = 0; }

    fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> { None }
    fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
        Err(rill_core::ProcessError::parameter("AudioOutput has no parameters"))
    }

    fn input_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> { self.inputs.get(index) }
    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> { self.inputs.get_mut(index) }
    fn output_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
    fn output_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
    fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
    fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
    fn num_signal_inputs(&self) -> usize { 2 }
    fn num_signal_outputs(&self) -> usize { 0 }
    fn state(&self) -> &NodeState<T, BUF_SIZE> { &self.state }
    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> { &mut self.state }
}

impl<T: Transcendental, const BUF_SIZE: usize> Sink<T, BUF_SIZE>
    for AudioOutput<T, BUF_SIZE>
{
    fn consume(
        &mut self,
        _clock: &ClockTick,
        signal_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        if let Some(backend) = self.backend.as_ref() {
            if let (Some(l_buf), Some(r_buf)) = (signal_inputs.first(), signal_inputs.get(1)) {
                let mut out_l = [0.0f32; BUF_SIZE];
                let mut out_r = [0.0f32; BUF_SIZE];
                for i in 0..BUF_SIZE {
                    out_l[i] = l_buf[i].to_f32();
                    out_r[i] = r_buf[i].to_f32();
                }
                backend.write_output(&out_l, &out_r);
            }
        }
        self.state.advance();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::traits::SignalNode;

    #[test]
    fn test_audio_output_creation() {
        let out = AudioOutput::<f32, 64>::new();
        assert_eq!(out.metadata().signal_inputs, 2);
        assert_eq!(out.metadata().signal_outputs, 0);
        assert!(out.input_port(0).is_some());
        assert!(out.input_port(1).is_some());
    }

    #[test]
    fn test_audio_output_consume() {
        let mut out = AudioOutput::<f32, 64>::new();
        let clock = ClockTick::new(0, 64, 48000.0);
        let signal_inputs: &[&[f32; 64]] = &[];
        assert!(out.consume(&clock, signal_inputs, &[], &[], &[]).is_ok());
    }
}
