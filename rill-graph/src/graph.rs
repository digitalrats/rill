use crate::backend_factory::BackendFactory;
use crate::factory::{NodeFactory, RegistryError};
use rill_core::buffer::{Buffer, BufferRegistry, FixedBuffer, TapeLoop};
use rill_core::math::Transcendental;
use rill_core::queues::{MpscQueue, SetParameter};
use rill_core::time::ClockTick;
use rill_core::traits::active::GraphHandle;
use rill_core::traits::port::Port;
use rill_core::traits::ParamValue;
use rill_core::traits::{Node, NodeId, NodeVariant, Params};
use rill_core_actor::{ActorCell, ActorRef};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// ============================================================================
// Internal routing metadata
// ============================================================================

// ============================================================================
// Build Errors
// ============================================================================

/// Errors that can occur during graph construction.
#[derive(Debug, Clone)]
pub enum BuildError {
    /// A cycle was detected in the signal edge graph.
    CycleDetected,
    /// Backend creation failed.
    Backend(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CycleDetected => write!(f, "graph cycle detected"),
            Self::Backend(msg) => write!(f, "backend error: {msg}"),
        }
    }
}

// ============================================================================
// Graph Builder
// ============================================================================

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
///
/// # Node factory
///
/// The builder holds an [`Arc<NodeFactory>`] for constructing nodes by
/// type name, provided at construction via [`GraphBuilder::new`].
pub struct GraphBuilder<T: Transcendental, const BUF_SIZE: usize> {
    nodes: Vec<NodeEntry<T, BUF_SIZE>>,
    signal_edges: Vec<(usize, usize, usize, usize)>,
    control_edges: Vec<(usize, usize, usize, usize)>,
    clock_edges: Vec<(usize, usize, usize, usize)>,
    feedback_edges: Vec<(usize, usize, usize, usize)>,
    resources: Vec<GraphResource>,
    /// Shared node factory (required, from Runtime).
    factory: Arc<NodeFactory<T, BUF_SIZE>>,
    /// Shared backend factory (required, from Runtime).
    backends: Arc<BackendFactory<T>>,
    /// Optional backend configuration (set via [`with_backend`](Self::with_backend)).
    backend_config: Option<BackendConfig>,
}

struct BackendConfig {
    name: String,
    params: HashMap<String, ParamValue>,
}

impl<T: Transcendental, const BUF_SIZE: usize> GraphBuilder<T, BUF_SIZE> {
    /// Create a new empty graph builder without a node factory.
    pub fn new(factory: Arc<NodeFactory<T, BUF_SIZE>>, backends: Arc<BackendFactory<T>>) -> Self {
        Self {
            nodes: Vec::new(),
            signal_edges: Vec::new(),
            control_edges: Vec::new(),
            clock_edges: Vec::new(),
            feedback_edges: Vec::new(),
            resources: Vec::new(),
            factory,
            backends,
            backend_config: None,
        }
    }

    /// Add a node by type name using the internal factory.
    ///
    /// The type must have been registered in the factory before calling
    /// this method.
    ///
    /// Returns the index of the newly added node.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError`] if no factory is set or the type name
    /// is not registered.
    pub fn add_node(&mut self, type_name: &str, params: &Params) -> Result<usize, RegistryError> {
        let id = NodeId(self.nodes.len() as u32);
        self.add_node_with_id(type_name, params, id)
    }

    /// Add a node with an explicit [`NodeId`].
    ///
    /// Like [`add_node`](Self::add_node) but uses the provided `id`.
    /// Important for serialization where external references depend on
    /// exact IDs.
    pub fn add_node_with_id(
        &mut self,
        type_name: &str,
        params: &Params,
        id: NodeId,
    ) -> Result<usize, RegistryError> {
        let node = self.factory.construct(type_name, id, params)?;
        let idx = self.nodes.len();
        self.nodes.push(NodeEntry { node });
        Ok(idx)
    }

    /// Register a named resource (tape loop, buffer, etc.).
    pub fn add_resource(&mut self, resource: GraphResource) {
        self.resources.push(resource);
    }

    /// Number of nodes added to the builder so far.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Access the shared backend factory (for building with external configs).
    pub fn backend_factory(&self) -> &Arc<BackendFactory<T>> {
        &self.backends
    }

