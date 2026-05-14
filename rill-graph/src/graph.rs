use crate::backend_factory::BackendFactory;
use crate::factory::NodeFactory;
use rill_core::buffer::{Buffer, BufferRegistry, FixedBuffer, TapeLoop};
use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::time::ClockTick;
use rill_core::traits::algorithm::ActionContext;
use rill_core::traits::port::Port;
use rill_core::traits::processable::{ProcessContext, Processable};
use rill_core::traits::ParamValue;
use rill_core::traits::{Node, NodeId, NodeVariant, Params};
use rill_core_actor::{Actor, ActorRef, ActorSystem};
use std::cell::UnsafeCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
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
    /// Factory registration error (unknown node type).
    Registry(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CycleDetected => write!(f, "graph cycle detected"),
            Self::Backend(msg) => write!(f, "backend error: {msg}"),
            Self::Registry(msg) => write!(f, "registry error: {msg}"),
        }
    }
}

// ============================================================================
// Graph Builder
// ============================================================================

// ============================================================================
// Node Storage
// ============================================================================

/// A deferred node recipe — constructed at [`build`](GraphBuilder::build) time.
struct NodeRecipe<T: Transcendental, const BUF_SIZE: usize> {
    type_name: String,
    id: NodeId,
    params: Params,
    backend: Option<String>,
    routing_entries: Vec<(usize, usize, f32)>,
    _phantom: std::marker::PhantomData<(T, [(); BUF_SIZE])>,
}

/// Temporary holder during build — wraps a constructed node for wiring.
struct NodeEntry<T: Transcendental, const BUF_SIZE: usize> {
    node: NodeVariant<T, BUF_SIZE>,
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
/// Stores deferred node recipes until [`build`](Self::build), which
/// constructs all nodes, wires connections, and performs topological
/// sort.  This keeps `GraphBuilder` `Send` — all non‑`Send` data
/// is constructed inside the target thread.
///
/// # Node factory
///
/// The builder holds an [`Arc<NodeFactory>`] for constructing nodes by
/// type name. Nodes registered via [`add_node_with_id`](Self::add_node_with_id)
/// are only validated and constructed at [`build`](Self::build) time.
pub struct GraphBuilder<T: Transcendental, const BUF_SIZE: usize> {
    recipes: Vec<NodeRecipe<T, BUF_SIZE>>,
    signal_edges: Vec<(usize, usize, usize, usize)>,
    control_edges: Vec<(usize, usize, usize, usize)>,
    clock_edges: Vec<(usize, usize, usize, usize)>,
    feedback_edges: Vec<(usize, usize, usize, usize)>,
    resources: Vec<GraphResource>,
    /// Shared node factory (required, from Runtime).
    factory: Arc<NodeFactory<T, BUF_SIZE>>,
    /// Shared backend factory (required, from Runtime).
    backend_factory: Arc<BackendFactory<T>>,
    /// Default backend name for nodes that don't specify one explicitly.
    default_backend: Option<String>,
    /// Default backend parameters (sample_rate, buffer_size, channels).
    backend_params: HashMap<String, ParamValue>,
    /// Sample rate override. When set, used in [`build`](Self::build).
    /// Populated from [`GraphDef::sample_rate`] during deserialization.
    sample_rate: Option<f32>,
    /// Parent RackCase ActorRef — Graph sends ClockTick here.
    parent_ref: Option<ActorRef<CommandEnum>>,
}

impl<T: Transcendental, const BUF_SIZE: usize> GraphBuilder<T, BUF_SIZE> {
    /// Create a new empty graph builder without a node factory.
    pub fn new(
        factory: Arc<NodeFactory<T, BUF_SIZE>>,
        backend_factory: Arc<BackendFactory<T>>,
    ) -> Self {
        Self {
            recipes: Vec::new(),
            signal_edges: Vec::new(),
            control_edges: Vec::new(),
            clock_edges: Vec::new(),
            feedback_edges: Vec::new(),
            resources: Vec::new(),
            factory,
            backend_factory,
            default_backend: None,
            backend_params: HashMap::new(),
            sample_rate: None,
            parent_ref: None,
        }
    }

