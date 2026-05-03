use crate::registry::{NodeRegistry, RegistryError};
use rill_core::buffer::{Buffer, TapeLoop};
use rill_core::math::Transcendental;
use rill_core::time::{ClockSource, ClockTick, SystemClock};
use rill_core::traits::{SignalNode, NodeId, NodeParams, NodeVariant, PortId};
use std::collections::VecDeque;

// ============================================================================
// Internal routing metadata
// ============================================================================

/// Describes how an signal input port routes to an signal output port within a node.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct InternalRoute {
    pub from: PortId,
    pub to: PortId,
}

// ============================================================================
// Connection classification (auto-detected by the builder)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionKind {
    Direct,
    FanOut,
    FanIn,
}

// ============================================================================
// Build Errors
// ============================================================================

#[derive(Debug, Clone)]
pub enum BuildError {
    CycleDetected,
}

// ============================================================================
// Graph Builder
// ============================================================================

#[derive(Debug, Clone, Copy, Default)]
pub struct GraphStats {
    pub blocks_processed: u64,
    pub max_process_time_ns: u64,
    pub avg_process_time_ns: f64,
}

// ============================================================================
// Node Storage
// ============================================================================

pub(crate) struct NodeEntry<T: Transcendental, const BUF_SIZE: usize> {
    pub(crate) node: NodeVariant<T, BUF_SIZE>,
}

// ============================================================================
// GraphBuilder (Mutable Construction)
// ============================================================================

/// A named resource (tape loop) shared between nodes in the graph.
#[derive(Clone)]
pub struct GraphResource {
    /// Unique name referenced by node parameters.
    pub name: String,
    /// Resource kind string (`"tape"`).
    pub kind: String,
    /// Capacity in samples (for `"tape"` kind).
    pub capacity: usize,
}

