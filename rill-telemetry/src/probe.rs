use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rill_core::math::Transcendental;
use rill_core::prelude::{
    Node, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue, ParameterId, Port,
    ProcessResult,
};
use rill_core::queues::spsc::SpscQueue;
use rill_core::queues::TelemetryBlock;
use rill_core::time::RenderContext;
use rill_core::traits::Processor;

/// Passive telemetry probe that passes audio through while periodically
/// capturing block data + metrics into a shared ring buffer.
///
/// # Type Parameters
/// - `T` — sample type (f32/f64)
/// - `BUF_SIZE` — signal block size
/// - `QUEUE_CAP` — capacity of the telemetry ring buffer (must be power of two)
///
/// # Design
/// - 1 audio input → 1 audio output (passthrough, zero-latency)
/// - Every `interval` blocks, computes peak/RMS/DC and pushes into the queue
/// - Queue is `SpscQueue` with overwrite-oldest policy (default)
/// - Non-blocking: if the queue is full, old entries are overwritten
pub struct TelemetryProbe<T: Transcendental, const BUF_SIZE: usize, const QUEUE_CAP: usize> {
    id: NodeId,
    inputs: Vec<Port<T, BUF_SIZE>>,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,

    /// Shared telemetry ring buffer (Arc for shared ownership)
    queue: Arc<SpscQueue<TelemetryBlock<T, BUF_SIZE>, QUEUE_CAP>>,

    /// Send telemetry every N blocks
    interval: u32,
    /// Block counter
    counter: u32,
    /// Monotonic block index (for telemetry frames)
    block_index: u64,
    /// Audio channel index reported in telemetry
    channel: u32,
    /// Node name reported in telemetry
    node_name: String,
}

impl<T: Transcendental, const BUF_SIZE: usize, const QUEUE_CAP: usize>
    TelemetryProbe<T, BUF_SIZE, QUEUE_CAP>
{
    /// Create a new telemetry probe.
    ///
    /// # Arguments
    /// * `queue` — shared `SpscQueue` for telemetry output
    /// * `interval` — send telemetry every N blocks (1 = every block)
    /// * `channel` — channel index to report in telemetry frames
    /// * `node_name` — human-readable node name for metadata
    pub fn new(
        queue: Arc<SpscQueue<TelemetryBlock<T, BUF_SIZE>, QUEUE_CAP>>,
        interval: u32,
        channel: u32,
        node_name: &str,
    ) -> Self {
        assert!(interval > 0, "interval must be positive");

        let id = NodeId(0);
        let inputs = vec![Port::input(id, 0, "signal_in")];

        let outputs = vec![Port::output(id, 0, "signal_out")];

        Self {
            id,
            inputs,
            outputs,
            state: NodeState::new(44100.0),
            queue,
            interval,
            counter: 0,
            block_index: 0,
            channel,
            node_name: node_name.to_string(),
        }
    }

    /// Get a reference to the shared telemetry queue.
    pub fn queue(&self) -> &Arc<SpscQueue<TelemetryBlock<T, BUF_SIZE>, QUEUE_CAP>> {
        &self.queue
    }
}

// ── Node ──────────────────────────────────────────────────────────────

impl<T: Transcendental, const BUF_SIZE: usize, const QUEUE_CAP: usize> Node<T, BUF_SIZE>
    for TelemetryProbe<T, BUF_SIZE, QUEUE_CAP>
{
    fn metadata(&self) -> NodeMetadata {
        let mut meta = NodeMetadata::new(&self.node_name, NodeCategory::Analyzer);
        meta.description = "Pass-through telemetry probe".to_string();
        meta.author = "Rill".to_string();
        meta.version = env!("CARGO_PKG_VERSION").to_string();
        meta.signal_inputs = self.inputs.len();
        meta.signal_outputs = self.outputs.len();
        meta
    }

    fn init(&mut self, sample_rate: f32) {
        self.state = NodeState::new(sample_rate);
    }

    fn reset(&mut self) {
        self.state.reset();
        self.counter = 0;
        self.block_index = 0;
    }

    fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
        None
    }

    fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
        Ok(())
    }

    fn id(&self) -> NodeId {
        self.id
    }

    fn set_id(&mut self, id: NodeId) {
        self.id = id;
    }

    fn input_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.inputs.get(index)
    }

    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.inputs.get_mut(index)
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
        self.inputs.len()
    }

    fn num_signal_outputs(&self) -> usize {
        self.outputs.len()
    }

    fn num_control_inputs(&self) -> usize {
        0
    }

    fn num_control_outputs(&self) -> usize {
        0
    }

    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }

    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
}

// ── Processor ──────────────────────────────────────────────────────────────

impl<T: Transcendental, const BUF_SIZE: usize, const QUEUE_CAP: usize> Processor<T, BUF_SIZE>
    for TelemetryProbe<T, BUF_SIZE, QUEUE_CAP>
{
    fn process(
        &mut self,
        _ctx: &RenderContext,
        signal_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        // ── Passthrough: copy input[0] → output[0] ──────────────────────
        let silence = [T::ZERO; BUF_SIZE];
        let input = signal_inputs.first().copied().unwrap_or(&silence);
        if let Some(port) = self.outputs.first_mut() {
            port.write().copy_from_slice(input);
        }

        // ── Telemetry capture (every N blocks) ──────────────────────────
        self.counter += 1;
        if self.counter >= self.interval {
            self.counter = 0;

            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64;

            let mut frame = TelemetryBlock {
                node_id: self.id,
                channel: self.channel,
                sample_rate: self.state.sample_rate,
                block_index: self.block_index,
                timestamp,
                ..Default::default()
            };
            frame.data.copy_from_slice(input);
            frame.compute_metrics();

            self.block_index += 1;

            // Non-blocking push with overwrite-oldest semantics
            let _ = self.queue.push(frame);
        }

        self.state.advance();
        Ok(())
    }

    fn latency(&self) -> usize {
        0
    }
}
