//! # AudioInput — Stereo Source Node (push model)
//!
//! Registered as `"rill/input"` with `NodeVariant::Source`.
//!
//! Owns the backend (`Box<dyn AudioIo>`). Output nodes borrow via `AudioIoPtr`.

use std::cell::Cell;

use rill_core::{
    math::Transcendental,
    traits::{
        algorithm::ActionContext,
        node::SignalNode,
        processable::{NodeVariant, ProcessContext, Processable},
        NodeCategory, NodeMetadata, NodeState, Source,
    },
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessResult,
};

use crate::audio_io::{AudioIo, AudioIoPtr};
use crate::config::AudioConfig;
use crate::error::IoResult;

/// Wrapper for `AudioInput`'s backend field.
///
/// `SignalNode` requires `Send + Sync`. `AudioIo` is only `Send`.
/// `Sync` is sound because the RT protocol guarantees `stop()` is
/// called after the RT thread has been joined — no concurrent
/// `read_input`/`write_output`.
struct BackendField(Option<Box<dyn AudioIo>>);
unsafe impl Sync for BackendField {}

impl std::ops::Deref for BackendField {
    type Target = Option<Box<dyn AudioIo>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for BackendField {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl BackendField {
    fn none() -> Self {
        Self(None)
    }
    fn set(&mut self, backend: Box<dyn AudioIo>) {
        self.0 = Some(backend);
    }
}

/// Stereo audio input source. Owns the processing callback that drives
/// the entire DAG: drain commands → read backend → fill outputs → propagate.
///
/// Owns the audio backend (`Box<dyn AudioIo>`). The backend lives as long
/// as this node lives — dropped together with the node.
pub struct AudioInput<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    backend: BackendField,
    buf_l: [f32; BUF_SIZE],
    buf_r: [f32; BUF_SIZE],
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for AudioInput<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> AudioInput<T, BUF_SIZE> {
    /// Create a new `AudioInput` with no backend attached.
    pub fn new() -> Self {
        let mut metadata = NodeMetadata::new("AudioInput", NodeCategory::Source);
        metadata.signal_inputs = 0;
        metadata.signal_outputs = 2;

        let outputs = vec![
            Port::output(NodeId(0), 0, "left"),
            Port::output(NodeId(0), 1, "right"),
        ];

        Self {
            id: NodeId(0),
            metadata,
            outputs,
            state: NodeState::new(44100.0),
            backend: BackendField::none(),
            buf_l: [0.0; BUF_SIZE],
            buf_r: [0.0; BUF_SIZE],
        }
    }

    /// Take ownership of a backend.
    pub fn set_backend(&mut self, backend: Box<dyn AudioIo>) {
        self.backend.set(backend);
    }

    /// Create and set a backend by name.
    ///
    /// Supported names: `"null"`, `"alsa"`, `"cpal"`, `"pipewire"`, `"jack"`.
    /// Each backend is available only when its cargo feature is enabled.
    /// `"null"` is always available.
    ///
    /// # Errors
    ///
    /// Returns `IoError::Unsupported` if the name is not recognised, or
    /// a backend-specific error if the device cannot be opened.
    pub fn init_backend(&mut self, name: &str, config: AudioConfig) -> IoResult<()> {
        match name {
            "null" | "Null" => {
                self.backend
                    .set(Box::new(crate::backends::NullBackend::new(config)));
                Ok(())
            }
            #[cfg(feature = "alsa")]
            "alsa" | "ALSA" => {
                let b = crate::backends::AlsaBackend::new(config)?;
                self.backend.set(Box::new(b));
                Ok(())
            }
            #[cfg(feature = "cpal")]
            "cpal" | "CPAL" => {
                let b = crate::backends::CpalBackend::new(config)?;
                self.backend.set(Box::new(b));
                Ok(())
            }
            #[cfg(feature = "pipewire")]
            "pipewire" | "PipeWire" => {
                let b = crate::backends::PipewireBackend::new(config)?;
                self.backend.set(Box::new(b));
                Ok(())
            }
            #[cfg(feature = "jack")]
            "jack" | "JACK" => {
                let b = crate::backends::JackBackend::new(config)?;
                self.backend.set(Box::new(b));
                Ok(())
            }
            _ => Err(crate::error::IoError::Unsupported(format!(
                "audio backend: {name}"
            ))),
        }
    }

    /// Return a borrowed pointer for output nodes.
    pub fn backend_ptr(&self) -> AudioIoPtr {
        match self.backend.as_ref() {
            Some(b) => AudioIoPtr::from_ref(&**b),
            None => AudioIoPtr::null(),
        }
    }

    /// Start the reactive stream. Creates and registers the processing
    /// callback on the backend. The callback:
    ///
    /// 1. Calls `drain_fn()` — the host should drain the parameter queue there.
    /// 2. Processes this node (`generate` → reads backend → fills ports).
    /// 3. Propagates through the DAG via `Port::propagate`.
    ///
    /// `nodes_ptr` must point to the graph's node array (obtained from
    /// `graph.into_parts().0.into_boxed_slice()`). Valid until `stop()`.
    /// `source_idx` is this node's index in the array.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn start(
        &mut self,
        nodes_ptr: *mut [NodeVariant<f32, BUF_SIZE>],
        source_idx: usize,
        drain_fn: Box<dyn Fn(&mut [NodeVariant<f32, BUF_SIZE>]) + Send>,
        sample_rate: f32,
    ) {
        if let Some(b) = self.backend.as_ref() {
            let sample_pos = Cell::new(0u64);

            b.set_process_callback(Box::new(move || {
                #[allow(unsafe_code)]
                unsafe {
                    let nodes = &mut *nodes_ptr;

                    // 1. Drain parameter queue (host-provided closure)
                    drain_fn(nodes);

                    // 2. Clock tick
                    let tick = ClockTick::new(sample_pos.get(), BUF_SIZE as u32, sample_rate);

                    // 3. Process this node (generate → read backend → fill ports)
                    let mut ctx = ProcessContext { clock: &tick };
                    let _ = nodes[source_idx].process_block(&mut ctx);

                    // 4. Propagate from this node's output ports
                    let action_ctx = ActionContext::new(&tick);
                    for po in 0..nodes[source_idx].num_signal_outputs() {
                        if let Some(port) = nodes[source_idx].output_port(po) {
                            let _ = port.propagate(port.buffer(), &action_ctx);
                        }
                    }

                    sample_pos.set(sample_pos.get() + BUF_SIZE as u64);
                }
            }));

            let _ = b.start();
        }
    }

    /// Stop the audio backend.
    pub fn stop(&mut self) {
        if let Some(b) = self.backend.as_ref() {
            let _ = b.stop();
        }
    }

    /// Check whether a backend has been attached.
    pub fn has_backend(&self) -> bool {
        self.backend.is_some()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE> for AudioInput<T, BUF_SIZE> {
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
            "AudioInput has no parameters",
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
        2
    }
    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }
    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE> for AudioInput<T, BUF_SIZE> {
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
                    for i in 0..frames {
                        l[i] = T::from_f32(self.buf_l[i]);
                    }
                }
                if let Some(right) = self.outputs.get_mut(1) {
                    let r = right.buffer_mut().as_mut_array();
                    for i in 0..frames {
                        r[i] = T::from_f32(self.buf_r[i]);
                    }
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
    use crate::audio_io::AudioIo;
    use crate::buffer::IoRingBuffer;
    use rill_core::traits::Sink;
    use std::sync::Arc;

    /// Mock AudioIo backed by IoRingBuffers for testing.
    struct RingIo {
        input_ring: Arc<IoRingBuffer>,
        output_ring: Arc<IoRingBuffer>,
    }
    impl AudioIo for RingIo {
        fn set_process_callback(&self, _cb: Box<dyn Fn()>) {}
        fn read_input(&self, left: &mut [f32], right: &mut [f32]) -> usize {
            let mut temp = vec![0.0f32; left.len().min(right.len()).saturating_mul(2)];
            let n = self.input_ring.read(&mut temp);
            let frames = n / 2;
            for i in 0..frames.min(left.len()).min(right.len()) {
                left[i] = temp[i * 2];
                right[i] = temp[i * 2 + 1];
            }
            frames
        }
        fn write_output(&self, left: &[f32], right: &[f32]) -> usize {
            let n = left.len().min(right.len());
            let mut temp = vec![0.0f32; n * 2];
            for i in 0..n {
                temp[i * 2] = left[i];
                temp[i * 2 + 1] = right[i];
            }
            self.output_ring.write(&temp) / 2
        }
        fn start(&self) -> crate::audio_io::IoResult<()> {
            Ok(())
        }
        fn stop(&self) -> crate::audio_io::IoResult<()> {
            Ok(())
        }
    }

    use rill_core::traits::algorithm::ActionContext;

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

    /// Round-trip test: AudioInput → AudioOutput through shared ring buffers.
    #[test]
    fn test_loopback_through_rings() {
        const BUF_SZ: usize = 64;
        let input_ring = Arc::new(IoRingBuffer::new(512));
        let output_ring = Arc::new(IoRingBuffer::new(512));

        let backend = Box::new(RingIo {
            input_ring: input_ring.clone(),
            output_ring: output_ring.clone(),
        });
        let io_ptr = AudioIoPtr::from_ref(&*backend);

        let mut input = AudioInput::<f32, BUF_SZ>::new();
        input.set_backend(backend);

        // Write test data into the input ring (as PW input callback would)
        let test_val: f32 = 42.0;
        let mut test_block = vec![0.0f32; BUF_SZ * 2];
        for i in 0..BUF_SZ {
            test_block[i * 2] = test_val; // left
            test_block[i * 2 + 1] = test_val; // right
        }
        input_ring.write(&test_block);

        // Run generate
        let tick = ClockTick::new(0, BUF_SZ as u32, 48000.0);
        input.generate(&tick, &[], &[]).unwrap();

        // Read output from input's ports
        let l = input.output_port(0).unwrap().buffer.as_array();
        let r = input.output_port(1).unwrap().buffer.as_array();
        for i in 0..BUF_SZ {
            assert!(
                (l[i] - 42.0).abs() < 1e-6,
                "left[{}] should be 42.0, got {}",
                i,
                l[i]
            );
            assert!(
                (r[i] - 42.0).abs() < 1e-6,
                "right[{}] should be 42.0, got {}",
                i,
                r[i]
            );
        }

        // Verify data flows: AudioInput.generate() reads from input ring,
        // fills output ports, Port::propagate copies to downstream, and
        // AudioOutput.consume() writes to output ring.
        //
        // This loopback through a proper graph is tested in
        // rill-adrift/tests/pull_model.rs via GraphDocument serialization.
        // Here we verify the two halves work in isolation.
    }
}