/// Mutable builder for an immutable signal graph.
pub struct GraphBuilder<T: Transcendental, const BUF_SIZE: usize> {
    nodes: Vec<NodeEntry<T, BUF_SIZE>>,
    audio_edges: Vec<(usize, usize, usize, usize)>,
    control_edges: Vec<(usize, usize, usize, usize)>,
    clock_edges: Vec<(usize, usize, usize, usize)>,
    feedback_edges: Vec<(usize, usize, usize, usize)>,
    resources: Vec<GraphResource>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for GraphBuilder<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> GraphBuilder<T, BUF_SIZE> {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            audio_edges: Vec::new(),
            control_edges: Vec::new(),
            clock_edges: Vec::new(),
            feedback_edges: Vec::new(),
            resources: Vec::new(),
        }
    }

    /// Register a named resource.
    pub fn add_resource(&mut self, resource: GraphResource) {
        self.resources.push(resource);
    }

    pub fn add_source(&mut self, source: Box<dyn rill_core::traits::Source<T, BUF_SIZE>>) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(NodeEntry {
            node: NodeVariant::Source(source),
        });
        idx
    }

    pub fn add_processor(
        &mut self,
        processor: Box<dyn rill_core::traits::Processor<T, BUF_SIZE>>,
    ) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(NodeEntry {
            node: NodeVariant::Processor(processor),
        });
        idx
    }

    pub fn add_sink(&mut self, sink: Box<dyn rill_core::traits::Sink<T, BUF_SIZE>>) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(NodeEntry {
            node: NodeVariant::Sink(sink),
        });
        idx
    }

    /// Add a node by type name via the registry.
    ///
    /// Looks up the type name in `registry`, calls its
    /// [`NodeConstructor::construct`], and pushes the resulting
    /// [`NodeVariant`] into the graph. The node's [`NodeId`] is
    /// automatically assigned from its position in the graph.
    ///
    /// Returns the index of the newly added node.
    pub fn add_node(
        &mut self,
        registry: &NodeRegistry<T, BUF_SIZE>,
        type_name: &str,
        params: &NodeParams,
    ) -> Result<usize, RegistryError> {
        let id = NodeId(self.nodes.len() as u32);
        self.add_node_with_id(registry, type_name, params, id)
    }

    /// Add a node with an explicit [`NodeId`].
    ///
    /// Unlike [`add_node`](Self::add_node) which auto-assigns IDs, this
    /// method uses the provided `id` directly. Important for serialization
    /// where external references (e.g. patchbay bindings) depend on exact IDs.
    ///
    /// Returns the index (position) of the newly added node.
    ///
    /// # Panics
    ///
    /// If `id` duplicates a previously registered ID the error is reported
    /// by the caller — this method does not check for duplicates.
    pub fn add_node_with_id(
        &mut self,
        registry: &NodeRegistry<T, BUF_SIZE>,
        type_name: &str,
        params: &NodeParams,
        id: NodeId,
    ) -> Result<usize, RegistryError> {
        let node = registry.construct(type_name, id, params)?;
        let idx = self.nodes.len();
        self.nodes.push(NodeEntry { node });
        Ok(idx)
    }

    /// Return the number of nodes added so far.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Connect signal output port `from_port` of node `from_node`
    /// to signal input port `to_port` of node `to_node`.
    pub fn connect_signal(
        &mut self,
        from_node: usize,
        from_port: usize,
        to_node: usize,
        to_port: usize,
    ) {
        self.audio_edges
            .push((from_node, from_port, to_node, to_port));
    }

    /// Connect a control output to a control input.
    pub fn connect_control(
        &mut self,
        from_node: usize,
        from_port: usize,
        to_node: usize,
        to_port: usize,
    ) {
        self.control_edges
            .push((from_node, from_port, to_node, to_port));
    }

    /// Connect a clock output to a clock input.
    pub fn connect_clock(
        &mut self,
        from_node: usize,
        from_port: usize,
        to_node: usize,
        to_port: usize,
    ) {
        self.clock_edges
            .push((from_node, from_port, to_node, to_port));
    }

    /// Connect a feedback output to a feedback input.
    /// This creates a feedback path (previous output → current input).
    pub fn connect_feedback(
        &mut self,
        from_node: usize,
        from_port: usize,
        to_node: usize,
        to_port: usize,
    ) {
        self.feedback_edges
            .push((from_node, from_port, to_node, to_port));
    }

    /// Build the immutable SignalGraph.
    pub fn build(
        mut self,
        clock_source: Box<dyn ClockSource>,
    ) -> Result<SignalGraph<T, BUF_SIZE>, BuildError> {
        let num_nodes = self.nodes.len();

        // --- adjacency for Kahn (audio edges only; feedback is not a DAG edge) ---
        let mut in_degree = vec![0usize; num_nodes];
        let mut out_edges: Vec<Vec<(usize, usize, usize)>> = vec![Vec::new(); num_nodes];

        for &(from_n, from_p, to_n, to_p) in &self.audio_edges {
            in_degree[to_n] += 1;
            out_edges[from_n].push((from_p, to_n, to_p));
        }

        // --- Kahn's algorithm ---
        let mut queue: VecDeque<usize> = in_degree
            .iter()
            .enumerate()
            .filter(|(_, &d)| d == 0)
            .map(|(i, _)| i)
            .collect();

        let mut topo = Vec::with_capacity(num_nodes);
        let mut indeg = in_degree;
        while let Some(idx) = queue.pop_front() {
            topo.push(idx);
            for &(_, to_n, _) in &out_edges[idx] {
                indeg[to_n] -= 1;
                if indeg[to_n] == 0 {
                    queue.push_back(to_n);
                }
            }
        }

        if topo.len() != num_nodes {
            return Err(BuildError::CycleDetected);
        }

        // --- populate Port::downstream and Port::upstream_buffer ---
        for &(from_n, from_p, to_n, to_p) in &self.audio_edges {
            if let Some(port) = self.nodes[from_n].node.output_port_mut(from_p) {
                port.downstream.push((to_n, to_p));
            }
        }
        // upstream_buffer: set on input ports for zero-copy 1:1 connections.
        // Fan-in (multiple outputs → same input) falls back to copy-based.
        for &(from_n, from_p, to_n, to_p) in &self.audio_edges {
            let upstream = self.nodes[from_n]
                .node
                .output_port(from_p)
                .map(|p| &p.buffer as *const Buffer<T, BUF_SIZE>);
            if let Some(port) = self.nodes[to_n].node.input_port_mut(to_p) {
                if port.upstream_buffer.is_none() {
                    port.upstream_buffer = upstream;
                } else {
                    port.upstream_buffer = None;
                }
            }
        }

        // --- enable feedback buffers on both output and input ports ---
        for &(from_n, from_p, to_n, to_p) in &self.feedback_edges {
            if let Some(port) = self.nodes[from_n].node.output_port_mut(from_p) {
                port.feedback_buffer = Some(Buffer::new());
                port.feedback_downstream.push((to_n, to_p));
            }
            if let Some(port) = self.nodes[to_n].node.input_port_mut(to_p) {
                port.feedback_buffer = Some(Buffer::new());
            }
        }
        // --- populate Port::feedback_ptrs on output ports ---
        for &(from_n, from_p, to_n, to_p) in &self.feedback_edges {
            let ptr = self.nodes[to_n]
                .node
                .input_port(to_p)
                .map(|p| &p.feedback_buffer as *const Option<Buffer<T, BUF_SIZE>>)
                .map(|r| r as *mut Option<Buffer<T, BUF_SIZE>>);
            if let Some(port) = self.nodes[from_n].node.output_port_mut(from_p) {
                if let Some(p) = ptr {
                    port.feedback_ptrs.push(p);
                }
            }
        }

        let sample_rate = clock_source.sample_rate();

        // ── allocate tape resources and bind tape pointer to all nodes ──
        // Every node receives the pointer; only ReadHead/WriteHead use it.
        for r in &self.resources {
            if r.kind == "tape" {
                let tape = Box::new(TapeLoop::<T>::new(r.capacity)
                    .expect("tape allocation failed"));
                let ptr: *const TapeLoop<T> = Box::leak(tape) as *const TapeLoop<T>;
                for entry in self.nodes.iter_mut() {
                    entry.node.set_tape(ptr);
                }
            }
        }
        let allocated = self.resources.clone();

        Ok(SignalGraph {
            nodes: self.nodes,
            topo_order: topo,
            clock_source,
            resources: allocated,
            current_tick: ClockTick::new(0, BUF_SIZE as u32, sample_rate),
        })
    }
}

