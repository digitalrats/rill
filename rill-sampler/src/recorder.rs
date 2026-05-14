//! Recording sink — captures audio into a Vec<f32> for offline analysis.
//!
//! RT-safe: the Mutex is only used during `consume()` (audio thread, single writer)
//! and for post-processing (read after graph stops). No contention.

use std::sync::{Arc, Mutex};

use rill_core::{
    math::Transcendental,
    time::ClockTick,
    traits::{
        Node, NodeCategory, NodeMetadata, NodeState, ParamValue, ParameterId, Port, PortDirection,
        PortId, PortType, ProcessError, ProcessResult, Sink,
    },
    NodeId,
};

/// Sink node that records stereo audio into a shared buffer.
///
/// # Usage
///
/// ```no_run
/// # use std::sync::{Arc, Mutex};
/// # use rill_sampler::recorder::RecordingSink;
/// let buf = Arc::new(Mutex::new(Vec::new()));
/// let sink = RecordingSink::<f32, 256>::new(buf.clone(), 2);
/// // ... build graph, run ...
/// let data = buf.lock().unwrap();
/// RecordingSink::<f32, 256>::write_wav("output.wav", 44100, 2, &data).unwrap();
/// ```
pub struct RecordingSink<T: Transcendental, const B: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, B>>,
    state: NodeState<T, B>,
    recorded: Arc<Mutex<Vec<f32>>>,
}

impl<T: Transcendental, const B: usize> RecordingSink<T, B> {
    /// Create a stereo RecordingSink.
    pub fn new(recorded: Arc<Mutex<Vec<f32>>>, channels: usize) -> Self {
        let ch = channels.max(1).min(2);
        let inputs: Vec<_> = if ch == 1 {
            vec![Port::input(NodeId(0), 0, "mono")]
        } else {
            vec![
                Port::input(NodeId(0), 0, "left"),
                Port::input(NodeId(0), 1, "right"),
            ]
        };
        Self {
            id: NodeId(0),
            metadata: NodeMetadata::new("RecordingSink", NodeCategory::Sink),
            inputs,
            state: NodeState::new(44100.0),
            recorded,
        }
    }

    #[cfg(feature = "wav")]
    pub fn write_wav(
        path: &str,
        sample_rate: u32,
        channels: u16,
        samples: &[f32],
    ) -> Result<(), String> {
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
        for &s in samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            writer.write_sample(v).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())
    }
}

impl<T: Transcendental, const B: usize> Node<T, B> for RecordingSink<T, B> {
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
        for (i, p) in self.inputs.iter_mut().enumerate() {
            p.id = PortId::signal_in(id, i as u16);
        }
    }
    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }
    fn init(&mut self, sample_rate: f32) {
        self.state = NodeState::new(sample_rate);
    }
    fn reset(&mut self) {
        self.state.sample_pos = 0;
    }
    fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
        None
    }
    fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
        Err(ProcessError::parameter("RecordingSink has no parameters"))
    }
    fn input_port(&self, index: usize) -> Option<&Port<T, B>> {
        self.inputs.get(index)
    }
    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, B>> {
        self.inputs.get_mut(index)
    }
    fn output_port(&self, _index: usize) -> Option<&Port<T, B>> {
        None
    }
    fn output_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, B>> {
        None
    }
    fn control_port(&self, _index: usize) -> Option<&Port<T, B>> {
        None
    }
    fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, B>> {
        None
    }
    fn num_signal_inputs(&self) -> usize {
        self.inputs.len()
    }
    fn num_signal_outputs(&self) -> usize {
        0
    }
    fn state(&self) -> &NodeState<T, B> {
        &self.state
    }
    fn state_mut(&mut self) -> &mut NodeState<T, B> {
        &mut self.state
    }
}

impl<T: Transcendental, const B: usize> Sink<T, B> for RecordingSink<T, B> {
    fn consume(
        &mut self,
        _clock: &ClockTick,
        _signal_inputs: &[&[T; B]],
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
        _feedback_inputs: &[&[T; B]],
    ) -> ProcessResult<()> {
        if self.inputs.is_empty() {
            return Ok(());
        }
        let nch = self.inputs.len();
        let ch0 = self.inputs[0].buffer.as_array();
        let ch1 = if nch > 1 {
            Some(self.inputs[1].buffer.as_array())
        } else {
            None
        };
        let mut dst = self.recorded.lock().unwrap();
        for i in 0..B {
            dst.push(ch0[i].to_f32());
            if let Some(ref c1) = ch1 {
                dst.push(c1[i].to_f32());
            }
        }
        self.state.advance();
        Ok(())
    }
}