    /// Add a source node and return its index.
    pub fn add_source(&mut self, source: Box<dyn rill_core::traits::Source<T, BUF_SIZE>>) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(NodeEntry {
            node: NodeVariant::Source(source),
        });
        idx
    }

    /// Add a processor node and return its index.
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

    /// Add a sink node and return its index.
    pub fn add_sink(&mut self, sink: Box<dyn rill_core::traits::Sink<T, BUF_SIZE>>) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(NodeEntry {
            node: NodeVariant::Sink(sink),
        });
        idx
    }

    /// Add a Router node (N→M configurable routing, no DSP).
    pub fn add_router(&mut self, router: Box<dyn rill_core::traits::Router<T, BUF_SIZE>>) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(NodeEntry {
            node: NodeVariant::Router(router),
        });
        idx
    }

    /// Connect signal ports (audio data).
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

    /// Connect control ports (modulation values).
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

    /// Connect clock ports (timing events).
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

    /// Connect feedback ports (delay lines, state carryover).
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

    /// Configure an audio backend for this builder.
    ///
    /// When set, [`build`](Self::build) looks for a driver node in the graph
    /// and auto-starts it with a command queue and the given audio backend.
    /// Without this method, the graph is purely structural (no audio I/O,
    /// no command queue).
    /// Configure an audio backend for this builder.
    ///
    /// Params are passed blindly to the backend factory — keys like
    /// `"sample_rate"`, `"buffer_size"`, `"channels"` are interpreted
    /// by each backend constructor.
    pub fn with_backend(mut self, backend_name: &str, params: HashMap<String, ParamValue>) -> Self {
        self.backend_config = Some(BackendConfig {
            name: backend_name.to_string(),
            params,
        });
        self
    }

    /// Build the graph.
    ///
    /// If [`with_backend`](Self::with_backend) was called before, the builder
    /// auto-starts a driver node with a command queue and the configured
    /// audio backend. Otherwise the graph is purely structural (no audio I/O,
    /// no command queue).
    pub fn build(mut self) -> Result<Graph<T, BUF_SIZE>, BuildError> {
        let (backend_name, params) = match self.backend_config.as_ref() {
            Some(cfg) => (Some(cfg.name.as_str()), &cfg.params),
            None => (None, &HashMap::new()),
        };
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
                .node
                .input_port_mut(to_p)
                .map(|p| p as *mut Port<T, BUF_SIZE>)
                .unwrap_or(std::ptr::null_mut());
            let parent: *mut NodeVariant<T, BUF_SIZE> = &mut self.nodes[to_n].node;
            let out_ptr: *mut Port<T, BUF_SIZE> = self.nodes[from_n]
                .node
                .output_port_mut(from_p)
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
                .node
                .output_port(from_p)
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

        let sr = params
            .get("sample_rate")
            .and_then(|v| v.as_i32())
            .unwrap_or(44100) as f32;

        // Allocate named buffers (tape loops, etc.) from resource definitions.
        let mut buffers = BufferRegistry::new();
        for r in &self.resources {
            if r.kind == "tape" {
                if let Some(tape) = TapeLoop::<T>::new(r.capacity) {
                    buffers.register(&r.name, Box::new(tape));
                }
            }
        }

        // Resolve resources — each node that needs a shared buffer
        // (e.g. WriteHead, ReadHead) looks it up by name and caches the
        // pointer.  This is a single‑threaded, one‑time setup step.
        for entry in &mut self.nodes {
            entry.node.resolve_resources(&buffers);
        }

        // Resolve audio backend pointer for I/O nodes.
        let backend_box = if let Some(name) = backend_name {
            let b = self
                .backends
                .create(name, params)
                .map_err(BuildError::Backend)?;
            let ptr: *mut dyn rill_core::io::IoBackend<T> = &*b
                as *const dyn rill_core::io::IoBackend<T>
                as *mut dyn rill_core::io::IoBackend<T>;
            for entry in &mut self.nodes {
                entry.node.resolve_backend(ptr);
            }
            Some(b)
        } else {
            None
        };

        let mut nodes: Vec<NodeVariant<T, BUF_SIZE>> =
            self.nodes.into_iter().map(|e| e.node).collect();

        // Auto-start driver node (registers process callback on backend).
        let cmd_queue = Arc::new(MpscQueue::<SetParameter>::with_capacity(64));
        let have_queue = if let Some(ref _backend) = backend_box {
            let driver_idx = nodes
                .iter()
                .position(|n| {
                    let name = n.metadata().name;
                    name == "AudioInput" || name == "Input"
                })
                .or_else(|| {
                    nodes.iter().position(|n| {
                        let name = n.metadata().name;
                        name == "AudioOutput" || name == "Output"
                    })
                });
            if let Some(driver_idx) = driver_idx {
                let nodes_ptr = nodes.as_mut_ptr();
                let len = nodes.len();
                let source_idx = topo[0];
                let queue_ptr: *const MpscQueue<SetParameter> = Arc::as_ptr(&cmd_queue);
                let handle = GraphHandle {
                    nodes: nodes_ptr as *mut u8,
                    len,
                    source_idx,
                    sample_rate: sr,
                    queue: queue_ptr,
                };
                nodes[driver_idx].start(handle);
                true
            } else {
                false
            }
        } else {
            false
        };
        let command_queue = if have_queue { Some(cmd_queue) } else { None };

        let owned_buffers = buffers.into_inner();

        let allocated = self.resources.clone();

        Ok(Graph {
            nodes,
            topo_order: topo,
            resources: allocated,
            current_tick: ClockTick::new(0, BUF_SIZE as u32, sr),
            buffers: owned_buffers,
            backend: backend_box,
            command_queue,
        })
    }
}

