//! # AudioInput — Stereo Source Node (push model)
//!
//! Registered as `"rill/input"` with `NodeVariant::Source`.

use rill_core::{
    math::{Scalar, Transcendental},
    traits::{NodeCategory, NodeMetadata, NodeState, SignalNode, Source},
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessResult,
};

use crate::audio_io::{AudioIo, AudioIoPtr};

/// Stereo audio input source.
pub struct AudioInput<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    backend: AudioIoPtr,
    buf_l: [f32; BUF_SIZE],
    buf_r: [f32; BUF_SIZE],
}

impl<T: Transcendental, const BUF_SIZE: usize> AudioInput<T, BUF_SIZE> {
    pub fn new() -> Self {
        let mut metadata = NodeMetadata::new("AudioInput", NodeCategory::Source);
        metadata.signal_inputs = 0;
        metadata.signal_outputs = 2;

        let mut outputs = Vec::new();
        outputs.push(Port::output(NodeId(0), 0, "left"));
        outputs.push(Port::output(NodeId(0), 1, "right"));

        Self {
            id: NodeId(0),
            metadata,
            outputs,
            state: NodeState::new(44100.0),
            backend: AudioIoPtr::null(),
            buf_l: [0.0; BUF_SIZE],
            buf_r: [0.0; BUF_SIZE],
        }
    }

    pub fn set_backend(&mut self, ptr: AudioIoPtr) {
        self.backend = ptr;
    }

    /// Start the reactive stream: register the process callback
    /// and begin. The caller provides the closure that drives the
    /// engine (typically `|| engine.process_block(&tick)`).
    pub fn start(&mut self, process_cb: Box<dyn Fn()>) {
        if let Some(b) = self.backend.as_ref() {
            b.set_process_callback(process_cb);
            let _ = b.start();
        }
    }

    pub fn stop(&mut self) {
        if let Some(b) = self.backend.as_ref() {
            let _ = b.stop();
        }
    }

    pub fn has_backend(&self) -> bool { !self.backend.is_null() }
}

impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE>
    for AudioInput<T, BUF_SIZE>
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
        Err(rill_core::ProcessError::parameter("AudioInput has no parameters"))
    }
    fn input_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
    fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
    fn output_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> { self.outputs.get(index) }
    fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> { self.outputs.get_mut(index) }
    fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
    fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
    fn num_signal_inputs(&self) -> usize { 0 }
    fn num_signal_outputs(&self) -> usize { 2 }
    fn state(&self) -> &NodeState<T, BUF_SIZE> { &self.state }
    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> { &mut self.state }
}

impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE>
    for AudioInput<T, BUF_SIZE>
{
    fn generate(
        &mut self,
        _clock: &ClockTick,
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
    ) -> ProcessResult<()> {
        if let Some(backend) = self.backend.as_ref() {
            let n = backend.read_input(&mut self.buf_l, &mut self.buf_r);
            if n > 0 {
                let frames = n.min(BUF_SIZE);
                if let Some(left) = self.outputs.get_mut(0) {
                    let l = left.buffer_mut().as_mut_array();
                    for i in 0..frames { l[i] = T::from_f32(self.buf_l[i]); }
                }
                if let Some(right) = self.outputs.get_mut(1) {
                    let r = right.buffer_mut().as_mut_array();
                    for i in 0..frames { r[i] = T::from_f32(self.buf_r[i]); }
                }
            }
        }
        self.state.advance();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_input_creation() {
        let inp = AudioInput::<f32, 64>::new();
        assert_eq!(inp.metadata().signal_outputs, 2);
        assert!(!inp.has_backend());
    }

    #[test]
    fn test_audio_input_generate_without_backend() {
        let mut inp = AudioInput::<f32, 64>::new();
        let clock = ClockTick::new(0, 64, 48000.0);
        assert!(inp.generate(&clock, &[], &[]).is_ok());
    }
}
