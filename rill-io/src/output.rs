//! # Output — generic signal output sink node
//!
//! Registered as `"rill/output"` with `NodeVariant::Sink`.

use std::sync::Arc;

use rill_core::{
    io::IoPlayback,
    math::Transcendental,
    time::ClockTick,
    traits::{Node, NodeCategory, NodeMetadata, NodeState, Sink},
    NodeId, ParamValue, ParameterId, Port, ProcessResult, RenderContext,
};

/// Signal output sink. Writes to [`IoPlayback`] in `consume()`.
///
/// # Ports
/// - `n` input ports (one per channel), set via [`Self::with_channels`].
pub struct Output<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    playback: Arc<dyn IoPlayback>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Output<T, BUF_SIZE> {
    /// Create a new output sink with a playback backend.
    pub fn new(playback: Arc<dyn IoPlayback>) -> Self {
        Self::with_channels(playback, 2)
    }

    /// Create a new output sink with the given number of channels.
    pub fn with_channels(playback: Arc<dyn IoPlayback>, num: usize) -> Self {
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
            playback,
        }
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

impl<T: Transcendental, const BUF_SIZE: usize> Sink<T, BUF_SIZE> for Output<T, BUF_SIZE> {
    fn consume(
        &mut self,
        _ctx: &RenderContext,
        _signal_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
        _feedback_inputs: &[&[T; BUF_SIZE]],
        _tick: &ClockTick,
    ) -> ProcessResult<()> {
        for (ch, port) in self.inputs.iter().enumerate() {
            if port.data_received {
                let buf = port.signal_buffer();
                #[allow(unsafe_code)]
                unsafe {
                    let buf_f32: &[f32] =
                        std::slice::from_raw_parts(buf.as_ptr() as *const f32, buf.len());
                    self.playback.write_output(ch, buf_f32);
                }
            }
        }
        self.state.advance();
        Ok(())
    }

    fn set_playback(&mut self, playback: Arc<dyn IoPlayback>) {
        self.playback = playback;
    }
}

/// Backward-compatible alias.
pub type AudioOutput<T, const B: usize> = Output<T, B>;

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::io::IoPlayback;

    struct NullPlayback {
        channels: usize,
    }
    impl IoPlayback for NullPlayback {
        fn write_output(&self, _channel: usize, _src: &[f32]) -> usize {
            _src.len()
        }
        fn num_output_channels(&self) -> usize {
            self.channels
        }
    }

    #[test]
    fn test_output_creation() {
        let pb = Arc::new(NullPlayback { channels: 2 });
        let out = Output::<f32, 64>::new(pb);
        assert_eq!(out.metadata().signal_inputs, 2);
        assert_eq!(out.metadata().signal_outputs, 0);
        assert!(out.input_port(0).is_some());
        assert!(out.input_port(1).is_some());
    }

    #[test]
    fn test_output_mono() {
        let pb = Arc::new(NullPlayback { channels: 1 });
        let out = Output::<f32, 64>::with_channels(pb, 1);
        assert_eq!(out.metadata().signal_inputs, 1);
        assert!(out.input_port(0).is_some());
        assert!(out.input_port(1).is_none());
    }

    #[test]
    fn test_output_consume() {
        let pb = Arc::new(NullPlayback { channels: 2 });
        let mut out = Output::<f32, 64>::new(pb);
        let ctx = RenderContext::new(0, 64, 48000.0);
        let signal_inputs: &[&[f32; 64]] = &[];
        let tick = ClockTick::new(0, 64, 48000.0, "test".into());
        assert!(out
            .consume(&ctx, signal_inputs, &[], &[], &[], &tick)
            .is_ok());
    }
}
