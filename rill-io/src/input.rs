//! # Input — generic signal input source node (push model)
//!
//! Registered as `"rill/input"` with `NodeVariant::Source`.

use std::sync::Arc;

use rill_core::{
    io::IoCapture,
    math::Transcendental,
    time::ClockTick,
    traits::{Node, NodeCategory, NodeMetadata, NodeState, Source},
    NodeId, ParamValue, ParameterId, Port, ProcessResult, RenderContext,
};

/// Signal input source. Reads from [`IoCapture`] in `generate()`, fills output ports.
///
/// # Ports
/// - `n` output ports (one per channel), set via [`Self::with_channels`].
pub struct Input<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    capture: Arc<dyn IoCapture>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Input<T, BUF_SIZE> {
    /// Create a new input source with a capture backend.
    pub fn new(capture: Arc<dyn IoCapture>) -> Self {
        Self::with_channels(capture, 2)
    }

    /// Create a new input source with the given number of channels.
    pub fn with_channels(capture: Arc<dyn IoCapture>, num: usize) -> Self {
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
            capture,
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

impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE> for Input<T, BUF_SIZE> {
    fn generate(
        &mut self,
        _ctx: &RenderContext,
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
        _tick: &ClockTick,
    ) -> ProcessResult<()> {
        for (ch, port) in self.outputs.iter_mut().enumerate() {
            let buf = port.write();
            #[allow(unsafe_code)]
            unsafe {
                let buf_f32: &mut [f32] =
                    std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut f32, buf.len());
                self.capture.read_input(ch, buf_f32);
            }
        }
        self.state.advance();
        Ok(())
    }

    fn set_capture(&mut self, capture: Arc<dyn IoCapture>) {
        self.capture = capture;
    }
}

/// Backward-compatible alias.
pub type AudioInput<T, const B: usize> = Input<T, B>;

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::io::IoCapture;

    struct NullCapture {
        channels: usize,
    }
    impl IoCapture for NullCapture {
        fn read_input(&self, _channel: usize, dst: &mut [f32]) -> usize {
            dst.fill(0.0);
            dst.len()
        }
        fn num_input_channels(&self) -> usize {
            self.channels
        }
    }

    #[test]
    fn test_input_creation() {
        let capture = Arc::new(NullCapture { channels: 2 });
        let inp = Input::<f32, 64>::new(capture);
        assert_eq!(inp.metadata().signal_outputs, 2);
    }

    #[test]
    fn test_input_mono_creation() {
        let capture = Arc::new(NullCapture { channels: 1 });
        let inp = Input::<f32, 64>::with_channels(capture, 1);
        assert_eq!(inp.metadata().signal_outputs, 1);
        assert!(inp.output_port(0).is_some());
        assert!(inp.output_port(1).is_none());
    }

    #[test]
    fn test_input_generate() {
        let capture = Arc::new(NullCapture { channels: 2 });
        let mut inp = Input::<f32, 64>::new(capture);
        let ctx = RenderContext::new(0, 64, 48000.0);
        let tick = ClockTick::new(0, 64, 48000.0, "test".into());
        assert!(inp.generate(&ctx, &[], &[], &tick).is_ok());
    }
}