    /// Add a node by type name using the internal factory.
    ///
    /// The type must be registered in the factory before [`build`](Self::build)
    /// is called.
    ///
    /// Returns the index of the newly added node.
    pub fn add_node(&mut self, type_name: &str, params: &Params) -> usize {
        let id = NodeId(self.recipes.len() as u32);
        self.add_node_with_id(type_name, params, id)
    }

    /// Add a node with an explicit [`NodeId`].
    ///
    /// Like [`add_node`](Self::add_node) but uses the provided `id`.
    /// Important for serialization where external references depend on
    /// exact IDs.
    pub fn add_node_with_id(&mut self, type_name: &str, params: &Params, id: NodeId) -> usize {
        let idx = self.recipes.len();
        self.recipes.push(NodeRecipe {
            type_name: type_name.to_string(),
            id,
            params: params.clone(),
            backend: None,
            routing_entries: Vec::new(),
            _phantom: std::marker::PhantomData,
        });
        idx
    }

    /// Assign a named backend to the node at the given index.
    pub fn set_node_backend(&mut self, idx: usize, name: String) {
        if let Some(recipe) = self.recipes.get_mut(idx) {
            recipe.backend = Some(name);
        }
    }

    /// Store a routing matrix entry to be applied at build time.
    pub fn add_routing_entry(&mut self, idx: usize, from: usize, to: usize, gain: f32) {
        if let Some(recipe) = self.recipes.get_mut(idx) {
            recipe.routing_entries.push((from, to, gain));
        }
    }

    /// Register a named resource (tape loop, buffer, etc.).
    pub fn add_resource(&mut self, resource: GraphResource) {
        self.resources.push(resource);
    }

    /// Number of nodes added to the builder so far.
    pub fn node_count(&self) -> usize {
        self.recipes.len()
    }

    /// Set the default backend name and parameters. Nodes without an explicit
    pub fn set_default_backend(&mut self, name: String, params: HashMap<String, ParamValue>) {
        self.default_backend = Some(name);
        self.backend_params = params;
    }

    /// Get the default backend name, if set.
    pub fn default_backend_name(&self) -> Option<&String> {
        self.default_backend.as_ref()
    }

    /// Set the sample rate for this builder.
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = Some(sr);
    }

    /// Set the parent RackCase actor reference (Graph → parent ClockTick).
    pub fn set_parent_ref(&mut self, parent: ActorRef<CommandEnum>) {
        self.parent_ref = Some(parent);
    }