// ============================================================================
// SignalGraph (Static DAG)
// ============================================================================

/// Immutable signal graph with static DAG topology.
///
/// Once built the graph cannot be modified. The graph owns no processing
/// logic — it is a pure topology description. Processing is driven by
/// port-level methods (`pre_process`, `snapshot_feedback`, `propagate`)
/// called from external code (e.g. a real-time signal callback or an
/// offline renderer).
pub struct SignalGraph<T: Transcendental, const BUF_SIZE: usize> {
    nodes: Vec<NodeEntry<T, BUF_SIZE>>,
    topo_order: Vec<usize>,
    #[allow(dead_code)]
    clock_source: Box<dyn ClockSource>,
    current_tick: ClockTick,
    /// Named resources (tape loops, etc.) allocated during build.
    pub(crate) resources: Vec<GraphResource>,
}

impl<T: Transcendental, const BUF_SIZE: usize> SignalGraph<T, BUF_SIZE> {
    /// Create an empty graph with the given clock source.
    pub fn new(clock_source: Box<dyn ClockSource>) -> Self {
        let sample_rate = clock_source.sample_rate();
        Self {
            nodes: Vec::new(),
            topo_order: Vec::new(),
            clock_source,
            current_tick: ClockTick::new(0, BUF_SIZE as u32, sample_rate),
            resources: Vec::new(),
        }
    }

    /// Create an empty graph with a system clock at the given sample rate.
    pub fn with_sample_rate(sample_rate: f32) -> Self {
        Self::new(Box::new(SystemClock::with_sample_rate(sample_rate)))
    }

    /// Borrow an output port buffer (for inspection in tests).
    pub fn output_buffer(&self, node_idx: usize, port_idx: usize) -> Option<&[T; BUF_SIZE]> {
        self.nodes
            .get(node_idx)?
            .node
            .output_port(port_idx)
            .map(|p| p.buffer.as_array())
    }

    // ========================================================================
    // Accessors
    // ========================================================================

