//! # Output — generic signal output sink node
//!
//! Registered as `"rill/output"` with `NodeVariant::Sink`.

use std::cell::Cell;

use rill_core::{
    math::Transcendental,
    traits::{
        active::{ActiveNode, GraphHandle},
        algorithm::ActionContext,
        node::Node,
        processable::{NodeVariant, ProcessContext, Processable},
        NodeCategory, NodeMetadata, NodeState, Sink,
    },
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessResult,
};

use crate::signal_io::IoBackendPtr;

/// Signal output sink. Writes to backend in `consume()`.
///
/// # Ports
/// - `n` input ports (one per channel), set via [`Self::with_channels`].
pub struct Output<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    backend: IoBackendPtr<T>,
    active: bool,
    source_idx: usize,
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for Output<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Output<T, BUF_SIZE> {
    /// Create a new stereo output sink.
    pub fn new() -> Self {
        Self::with_channels(2)
    }

    /// Create a new output sink with the given number of channels.
    pub fn with_channels(num: usize) -> Self {
        let mut metadata = NodeMetadata::new("Output", NodeCategory::Sink);
        metadata.signal_inputs = num;
        metadata.signal_outputs = 0;

        let name = move |i: usize| -> String {
            if num == 1 {
                "in".into()
            } else {
                format!("ch_{i}")
            }
        };
        let inputs: Vec<_> = (0..num)
            .map(|i| Port::input(NodeId(0), i as u16, &name(i)))
            .collect();

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            state: NodeState::new(44100.0),
            backend: IoBackendPtr::<T>::null(),
            active: true,
            source_idx: 0,
        }
    }

    /// Attach an I/O backend to this output sink.
    pub fn set_backend(&mut self, backend: IoBackendPtr<T>) {
        self.backend = backend;
    }

    /// Mark this output as active, setting its source node index.
    pub fn set_active(&mut self, source_idx: usize) {
        self.active = true;
        self.source_idx = source_idx;
    }
}

/// Backward-compatible alias.
pub type AudioOutput<T, const B: usize> = Output<T, B>;

impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for Output<T, BUF_SIZE> {
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

    fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
        None
    }
    fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
        Err(rill_core::ProcessError::parameter(
            "Output has no parameters",
        ))
    }

    fn input_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.inputs.get(index)
    }
    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.inputs.get_mut(index)
    }
    fn output_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
        None
    }
    fn output_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        None
    }
    fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
        None
    }
    fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        None
    }
    fn num_signal_inputs(&self) -> usize {
        self.inputs.len()
    }
    fn num_signal_outputs(&self) -> usize {
        0
    }
    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }
    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn resolve_backend(&mut self, backend: *mut dyn rill_core::io::IoBackend<T>) {
        if !backend.is_null() {
            self.backend = crate::signal_io::IoBackendPtr::from_ref(unsafe { &*backend });
        }
    }
    fn start(&mut self, handle: GraphHandle) {
        if self.active {
            let idx = self.source_idx;
            if let Some(backend) = self.backend.as_ref() {
                let nodes_ptr = handle.nodes as *mut NodeVariant<T, BUF_SIZE>;
                let len = handle.len;
                let queue_ptr = handle.queue;
                let sample_rate = handle.sample_rate;
                let sample_pos = Cell::new(0u64);
                backend.set_process_callback(Box::new(move || unsafe {
                    let nodes = std::slice::from_raw_parts_mut(nodes_ptr, len);
                    if let Some(q) = queue_ptr.as_ref() {
                        while let Some(cmd) = q.pop() {
                            let nid = cmd.port.node_id().inner() as usize;
                            if nid < len {
                                let _ = nodes[nid].set_parameter(&cmd.parameter, cmd.value.clone());
                            }
                        }
                    }
                    let tick = ClockTick::new(sample_pos.get(), BUF_SIZE as u32, sample_rate);
                    let mut ctx = ProcessContext { clock: &tick };
                    let _ = nodes[idx].process_block(&mut ctx);
                    let action_ctx = ActionContext::new(&tick);
                    for po in 0..nodes[idx].num_signal_outputs() {
                        if let Some(port) = nodes[idx].output_port(po) {
                            let _ = port.propagate(port.buffer(), &action_ctx);
                        }
                    }
                    sample_pos.set(sample_pos.get() + BUF_SIZE as u64);
                }));
            }
        }
    }
    fn stop(&mut self) {}
}

impl<T: Transcendental, const BUF_SIZE: usize> ActiveNode for Output<T, BUF_SIZE> {
    fn start(&mut self, handle: GraphHandle) {
        Node::start(self, handle);
    }
    fn stop(&mut self) {
        Node::stop(self);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Sink<T, BUF_SIZE> for Output<T, BUF_SIZE> {
    fn consume(
        &mut self,
        _clock: &ClockTick,
        _signal_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        if let Some(backend) = self.backend.as_ref() {
            let nch = self.inputs.len();
            if nch > 0 {
                let mut channels: Vec<&[T]> = Vec::with_capacity(nch);
                for i in 0..nch {
                    if let Some(port) = self.inputs.get(i) {
                        channels.push(port.buffer.as_array());
                    }
                }
                backend.write(&channels);
            }
        }
        self.state.advance();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::traits::Node;

    #[test]
    fn test_audio_output_creation() {
        let out = Output::<f32, 64>::new();
        assert_eq!(out.metadata().signal_inputs, 2);
        assert_eq!(out.metadata().signal_outputs, 0);
        assert!(out.input_port(0).is_some());
        assert!(out.input_port(1).is_some());
    }

    #[test]
    fn test_audio_output_mono() {
        let out = Output::<f32, 64>::with_channels(1);
        assert_eq!(out.metadata().signal_inputs, 1);
        assert!(out.input_port(0).is_some());
        assert!(out.input_port(1).is_none());
    }

    #[test]
    fn test_audio_output_consume() {
        let mut out = Output::<f32, 64>::new();
        let clock = ClockTick::new(0, 64, 48000.0);
        let signal_inputs: &[&[f32; 64]] = &[];
        assert!(out.consume(&clock, signal_inputs, &[], &[], &[]).is_ok());
    }
}