    /// Access the shared backend factory.
    pub fn backend_factory(&self) -> &Arc<BackendFactory<T>> {
        &self.backend_factory
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

    /// Build the graph.
    ///
    /// Creates backends for nodes that have a backend name set (via
    /// `SourceDef::backend` / `SinkDef::backend` or the builder's default).  Finds the active
    /// (driver) node and stores its index for [`Graph::run`].
    pub fn build(self, system: &ActorSystem) -> Result<Graph<T, BUF_SIZE>, BuildError> {
        // Phase 1: Construct all nodes from recipes
        let mut node_entries: Vec<NodeEntry<T, BUF_SIZE>> = Vec::with_capacity(self.recipes.len());
        for recipe in &self.recipes {
            let node = self
                .factory
                .construct(&recipe.type_name, recipe.id, &recipe.params)
                .map_err(|e| BuildError::Registry(format!("{e}")))?;
            node_entries.push(NodeEntry { node });
        }

        // Apply pre-configured routing entries
        for (idx, node) in node_entries.iter_mut().enumerate() {
            for &(from, to, gain) in &self.recipes[idx].routing_entries {
                if let NodeVariant::Router(ref mut router) = node.node {
                    router.set_connection(from, to, T::from_f32(gain)).ok();
                }
            }
        }

        let num_nodes = node_entries.len();

        // --- Phase 2: Resolve audio backends for I/O nodes ---
        let sr = self.sample_rate.unwrap_or(44100.0);
        for (idx, recipe) in self.recipes.iter().enumerate() {
            let name = match recipe.backend.as_ref() {
                Some(n) => Some(n.clone()),
                None => self.default_backend.clone(),
            };
            if let Some(ref name) = name {
                let mut be_params = HashMap::new();
                be_params.insert("sample_rate".into(), ParamValue::Float(sr));
                be_params.insert("buffer_size".into(), ParamValue::Int(BUF_SIZE as i32));
                if self.default_backend.as_ref() == Some(name) {
                    for (k, v) in &self.backend_params {
                        be_params.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
                let backend = self
                    .backend_factory
                    .create(name, &be_params)
                    .map_err(BuildError::Backend)?;
                if let Some(io_node) = node_entries[idx].node.as_io_node_mut() {
                    io_node.resolve_backend(backend);
                }
            }
        }

        // --- Phase 3: adjacency for Kahn (audio edges only) ---
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

        // --- collect nodes into final Vec ---
        let mut nodes: Vec<NodeVariant<T, BUF_SIZE>> =
            node_entries.into_iter().map(|e| e.node).collect();

        // --- Phase 4: port pointer wiring on the final nodes Vec ---
        for &(from_n, from_p, to_n, to_p) in &self.signal_edges {
            if let Some(port) = nodes[from_n].output_port_mut(from_p) {
                port.downstream.push((to_n, to_p));
            }
            let in_ptr: *mut Port<T, BUF_SIZE> = nodes[to_n]
                .input_port_mut(to_p)
                .map(|p| p as *mut Port<T, BUF_SIZE>)
                .unwrap_or(std::ptr::null_mut());
            let parent: *mut NodeVariant<T, BUF_SIZE> = &mut nodes[to_n];
            let out_ptr: *mut Port<T, BUF_SIZE> = nodes[from_n]
                .output_port_mut(from_p)
                .map(|p| p as *mut Port<T, BUF_SIZE>)
                .unwrap_or(std::ptr::null_mut());
            if !in_ptr.is_null() && !out_ptr.is_null() {
                #[allow(unsafe_code)]
                unsafe {
                    (*in_ptr).parent = parent;
                    (*out_ptr).downstream_input_ptrs.push(in_ptr);
                }
            }
        }

        // --- downstream_nodes ---
        for &(from_n, from_p, to_n, _) in &self.signal_edges {
            let parent: *mut NodeVariant<T, BUF_SIZE> = &mut nodes[to_n];
            if let Some(port) = nodes[from_n].output_port_mut(from_p) {
                let ptr_val = parent as usize;
                let already = port.downstream_nodes.iter().any(|&p| p as usize == ptr_val);
                if !already {
                    port.downstream_nodes.push(parent);
                }
            }
        }

        // --- upstream_buffer ---
        for &(from_n, from_p, to_n, to_p) in &self.signal_edges {
            let upstream = nodes[from_n]
                .output_port(from_p)
                .map(|p| &p.buffer as *const FixedBuffer<T, BUF_SIZE>);
            if let Some(port) = nodes[to_n].input_port_mut(to_p) {
                if port.upstream_buffer.is_none() {
                    port.upstream_buffer = upstream;
                } else {
                    port.upstream_buffer = None;
                }
            }
        }

        // --- feedback buffers ---
        for &(from_n, from_p, to_n, to_p) in &self.feedback_edges {
            if let Some(port) = nodes[from_n].output_port_mut(from_p) {
                port.feedback_buffer = Some(FixedBuffer::new());
                port.feedback_downstream.push((to_n, to_p));
            }
            if let Some(port) = nodes[to_n].input_port_mut(to_p) {
                port.feedback_buffer = Some(FixedBuffer::new());
            }
        }
        for &(from_n, from_p, to_n, to_p) in &self.feedback_edges {
            let ptr = nodes[to_n]
                .input_port(to_p)
                .map(|p| &p.feedback_buffer as *const Option<FixedBuffer<T, BUF_SIZE>>)
                .map(|r| r as *mut Option<FixedBuffer<T, BUF_SIZE>>);
            if let Some(port) = nodes[from_n].output_port_mut(from_p) {
                if let Some(p) = ptr {
                    port.feedback_ptrs.push(p);
                }
            }
        }

        // Allocate named buffers
        let mut buffers = BufferRegistry::new();
        for r in &self.resources {
            if r.kind == "tape" {
                if let Some(tape) = TapeLoop::<T>::new(r.capacity) {
                    buffers.register(&r.name, Box::new(tape));
                }
            }
        }
        for entry in nodes.iter_mut() {
            entry.resolve_resources(&buffers);
        }

        let source_idx = topo.first().copied().unwrap_or(0);
        let mut active_node_idx = 0;
        for (i, n) in nodes.iter_mut().enumerate() {
            if n.as_active_node_mut().is_some() {
                active_node_idx = i;
                break;
            }
        }

        let owned_buffers = buffers.into_inner();
        let allocated = self.resources.clone();

        // Wrap nodes in Rc<UnsafeCell<Vec<>>> — port pointers already valid in this Vec.
        let nodes: Rc<UnsafeCell<Vec<NodeVariant<T, BUF_SIZE>>>> = Rc::new(UnsafeCell::new(nodes));

        let actor = system.spawn("graph", {
            let n = nodes.clone();
            #[allow(unsafe_code)]
            move |msg: CommandEnum| {
                if let CommandEnum::SetParameter(param) = msg {
                    let idx = param.port.node_id().inner() as usize;
                    unsafe {
                        let nv = &mut *n.get();
                        if idx < nv.len() {
                            let _ = nv[idx].set_parameter(&param.parameter, param.value);
                        }
                    }
                }
            }
        });

        let actor_ref = actor.actor_ref();

        Ok(Graph {
            nodes,
            topo_order: topo,
            resources: allocated,
            current_tick: ClockTick::new(0, BUF_SIZE as u32, sr),
            buffers: owned_buffers,
            source_idx,
            active_node_idx,
            actor: Some(actor),
            actor_ref,
            parent_ref: self.parent_ref.clone(),
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
    nodes: Rc<UnsafeCell<Vec<NodeVariant<T, BUF_SIZE>>>>,
    topo_order: Vec<usize>,
    source_idx: usize,
    active_node_idx: usize,
    current_tick: ClockTick,
    pub(crate) resources: Vec<GraphResource>,
    #[allow(dead_code)]
    buffers: Vec<Box<dyn Buffer<T> + Send>>,
    actor: Option<Actor<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
    parent_ref: Option<ActorRef<CommandEnum>>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Graph<T, BUF_SIZE> {
    // ========================================================================
    // Accessors
    // ========================================================================

    /// Borrow the node array (read-only).
    #[allow(unsafe_code)]
    pub fn nodes(&self) -> &[NodeVariant<T, BUF_SIZE>] {
        unsafe { &*self.nodes.get() }
    }

    /// Return the current clock tick.
    pub fn current_tick(&self) -> ClockTick {
        self.current_tick
    }

    /// Return the number of nodes in the graph.
    #[allow(unsafe_code)]
    pub fn node_count(&self) -> usize {
        unsafe { (*self.nodes.get()).len() }
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

    /// Run graph processing through the active node.
    #[allow(unsafe_code)]
    pub fn run(&mut self, running: Arc<AtomicBool>) -> Result<(), String> {
        let mut actor = self.actor.take().ok_or("graph already running")?;
        let source = self.source_idx;
        let parent = self.parent_ref.clone();
        let nodes = self.nodes.clone();
        let idx = self.active_node_idx;

        let tick: Box<dyn FnMut(u64, f32)> = Box::new(move |sample_pos, sample_rate| {
            actor.drain();
            let tick = ClockTick::new(sample_pos, BUF_SIZE as u32, sample_rate);
            let mut ctx = ProcessContext { clock: &tick };
            unsafe {
                let nv = &mut *nodes.get();
                let _ = nv[source].process_block(&mut ctx);
                let action_ctx = ActionContext::new(&tick);
                for po in 0..nv[source].num_signal_outputs() {
                    if let Some(port) = nv[source].output_port(po) {
                        let _ = port.propagate(port.buffer(), &action_ctx);
                    }
                }
            }
            if let Some(ref parent) = parent {
                parent.send(CommandEnum::ClockTick(tick));
            }
        });

        unsafe {
            self.nodes.get().as_mut().unwrap()[idx]
                .as_active_node_mut()
                .ok_or("no active node")?
                .run(tick, running)
        }
    }

    /// Obtain an [`ActorRef`] for sending commands to this graph.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
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
            active_node_idx: _,
            source_idx: _,
            actor,
            actor_ref: _,
            parent_ref: _,
        } = self;
        drop(actor);
        let nodes = Rc::try_unwrap(nodes).unwrap().into_inner();
        (nodes, topo_order, current_tick, buffers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::math::Transcendental;
    use rill_core::time::ClockTick;
    use rill_core::traits::{
        Node, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue, ParameterId, Port,
        ProcessResult, Processor, Sink, Source,
    };
    use rill_core_actor::ActorSystem;
    use std::sync::Arc;

    fn test_system() -> ActorSystem {
        ActorSystem::new()
    }

    fn test_factory<const B: usize>() -> Arc<NodeFactory<f32, B>> {
        let mut f = NodeFactory::<f32, B>::new();

        f.register_fn("test/const", |id, params| {
            let value = params.get_f32("value", 1.0);
            let mut node = ConstantSource::<f32, B>::new(id, value, params.sample_rate);
            node.init(params.sample_rate);
            NodeVariant::Source(Box::new(node))
        });

        f.register_fn("test/gain", |id, params| {
            let gain = params.get_f32("gain", 1.0);
            let mut node = GainProcessor::<f32, B>::new(id, params.sample_rate, gain);
            node.init(params.sample_rate);
            NodeVariant::Processor(Box::new(node))
        });

        f.register_fn("test/capture", |id, params| {
            let mut node = CaptureSink::<f32, B>::new(id, params.sample_rate);
            node.init(params.sample_rate);
            NodeVariant::Sink(Box::new(node))
        });

        Arc::new(f)
    }

    fn test_builder<const B: usize>(factory: &Arc<NodeFactory<f32, B>>) -> GraphBuilder<f32, B> {
        GraphBuilder::new(factory.clone(), Arc::new(BackendFactory::new()))
    }

    fn test_params(sample_rate: f32) -> Params {
        let mut p = Params::new(sample_rate);
        p.insert("value".to_string(), ParamValue::Float(sample_rate));
        p
    }

    // ------------------------------------------------------------------------
    // Test node implementations
    // ------------------------------------------------------------------------

    pub(crate) struct ConstantSource<T: Transcendental, const B: usize> {
        id: NodeId,
        value: T,
        state: NodeState<T, B>,
        output: Port<T, B>,
    }

    impl<T: Transcendental, const B: usize> ConstantSource<T, B> {
        pub fn new(id: NodeId, value: T, sample_rate: f32) -> Self {
            let state = NodeState::new(sample_rate);
            let mut output = Port::output(id, 0, "out");
            output.buffer = FixedBuffer::new();
            Self {
                id,
                value,
                state,
                output,
            }
        }
    }

    impl<T: Transcendental, const B: usize> Node<T, B> for ConstantSource<T, B> {
        fn id(&self) -> NodeId {
            self.id
        }
        fn set_id(&mut self, id: NodeId) {
            self.id = id;
        }
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "ConstantSource".into(),
                type_name: Some("test/const".into()),
                category: NodeCategory::Source,
                description: String::new(),
                author: String::new(),
                version: String::new(),
                parameters: vec![],
                signal_inputs: 0,
                signal_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
            }
        }
        fn init(&mut self, _: f32) {}
        fn reset(&mut self) {}
        fn get_parameter(&self, _: &ParameterId) -> Option<ParamValue> {
            None
        }
        fn set_parameter(&mut self, _: &ParameterId, _: ParamValue) -> ProcessResult<()> {
            Ok(())
        }
        fn control_port(&self, _: usize) -> Option<&Port<T, B>> {
            None
        }
        fn control_port_mut(&mut self, _: usize) -> Option<&mut Port<T, B>> {
            None
        }
        fn output_port(&self, i: usize) -> Option<&Port<T, B>> {
            if i == 0 {
                Some(&self.output)
            } else {
                None
            }
        }
        fn output_port_mut(&mut self, i: usize) -> Option<&mut Port<T, B>> {
            if i == 0 {
                Some(&mut self.output)
            } else {
                None
            }
        }
        fn input_port(&self, _: usize) -> Option<&Port<T, B>> {
            None
        }
        fn input_port_mut(&mut self, _: usize) -> Option<&mut Port<T, B>> {
            None
        }
        fn state(&self) -> &NodeState<T, B> {
            &self.state
        }
        fn state_mut(&mut self) -> &mut NodeState<T, B> {
            &mut self.state
        }
    }

    impl<T: Transcendental, const B: usize> Source<T, B> for ConstantSource<T, B> {
        fn generate(&mut self, _: &ClockTick, _: &[T], _: &[ClockTick]) -> ProcessResult<()> {
            self.output.buffer.as_mut_array().fill(self.value);
            Ok(())
        }
    }

    // ------------------------------------------------------------------------
    // GainProcessor
    // ------------------------------------------------------------------------

    pub(crate) struct GainProcessor<T: Transcendental, const B: usize> {
        id: NodeId,
        gain: T,
        state: NodeState<T, B>,
        input: Port<T, B>,
        output: Port<T, B>,
    }

    impl<T: Transcendental, const B: usize> GainProcessor<T, B> {
        pub fn new(id: NodeId, sample_rate: f32, gain: T) -> Self {
            let state = NodeState::new(sample_rate);
            let input = Port::input(id, 0, "in");
            let output = Port::output(id, 0, "out");
            Self {
                id,
                gain,
                state,
                input,
                output,
            }
        }
    }

    impl<T: Transcendental, const B: usize> Node<T, B> for GainProcessor<T, B> {
        fn id(&self) -> NodeId {
            self.id
        }
        fn set_id(&mut self, id: NodeId) {
            self.id = id;
        }
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "GainProcessor".into(),
                type_name: Some("test/gain".into()),
                category: NodeCategory::Processor,
                description: String::new(),
                author: String::new(),
                version: String::new(),
                parameters: vec![],
                signal_inputs: 1,
                signal_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
            }
        }
        fn init(&mut self, _: f32) {}
        fn reset(&mut self) {}
        fn get_parameter(&self, _: &ParameterId) -> Option<ParamValue> {
            None
        }
        fn set_parameter(&mut self, _: &ParameterId, _: ParamValue) -> ProcessResult<()> {
            Ok(())
        }
        fn control_port(&self, _: usize) -> Option<&Port<T, B>> {
            None
        }
        fn control_port_mut(&mut self, _: usize) -> Option<&mut Port<T, B>> {
            None
        }
        fn input_port(&self, i: usize) -> Option<&Port<T, B>> {
            if i == 0 {
                Some(&self.input)
            } else {
                None
            }
        }
        fn input_port_mut(&mut self, i: usize) -> Option<&mut Port<T, B>> {
            if i == 0 {
                Some(&mut self.input)
            } else {
                None
            }
        }
        fn num_signal_outputs(&self) -> usize {
            1
        }
        fn output_port(&self, i: usize) -> Option<&Port<T, B>> {
            if i == 0 {
                Some(&self.output)
            } else {
                None
            }
        }
        fn output_port_mut(&mut self, i: usize) -> Option<&mut Port<T, B>> {
            if i == 0 {
                Some(&mut self.output)
            } else {
                None
            }
        }
        fn state(&self) -> &NodeState<T, B> {
            &self.state
        }
        fn state_mut(&mut self) -> &mut NodeState<T, B> {
            &mut self.state
        }
    }

    impl<T: Transcendental, const B: usize> Processor<T, B> for GainProcessor<T, B> {
        fn process(
            &mut self,
            _: &ClockTick,
            _: &[&[T; B]],
            _: &[T],
            _: &[ClockTick],
            _: &[&[T; B]],
        ) -> ProcessResult<()> {
            let src = self.input.buffer.as_array();
            let buf = self.output.buffer.as_mut_array();
            for i in 0..B {
                buf[i] = src[i] * self.gain;
            }
            Ok(())
        }
    }

    // ------------------------------------------------------------------------
    // CaptureSink — captures first sample of each block
    // ------------------------------------------------------------------------

    pub(crate) struct CaptureSink<T: Transcendental, const B: usize> {
        id: NodeId,
        state: NodeState<T, B>,
        input: Port<T, B>,
    }

    impl<T: Transcendental, const B: usize> CaptureSink<T, B> {
        pub fn new(id: NodeId, sample_rate: f32) -> Self {
            let state = NodeState::new(sample_rate);
            let input = Port::input(id, 0, "in");
            Self { id, state, input }
        }
    }

    impl<T: Transcendental, const B: usize> Node<T, B> for CaptureSink<T, B> {
        fn id(&self) -> NodeId {
            self.id
        }
        fn set_id(&mut self, id: NodeId) {
            self.id = id;
        }
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "CaptureSink".into(),
                type_name: Some("test/capture".into()),
                category: NodeCategory::Sink,
                description: String::new(),
                author: String::new(),
                version: String::new(),
                parameters: vec![],
                signal_inputs: 1,
                signal_outputs: 0,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
            }
        }
        fn init(&mut self, _: f32) {}
        fn reset(&mut self) {}
        fn get_parameter(&self, _: &ParameterId) -> Option<ParamValue> {
            None
        }
        fn set_parameter(&mut self, _: &ParameterId, _: ParamValue) -> ProcessResult<()> {
            Ok(())
        }
        fn control_port(&self, _: usize) -> Option<&Port<T, B>> {
            None
        }
        fn control_port_mut(&mut self, _: usize) -> Option<&mut Port<T, B>> {
            None
        }
        fn output_port(&self, _: usize) -> Option<&Port<T, B>> {
            None
        }
        fn output_port_mut(&mut self, _: usize) -> Option<&mut Port<T, B>> {
            None
        }
        fn input_port(&self, i: usize) -> Option<&Port<T, B>> {
            if i == 0 {
                Some(&self.input)
            } else {
                None
            }
        }
        fn input_port_mut(&mut self, i: usize) -> Option<&mut Port<T, B>> {
            if i == 0 {
                Some(&mut self.input)
            } else {
                None
            }
        }
        fn state(&self) -> &NodeState<T, B> {
            &self.state
        }
        fn state_mut(&mut self) -> &mut NodeState<T, B> {
            &mut self.state
        }
    }

    impl<T: Transcendental, const B: usize> Sink<T, B> for CaptureSink<T, B> {
        fn consume(
            &mut self,
            _: &ClockTick,
            _: &[&[T; B]],
            _: &[T],
            _: &[ClockTick],
            _: &[&[T; B]],
        ) -> ProcessResult<()> {
            Ok(())
        }
    }

    // ------------------------------------------------------------------------
    // Graph signal flow tests
    // ------------------------------------------------------------------------

    const BUF: usize = 64;

    #[test]
    #[allow(unsafe_code)]
    fn test_graph_source_to_sink() {
        let factory = test_factory::<BUF>();
        let mut builder = test_builder::<BUF>(&factory);
        let system = test_system();

        let src_idx = builder.add_node("test/const", &test_params(44100.0));
        let snk_idx = builder.add_node("test/capture", &test_params(44100.0));
        builder.connect_signal(src_idx, 0, snk_idx, 0);

        let graph = builder.build(&system).unwrap();
        let source_idx = graph.source_idx;

        let tick = ClockTick::new(0, BUF as u32, 44100.0);
        let mut ctx = ProcessContext { clock: &tick };
        let nodes = graph.nodes.clone();
        unsafe {
            let nv = &mut *nodes.get();
            nv[source_idx].process_block(&mut ctx).unwrap();
            let action_ctx = ActionContext::new(&tick);
            if let Some(port) = nv[source_idx].output_port(0) {
                port.propagate(port.buffer(), &action_ctx).unwrap();
            }
        }
        unsafe {
            let nv = &*nodes.get();
            let val = nv[snk_idx].input_port(0).unwrap().buffer.as_array()[0];
            assert!(val != 0.0, "signal should have propagated, got {}", val);
        }
    }

    #[test]
    #[allow(unsafe_code)]
    fn test_graph_source_proc_sink() {
        let factory = test_factory::<BUF>();
        let mut builder = test_builder::<BUF>(&factory);
        let system = test_system();

        let mut params = test_params(44100.0);
        params.insert("value".to_string(), ParamValue::Float(5.0));
        let src_idx = builder.add_node("test/const", &params);

        let mut gain_params = test_params(44100.0);
        gain_params.insert("gain".to_string(), ParamValue::Float(3.0));
        let proc_idx = builder.add_node("test/gain", &gain_params);

        let snk_idx = builder.add_node("test/capture", &test_params(44100.0));

        builder.connect_signal(src_idx, 0, proc_idx, 0);
        builder.connect_signal(proc_idx, 0, snk_idx, 0);

        let graph = builder.build(&system).unwrap();
        let source_idx = graph.source_idx;

        eprintln!("topo: {:?}", graph.topo_order);
        eprintln!("source_idx: {source_idx}, src_idx: {src_idx}, proc_idx: {proc_idx}, snk_idx: {snk_idx}");

        let tick = ClockTick::new(0, BUF as u32, 44100.0);
        let mut ctx = ProcessContext { clock: &tick };
        let nodes = graph.nodes.clone();
        unsafe {
            let nv = &mut *nodes.get();
            eprintln!(
                "node types: src={:?}, proc={:?}, snk={:?}",
                std::mem::discriminant(&nv[0]),
                std::mem::discriminant(&nv[1]),
                std::mem::discriminant(&nv[2]),
            );

            let _ = nv[source_idx].process_block(&mut ctx);
            let src_val = nv[source_idx].output_port(0).unwrap().buffer.as_array()[0];
            eprintln!("source output: {src_val}");

            let action_ctx = ActionContext::new(&tick);
            let out_port = nv[source_idx].output_port(0).unwrap();
            eprintln!(
                "source output port downstream_nodes: {}",
                out_port.downstream_nodes.len()
            );
            eprintln!(
                "source output port downstream_input_ptrs: {}",
                out_port.downstream_input_ptrs.len()
            );

            // Check processor output port connections BEFORE propagate
            {
                let proc_port = nv[proc_idx].output_port(0).unwrap();
                eprintln!(
                    "PROC OUT port downstream_nodes: {}",
                    proc_port.downstream_nodes.len()
                );
                eprintln!(
                    "PROC OUT port downstream_input_ptrs: {}",
                    proc_port.downstream_input_ptrs.len()
                );
                for (i, &dn) in proc_port.downstream.iter().enumerate() {
                    eprintln!("  downstream[{}]: (node={}, port={})", i, dn.0, dn.1);
                }
            }

            // --- BUFFER ADDRESS DEBUG ---
            let src_out = nv[source_idx].output_port(0).unwrap();
            let proc_in = nv[proc_idx].input_port(0).unwrap();
            let proc_out = nv[proc_idx].output_port(0).unwrap();
            let snk_in = nv[snk_idx].input_port(0).unwrap();
            eprintln!("BUFFER ADDRESSES:");
            eprintln!(
                "  src output buf:  {:p}",
                src_out.buffer.as_array().as_ptr()
            );
            eprintln!(
                "  proc input buf:  {:p}",
                proc_in.buffer.as_array().as_ptr()
            );
            eprintln!(
                "  proc output buf: {:p}",
                proc_out.buffer.as_array().as_ptr()
            );
            eprintln!("  snk input buf:   {:p}", snk_in.buffer.as_array().as_ptr());
            eprintln!(
                "  proc_in.upstream_buffer.is_some(): {}",
                proc_in.upstream_buffer.is_some()
            );
            eprintln!(
                "  snk_in.upstream_buffer.is_some(): {}",
                snk_in.upstream_buffer.is_some()
            );
            // --- END DEBUG ---

            out_port.propagate(out_port.buffer(), &action_ctx).unwrap();

            // --- AFTER PROPAGATE: debug buffer values ---
            {
                let nv = &*nodes.get();
                let snk_in = nv[snk_idx].input_port(0).unwrap();
                eprintln!(
                    "AFTER propagate - snk input buf[0] via .buffer: {}",
                    snk_in.buffer.as_array()[0]
                );
                if let Some(up) = snk_in.upstream_buffer {
                    eprintln!(
                        "AFTER propagate - snk input via upstream ptr: {}",
                        (*up).as_array()[0]
                    );
                }
            }

            let sink_buf = nv[snk_idx].input_port(0).unwrap().buffer.as_array();
            eprintln!("SINK input port buffer first sample: {}", sink_buf[0]);

            // Check processor output port propagation
            let proc_out_port = nv[proc_idx].output_port(0).unwrap();
            eprintln!(
                "proc output port downstream_nodes: {}",
                proc_out_port.downstream_nodes.len()
            );
            eprintln!(
                "proc output port downstream_input_ptrs: {}",
                proc_out_port.downstream_input_ptrs.len()
            );

            // Sink
            let sink_val = nv[snk_idx].input_port(0).unwrap().buffer.as_array()[0];
            eprintln!("sink input AFTER propagate: {sink_val}");

            assert!(
                (sink_val - 15.0).abs() < 1e-4,
                "expected 15.0, got {}",
                sink_val
            );
        }
    }
}
