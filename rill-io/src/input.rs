//! # AudioInput — Stereo Source Node (push model)
//!
//! Registered as `"rill/input"` with `NodeVariant::Source`.
//!
//! Owns the backend (`Box<dyn AudioIo>`). Output nodes borrow via `AudioIoPtr`.

use std::cell::Cell;

use rill_core::{
    math::Transcendental,
    traits::{
        active::{ActiveNode, GraphHandle},
        algorithm::ActionContext,
        node::SignalNode,
        processable::{NodeVariant, ProcessContext, Processable},
        NodeCategory, NodeMetadata, NodeState, Source,
    },
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessResult,
};

use crate::signal_io::IoBackendPtr;

/// Stereo audio input source. Drives the graph by reading from a backend
/// in `generate()`, then propagating through the DAG.
///
/// The backend is owned by the graph's `BackendRegistry` — this node stores
/// only a non‑owning [`IoBackendPtr<T>`].
pub struct AudioInput<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    io_ptr: IoBackendPtr<T>,
    buf_l: [T; BUF_SIZE],
    buf_r: [T; BUF_SIZE],
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
            io_ptr: IoBackendPtr::<T>::null(),
            buf_l: [T::ZERO; BUF_SIZE],
            buf_r: [T::ZERO; BUF_SIZE],
        }
    }

    /// Attach an `IoBackendPtr` (called during graph assembly).
    pub fn set_io_ptr(&mut self, ptr: IoBackendPtr<T>) {
        self.io_ptr = ptr;
    }

    pub fn io_ptr(&self) -> IoBackendPtr<T> {
        self.io_ptr
    }

    /// Check whether a backend has been attached.
    pub fn has_backend(&self) -> bool {
        !self.io_ptr.is_null()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> ActiveNode for AudioInput<T, BUF_SIZE> {
    #[allow(clippy::not_unsafe_ptr_arg_deref, clippy::type_complexity)]
    fn start(&mut self, handle: GraphHandle) {
        if let Some(b) = self.io_ptr.as_ref() {
            let nodes_ptr = handle.nodes as *mut NodeVariant<T, BUF_SIZE>;
            let len = handle.len;
            let source_idx = handle.source_idx;
            let sample_rate = handle.sample_rate;
            let queue_ptr = handle.queue;
            let sample_pos = Cell::new(0u64);

            b.set_process_callback(Box::new(move || {
                #[allow(unsafe_code)]
                unsafe {
                    let nodes = std::slice::from_raw_parts_mut(nodes_ptr, len);

                    // 1. Drain command queue → apply parameters.
                    if let Some(q) = queue_ptr.as_ref() {
                        while let Some(cmd) = q.pop() {
                            let idx = cmd.port.node_id().inner() as usize;
                            if idx < len {
                                let _ = nodes[idx]
                                    .set_parameter(&cmd.parameter, ParamValue::Float(cmd.value));
                            }
                        }
                    }

                    // 2. Clock tick.
                    let tick = ClockTick::new(sample_pos.get(), BUF_SIZE as u32, sample_rate);

                    // 3. Process source node (generate → fills ports).
                    let mut ctx = ProcessContext { clock: &tick };
                    let _ = nodes[source_idx].process_block(&mut ctx);

                    // 4. Propagate through the DAG.
                    let action_ctx = ActionContext::new(&tick);
                    for po in 0..nodes[source_idx].num_signal_outputs() {
                        if let Some(port) = nodes[source_idx].output_port(po) {
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

    fn stop(&mut self) {
        if let Some(b) = self.io_ptr.as_ref() {
            let _ = b.stop();
        }
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
    fn resolve_backend(&mut self, backend: *mut dyn rill_core::io::IoBackend<T>) {
        if !backend.is_null() {
            self.io_ptr = IoBackendPtr::from_ref(unsafe { &*backend });
        }
    }
    fn start(&mut self, handle: GraphHandle) {
        ActiveNode::start(self, handle);
    }
    fn stop(&mut self) {
        ActiveNode::stop(self);
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
        if let Some(io) = self.io_ptr.as_ref() {
            let channels = &mut [&mut self.buf_l[..], &mut self.buf_r[..]];
            let n = io.read(channels);
            if n >= BUF_SIZE {
                if let Some(left) = self.outputs.get_mut(0) {
                    let l = left.buffer_mut().as_mut_array();
                    l[..BUF_SIZE].copy_from_slice(&self.buf_l[..BUF_SIZE]);
                }
                if let Some(right) = self.outputs.get_mut(1) {
                    let r = right.buffer_mut().as_mut_array();
                    r[..BUF_SIZE].copy_from_slice(&self.buf_r[..BUF_SIZE]);
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
    use crate::audio_io::IoResult;
    use crate::buffer::IoRingBuffer;
    use crate::signal_io::IoBackendPtr;
    use rill_core::io::IoBackend;
    use std::sync::Arc;

    /// Mock backend for testing.
    struct RingIo {
        input_ring: Arc<IoRingBuffer>,
        output_ring: Arc<IoRingBuffer>,
    }
    impl IoBackend<f32> for RingIo {
        fn set_process_callback(&self, _cb: Box<dyn Fn()>) {}
        fn read(&self, channels: &mut [&mut [f32]]) -> usize {
            let frames = channels.first().map(|c| c.len()).unwrap_or(0);
            let mut temp = vec![0.0f32; frames * 2];
            let n = self.input_ring.read(&mut temp);
            let out = n / 2;
            for i in 0..out.min(frames) {
                if let Some(ch) = channels.get_mut(0) {
                    ch[i] = temp[i * 2];
                }
                if let Some(ch) = channels.get_mut(1) {
                    ch[i] = temp[i * 2 + 1];
                }
            }
            out
        }
        fn write(&self, channels: &[&[f32]]) -> usize {
            let frames = channels.first().map(|c| c.len()).unwrap_or(0);
            let mut temp = vec![0.0f32; frames * 2];
            for i in 0..frames {
                if let Some(ch) = channels.get(0) {
                    temp[i * 2] = ch[i];
                }
                if let Some(ch) = channels.get(1) {
                    temp[i * 2 + 1] = ch[i];
                }
            }
            self.output_ring.write(&temp) / 2
        }
        fn run(&self, _running: Arc<std::sync::atomic::AtomicBool>) -> IoResult<()> {
            Ok(())
        }
        fn stop(&self) -> IoResult<()> {
            Ok(())
        }
    }

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
        let mut input = AudioInput::<f32, BUF_SZ>::new();
        input.set_io_ptr(IoBackendPtr::from_ref(&*backend));

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
