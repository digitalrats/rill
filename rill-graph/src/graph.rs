use crate::factory::NodeFactory;
use std::sync::Arc;

use rill_core::buffer::{Buffer, BufferRegistry, FixedBuffer, TapeLoop};
use rill_core::io::{IoCapture, IoDriver, IoPlayback};
use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::time::{ClockTick, RenderContext, SystemClock};
use rill_core::traits::port::Port;
use rill_core::traits::processable::Processable;
use rill_core::traits::{Node, NodeId, NodeVariant, Params, ProcessResult};
use rill_core_actor::{Actor, ActorRef, ActorSystem};
use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

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
    /// Sample rate override. When set, used in [`build`](Self::build).
    /// Populated from [`GraphDef::sample_rate`] during deserialization.
    sample_rate: Option<f32>,
    /// Parent RackCase ActorRef — Graph sends ClockTick here.
    parent_ref: Option<ActorRef<CommandEnum>>,
}

impl<T: Transcendental, const BUF_SIZE: usize> GraphBuilder<T, BUF_SIZE> {
    /// Create a new empty graph builder.
    pub fn new(factory: Arc<NodeFactory<T, BUF_SIZE>>) -> Self {
        Self {
            recipes: Vec::new(),
            signal_edges: Vec::new(),
            control_edges: Vec::new(),
            clock_edges: Vec::new(),
            feedback_edges: Vec::new(),
            resources: Vec::new(),
            factory,
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
            routing_entries: Vec::new(),
            _phantom: std::marker::PhantomData,
        });
        idx
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

    /// Set the sample rate for this builder.
    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = Some(sr);
    }

    /// Set the parent RackCase actor reference (Graph → parent ClockTick).
    pub fn set_parent_ref(&mut self, parent: ActorRef<CommandEnum>) {
        self.parent_ref = Some(parent);
    }

    /// Connect signal ports.
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

        // --- Phase 2: adjacency for Kahn (signal edges only) ---
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

        // --- Phase 5: port pointer wiring on the final nodes Vec ---
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
            current_tick: ClockTick::new(
                0,
                BUF_SIZE as u32,
                self.sample_rate.unwrap_or(44100.0),
                String::new(),
            ),
            buffers: owned_buffers,
            source_idx,
            actor: Some(actor),
            actor_ref,
            parent_ref: self.parent_ref.clone(),
            system_clock: None,
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
    current_tick: ClockTick,
    pub(crate) resources: Vec<GraphResource>,
    #[allow(dead_code)]
    buffers: Vec<Box<dyn Buffer<T> + Send>>,
    actor: Option<Actor<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
    parent_ref: Option<ActorRef<CommandEnum>>,
    /// Optional shared system clock, updated by external sync sources (MIDI, JACK transport).
    /// When set, the I/O callback reads BPM from it and creates `ClockTick::with_tempo`.
    pub system_clock: Option<Arc<SystemClock>>,
}

/// Owned processing state extracted from a [`Graph`].
///
/// Holds the parts needed for the I/O callback loop: the actor mailbox
/// for draining `SetParameter` commands, the node array, and routing
/// metadata.  The state is `!Send + !Sync` — it stays on the I/O thread.
///
/// Created via [`Graph::into_processing_state`].
pub struct ProcessingState<T: Transcendental, const BUF_SIZE: usize> {
    actor: Actor<CommandEnum>,
    nodes: Rc<UnsafeCell<Vec<NodeVariant<T, BUF_SIZE>>>>,
    source_idx: usize,
    parent_ref: Option<ActorRef<CommandEnum>>,
    system_clock: Option<Arc<SystemClock>>,
    #[allow(dead_code)]
    buffers: Vec<Box<dyn Buffer<T> + Send>>,
}

impl<T: Transcendental, const BUF_SIZE: usize> ProcessingState<T, BUF_SIZE> {
    /// Process one block of signal data driven by an external [`ClockTick`].
    ///
    /// Same logic as [`Graph::process_block`] but operates on independently
    /// owned state (no borrow of the original `Graph`).
    #[allow(unsafe_code)]
    pub fn process_block(&mut self, tick: &ClockTick) -> ProcessResult<()> {
        self.actor.drain();
        let mut ctx = if let Some(ref clock) = self.system_clock {
            RenderContext::with_tempo(
                tick.sample_pos,
                tick.samples_since_last,
                tick.sample_rate,
                clock.bpm() as f32,
            )
        } else {
            RenderContext::new(tick.sample_pos, tick.samples_since_last, tick.sample_rate)
        };
        ctx.speed_ratio = tick.speed_ratio;
        unsafe {
            let nv = &mut *self.nodes.get();
            let _ = nv[self.source_idx].process_block(&ctx, tick);
            for po in 0..nv[self.source_idx].num_signal_outputs() {
                if let Some(port) = nv[self.source_idx].output_port(po) {
                    let _ = port.propagate(port.buffer(), &ctx, tick);
                }
            }
        }
        Ok(())
    }

    /// Send a ClockTick to the parent actor (rack fan-out).
    ///
    /// Called by the backend's process callback at the appropriate time
    /// (once per I/O callback for standard backends, once per DMA buffer
    /// for chunking backends).
    pub fn send_clock_tick(&self, tick: &ClockTick) {
        if tick.is_final {
            if let Some(ref parent) = self.parent_ref {
                parent.send(CommandEnum::ClockTick(tick.clone()));
            }
        }
    }