// ============================================================================
// Graph (Static DAG)
// ============================================================================

/// Immutable signal graph with static DAG topology.
///
/// Once built the graph cannot be modified. The graph owns no processing
/// logic — it is a pure topology description. Processing is driven by
/// port-level methods (`pre_process`, `snapshot_feedback`, `propagate`)
/// called from external code (e.g. a real-time signal callback or an
/// offline renderer).
pub struct Graph<T: Transcendental, const BUF_SIZE: usize> {
    nodes: Vec<NodeVariant<T, BUF_SIZE>>,
    topo_order: Vec<usize>,
    current_tick: ClockTick,
    /// Resource metadata (name, kind, capacity) for serialization.
    pub(crate) resources: Vec<GraphResource>,
    /// Named buffers (tape loops, etc.) shared between nodes.
    #[allow(dead_code)]
    buffers: Vec<Box<dyn Buffer<T> + Send>>,
    /// Shared audio backend (alive for the graph's lifetime).
    backend: Option<Box<dyn rill_core::io::IoBackend<T>>>,
    /// Command queue for sending parameters from control to audio thread.
    command_queue: Option<Arc<MpscQueue<SetParameter>>>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Graph<T, BUF_SIZE> {
    // ========================================================================
    // Accessors
    // ========================================================================

    /// Borrow the node array.
    pub fn nodes(&self) -> &[NodeVariant<T, BUF_SIZE>] {
        &self.nodes
    }

    /// Mutably borrow the node array.
    pub fn nodes_mut(&mut self) -> &mut [NodeVariant<T, BUF_SIZE>] {
        &mut self.nodes
    }

    /// Return the current clock tick.
    pub fn current_tick(&self) -> ClockTick {
        self.current_tick
    }

    /// Return the number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Return the topological ordering of node indices.
    pub fn topo_order(&self) -> &[usize] {
        &self.topo_order
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

    /// Run the audio backend until `running` becomes false.
    ///
    /// For blocking backends (ALSA, PipeWire) this blocks inside
    /// `backend.run()`. For non-blocking backends (CPAL, JACK) it
    /// parks after setup. An external signal must unpark the thread
    /// after setting `running` to false.
    pub fn run(&self, running: Arc<AtomicBool>) -> Result<(), String> {
        if let Some(ref backend) = self.backend {
            backend.run(running.clone())?;
            while running.load(Ordering::Acquire) {
                std::thread::park();
            }
            backend.stop()
        } else {
            Ok(())
        }
    }

    /// Obtain an [`ActorRef`] for sending commands to this graph.
    ///
    /// The returned handle holds a weak reference — when the `Graph` is
    /// dropped, all subsequent `send` calls route to dead letters.
    /// Returns `None` if no audio backend was configured (no queue created).
    pub fn handle(&self) -> Option<ActorRef<SetParameter>> {
        let mailbox = self.command_queue.as_ref()?;
        Some(ActorRef::new(mailbox))
    }

    /// Consume the graph and return its owned parts (test only).
    #[cfg(test)]
    pub fn into_parts(
        self,
    ) -> (
        Vec<NodeVariant<T, BUF_SIZE>>,
        Vec<usize>,
        ClockTick,
        Vec<Box<dyn Buffer<T> + Send>>,
    ) {
        let Self {
            nodes,
            topo_order,
            current_tick,
            resources: _,
            buffers,
            backend: _,
            command_queue: _,
        } = self;
        (nodes, topo_order, current_tick, buffers)
    }
}

// ============================================================================
// ActorCell implementation
// ============================================================================

impl<T: Transcendental, const BUF_SIZE: usize> ActorCell for Graph<T, BUF_SIZE> {
    type Msg = SetParameter;

    /// Process a single parameter command by writing to the target node.
    fn receive(&mut self, msg: SetParameter) {
        let idx = msg.port.node_id().inner() as usize;
        if idx < self.nodes.len() {
            let _ = self.nodes[idx].set_parameter(&msg.parameter, msg.value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::math::Transcendental;
    use rill_core::time::ClockTick;
    use rill_core::traits::active::ActiveNode;
    use rill_core::traits::algorithm::ActionContext;
    use rill_core::traits::processable::{ProcessContext, Processable};
    use rill_core::traits::{
        Node, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue, ParameterId, Port,
        PortDirection, PortId, ProcessResult, Processor, Sink, Source,
    };
    use std::sync::Arc;

    /// Create a test builder with empty factories.
    fn test_builder<const B: usize>() -> GraphBuilder<f32, B> {
        GraphBuilder::new(
            Arc::new(NodeFactory::new()),
            Arc::new(BackendFactory::new()),
        )
    }

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
                upstream_buffer: None,
            });
            Self {
                value,
                state: NodeState::new(sample_rate),
                outputs,
            }
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for ConstantSource<T, BUF_SIZE> {
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

    impl<const BUF_SIZE: usize> ActiveNode for ConstantSource<f32, BUF_SIZE> {
        fn start(&mut self, handle: GraphHandle) {
            #[allow(unsafe_code)]
            unsafe {
                let nodes = std::slice::from_raw_parts_mut(
                    handle.nodes as *mut NodeVariant<f32, BUF_SIZE>,
                    handle.len,
                );
                let idx = handle.source_idx;
                let tick = ClockTick::new(0, BUF_SIZE as u32, handle.sample_rate);
                let mut ctx = ProcessContext { clock: &tick };
                let _ = nodes[idx].process_block(&mut ctx);
                let action_ctx = ActionContext::new(&tick);
                for po in 0..nodes[idx].num_signal_outputs() {
                    if let Some(port) = nodes[idx].output_port(po) {
                        let _ = port.propagate(port.buffer(), &action_ctx);
                    }
                }
            }
        }
        fn stop(&mut self) {}
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

    impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for NoopProcessor<T, BUF_SIZE> {
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

    impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE>
        for NoopProcessor<T, BUF_SIZE>
    {
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

    impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for NoopSink<T, BUF_SIZE> {
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
    fn test_topo_order_correct() {
        const BUF: usize = 64;
        let mut builder = test_builder::<BUF>();

        let src = builder.add_source(Box::new(ConstantSource::new(1.0, 44100.0)));
        let proc = builder.add_processor(Box::new(NoopProcessor::new(44100.0)));
        let sink = builder.add_sink(Box::new(NoopSink::new(44100.0)));

        builder.connect_signal(src, 0, proc, 0);
        builder.connect_signal(proc, 0, sink, 0);

        let graph = builder.build().expect("build failed");

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
        let mut builder = test_builder::<BUF>();

        let a = builder.add_processor(Box::new(NoopProcessor::new(44100.0)));
        let b = builder.add_processor(Box::new(NoopProcessor::new(44100.0)));

        builder.connect_signal(a, 0, b, 0);
        builder.connect_signal(b, 0, a, 0);

        let result = builder.build();
        assert!(matches!(result, Err(BuildError::CycleDetected)));
    }

    #[test]
    fn test_source_node_create() {
        const BUF: usize = 64;
        let mut builder = test_builder::<BUF>();
        let idx = builder.add_source(Box::new(ConstantSource::new(0.5, 44100.0)));
        let graph = builder.build().expect("build failed");
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
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for TestSink<T, BUF_SIZE> {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                type_name: None,
                name: "TestSink".into(),
                category: NodeCategory::Sink,
                description: String::new(),
                author: String::new(),
                version: "1.0".into(),
                signal_inputs: 1,
                signal_outputs: 0,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            }
        }
        fn init(&mut self, _: f32) {}
        fn reset(&mut self) {
            self.state.sample_pos = 0;
            self.state.blocks_processed = 0;
        }
        fn id(&self) -> NodeId {
            self.id
        }
        fn set_id(&mut self, id: NodeId) {
            self.id = id;
        }
        fn get_parameter(&self, _: &ParameterId) -> Option<ParamValue> {
            None
        }
        fn set_parameter(&mut self, _: &ParameterId, _: ParamValue) -> ProcessResult<()> {
            Ok(())
        }
        fn input_port(&self, i: usize) -> Option<&Port<T, BUF_SIZE>> {
            self.inputs.get(i)
        }
        fn input_port_mut(&mut self, i: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            self.inputs.get_mut(i)
        }
        fn output_port(&self, _: usize) -> Option<&Port<T, BUF_SIZE>> {
            None
        }
        fn output_port_mut(&mut self, _: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            None
        }
        fn control_port(&self, _: usize) -> Option<&Port<T, BUF_SIZE>> {
            None
        }
        fn control_port_mut(&mut self, _: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            None
        }
        fn num_signal_inputs(&self) -> usize {
            1
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

    impl<T: Transcendental, const BUF_SIZE: usize> Sink<T, BUF_SIZE> for TestSink<T, BUF_SIZE> {
        fn consume(
            &mut self,
            _clock: &ClockTick,
            _signal_inputs: &[&[T; BUF_SIZE]],
            _control_inputs: &[T],
            _clock_inputs: &[ClockTick],
            _feedback_inputs: &[&[T; BUF_SIZE]],
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
                id,
                state: NodeState::new(sample_rate),
                inputs,
                outputs,
                multiplier,
            }
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for GainProcessor<T, BUF_SIZE> {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                type_name: None,
                name: "GainProcessor".into(),
                category: NodeCategory::Processor,
                description: String::new(),
                author: String::new(),
                version: "1.0".into(),
                signal_inputs: 1,
                signal_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            }
        }
        fn init(&mut self, _: f32) {}
        fn reset(&mut self) {
            self.state.sample_pos = 0;
            self.state.blocks_processed = 0;
        }
        fn id(&self) -> NodeId {
            self.id
        }
        fn set_id(&mut self, id: NodeId) {
            self.id = id;
        }
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
                    } else {
                        Err(rill_core::ProcessError::parameter("expected float"))
                    }
                }
                _ => Err(rill_core::ProcessError::parameter("unknown")),
            }
        }
        fn input_port(&self, i: usize) -> Option<&Port<T, BUF_SIZE>> {
            self.inputs.get(i)
        }
        fn input_port_mut(&mut self, i: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            self.inputs.get_mut(i)
        }
        fn output_port(&self, i: usize) -> Option<&Port<T, BUF_SIZE>> {
            self.outputs.get(i)
        }
        fn output_port_mut(&mut self, i: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            self.outputs.get_mut(i)
        }
        fn control_port(&self, _: usize) -> Option<&Port<T, BUF_SIZE>> {
            None
        }
        fn control_port_mut(&mut self, _: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            None
        }
        fn num_signal_inputs(&self) -> usize {
            1
        }
        fn num_signal_outputs(&self) -> usize {
            1
        }
        fn state(&self) -> &NodeState<T, BUF_SIZE> {
            &self.state
        }
        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
            &mut self.state
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE>
        for GainProcessor<T, BUF_SIZE>
    {
        fn process(
            &mut self,
            _clock: &ClockTick,
            _signal_inputs: &[&[T; BUF_SIZE]],
            _control_inputs: &[T],
            _clock_inputs: &[ClockTick],
            _feedback_inputs: &[&[T; BUF_SIZE]],
        ) -> ProcessResult<()> {
            let inp = *self.inputs[0].buffer.as_array();
            let out = self.outputs[0].buffer.as_mut_array();
            for i in 0..BUF_SIZE {
                out[i] = inp[i] * self.multiplier;
            }
            self.state.advance();
            Ok(())
        }
        fn latency(&self) -> usize {
            0
        }
    }

    // ── Test: Source → Sink via GraphBuilder ────────────────────────

    #[test]
    fn test_graph_source_to_sink() {
        use rill_core::traits::algorithm::ActionContext;
        use rill_core::traits::processable::{ProcessContext, Processable};
        const BUF: usize = 64;
        let mut builder = test_builder::<BUF>();
        let src = builder.add_source(Box::new(ConstantSource::new(42.0, 44100.0)));
        let snk = builder.add_sink(Box::new(TestSink::<f32, BUF>::new(NodeId(1), 44100.0)));
        builder.connect_signal(src, 0, snk, 0);
        let graph = builder.build().unwrap();
        let (mut nodes, topo, _, _bufs) = graph.into_parts();
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        let mut ctx = ProcessContext { clock: &tick };
        let _ = nodes[topo[0]].process_block(&mut ctx);
        let action_ctx = ActionContext::new(&tick);
        let out_port = nodes[topo[0]].output_port(0).unwrap();
        out_port.propagate(out_port.buffer(), &action_ctx).unwrap();

        let sink_val = nodes[topo[1]].input_port(0).unwrap().buffer.as_array()[0];
        assert_eq!(sink_val, 42.0, "sink should receive source value");
    }

    // ── Test: Source → Processor → Sink via GraphBuilder ────────────

    #[test]
    fn test_graph_source_proc_sink() {
        use rill_core::traits::algorithm::ActionContext;
        use rill_core::traits::processable::{ProcessContext, Processable};
        const BUF: usize = 64;
        let mut builder = test_builder::<BUF>();
        let src = builder.add_source(Box::new(ConstantSource::new(10.0, 44100.0)));
        let proc = builder.add_processor(Box::new(GainProcessor::<f32, BUF>::new(
            NodeId(1),
            44100.0,
            3.0,
        )));
        let snk = builder.add_sink(Box::new(TestSink::<f32, BUF>::new(NodeId(2), 44100.0)));
        builder.connect_signal(src, 0, proc, 0);
        builder.connect_signal(proc, 0, snk, 0);
        let graph = builder.build().unwrap();
        let (mut nodes, topo, _, _bufs) = graph.into_parts();
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        let mut ctx = ProcessContext { clock: &tick };
        let _ = nodes[topo[0]].process_block(&mut ctx);
        let action_ctx = ActionContext::new(&tick);
        let out_port = nodes[topo[0]].output_port(0).unwrap();
        out_port.propagate(out_port.buffer(), &action_ctx).unwrap();

        let sink_val = nodes[topo[2]].input_port(0).unwrap().buffer.as_array()[0];
        assert!(
            (sink_val - 30.0).abs() < 1e-6,
            "source(10)×gain(3)=30, got {}",
            sink_val
        );
    }

    // ── Test: Command queue drain ───────────────────────────────────

    #[test]
    fn test_command_queue_drain() {
        use rill_core::queues::{MpscQueue, SetParameter, SignalOrigin};
        use rill_core::traits::PortId;

        const BUF: usize = 64;
        let queue: Arc<MpscQueue<SetParameter>> = Arc::new(MpscQueue::new());

        let mut builder = test_builder::<BUF>();
        builder.add_processor(Box::new(GainProcessor::<f32, BUF>::new(
            NodeId(0),
            44100.0,
            2.0,
        )));
        let graph = builder.build().unwrap();
        let (mut nodes, _, _, _bufs) = graph.into_parts();

        let _ = queue.push(SetParameter::new(
            PortId::control_in(NodeId(0), 0),
            ParameterId::new("multiplier").unwrap(),
            ParamValue::Float(5.0),
            SignalOrigin::Manual,
        ));

        while let Some(cmd) = queue.pop() {
            let idx = cmd.port.node_id().inner() as usize;
            let pid = cmd.parameter.clone();
            let _ = nodes[idx].set_parameter(&pid, cmd.value.clone());
        }

        let pid = ParameterId::new("multiplier").unwrap();
        let val = nodes[0].get_parameter(&pid).unwrap().as_f32().unwrap();
        assert!(
            (val - 5.0).abs() < 1e-6,
            "multiplier should be 5.0, got {}",
            val
        );
    }

    // ── Test: Queue + propagate ─────────────────────────────────────

    #[test]
    fn test_command_then_propagate() {
        use rill_core::queues::{MpscQueue, SetParameter, SignalOrigin};
        use rill_core::traits::algorithm::ActionContext;
        use rill_core::traits::processable::{ProcessContext, Processable};
        use rill_core::traits::PortId;

        const BUF: usize = 64;
        let queue: Arc<MpscQueue<SetParameter>> = Arc::new(MpscQueue::new());

        let mut builder = test_builder::<BUF>();
        let src = builder.add_source(Box::new(ConstantSource::new(7.0, 44100.0)));
        let proc = builder.add_processor(Box::new(GainProcessor::<f32, BUF>::new(
            NodeId(1),
            44100.0,
            2.0,
        )));
        let snk = builder.add_sink(Box::new(TestSink::<f32, BUF>::new(NodeId(2), 44100.0)));
        builder.connect_signal(src, 0, proc, 0);
        builder.connect_signal(proc, 0, snk, 0);
        let graph = builder.build().unwrap();
        let (mut nodes, topo, _, _bufs) = graph.into_parts();
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        // Push command and drain
        let _ = queue.push(SetParameter::new(
            PortId::control_in(NodeId(1), 0),
            ParameterId::new("multiplier").unwrap(),
            ParamValue::Float(4.0),
            SignalOrigin::Manual,
        ));
        while let Some(cmd) = queue.pop() {
            let idx = cmd.port.node_id().inner() as usize;
            let pid = cmd.parameter.clone();
            let _ = nodes[idx].set_parameter(&pid, cmd.value.clone());
        }

        // Verify multiplier changed
        let pid = ParameterId::new("multiplier").unwrap();
        let val = nodes[1].get_parameter(&pid).unwrap().as_f32().unwrap();
        assert!((val - 4.0).abs() < 1e-6);

        // Process + propagate
        let mut ctx = ProcessContext { clock: &tick };
        let _ = nodes[topo[0]].process_block(&mut ctx);
        let action_ctx = ActionContext::new(&tick);
        let out_port = nodes[topo[0]].output_port(0).unwrap();
        out_port.propagate(out_port.buffer(), &action_ctx).unwrap();

        let sink_val = nodes[topo[2]].input_port(0).unwrap().buffer.as_array()[0];
        assert!(
            (sink_val - 28.0).abs() < 1e-6,
            "source(7)×gain(4)=28, got {}",
            sink_val
        );
    }

    // ── Test: Feedback propagation ──────────────────────────────────

    #[test]
    fn test_feedback_propagation() {
        use rill_core::traits::algorithm::ActionContext;
        use rill_core::traits::processable::{ProcessContext, Processable};

        const BUF: usize = 64;
        let mut builder = test_builder::<BUF>();
        let src = builder.add_source(Box::new(ConstantSource::new(1.0, 44100.0)));
        let proc = builder.add_processor(Box::new(GainProcessor::<f32, BUF>::new(
            NodeId(1),
            44100.0,
            2.0,
        )));
        let snk = builder.add_sink(Box::new(TestSink::<f32, BUF>::new(NodeId(2), 44100.0)));
        builder.connect_signal(src, 0, proc, 0);
        builder.connect_signal(proc, 0, snk, 0);
        builder.connect_feedback(proc, 0, proc, 0);
        let graph = builder.build().unwrap();
        let (mut nodes, topo, _, _bufs) = graph.into_parts();

        // ── Block 1: no feedback yet ──
        let tick1 = ClockTick::new(0, BUF as u32, 44100.0);
        let mut ctx = ProcessContext { clock: &tick1 };
        let _ = nodes[topo[0]].process_block(&mut ctx);
        let ctx1 = ActionContext::new(&tick1);
        let out_port = nodes[topo[0]].output_port(0).unwrap();
        out_port.propagate(out_port.buffer(), &ctx1).unwrap();
        let block1 = nodes[topo[2]].input_port(0).unwrap().buffer.as_array()[0];
        assert!(
            (block1 - 2.0).abs() < 1e-6,
            "block1: 1.0×2.0=2.0, got {}",
            block1
        );

        // ── Block 2: feedback from block1 should be mixed in ──
        let tick2 = ClockTick::new(BUF as u64, BUF as u32, 44100.0);
        let mut ctx = ProcessContext { clock: &tick2 };
        let _ = nodes[topo[0]].process_block(&mut ctx);
        let ctx2 = ActionContext::new(&tick2);
        let out_port = nodes[topo[0]].output_port(0).unwrap();
        out_port.propagate(out_port.buffer(), &ctx2).unwrap();
        let block2 = nodes[topo[2]].input_port(0).unwrap().buffer.as_array()[0];
        assert!(
            (block2 - 6.0).abs() < 1e-6,
            "block2: (1+2)×2=6.0, got {}",
            block2
        );
    }

    // ── Test: drain_fn pattern (as used by AudioInput) ──────────────

    #[test]
    fn test_drain_fn_before_propagate() {
        use rill_core::queues::{MpscQueue, SetParameter, SignalOrigin};
        use rill_core::traits::algorithm::ActionContext;
        use rill_core::traits::processable::{ProcessContext, Processable};
        use rill_core::traits::PortId;

        const BUF: usize = 64;
        let queue: Arc<MpscQueue<SetParameter>> = Arc::new(MpscQueue::new());

        let mut builder = test_builder::<BUF>();
        let src = builder.add_source(Box::new(ConstantSource::new(5.0, 44100.0)));
        let proc = builder.add_processor(Box::new(GainProcessor::<f32, BUF>::new(
            NodeId(1),
            44100.0,
            1.0,
        )));
        let snk = builder.add_sink(Box::new(TestSink::<f32, BUF>::new(NodeId(2), 44100.0)));
        builder.connect_signal(src, 0, proc, 0);
        builder.connect_signal(proc, 0, snk, 0);
        let graph = builder.build().unwrap();
        let (mut nodes, topo, _, _bufs) = graph.into_parts();
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        // Push command BEFORE processing
        let _ = queue.push(SetParameter::new(
            PortId::control_in(NodeId(1), 0),
            ParameterId::new("multiplier").unwrap(),
            ParamValue::Float(3.0),
            SignalOrigin::Manual,
        ));

        // Drain
        while let Some(cmd) = queue.pop() {
            let idx = cmd.port.node_id().inner() as usize;
            let pid = cmd.parameter.clone();
            let _ = nodes[idx].set_parameter(&pid, cmd.value.clone());
        }

        // Verify parameter applied
        let pid = ParameterId::new("multiplier").unwrap();
        let val = nodes[1].get_parameter(&pid).unwrap().as_f32().unwrap();
        assert!(
            (val - 3.0).abs() < 1e-6,
            "multiplier should be 3.0, got {}",
            val
        );

        // Source generate
        let mut ctx = ProcessContext { clock: &tick };
        let _ = nodes[topo[0]].process_block(&mut ctx).unwrap();

        // Propagate
        let action_ctx = ActionContext::new(&tick);
        let out_port = nodes[topo[0]].output_port(0).unwrap();
        out_port.propagate(out_port.buffer(), &action_ctx).unwrap();

        // Verify: source(5) × gain(3) = 15
        let sink_val = nodes[topo[2]].input_port(0).unwrap().buffer.as_array()[0];
        assert!(
            (sink_val - 15.0).abs() < 1e-6,
            "source(5)×gain(3)=15, got {}",
            sink_val
        );
    }
}
