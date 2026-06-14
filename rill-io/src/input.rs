//! # Input — generic signal input source node (push model)
//!
//! Registered as `"rill/input"` with `NodeVariant::Source`.

use std::cell::Cell;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rill_core::{
    io::IoBackend,
    math::Transcendental,
    time::ClockTick,
    traits::{ActiveNode, IoNode, Node, NodeCategory, NodeMetadata, NodeState, Source},
    NodeId, ParamValue, ParameterId, Port, ProcessResult, RenderContext,
};

/// Signal input source. Reads from a backend in `generate()`, fills output ports.
///
/// The backend is owned by this node via `Arc`.  When used as the active
/// (driver) node, [`ActiveNode::run`] sets up the process callback and
/// blocks on the audio thread.
///
/// # Ports
/// - `n` output ports (one per channel), set via [`Self::with_channels`].
pub struct Input<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    backend: Option<Box<dyn IoBackend>>,
    bufs: Vec<[T; BUF_SIZE]>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for Input<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Input<T, BUF_SIZE> {
    /// Create a new stereo input source.
    pub fn new() -> Self {
        Self::with_channels(2)
    }

    /// Create a new input source with the given number of channels.
    pub fn with_channels(num: usize) -> Self {
        let mut metadata = NodeMetadata::new("Input", NodeCategory::Source);
        metadata.signal_inputs = 0;
        metadata.signal_outputs = num;

        let name = move |i: usize| -> String {
            if num == 1 {
                "out".into()
            } else {
                format!("ch_{i}")
            }
        };
        let outputs: Vec<_> = (0..num)
            .map(|i| Port::output(NodeId(0), i as u16, &name(i)))
            .collect();
        let bufs = vec![[T::ZERO; BUF_SIZE]; num];

        Self {
            id: NodeId(0),
            metadata,
            outputs,
            state: NodeState::new(44100.0),
            backend: None,
            bufs,
        }
    }

    /// Returns `true` if a backend is attached.
    pub fn has_backend(&self) -> bool {
        self.backend.is_some()
    }

    /// Transfer backend ownership to this node.
    ///
    /// Convenience inherent method — delegates to [`IoNode::resolve_backend`].
    pub fn resolve_backend(&mut self, backend: Box<dyn IoBackend>) {
        <Self as IoNode<T, BUF_SIZE>>::resolve_backend(self, backend);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for Input<T, BUF_SIZE> {
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
            "Input has no parameters",
        ))
    }
    fn input_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
        None
    }
    fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        None
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
        0
    }
    fn num_signal_outputs(&self) -> usize {
        self.outputs.len()
    }
    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }
    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> IoNode<T, BUF_SIZE> for Input<T, BUF_SIZE> {
    fn resolve_backend(&mut self, backend: Box<dyn IoBackend>) {
        self.backend = Some(backend);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> ActiveNode<T, BUF_SIZE> for Input<T, BUF_SIZE> {
    fn run(
        &mut self,
        tick: Box<dyn FnMut(u64, f32)>,
        running: Arc<AtomicBool>,
    ) -> rill_core::io::IoResult<()> {
        let Some(ref backend) = self.backend else {
            return Err("Input: no backend".into());
        };
        let tick_ptr = Box::into_raw(Box::new(tick));
        let sample_pos = Cell::new(0u64);
        backend.set_process_callback(Box::new(move |tick: &ClockTick| {
            unsafe {
                (*tick_ptr)(sample_pos.get(), tick.sample_rate);
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

impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE> for Input<T, BUF_SIZE> {
    fn generate(
        &mut self,
        _ctx: &RenderContext,
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
    ) -> ProcessResult<()> {
        self.state.advance();
        Ok(())
    }
}

/// Backward-compatible alias.
pub type AudioInput<T, const B: usize> = Input<T, B>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_io::IoResult;
    use crate::buffer::IoRingBuffer;
    use rill_core::time::ClockTick;
    use rill_core::traits::buffer_view::{BufferView, NullBufferView};
    use std::sync::Arc;

    struct RingIo {
        input_ring: Arc<IoRingBuffer>,
        output_ring: Arc<IoRingBuffer>,
    }
    impl IoBackend for RingIo {
        fn create_view(&self) -> Arc<dyn BufferView> {
            Arc::new(NullBufferView::new(2, 2))
        }

        fn set_process_callback(&self, _cb: Box<dyn FnMut(&ClockTick)>) {}

        fn run(&self, _running: Arc<AtomicBool>) -> IoResult<()> {
            Ok(())
        }
        fn stop(&self) -> IoResult<()> {
            Ok(())
        }
    }

    #[test]
    fn test_audio_input_creation() {
        let inp = Input::<f32, 64>::new();
        assert_eq!(inp.metadata().signal_outputs, 2);
        assert!(!inp.has_backend());
    }

    #[test]
    fn test_audio_input_mono_creation() {
        let inp = Input::<f32, 64>::with_channels(1);
        assert_eq!(inp.metadata().signal_outputs, 1);
        assert!(inp.output_port(0).is_some());
        assert!(inp.output_port(1).is_none());
    }

    #[test]
    fn test_audio_input_generate_without_backend() {
        let mut inp = Input::<f32, 64>::new();
        let ctx = RenderContext::new(0, 64, 48000.0);
        assert!(inp.generate(&ctx, &[], &[]).is_ok());
    }

    #[test]
    fn test_input_resolve_backend() {
        const BUF_SZ: usize = 64;
        let input_ring = Arc::new(IoRingBuffer::new(512));
        let output_ring = Arc::new(IoRingBuffer::new(512));

        let backend = Box::new(RingIo {
            input_ring,
            output_ring,
        });
        let mut input = Input::<f32, BUF_SZ>::new();
        input.resolve_backend(backend);

        assert!(input.has_backend());
        let ctx = RenderContext::new(0, BUF_SZ as u32, 48000.0);
        assert!(input.generate(&ctx, &[], &[]).is_ok());
    }
}