    pub fn current_tick(&self) -> ClockTick {
        self.current_tick
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn topo_order(&self) -> &[usize] {
        &self.topo_order
    }

    // ── pub(crate) accessors for serialization ─────────────────────

    pub(crate) fn node_entries(&self) -> &[NodeEntry<T, BUF_SIZE>] {
        &self.nodes
    }

    pub(crate) fn sample_rate(&self) -> f32 {
        self.current_tick.sample_rate
    }

    /// Access the named resources (tape loops, etc.) allocated for this graph.
    #[allow(dead_code)]
    pub fn resources(&self) -> &[GraphResource] {
        &self.resources
    }

    // ── Dispatch ──────────────────────────────────────────────────

    /// Dispatch `SetParameter` commands to their target nodes.
    ///
    /// Each command is routed to the node identified by `cmd.port.node_id()`
    /// via that node's `apply_set_parameter` method.
    pub fn dispatch_set_parameters(
        &mut self,
        commands: &[rill_core::queues::signal::SetParameter],
    ) {
        for cmd in commands {
            let target = cmd.port.node_id();
            for entry in self.nodes.iter_mut() {
                if entry.node.id() == target {
                    let _ = entry.node.apply_set_parameter(cmd);
                    break;
                }
            }
        }
    }

    /// Consume the graph and return its parts for the SignalEngine.
    pub fn into_parts(
        self,
    ) -> (Vec<NodeVariant<T, BUF_SIZE>>, Vec<usize>, ClockTick) {
        let nodes = self.nodes.into_iter().map(|e| e.node).collect();
        (nodes, self.topo_order, self.current_tick)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::math::Transcendental;
    use rill_core::time::ClockTick;
    use rill_core::traits::{
        SignalNode, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue, ParameterId, Port,
        PortDirection, PortId, ProcessResult, Processor, Sink, Source,
    };

    // ------------------------------------------------------------------------
    // Mock: ConstantSource — fills output with a constant value
    // ------------------------------------------------------------------------
    struct ConstantSource<T: Transcendental, const BUF_SIZE: usize> {
        value: T,
        state: NodeState<T, BUF_SIZE>,
        outputs: Vec<Port<T, BUF_SIZE>>,
    }

    impl<T: Transcendental, const BUF_SIZE: usize> ConstantSource<T, BUF_SIZE> {
        fn new(value: T, sample_rate: f32) -> Self {
            let mut outputs = Vec::with_capacity(1);
            outputs.push(Port {
                id: PortId::audio_out(NodeId(0), 0),
                name: "output".into(),
                direction: PortDirection::Output,
                action: None,
                pending_command: None,
                buffer: Default::default(),
                feedback_buffer: None,
                downstream: Vec::new(),
                feedback_downstream: Vec::new(),
            feedback_ptrs: Vec::new(),
            upstream_buffer: None,
            });
            Self {
                value,
                state: NodeState::new(sample_rate),
                outputs,
            }
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE> for ConstantSource<T, BUF_SIZE> {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                type_name: None,
                name: "ConstantSource".into(),
                category: NodeCategory::Source,
                description: String::new(),
                author: String::new(),
                version: "1.0".into(),
                signal_inputs: 0,
                signal_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            }
        }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
            None
        }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
            Ok(())
        }
        fn id(&self) -> NodeId {
            NodeId(0)
        }
        fn set_id(&mut self, _id: NodeId) {}
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
        fn state(&self) -> &NodeState<T, BUF_SIZE> {
            &self.state
        }
        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
            &mut self.state
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE> for ConstantSource<T, BUF_SIZE> {
        fn generate(
            &mut self,
            _clock: &ClockTick,
            _control_inputs: &[T],
            _clock_inputs: &[ClockTick],
        ) -> ProcessResult<()> {
            let out = self.outputs[0].buffer.as_mut_array();
            for sample in out.iter_mut() {
                *sample = self.value;
            }
            Ok(())
        }
        fn num_signal_outputs(&self) -> usize {
            1
        }
    }

    // ------------------------------------------------------------------------
    // Mock: NoopProcessor — minimal processor for topology tests
    // ------------------------------------------------------------------------
    struct NoopProcessor<T: Transcendental, const BUF_SIZE: usize> {
        state: NodeState<T, BUF_SIZE>,
    }

    impl<T: Transcendental, const BUF_SIZE: usize> NoopProcessor<T, BUF_SIZE> {
        fn new(sample_rate: f32) -> Self {
            Self {
                state: NodeState::new(sample_rate),
            }
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE> for NoopProcessor<T, BUF_SIZE> {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                type_name: None,
                name: "NoopProcessor".into(),
                category: NodeCategory::Processor,
                description: String::new(),
                author: String::new(),
                version: "1.0".into(),
                signal_inputs: 0,
                signal_outputs: 0,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            }
        }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
            None
        }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
            Ok(())
        }
        fn id(&self) -> NodeId {
            NodeId(1)
        }
        fn set_id(&mut self, _id: NodeId) {}
        fn input_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
            None
        }
        fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            None
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
        fn state(&self) -> &NodeState<T, BUF_SIZE> {
            &self.state
        }
        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
            &mut self.state
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE> for NoopProcessor<T, BUF_SIZE> {
        fn process(
            &mut self,
            _clock: &ClockTick,
            _signal_inputs: &[&[T; BUF_SIZE]],
            _control_inputs: &[T],
            _clock_inputs: &[ClockTick],
            _feedback_inputs: &[&[T; BUF_SIZE]],
        ) -> ProcessResult<()> {
            Ok(())
        }
    }

    // ------------------------------------------------------------------------
    // Mock: NoopSink — minimal sink for topology tests
    // ------------------------------------------------------------------------
    struct NoopSink<T: Transcendental, const BUF_SIZE: usize> {
        state: NodeState<T, BUF_SIZE>,
    }

    impl<T: Transcendental, const BUF_SIZE: usize> NoopSink<T, BUF_SIZE> {
        fn new(sample_rate: f32) -> Self {
            Self {
                state: NodeState::new(sample_rate),
            }
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE> for NoopSink<T, BUF_SIZE> {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                type_name: None,
                name: "NoopSink".into(),
                category: NodeCategory::Sink,
                description: String::new(),
                author: String::new(),
                version: "1.0".into(),
                signal_inputs: 0,
                signal_outputs: 0,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            }
        }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
            None
        }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
            Ok(())
        }
        fn id(&self) -> NodeId {
            NodeId(2)
        }
        fn set_id(&mut self, _id: NodeId) {}
        fn input_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
            None
        }
        fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            None
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
        fn state(&self) -> &NodeState<T, BUF_SIZE> {
            &self.state
        }
        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
            &mut self.state
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Sink<T, BUF_SIZE> for NoopSink<T, BUF_SIZE> {
        fn consume(
            &mut self,
            _clock: &ClockTick,
            _signal_inputs: &[&[T; BUF_SIZE]],
            _control_inputs: &[T],
            _clock_inputs: &[ClockTick],
            _feedback_inputs: &[&[T; BUF_SIZE]],
        ) -> ProcessResult<()> {
            Ok(())
        }
    }

    // ========================================================================
    // Tests
    // ========================================================================

    #[test]
    fn test_graph_creation() {
        let graph = SignalGraph::<f32, 64>::with_sample_rate(44100.0);
        assert_eq!(graph.node_count(), 0);
    }

    #[test]
    fn test_topo_order_correct() {
        const BUF: usize = 64;
        let mut builder = GraphBuilder::<f32, BUF>::new();

        let src = builder.add_source(Box::new(ConstantSource::new(1.0, 44100.0)));
        let proc = builder.add_processor(Box::new(NoopProcessor::new(44100.0)));
        let sink = builder.add_sink(Box::new(NoopSink::new(44100.0)));

        builder.connect_signal(src, 0, proc, 0);
        builder.connect_signal(proc, 0, sink, 0);

        let graph = builder
            .build(Box::new(SystemClock::with_sample_rate(44100.0)))
            .expect("build failed");

        let order = graph.topo_order();
        let src_pos = order.iter().position(|&i| i == src).unwrap();
        let proc_pos = order.iter().position(|&i| i == proc).unwrap();
        let sink_pos = order.iter().position(|&i| i == sink).unwrap();
        assert!(src_pos < proc_pos);
        assert!(proc_pos < sink_pos);
    }

    #[test]
    fn test_cycle_detection() {
        const BUF: usize = 64;
        let mut builder = GraphBuilder::<f32, BUF>::new();

        let a = builder.add_processor(Box::new(NoopProcessor::new(44100.0)));
        let b = builder.add_processor(Box::new(NoopProcessor::new(44100.0)));

        builder.connect_signal(a, 0, b, 0);
        builder.connect_signal(b, 0, a, 0);

        let result = builder.build(Box::new(SystemClock::with_sample_rate(44100.0)));
        assert!(matches!(result, Err(BuildError::CycleDetected)));
    }

    #[test]
    fn test_source_node_create() {
        const BUF: usize = 64;
        let mut builder = GraphBuilder::<f32, BUF>::new();
        let idx = builder.add_source(Box::new(ConstantSource::new(0.5, 44100.0)));
        let graph = builder
            .build(Box::new(SystemClock::with_sample_rate(44100.0)))
            .expect("build failed");
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.topo_order(), &[idx]);
    }
}
