//! # Input — generic signal input source node (push model)
//!
//! Registered as `"rill/input"` with `NodeVariant::Source`.

use std::sync::Arc;

use rill_core::{
    io::IoBackend,
    math::Transcendental,
    time::ClockTick,
    traits::{IoNode, Node, NodeCategory, NodeMetadata, NodeState, Source},
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

        Self {
            id: NodeId(0),
            metadata,
            outputs,
            state: NodeState::new(44100.0),
        }
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
    fn resolve_backend(&mut self, _backend: Box<dyn IoBackend>) {
        // Backend is no longer stored in the node — I/O flows through
        // ClockTick::view which is provided by the external backend.
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE> for Input<T, BUF_SIZE> {
    fn generate(
        &mut self,
        _ctx: &RenderContext,
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
        tick: &ClockTick,
    ) -> ProcessResult<()> {
        for (ch, port) in self.outputs.iter_mut().enumerate() {
            let buf = port.buffer_mut();
            #[allow(unsafe_code)]
            unsafe {
                let buf_f32: &mut [f32] =
                    std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut f32, buf.len());
                tick.view.read_input(ch, buf_f32);
            }
        }
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

        fn run(&self, _running: Arc<std::sync::atomic::AtomicBool>) -> IoResult<()> {
            Ok(())
        }
        fn stop(&self) -> IoResult<()> {
            Ok(())
        }
    }

    fn null_tick(sample_pos: u64, samples_since_last: u32, sample_rate: f32) -> ClockTick {
        ClockTick::new(
            sample_pos,
            samples_since_last,
            sample_rate,
            "test".to_string(),
            Arc::new(NullBufferView::new(2, 2)),
        )
    }

    #[test]
    fn test_audio_input_creation() {
        let inp = Input::<f32, 64>::new();
        assert_eq!(inp.metadata().signal_outputs, 2);
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
        let tick = null_tick(0, 64, 48000.0);
        assert!(inp.generate(&ctx, &[], &[], &tick).is_ok());
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
        // resolve_backend is now a no-op — I/O flows through tick.view
        IoNode::<f32, BUF_SZ>::resolve_backend(&mut input, backend);

        let ctx = RenderContext::new(0, BUF_SZ as u32, 48000.0);
        let tick = null_tick(0, BUF_SZ as u32, 48000.0);
        assert!(input.generate(&ctx, &[], &[], &tick).is_ok());
    }
}
