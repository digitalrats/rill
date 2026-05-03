use crate::registry::{NodeRegistry, RegistryError};
use rill_core::buffer::{BufferRegistry, FixedBuffer, TapeLoop};
use rill_core::math::Transcendental;
use rill_core::time::{ClockSource, ClockTick, SystemClock};
use rill_core::traits::port::Port;
use rill_core::traits::{SignalNode, NodeId, NodeParams, NodeVariant};
use std::collections::VecDeque;

// ============================================================================
// Internal routing metadata
// ============================================================================

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
    signal_edges: Vec<(usize, usize, usize, usize)>,
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
            signal_edges: Vec::new(),
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

    /// Add a Router node (N→M configurable routing, no DSP).
    pub fn add_router(
        &mut self,
        router: Box<dyn rill_core::traits::Router<T, BUF_SIZE>>,
    ) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(NodeEntry {
            node: NodeVariant::Router(router),
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
        self.signal_edges
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

        for &(from_n, from_p, to_n, to_p) in &self.signal_edges {
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

        // --- populate Port::downstream, downstream_input_ptrs, parent ---
        for &(from_n, from_p, to_n, to_p) in &self.signal_edges {
            // downstream list (serialization)
            if let Some(port) = self.nodes[from_n].node.output_port_mut(from_p) {
                port.downstream.push((to_n, to_p));
            }
            // Prepare pointers (safe: distinct indices in static DAG).
            let in_ptr: *mut Port<T, BUF_SIZE> = self.nodes[to_n]
                .node.input_port_mut(to_p)
                .map(|p| p as *mut Port<T, BUF_SIZE>)
                .unwrap_or(std::ptr::null_mut());
            let parent: *mut NodeVariant<T, BUF_SIZE> = &mut self.nodes[to_n].node;
            let out_ptr: *mut Port<T, BUF_SIZE> = self.nodes[from_n]
                .node.output_port_mut(from_p)
                .map(|p| p as *mut Port<T, BUF_SIZE>)
                .unwrap_or(std::ptr::null_mut());
            // Assign (safe: pointers were obtained without overlapping borrows).
            if !in_ptr.is_null() && !out_ptr.is_null() {
                #[allow(unsafe_code)]
                unsafe {
                    (*in_ptr).parent = parent;
                    (*out_ptr).downstream_input_ptrs.push(in_ptr);
                }
            }
        }

        // --- downstream_nodes: unique downstream node pointers ---
        for &(from_n, from_p, to_n, _) in &self.signal_edges {
            let parent: *mut NodeVariant<T, BUF_SIZE> = &mut self.nodes[to_n].node;
            if let Some(port) = self.nodes[from_n].node.output_port_mut(from_p) {
                let ptr_val = parent as usize;
                let already = port.downstream_nodes.iter().any(|&p| p as usize == ptr_val);
                if !already {
                    port.downstream_nodes.push(parent);
                }
            }
        }

        // --- upstream_buffer: zero-copy routing for 1:1 and fan-out ---
        for &(from_n, from_p, to_n, to_p) in &self.signal_edges {
            let upstream = self.nodes[from_n]
                .node.output_port(from_p)
                .map(|p| &p.buffer as *const FixedBuffer<T, BUF_SIZE>);
            if let Some(port) = self.nodes[to_n].node.input_port_mut(to_p) {
                if port.upstream_buffer.is_none() {
                    // First upstream: set zero-copy pointer
                    port.upstream_buffer = upstream;
                } else {
                    // Fan-in: copy-based fallback
                    port.upstream_buffer = None;
                }
            }
        }

        // --- enable feedback buffers on both output and input ports ---
        for &(from_n, from_p, to_n, to_p) in &self.feedback_edges {
            if let Some(port) = self.nodes[from_n].node.output_port_mut(from_p) {
                port.feedback_buffer = Some(FixedBuffer::new());
                port.feedback_downstream.push((to_n, to_p));
            }
            if let Some(port) = self.nodes[to_n].node.input_port_mut(to_p) {
                port.feedback_buffer = Some(FixedBuffer::new());
            }
        }
        // --- populate Port::feedback_ptrs on output ports ---
        for &(from_n, from_p, to_n, to_p) in &self.feedback_edges {
            let ptr = self.nodes[to_n]
                .node
                .input_port(to_p)
                .map(|p| &p.feedback_buffer as *const Option<FixedBuffer<T, BUF_SIZE>>)
                .map(|r| r as *mut Option<FixedBuffer<T, BUF_SIZE>>);
            if let Some(port) = self.nodes[from_n].node.output_port_mut(from_p) {
                if let Some(p) = ptr {
                    port.feedback_ptrs.push(p);
                }
            }
        }

        let sample_rate = clock_source.sample_rate();

        // Allocate named buffers (tape loops, etc.) from resource definitions.
        let mut buffers = BufferRegistry::new();
        for r in &self.resources {
            if r.kind == "tape" {
                if let Some(tape) = TapeLoop::<T>::new(r.capacity) {
                    buffers.register(&r.name, Box::new(tape));
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
            buffers,
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
    /// Resource metadata (name, kind, capacity) for serialization.
    pub(crate) resources: Vec<GraphResource>,
    /// Allocated buffer registry — named buffers shared between nodes.
    pub buffers: BufferRegistry<T>,
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
            buffers: BufferRegistry::new(),
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

    #[allow(dead_code)]
    pub(crate) fn node_entries(&self) -> &[NodeEntry<T, BUF_SIZE>] {
        &self.nodes
    }

    #[allow(dead_code)]
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
    use std::sync::Arc;
    use rill_core::math::Transcendental;
    use rill_core::time::ClockTick;
    use rill_core::traits::{
        algorithm::ActionContext,
        processable::{ProcessContext, Processable},
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
                id: PortId::signal_out(NodeId(0), 0),
                name: "output".into(),
                direction: PortDirection::Output,
                action: None,
                pending_command: None,
                buffer: Default::default(),
                feedback_buffer: None,
                downstream: Vec::new(),
                feedback_downstream: Vec::new(),
                feedback_ptrs: Vec::new(),
                downstream_input_ptrs: Vec::new(),
                downstream_nodes: Vec::new(),
                parent: std::ptr::null_mut(),
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

    // ========================================================================
    // Port-based propagation tests
    // ========================================================================

    /// Simple Sink that captures its first input port for inspection.
    pub struct TestSink<T: Transcendental, const BUF_SIZE: usize> {
        id: NodeId,
        state: NodeState<T, BUF_SIZE>,
        pub inputs: Vec<Port<T, BUF_SIZE>>,
        last_value: T,
    }

    impl<T: Transcendental, const BUF_SIZE: usize> TestSink<T, BUF_SIZE> {
        fn new(id: NodeId, sample_rate: f32) -> Self {
            let mut inputs = Vec::new();
            inputs.push(Port::input(id, 0, "in"));
            Self {
                id,
                state: NodeState::new(sample_rate),
                inputs,
                last_value: T::ZERO,
            }
        }
        fn last_value(&self) -> T { self.last_value }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE> for TestSink<T, BUF_SIZE> {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                type_name: None, name: "TestSink".into(), category: NodeCategory::Sink,
                description: String::new(), author: String::new(), version: "1.0".into(),
                signal_inputs: 1, signal_outputs: 0, control_inputs: 0, control_outputs: 0,
                clock_inputs: 0, clock_outputs: 0, feedback_ports: 0, parameters: vec![],
            }
        }
        fn init(&mut self, _: f32) {}
        fn reset(&mut self) { self.state.sample_pos = 0; self.state.blocks_processed = 0; }
        fn id(&self) -> NodeId { self.id } fn set_id(&mut self, id: NodeId) { self.id = id; }
        fn get_parameter(&self, _: &ParameterId) -> Option<ParamValue> { None }
        fn set_parameter(&mut self, _: &ParameterId, _: ParamValue) -> ProcessResult<()> { Ok(()) }
        fn input_port(&self, i: usize) -> Option<&Port<T, BUF_SIZE>> { self.inputs.get(i) }
        fn input_port_mut(&mut self, i: usize) -> Option<&mut Port<T, BUF_SIZE>> { self.inputs.get_mut(i) }
        fn output_port(&self, _: usize) -> Option<&Port<T, BUF_SIZE>> { None }
        fn output_port_mut(&mut self, _: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
        fn control_port(&self, _: usize) -> Option<&Port<T, BUF_SIZE>> { None }
        fn control_port_mut(&mut self, _: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
        fn num_signal_inputs(&self) -> usize { 1 } fn num_signal_outputs(&self) -> usize { 0 }
        fn state(&self) -> &NodeState<T, BUF_SIZE> { &self.state }
        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> { &mut self.state }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Sink<T, BUF_SIZE> for TestSink<T, BUF_SIZE> {
        fn consume(
            &mut self, _clock: &ClockTick,
            _signal_inputs: &[&[T; BUF_SIZE]], _control_inputs: &[T],
            _clock_inputs: &[ClockTick], _feedback_inputs: &[&[T; BUF_SIZE]],
        ) -> ProcessResult<()> {
            if let Some(port) = self.inputs.first() {
                self.last_value = port.buffer.as_array()[0];
            }
            self.state.advance();
            Ok(())
        }
    }

    /// Processor with a `multiplier` parameter. Output = input × multiplier.
    pub struct GainProcessor<T: Transcendental, const BUF_SIZE: usize> {
        id: NodeId,
        state: NodeState<T, BUF_SIZE>,
        pub inputs: Vec<Port<T, BUF_SIZE>>,
        pub outputs: Vec<Port<T, BUF_SIZE>>,
        pub multiplier: T,
    }

    impl<T: Transcendental, const BUF_SIZE: usize> GainProcessor<T, BUF_SIZE> {
        fn new(id: NodeId, sample_rate: f32, multiplier: T) -> Self {
            let mut inputs = Vec::new();
            inputs.push(Port::input(id, 0, "in"));
            let mut outputs = Vec::new();
            outputs.push(Port::output(id, 0, "out"));
            Self {
                id, state: NodeState::new(sample_rate),
                inputs, outputs, multiplier,
            }
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE> for GainProcessor<T, BUF_SIZE> {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                type_name: None, name: "GainProcessor".into(), category: NodeCategory::Processor,
                description: String::new(), author: String::new(), version: "1.0".into(),
                signal_inputs: 1, signal_outputs: 1, control_inputs: 0, control_outputs: 0,
                clock_inputs: 0, clock_outputs: 0, feedback_ports: 0, parameters: vec![],
            }
        }
        fn init(&mut self, _: f32) {}
        fn reset(&mut self) { self.state.sample_pos = 0; self.state.blocks_processed = 0; }
        fn id(&self) -> NodeId { self.id } fn set_id(&mut self, id: NodeId) { self.id = id; }
        fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
            match id.as_str() {
                "multiplier" => Some(ParamValue::Float(self.multiplier.to_f32())),
                _ => None,
            }
        }
        fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
            match id.as_str() {
                "multiplier" => {
                    if let Some(v) = value.as_f32() {
                        self.multiplier = T::from_f32(v);
                        Ok(())
                    } else { Err(rill_core::ProcessError::parameter("expected float")) }
                }
                _ => Err(rill_core::ProcessError::parameter("unknown")),
            }
        }
        fn input_port(&self, i: usize) -> Option<&Port<T, BUF_SIZE>> { self.inputs.get(i) }
        fn input_port_mut(&mut self, i: usize) -> Option<&mut Port<T, BUF_SIZE>> { self.inputs.get_mut(i) }
        fn output_port(&self, i: usize) -> Option<&Port<T, BUF_SIZE>> { self.outputs.get(i) }
        fn output_port_mut(&mut self, i: usize) -> Option<&mut Port<T, BUF_SIZE>> { self.outputs.get_mut(i) }
        fn control_port(&self, _: usize) -> Option<&Port<T, BUF_SIZE>> { None }
        fn control_port_mut(&mut self, _: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
        fn num_signal_inputs(&self) -> usize { 1 } fn num_signal_outputs(&self) -> usize { 1 }
        fn state(&self) -> &NodeState<T, BUF_SIZE> { &self.state }
        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> { &mut self.state }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE> for GainProcessor<T, BUF_SIZE> {
        fn process(
            &mut self, _clock: &ClockTick,
            _signal_inputs: &[&[T; BUF_SIZE]], _control_inputs: &[T],
            _clock_inputs: &[ClockTick], _feedback_inputs: &[&[T; BUF_SIZE]],
        ) -> ProcessResult<()> {
            let inp = *self.inputs[0].buffer.as_array();
            let out = self.outputs[0].buffer.as_mut_array();
            for i in 0..BUF_SIZE {
                out[i] = inp[i] * self.multiplier;
            }
            self.state.advance();
            Ok(())
        }
        fn latency(&self) -> usize { 0 }
    }

    // ── Test: Source → Sink via GraphBuilder ────────────────────────

    #[test]
    fn test_graph_source_to_sink() {
        const BUF: usize = 64;
        let mut builder = GraphBuilder::<f32, BUF>::new();
        let src = builder.add_source(Box::new(ConstantSource::new(42.0, 44100.0)));
        let snk = builder.add_sink(Box::new(TestSink::<f32, BUF>::new(NodeId(1), 44100.0)));
        builder.connect_signal(src, 0, snk, 0);
        let graph = builder.build(Box::new(SystemClock::with_sample_rate(44100.0))).unwrap();

        let (mut nodes, topo, _) = graph.into_parts();
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        // Process source
        let mut ctx = ProcessContext { clock: &tick };
        let _ = nodes[topo[0]].process_block(&mut ctx);

        // Propagate through builder-wired connections
        let action_ctx = rill_core::traits::algorithm::ActionContext::new(&tick);
        let out_port = nodes[topo[0]].output_port(0).unwrap();
        out_port.propagate(out_port.buffer(), &action_ctx).unwrap();

        let sink_val = nodes[topo[1]].input_port(0).unwrap().buffer.as_array()[0];
        assert_eq!(sink_val, 42.0, "sink should receive source value");
    }

    // ── Test: Source → Processor → Sink via GraphBuilder ────────────

    #[test]
    fn test_graph_source_proc_sink() {
        const BUF: usize = 64;
        let mut builder = GraphBuilder::<f32, BUF>::new();
        let src = builder.add_source(Box::new(ConstantSource::new(10.0, 44100.0)));
        let proc = builder.add_processor(Box::new(
            GainProcessor::<f32, BUF>::new(NodeId(1), 44100.0, 3.0)));
        let snk = builder.add_sink(Box::new(
            TestSink::<f32, BUF>::new(NodeId(2), 44100.0)));
        builder.connect_signal(src, 0, proc, 0);
        builder.connect_signal(proc, 0, snk, 0);
        let graph = builder.build(Box::new(SystemClock::with_sample_rate(44100.0))).unwrap();

        let (mut nodes, topo, _) = graph.into_parts();
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        // Process source
        let mut ctx = ProcessContext { clock: &tick };
        let _ = nodes[topo[0]].process_block(&mut ctx);

        // Propagate — should traverse source→processor→sink recursively
        let action_ctx = rill_core::traits::algorithm::ActionContext::new(&tick);
        let out_port = nodes[topo[0]].output_port(0).unwrap();
        out_port.propagate(out_port.buffer(), &action_ctx).unwrap();

        let sink_val = nodes[topo[2]].input_port(0).unwrap().buffer.as_array()[0];
        assert!((sink_val - 30.0).abs() < 1e-6,
            "source(10)×gain(3)=30, got {}", sink_val);
    }

    //     ── Test: Command queue drain ───────────────────────────────────

    #[test]
    fn test_command_queue_drain() {
        use rill_core::queues::MpscQueue;
        use rill_patchbay::control::ParameterCommand;

        const BUF: usize = 64;
        let queue: Arc<MpscQueue<ParameterCommand>> = Arc::new(MpscQueue::new());

        let mut builder = GraphBuilder::<f32, BUF>::new();
        builder.add_processor(Box::new(
            GainProcessor::<f32, BUF>::new(NodeId(0), 44100.0, 2.0)));
        let graph = builder.build(Box::new(SystemClock::with_sample_rate(44100.0))).unwrap();
        let (mut nodes, _, _) = graph.into_parts();

        queue.push(ParameterCommand::new(NodeId(0), "multiplier", 5.0));

        while let Some(cmd) = queue.pop() {
            let idx = cmd.node_id.inner() as usize;
            let pid = ParameterId::new(&cmd.param).unwrap();
            nodes[idx].set_parameter(&pid, ParamValue::Float(cmd.value)).unwrap();
        }

        let pid = ParameterId::new("multiplier").unwrap();
        let val = nodes[0].get_parameter(&pid).unwrap().as_f32().unwrap();
        assert!((val - 5.0).abs() < 1e-6, "multiplier should be 5.0, got {}", val);
    }

    // ── Test: Queue + propagate ─────────────────────────────────────

    #[test]
    fn test_command_then_propagate() {
        use rill_core::queues::MpscQueue;
        use rill_patchbay::control::ParameterCommand;

        const BUF: usize = 64;
        let queue: Arc<MpscQueue<ParameterCommand>> = Arc::new(MpscQueue::new());

        let mut builder = GraphBuilder::<f32, BUF>::new();
        let src = builder.add_source(Box::new(ConstantSource::new(7.0, 44100.0)));
        let proc = builder.add_processor(Box::new(
            GainProcessor::<f32, BUF>::new(NodeId(1), 44100.0, 2.0)));
        let snk = builder.add_sink(Box::new(
            TestSink::<f32, BUF>::new(NodeId(2), 44100.0)));
        builder.connect_signal(src, 0, proc, 0);
        builder.connect_signal(proc, 0, snk, 0);
        let graph = builder.build(Box::new(SystemClock::with_sample_rate(44100.0))).unwrap();
        let (mut nodes, topo, _) = graph.into_parts();
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        // Push command and drain
        queue.push(ParameterCommand::new(NodeId(1), "multiplier", 4.0));
        while let Some(cmd) = queue.pop() {
            let idx = cmd.node_id.inner() as usize;
            let pid = ParameterId::new(&cmd.param).unwrap();
            nodes[idx].set_parameter(&pid, ParamValue::Float(cmd.value)).unwrap();
        }

        // Verify multiplier changed
        let pid = ParameterId::new("multiplier").unwrap();
        let val = nodes[1].get_parameter(&pid).unwrap().as_f32().unwrap();
        assert!((val - 4.0).abs() < 1e-6);

        // Process + propagate
        let mut ctx = ProcessContext { clock: &tick };
        let _ = nodes[topo[0]].process_block(&mut ctx);
        let action_ctx = rill_core::traits::algorithm::ActionContext::new(&tick);
        let out_port = nodes[topo[0]].output_port(0).unwrap();
        out_port.propagate(out_port.buffer(), &action_ctx).unwrap();

        let sink_val = nodes[topo[2]].input_port(0).unwrap().buffer.as_array()[0];
        assert!((sink_val - 28.0).abs() < 1e-6,
            "source(7)×gain(4)=28, got {}", sink_val);
    }

    // ── Test: Feedback propagation ──────────────────────────────────

    #[test]
    fn test_feedback_propagation() {
        use rill_core::traits::algorithm::ActionContext;

        const BUF: usize = 64;
        let mut builder = GraphBuilder::<f32, BUF>::new();
        let src = builder.add_source(Box::new(ConstantSource::new(1.0, 44100.0)));
        let proc = builder.add_processor(Box::new(
            GainProcessor::<f32, BUF>::new(NodeId(1), 44100.0, 2.0)));
        let snk = builder.add_sink(Box::new(
            TestSink::<f32, BUF>::new(NodeId(2), 44100.0)));
        // Signal path: source → processor → sink
        builder.connect_signal(src, 0, proc, 0);
        builder.connect_signal(proc, 0, snk, 0);
        // Feedback: processor output → processor input
        builder.connect_feedback(proc, 0, proc, 0);
        let graph = builder.build(Box::new(SystemClock::with_sample_rate(44100.0))).unwrap();
        let (mut nodes, topo, _) = graph.into_parts();

        // ── Block 1: no feedback yet ──
        let tick1 = ClockTick::new(0, BUF as u32, 44100.0);
        let mut ctx = ProcessContext { clock: &tick1 };
        let _ = nodes[topo[0]].process_block(&mut ctx);  // source generates
        let ctx1 = ActionContext::new(&tick1);
        let out_port = nodes[topo[0]].output_port(0).unwrap();
        out_port.propagate(out_port.buffer(), &ctx1).unwrap();
        let block1 = nodes[topo[2]].input_port(0).unwrap().buffer.as_array()[0];
        assert!((block1 - 2.0).abs() < 1e-6, "block1: 1.0×2.0=2.0, got {}", block1);

        // ── Block 2: feedback from block1 should be mixed in ──
        let tick2 = ClockTick::new(BUF as u64, BUF as u32, 44100.0);
        let mut ctx = ProcessContext { clock: &tick2 };
        let _ = nodes[topo[0]].process_block(&mut ctx);  // source generates again
        let ctx2 = ActionContext::new(&tick2);
        let out_port = nodes[topo[0]].output_port(0).unwrap();
        out_port.propagate(out_port.buffer(), &ctx2).unwrap();
        let block2 = nodes[topo[2]].input_port(0).unwrap().buffer.as_array()[0];
        // pre_process: input = 1.0 (source) + 2.0 (feedback from block1) = 3.0
        // process: 3.0 × 2.0 = 6.0
        assert!((block2 - 6.0).abs() < 1e-6, "block2: (1+2)×2=6.0, got {}", block2);
    }

    // ── Test: drain_fn pattern (as used by AudioInput) ──────────────

    #[test]
    fn test_drain_fn_before_propagate() {
        use rill_core::queues::MpscQueue;
        use rill_patchbay::control::ParameterCommand;

        const BUF: usize = 64;
        let queue: Arc<MpscQueue<ParameterCommand>> = Arc::new(MpscQueue::new());

        let mut builder = GraphBuilder::<f32, BUF>::new();
        let src = builder.add_source(Box::new(ConstantSource::new(5.0, 44100.0)));
        let proc = builder.add_processor(Box::new(
            GainProcessor::<f32, BUF>::new(NodeId(1), 44100.0, 1.0)));
        let snk = builder.add_sink(Box::new(
            TestSink::<f32, BUF>::new(NodeId(2), 44100.0)));
        builder.connect_signal(src, 0, proc, 0);
        builder.connect_signal(proc, 0, snk, 0);
        let graph = builder.build(Box::new(SystemClock::with_sample_rate(44100.0))).unwrap();
        let (mut nodes, topo, _) = graph.into_parts();
        let nodes_ptr: *mut [NodeVariant<f32, BUF>] = &mut *nodes;

        // drain_fn — exactly as AudioInput creates it
        let drain_fn: Box<dyn Fn(&mut [NodeVariant<f32, BUF>])> = {
            let q = queue.clone();
            Box::new(move |nd: &mut [NodeVariant<f32, BUF>]| {
                while let Some(cmd) = q.pop() {
                    let idx = cmd.node_id.inner() as usize;
                    if idx < nd.len() {
                        if let Ok(pid) = ParameterId::new(&cmd.param) {
                            let _ = nd[idx].set_parameter(&pid, ParamValue::Float(cmd.value));
                        }
                    }
                }
            })
        };

        // Push command BEFORE processing
        queue.push(ParameterCommand::new(NodeId(1), "multiplier", 3.0));

        // Processing cycle exactly as AudioInput callback does:
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        // Step 1: drain
        #[allow(unsafe_code)]
        unsafe { drain_fn(&mut *nodes_ptr); }

        // Verify parameter applied
        let pid = ParameterId::new("multiplier").unwrap();
        #[allow(unsafe_code)]
        let val = unsafe { (*nodes_ptr)[1].get_parameter(&pid).unwrap().as_f32().unwrap() };
        assert!((val - 3.0).abs() < 1e-6, "multiplier should be 3.0, got {}", val);

        // Step 2: source generate
        let mut ctx = ProcessContext { clock: &tick };
        #[allow(unsafe_code)]
        unsafe { (*nodes_ptr)[topo[0]].process_block(&mut ctx).unwrap(); }

        // Step 3: propagate
        let action_ctx = rill_core::traits::algorithm::ActionContext::new(&tick);
        #[allow(unsafe_code)]
        let out_port = unsafe { (*nodes_ptr)[topo[0]].output_port(0).unwrap() };
        out_port.propagate(out_port.buffer(), &action_ctx).unwrap();

        // Verify: source(5) × gain(3) = 15
        #[allow(unsafe_code)]
        let sink_val = unsafe { (*nodes_ptr)[topo[2]].input_port(0).unwrap().buffer.as_array()[0] };
        assert!((sink_val - 15.0).abs() < 1e-6,
            "source(5)×gain(3)=15, got {}", sink_val);
    }
}
