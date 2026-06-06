//! # Output — generic signal output sink node
//!
//! Registered as `"rill/output"` with `NodeVariant::Sink`.

use std::cell::Cell;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rill_core::{
    io::IoBackend,
    math::Transcendental,
    traits::{ActiveNode, IoNode, Node, NodeCategory, NodeMetadata, NodeState, Sink},
    NodeId, ParamValue, ParameterId, Port, ProcessResult, RenderContext,
};

/// Signal output sink. Writes to backend in `consume()`.
///
/// When used as the active (driver) node, [`ActiveNode::run`] sets up the
/// process callback and blocks on the audio thread.
///
/// # Ports
/// - `n` input ports (one per channel), set via [`Self::with_channels`].
pub struct Output<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    backend: Option<Box<dyn IoBackend<T>>>,
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
            backend: None,
            active: true,
            source_idx: 0,
        }
    }

    /// Mark this output as active, setting its source node index.
    pub fn set_active(&mut self, source_idx: usize) {
        self.active = true;
        self.source_idx = source_idx;
    }

    /// Transfer backend ownership to this node.
    ///
    /// Convenience inherent method — delegates to [`IoNode::resolve_backend`].
    pub fn resolve_backend(&mut self, backend: Box<dyn IoBackend<T>>) {
        <Self as IoNode<T, BUF_SIZE>>::resolve_backend(self, backend);
    }
}

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

    fn as_io_node_mut(&mut self) -> Option<&mut dyn IoNode<T, BUF_SIZE>> {
        Some(self)
    }
    fn as_active_node_mut(&mut self) -> Option<&mut dyn ActiveNode<T, BUF_SIZE>> {
        Some(self)
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
}

impl<T: Transcendental, const BUF_SIZE: usize> IoNode<T, BUF_SIZE> for Output<T, BUF_SIZE> {
    fn resolve_backend(&mut self, backend: Box<dyn IoBackend<T>>) {
        self.backend = Some(backend);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> ActiveNode<T, BUF_SIZE> for Output<T, BUF_SIZE> {
    fn run(
        &mut self,
        tick: Box<dyn FnMut(u64, f32)>,
        running: Arc<AtomicBool>,
    ) -> rill_core::io::IoResult<()> {
        let Some(ref backend) = self.backend else {
            return Err("Output: no backend".into());
        };
        let tick_ptr = Box::into_raw(Box::new(tick));
        let sample_pos = Cell::new(0u64);
        backend.set_process_callback(Box::new(move |actual_sr: f32| {
            unsafe {
                (*tick_ptr)(sample_pos.get(), actual_sr);
            }
            sample_pos.set(sample_pos.get() + BUF_SIZE as u64);
        }));
        backend.run(running.clone())?;
        while running.load(std::sync::atomic::Ordering::Acquire) {
            std::thread::park();
        }
        let _ = backend.stop();
        drop(unsafe { Box::from_raw(tick_ptr) });
        Ok(())
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Sink<T, BUF_SIZE> for Output<T, BUF_SIZE> {
    fn consume(
        &mut self,
        _ctx: &RenderContext,
        _signal_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        if let Some(ref backend) = self.backend {
            let nch = self.inputs.len();
            if nch > 0 {
                let all_received = self.inputs.iter().all(|p| p.data_received);
                if all_received {
                    let mut channels: Vec<&[T]> = Vec::with_capacity(nch);
                    for i in 0..nch {
                        if let Some(port) = self.inputs.get(i) {
                            channels.push(port.buffer.as_array());
                        }
                    }
                    backend.write(&channels);
                    for p in &mut self.inputs {
                        p.data_received = false;
                    }
                    self.state.advance();
                }
            }
        }
        Ok(())
    }
}

/// Backward-compatible alias.
pub type AudioOutput<T, const B: usize> = Output<T, B>;

#[cfg(test)]
mod tests {
    use super::*;

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
        let ctx = RenderContext::new(0, 64, 48000.0);
        let signal_inputs: &[&[f32; 64]] = &[];
        assert!(out.consume(&ctx, signal_inputs, &[], &[], &[]).is_ok());
    }
}
