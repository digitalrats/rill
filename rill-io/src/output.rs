//! # AudioOutput — Stereo Sink Node
//!
//! Registered as `"rill/output"` with `NodeVariant::Sink`.

use std::cell::Cell;

use rill_core::{
    math::Transcendental,
    traits::{
        active::{ActiveNode, GraphHandle},
        algorithm::ActionContext,
        node::SignalNode,
        processable::{NodeVariant, ProcessContext, Processable},
        NodeCategory, NodeMetadata, NodeState, Sink,
    },
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessResult,
};

use crate::signal_io::IoBackendPtr;

/// Stereo audio output sink. Writes to backend's output buffer in `consume()`.
///
/// In pull model (active Sink), [`set_active`](AudioOutput::set_active) must be
/// set to the graph index of the Source node that drives the processing.
/// Then [`start`](Self::start) drives the graph from that Source.
pub struct AudioOutput<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    backend: IoBackendPtr<T>,
    /// Pull‑model mode: when `true`, [`start()`] drives the graph by calling
    /// `process_block` on the node at `source_idx` (the passive Source).
    active: bool,
    source_idx: usize,
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for AudioOutput<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> AudioOutput<T, BUF_SIZE> {
    /// Create a new `AudioOutput` with no backend attached.
    pub fn new() -> Self {
        let mut metadata = NodeMetadata::new("AudioOutput", NodeCategory::Sink);
        metadata.signal_inputs = 2;
        metadata.signal_outputs = 0;

        let inputs = vec![
            Port::input(NodeId(0), 0, "left"),
            Port::input(NodeId(0), 1, "right"),
        ];

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            state: NodeState::new(44100.0),
            backend: IoBackendPtr::<T>::null(),
            active: false,
            source_idx: 0,
        }
    }

    /// Attach a borrowed backend pointer.
    pub fn set_backend(&mut self, backend: IoBackendPtr<T>) {
        self.backend = backend;
    }

    /// Activate pull model and set the source node that drives the graph.
    ///
    /// When active, [`start()`](Self::start) drives the DAG by calling
    /// `process_block` on `source_idx`, then propagating from its output
    /// ports — the same pattern as push model (`AudioInput::start()`).
    ///
    /// `source_idx` is the index of the passive Source node in the graph's
    /// node array (typically `topo_order[0]`).
    pub fn set_active(&mut self, source_idx: usize) {
        self.active = true;
        self.source_idx = source_idx;
    }

    /// Start the reactive stream (pull model — active sink drives processing).
    ///
    /// Only drives the graph when [`set_active`](Self::set_active) was called
    /// (pull model).  Otherwise behaves as a passive sink — `consume()` is
    /// reached via `Port::propagate` from an upstream source.
    ///
    /// Callback sequence when active:
    /// 1. `drain_fn()` — drain `MpscQueue<SetParameter>` into graph nodes
    /// 2. `process_block()` on the source (`source_idx` in `nodes_ptr`)
    /// 3. `Port::propagate()` — recursive DAG traversal ending at `consume()`
    ///
    /// `nodes_ptr` must point to the graph's node array (obtained from
    /// `graph.into_parts().0.into_boxed_slice()`). Valid until `stop()`.
    #[allow(clippy::not_unsafe_ptr_arg_deref, clippy::type_complexity)]
    pub fn start(
        &mut self,
        nodes_ptr: *mut [NodeVariant<f32, BUF_SIZE>],
        drain_fn: Box<dyn Fn(&mut [NodeVariant<f32, BUF_SIZE>]) + Send>,
        sample_rate: f32,
    ) {
        if self.active {
            let idx = self.source_idx;
            if let Some(backend) = self.backend.as_ref() {
                let sample_pos = Cell::new(0u64);

                backend.set_process_callback(Box::new(move || {
                    #[allow(unsafe_code)]
                    unsafe {
                        let nodes = &mut *nodes_ptr;

                        // 1. Drain parameter queue
                        drain_fn(nodes);

                        // 2. Clock tick
                        let tick = ClockTick::new(sample_pos.get(), BUF_SIZE as u32, sample_rate);

                        // 3. Process source node (generate → fills output ports)
                        let mut ctx = ProcessContext { clock: &tick };
                        let _ = nodes[idx].process_block(&mut ctx);

                        // 4. Propagate from source's output ports (walks DAG)
                        let action_ctx = ActionContext::new(&tick);
                        for po in 0..nodes[idx].num_signal_outputs() {
                            if let Some(port) = nodes[idx].output_port(po) {
                                let _ = port.propagate(port.buffer(), &action_ctx);
                            }
                        }

                        sample_pos.set(sample_pos.get() + BUF_SIZE as u64);
                    }
                }));
                // Thread ownership moved to caller — backend.run(running) is called
                // on a pre-created audio thread (see rill-adrift examples).
            }
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE>
    for AudioOutput<T, BUF_SIZE>
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
            "AudioOutput has no parameters",
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
        2
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
                                let _ = nodes[nid].set_parameter(
                                    &cmd.parameter,
                                    rill_core::ParamValue::Float(cmd.value),
                                );
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

impl<T: Transcendental, const BUF_SIZE: usize> ActiveNode for AudioOutput<T, BUF_SIZE> {
    fn start(&mut self, handle: GraphHandle) {
        SignalNode::start(self, handle);
    }
    fn stop(&mut self) {
        SignalNode::stop(self);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Sink<T, BUF_SIZE> for AudioOutput<T, BUF_SIZE> {
    fn consume(
        &mut self,
        _clock: &ClockTick,
        _signal_inputs: &[&[T; BUF_SIZE]], // empty when called through propagate
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        if let Some(backend) = self.backend.as_ref() {
            if let (Some(lp), Some(rp)) = (self.inputs.first(), self.inputs.get(1)) {
                let l_buf = lp.buffer.as_array();
                let r_buf = rp.buffer.as_array();
                let out = [l_buf as &[T], r_buf as &[T]];
                backend.write(&out);
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