    /// Wire capture/playback backends into Source/Sink nodes after graph construction.
    ///
    /// Must be called after `into_processing_state()` and before the driver starts.
    /// Only Source nodes respond to `set_capture`; only Sink nodes respond to
    /// `set_playback`.  Processor and Router nodes ignore both.
    #[allow(unsafe_code)]
    pub fn wire_backends(
        &mut self,
        capture: Option<Arc<dyn IoCapture>>,
        playback: Option<Arc<dyn IoPlayback>>,
    ) {
        unsafe {
            let nv = &mut *self.nodes.get();
            for node in nv.iter_mut() {
                if let Some(ref c) = capture {
                    match node {
                        NodeVariant::Source(src) => src.set_capture(c.clone()),
                        _ => {}
                    }
                }
                if let Some(ref p) = playback {
                    match node {
                        NodeVariant::Sink(sink) => sink.set_playback(p.clone()),
                        _ => {}
                    }
                }
            }
        }
    }

    /// Run this processing state with a pre-created driver backend.
    ///
    /// Consumes `self`, wires the process callback, enters the I/O loop.
    /// The `running` flag controls shutdown.
    pub fn run_with_driver(
        mut self,
        driver: Arc<dyn IoDriver>,
        running: Arc<AtomicBool>,
    ) -> Result<(), String> {
        self.actor.drain();
        driver.set_process_callback(Box::new(move |tick: &ClockTick| {
            let _ = self.process_block(tick);
            self.send_clock_tick(tick);
        }));
        driver.run(running.clone())?;
        while running.load(std::sync::atomic::Ordering::Acquire) {
            std::thread::park();
        }
        let _ = driver.stop();
        Ok(())
    }
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
        self.current_tick.clone()
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

    /// Process one block of signal data driven by an external [`ClockTick`].
    ///
    /// Called from the backend's process callback. Performs:
    ///
    /// 1. Drains the graph's actor mailbox (applies queued `SetParameter`s).
    /// 2. Creates a [`RenderContext`] from the tick.
    /// 3. Calls `process_block` on the source node and recursively
    ///    propagates through the DAG via [`Port::propagate`].
    /// 4. Sends the tick to the parent [`ActorRef`] (if any).
    ///
    /// The graph is `!Send + !Sync` — it stays on the I/O callback thread.
    #[allow(unsafe_code)]
    pub fn process_block(&mut self, tick: &ClockTick) -> ProcessResult<()> {
        if let Some(ref mut actor) = self.actor {
            actor.drain();
        }
        let ctx = if let Some(ref clock) = self.system_clock {
            RenderContext::with_tempo(
                tick.sample_pos,
                tick.samples_since_last,
                tick.sample_rate,
                clock.bpm() as f32,
            )
        } else {
            RenderContext::new(tick.sample_pos, tick.samples_since_last, tick.sample_rate)
        };
        self.current_tick = tick.clone();
        unsafe {
            let nv = &mut *self.nodes.get();
            let _ = nv[self.source_idx].process_block(&ctx, tick);
            for po in 0..nv[self.source_idx].num_signal_outputs() {
                if let Some(port) = nv[self.source_idx].output_port(po) {
                    let _ = port.propagate(port.buffer(), &ctx, tick);
                }
            }
        }
        Ok(())
    }

    /// Consume the graph and return a [`ProcessingState`] that owns all
    /// parts needed for the I/O callback loop.
    ///
    /// `ProcessingState` is `!Send + !Sync` — it stays on the I/O thread
    /// and is moved into the backend's process callback closure.
    pub fn into_processing_state(mut self) -> ProcessingState<T, BUF_SIZE> {
        let actor = self.actor.take().expect("graph actor missing");
        ProcessingState {
            actor,
            nodes: self.nodes,
            source_idx: self.source_idx,
            parent_ref: self.parent_ref,
            system_clock: self.system_clock,
            buffers: self.buffers,
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
            source_idx: _,
            actor,
            actor_ref: _,
            parent_ref: _,
            system_clock: _,
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
    use rill_core::time::RenderContext;
    
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
        GraphBuilder::new(factory.clone())
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
        fn generate(
            &mut self,
            _: &RenderContext,
            _: &[T],
            _: &[RenderContext],
            _: &ClockTick,
        ) -> ProcessResult<()> {
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
            _: &RenderContext,
            _: &[&[T; B]],
            _: &[T],
            _: &[RenderContext],
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
            _: &RenderContext,
            _: &[&[T; B]],
            _: &[T],
            _: &[RenderContext],
            _: &[&[T; B]],
            _: &ClockTick,
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

        let ctx = RenderContext::new(0, BUF as u32, 44100.0);
        let tick = ClockTick::new(0, BUF as u32, 44100.0, String::new());
        let nodes = graph.nodes.clone();
        unsafe {
            let nv = &mut *nodes.get();
            nv[source_idx].process_block(&ctx, &tick).unwrap();
            if let Some(port) = nv[source_idx].output_port(0) {
                port.propagate(port.buffer(), &ctx, &tick).unwrap();
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

        let ctx = RenderContext::new(0, BUF as u32, 44100.0);
        let tick = ClockTick::new(0, BUF as u32, 44100.0, String::new());
        let nodes = graph.nodes.clone();
        unsafe {
            let nv = &mut *nodes.get();
            eprintln!(
                "node types: src={:?}, proc={:?}, snk={:?}",
                std::mem::discriminant(&nv[0]),
                std::mem::discriminant(&nv[1]),
                std::mem::discriminant(&nv[2]),
            );

            let _ = nv[source_idx].process_block(&ctx, &tick);
            let src_val = nv[source_idx].output_port(0).unwrap().buffer.as_array()[0];
            eprintln!("source output: {src_val}");

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

            out_port.propagate(out_port.buffer(), &ctx, &tick).unwrap();

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
