use crate::factory::NodeFactory;
use std::sync::Arc;

use rill_core::buffer::{FixedBuffer, ResourceRegistry, TapeLoop};
use rill_core::io::{IoCapture, IoDriver, IoPlayback};
use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::queues::SetParameter;
use rill_core::time::{ClockTick, RenderContext, SystemClock};
use rill_core::traits::port::Port;
use rill_core::traits::processable::Processable;
use rill_core::traits::{Node, NodeId, NodeVariant, Params, ProcessResult};
use rill_core_actor::{Actor, ActorRef, ActorSystem};
use std::cell::{RefCell, UnsafeCell};
use std::collections::{HashSet, VecDeque};
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
    /// A node type is not registered in the built-in registry.
    UnknownNodeType(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CycleDetected => write!(f, "graph cycle detected"),
            Self::Backend(msg) => write!(f, "backend error: {msg}"),
            Self::Registry(msg) => write!(f, "registry error: {msg}"),
            Self::UnknownNodeType(msg) => write!(f, "unknown node type: {msg}"),
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
    #[cfg(not(feature = "lang"))]
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

        // Categorize root nodes (in_degree == 0) by type_name
        let recording_roots: Vec<usize> = in_degree
            .iter()
            .enumerate()
            .filter(|(i, &d)| d == 0 && self.recipes[*i].type_name == "rill/input")
            .map(|(i, _)| i)
            .collect();
        let playback_roots: Vec<usize> = in_degree
            .iter()
            .enumerate()
            .filter(|(i, &d)| d == 0 && self.recipes[*i].type_name != "rill/input")
            .map(|(i, _)| i)
            .collect();

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
                port.add_downstream(to_n, to_p);
            }
            let in_ptr: *mut Port<T, BUF_SIZE> = nodes[to_n]
                .input_port_mut(to_p)
                .map(|p| p as *mut Port<T, BUF_SIZE>)
                .unwrap_or(std::ptr::null_mut());
            let out_ptr: *mut Port<T, BUF_SIZE> = nodes[from_n]
                .output_port_mut(from_p)
                .map(|p| p as *mut Port<T, BUF_SIZE>)
                .unwrap_or(std::ptr::null_mut());
            if !in_ptr.is_null() && !out_ptr.is_null() {
                #[allow(unsafe_code)]
                unsafe {
                    (*out_ptr).add_downstream_input_ptr(in_ptr);
                }
            }
        }

        // --- chain membership (recording vs playback) ---
        //
        // Recording roots are I/O input nodes (type "rill/input").
        let has_split_chain = !recording_roots.is_empty() && !playback_roots.is_empty();

        let mut recording_set: HashSet<usize> = HashSet::new();
        let mut playback_set: HashSet<usize> = HashSet::new();

        if has_split_chain {
            recording_set = recording_roots.iter().copied().collect();
            let mut queue: VecDeque<usize> = recording_roots.iter().copied().collect();
            while let Some(node) = queue.pop_front() {
                for &(_, to_n, _) in &out_edges[node] {
                    if recording_set.insert(to_n) {
                        queue.push_back(to_n);
                    }
                }
            }

            playback_set = playback_roots.iter().copied().collect();
            let mut queue: VecDeque<usize> = playback_roots.iter().copied().collect();
            while let Some(node) = queue.pop_front() {
                for &(_, to_n, _) in &out_edges[node] {
                    if playback_set.insert(to_n) {
                        queue.push_back(to_n);
                    }
                }
            }

            // Intersection → playback wins
            for node in recording_set.clone() {
                if playback_set.contains(&node) && !recording_roots.contains(&node) {
                    recording_set.remove(&node);
                }
            }
        }

        // --- downstream_nodes (chain-filtered) ---
        for &(from_n, from_p, to_n, _) in &self.signal_edges {
            let parent: *mut NodeVariant<T, BUF_SIZE> = &mut nodes[to_n];
            if let Some(port) = nodes[from_n].output_port_mut(from_p) {
                if has_split_chain {
                    let same_chain = (recording_set.contains(&from_n)
                        && recording_set.contains(&to_n))
                        || (playback_set.contains(&from_n) && playback_set.contains(&to_n));
                    if !same_chain {
                        continue;
                    }
                }
                port.add_downstream_node(parent);
            }
        }

        // --- upstream_node (pull-model, same-chain only) ---
        for &(from_n, _from_p, to_n, to_p) in &self.signal_edges {
            let same_chain = if has_split_chain {
                (recording_set.contains(&from_n) && recording_set.contains(&to_n))
                    || (playback_set.contains(&from_n) && playback_set.contains(&to_n))
            } else {
                true
            };
            if same_chain {
                let src: *mut NodeVariant<T, BUF_SIZE> = &mut nodes[from_n];
                if let Some(port) = nodes[to_n].input_port_mut(to_p) {
                    port.set_upstream_node(src);
                }
            }
        }

        // --- upstream_buffer (zero-copy alias, exclusive 1:1 edges only) ---
        //
        // An input port may read its upstream output buffer directly (no copy)
        // ONLY when the edge is exclusive: the source output has exactly one
        // consumer AND the input has exactly one producer. Fan-out branches
        // must each receive an independent copy so downstream processing is
        // isolated (per the zero-copy rules); fan-in ports need their own
        // buffer too. Both leave `upstream_buffer` as `None` → materialized.
        let mut out_degree: std::collections::HashMap<(usize, usize), usize> =
            std::collections::HashMap::new();
        let mut in_degree_port: std::collections::HashMap<(usize, usize), usize> =
            std::collections::HashMap::new();
        for &(from_n, from_p, to_n, to_p) in &self.signal_edges {
            *out_degree.entry((from_n, from_p)).or_insert(0) += 1;
            *in_degree_port.entry((to_n, to_p)).or_insert(0) += 1;
        }
        for &(from_n, from_p, to_n, to_p) in &self.signal_edges {
            let exclusive = out_degree.get(&(from_n, from_p)) == Some(&1)
                && in_degree_port.get(&(to_n, to_p)) == Some(&1);
            let upstream = if exclusive {
                nodes[from_n]
                    .output_port(from_p)
                    .map(|p| p.buffer() as *const FixedBuffer<T, BUF_SIZE>)
            } else {
                None
            };
            if let Some(port) = nodes[to_n].input_port_mut(to_p) {
                port.set_upstream_buffer(upstream);
            }
        }

        // --- feedback buffers ---
        for &(from_n, from_p, to_n, to_p) in &self.feedback_edges {
            if let Some(port) = nodes[from_n].output_port_mut(from_p) {
                port.init_feedback_buffer();
                port.add_feedback_downstream(to_n, to_p);
            }
            if let Some(port) = nodes[to_n].input_port_mut(to_p) {
                port.init_feedback_buffer();
            }
        }
        for &(from_n, from_p, to_n, to_p) in &self.feedback_edges {
            let ptr = nodes[to_n]
                .input_port(to_p)
                .map(|p| p.feedback_buffer_ptr());
            if let Some(port) = nodes[from_n].output_port_mut(from_p) {
                if let Some(p) = ptr {
                    port.add_feedback_ptr(p);
                }
            }
        }

        // Allocate named shared resources (tape loops) and hand out capability
        // handles. The registry is build-time only: nodes keep their handles,
        // which reference-count the resource, so it is dropped after resolution.
        let mut registry = ResourceRegistry::new();
        for r in &self.resources {
            if r.kind == "tape" {
                if let Some(tape) = TapeLoop::<T>::new(r.capacity) {
                    registry.register_tape(&r.name, tape);
                }
            }
        }
        for entry in nodes.iter_mut() {
            entry.resolve_resources(&mut registry);
        }

        let allocated = self.resources.clone();

        // Compute node pointers for branch-chain processing
        let rec_ptrs: Vec<_> = recording_roots
            .iter()
            .map(|&i| &mut nodes[i] as *mut _)
            .collect();

        // Real sink = last Sink node in topological order. Using the actual
        // Sink (rather than `topo.last()`) is robust against extra signal-DAG
        // leaves introduced by feedback-source branches (e.g. an effect chain
        // inside a tape feedback loop terminates on a feedback edge, becoming a
        // second leaf that could otherwise be mistaken for the sink).
        let sink_idx = if has_split_chain {
            topo.iter()
                .rev()
                .copied()
                .find(|&i| matches!(nodes[i], NodeVariant::Sink(_)))
                .or_else(|| topo.last().copied())
        } else {
            None
        };
        let sink_ptr = match sink_idx {
            Some(i) => &mut nodes[i] as *mut _,
            None => std::ptr::null_mut(),
        };

        // Feedback-branch nodes: signal-ancestors of feedback-edge sources that
        // are processed by NEITHER the recording push NOR the playback pull.
        // In split mode the pull only processes nodes upstream of the sink, so
        // side-branch nodes feeding a feedback edge (e.g. effects inside a tape
        // feedback loop) would never run and their `snapshot_feedback` would
        // never fire. They are processed explicitly, in topological order,
        // after the pull.
        let feedback_ptrs: Vec<*mut NodeVariant<T, BUF_SIZE>> =
            if has_split_chain && !self.feedback_edges.is_empty() {
                let mut rev: Vec<Vec<usize>> = vec![Vec::new(); num_nodes];
                for &(f, _, t, _) in &self.signal_edges {
                    rev[t].push(f);
                }
                // Nodes that can reach the sink (already processed by the pull).
                let mut sink_reachable = vec![false; num_nodes];
                if let Some(si) = sink_idx {
                    let mut q = VecDeque::new();
                    q.push_back(si);
                    sink_reachable[si] = true;
                    while let Some(n) = q.pop_front() {
                        for &u in &rev[n] {
                            if !sink_reachable[u] {
                                sink_reachable[u] = true;
                                q.push_back(u);
                            }
                        }
                    }
                }
                // Signal-ancestors (inclusive) of feedback-edge sources.
                let mut in_branch = vec![false; num_nodes];
                let mut q = VecDeque::new();
                for &(from_n, _, _, _) in &self.feedback_edges {
                    if !in_branch[from_n] {
                        in_branch[from_n] = true;
                        q.push_back(from_n);
                    }
                }
                while let Some(n) = q.pop_front() {
                    for &u in &rev[n] {
                        if !in_branch[u] {
                            in_branch[u] = true;
                            q.push_back(u);
                        }
                    }
                }
                topo.iter()
                    .copied()
                    .filter(|&i| in_branch[i] && !sink_reachable[i] && !recording_set.contains(&i))
                    .map(|i| &mut nodes[i] as *mut _)
                    .collect()
            } else {
                Vec::new()
            };

        // Wrap nodes in Rc<UnsafeCell<Vec<>>> — port pointers already valid in this Vec.
        let nodes: Rc<UnsafeCell<Vec<NodeVariant<T, BUF_SIZE>>>> = Rc::new(UnsafeCell::new(nodes));

        let pending_params: PendingParams = Rc::new(RefCell::new(Vec::new()));

        let actor = system.spawn("graph", {
            let n = nodes.clone();
            let pending = pending_params.clone();
            #[allow(unsafe_code)]
            move |msg: CommandEnum| {
                if let CommandEnum::SetParameter(param) = msg {
                    if param.sample_pos.is_some() {
                        // Sample-accurate: defer to the block containing sample_pos.
                        pending.borrow_mut().push(param);
                        return;
                    }
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
            recording_roots: recording_roots.clone(),
            playback_roots: playback_roots.clone(),
            recording_ptrs: rec_ptrs,
            sink_ptr,
            feedback_ptrs,
            actor: Some(actor),
            actor_ref,
            parent_ref: self.parent_ref.clone(),
            system_clock: None,
            pending_params,
        })
    }
}

#[cfg(feature = "lang")]
impl<T: Transcendental, const BUF_SIZE: usize> GraphBuilder<T, BUF_SIZE> {
    /// Build a [`rill_lang::graph_ir::GraphIr`] using the built-in [`Registry`].
    ///
    /// This is the new execution path replacing [`build`](Self::build) → [`Graph`].
    /// It looks up each node type in the registry, constructs placeholder IRs,
    /// and performs topological sort. Actual compilation to executable programs
    /// happens in a future phase.
    pub fn build_ir(
        self,
        registry: &rill_lang::builtin::Registry<T>,
    ) -> Result<rill_lang::graph_ir::GraphIr, BuildError> {
        use indexmap::IndexMap;
        use rill_lang::builtin::SignatureSource;
        use rill_lang::graph_ir::{EdgeKind, GraphEdge, GraphIr, GraphNode};
        use std::collections::HashMap;

        // 1. Build index → name mapping
        let idx_to_name: HashMap<usize, String> = self
            .recipes
            .iter()
            .enumerate()
            .map(|(idx, recipe)| (idx, format!("node_{}", recipe.id.inner())))
            .collect();

        // 2. Create GraphNodes from recipes
        let mut nodes: IndexMap<String, GraphNode> = IndexMap::new();
        let mut node_list: Vec<String> = Vec::new();

        for (idx, recipe) in self.recipes.iter().enumerate() {
            let name = idx_to_name[&idx].clone();
            node_list.push(name.clone());

            let sig = registry
                .builtin_sig(&recipe.type_name)
                .ok_or_else(|| BuildError::UnknownNodeType(recipe.type_name.clone()))?;

            let arity = (sig.signal_ins(), sig.signal_outs);

            let params: Vec<rill_lang::ir::ParamDef> = recipe
                .params
                .parameters
                .iter()
                .filter_map(|(k, v)| {
                    v.as_f32().map(|f| rill_lang::ir::ParamDef {
                        name: k.clone(),
                        default: f as f64,
                        min: f64::NEG_INFINITY,
                        max: f64::INFINITY,
                    })
                })
                .collect();

            let ir = rill_lang::ir::Ir {
                instrs: vec![],
                num_regs: 1,
                output_reg: 0,
                num_inputs: arity.0,
                num_outputs: arity.1,
                state: rill_lang::ir::StateLayout {
                    state_slots: 0,
                    delay_lens: vec![],
                    num_outputs: arity.1,
                },
                builtins: vec![],
                params: vec![],
            };

            nodes.insert(
                name.clone(),
                GraphNode {
                    arity,
                    ir,
                    params,
                    keep: false,
                    inline: false,
                    is_bridge: false,
                    feedback_read: vec![],
                    feedback_write: vec![],
                },
            );
        }

        // 3. Convert edges
        let mut edges = Vec::new();
        for (from_idx, from_port, to_idx, to_port) in &self.signal_edges {
            edges.push(GraphEdge {
                from_node: idx_to_name[from_idx].clone(),
                from_port: *from_port,
                to_node: idx_to_name[to_idx].clone(),
                to_port: *to_port,
                kind: EdgeKind::Signal,
            });
        }
        for (from_idx, from_port, to_idx, to_port) in &self.feedback_edges {
            edges.push(GraphEdge {
                from_node: idx_to_name[from_idx].clone(),
                from_port: *from_port,
                to_node: idx_to_name[to_idx].clone(),
                to_port: *to_port,
                kind: EdgeKind::Feedback,
            });
        }

        // 4. Compute topological order (Kahn's algorithm on signal edges only)
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        for name in &node_list {
            in_degree.insert(name.clone(), 0);
        }
        for edge in &edges {
            if edge.kind == EdgeKind::Signal {
                *in_degree.get_mut(&edge.to_node).unwrap() += 1;
            }
        }

        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for name in &node_list {
            adj.insert(name.clone(), vec![]);
        }
        for edge in &edges {
            if edge.kind == EdgeKind::Signal {
                adj.get_mut(&edge.from_node)
                    .unwrap()
                    .push(edge.to_node.clone());
            }
        }

        let mut queue: Vec<String> = in_degree
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(n, _)| n.clone())
            .collect();
        let mut topo_order = Vec::new();

        while let Some(node) = queue.pop() {
            topo_order.push(node.clone());
            if let Some(neighbors) = adj.get(&node) {
                for neighbor in neighbors {
                    let deg = in_degree.get_mut(neighbor).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(neighbor.clone());
                    }
                }
            }
        }

        if topo_order.len() != node_list.len() {
            return Err(BuildError::CycleDetected);
        }

        // 5. Compute graph-level inputs/outputs from root/leaf nodes
        let inputs = topo_order
            .iter()
            .filter(|n| in_degree.get(*n) == Some(&0))
            .map(|n| nodes[n].arity.0)
            .sum();
        let outputs = topo_order
            .iter()
            .filter(|n| adj.get(*n).is_none_or(|v| v.is_empty()))
            .map(|n| nodes[n].arity.1)
            .sum();

        Ok(GraphIr {
            inputs,
            outputs,
            nodes,
            edges,
            topo_order,
        })
    }
}

// ============================================================================
// Graph (Static DAG)
// ============================================================================

/// Shared queue of sample-accurate parameter changes awaiting application.
///
/// Populated by the graph actor handler when a [`SetParameter`] carries a
/// `sample_pos`; drained per processing block by [`apply_due_params`].
type PendingParams = Rc<RefCell<Vec<SetParameter>>>;

/// Apply all pending sample-accurate parameter changes that are due by
/// `chunk_end` (the absolute sample position just past the current block).
///
/// Changes are applied in ascending `sample_pos` order; anything scheduled for
/// a later block stays queued. This is what makes parameter automation land at
/// the right sample position regardless of how the backend batches blocks into
/// I/O callbacks.
#[cfg(not(feature = "lang"))]
#[allow(unsafe_code)]
fn apply_due_params<T: Transcendental, const BUF_SIZE: usize>(
    nodes: &UnsafeCell<Vec<NodeVariant<T, BUF_SIZE>>>,
    pending: &RefCell<Vec<SetParameter>>,
    chunk_end: u64,
) {
    let mut pend = pending.borrow_mut();
    if pend.is_empty() {
        return;
    }
    pend.sort_by_key(|p| p.sample_pos.unwrap_or(0));
    let split = pend.partition_point(|p| p.sample_pos.is_none_or(|sp| sp < chunk_end));
    if split == 0 {
        return;
    }
    unsafe {
        let nv = &mut *nodes.get();
        for p in pend.drain(0..split) {
            let idx = p.port.node_id().inner() as usize;
            if idx < nv.len() {
                let _ = nv[idx].set_parameter(&p.parameter, p.value);
            }
        }
    }
}

/// Owned parts of a [`Graph`] returned by `into_parts` (test only).
#[cfg(test)]
type GraphParts<T, const BUF_SIZE: usize> = (Vec<NodeVariant<T, BUF_SIZE>>, Vec<usize>, ClockTick);

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
    recording_roots: Vec<usize>,
    playback_roots: Vec<usize>,
    recording_ptrs: Vec<*mut NodeVariant<T, BUF_SIZE>>,
    sink_ptr: *mut NodeVariant<T, BUF_SIZE>,
    feedback_ptrs: Vec<*mut NodeVariant<T, BUF_SIZE>>,
    current_tick: ClockTick,
    pub(crate) resources: Vec<GraphResource>,
    actor: Option<Actor<CommandEnum>>,
    actor_ref: ActorRef<CommandEnum>,
    parent_ref: Option<ActorRef<CommandEnum>>,
    /// Optional shared system clock, updated by external sync sources (MIDI, JACK transport).
    /// When set, the I/O callback reads BPM from it and creates `ClockTick::with_tempo`.
    pub system_clock: Option<Arc<SystemClock>>,
    /// Sample-accurate parameter changes awaiting application (shared with the actor handler).
    pending_params: PendingParams,
}

/// Owned processing state extracted from a [`Graph`].
///
/// Holds the parts needed for the I/O callback loop: the actor mailbox
/// for draining `SetParameter` commands, the node array, and routing
/// metadata.  The state is `!Send + !Sync` — it stays on the I/O thread.
///
/// Created via [`Graph::into_processing_state`].
#[cfg(not(feature = "lang"))]
pub struct ProcessingState<T: Transcendental, const BUF_SIZE: usize> {
    actor: Actor<CommandEnum>,
    nodes: Rc<UnsafeCell<Vec<NodeVariant<T, BUF_SIZE>>>>,
    recording_roots: Vec<usize>,
    playback_roots: Vec<usize>,
    recording_ptrs: Vec<*mut NodeVariant<T, BUF_SIZE>>,
    sink_ptr: *mut NodeVariant<T, BUF_SIZE>,
    feedback_ptrs: Vec<*mut NodeVariant<T, BUF_SIZE>>,
    parent_ref: Option<ActorRef<CommandEnum>>,
    system_clock: Option<Arc<SystemClock>>,
    /// Sample rate the graph nodes are currently initialised for.
    ///
    /// The graph has no clock of its own — it runs entirely inside the backend
    /// process callback and adopts the rate carried by each [`ClockTick`]. When
    /// a backend reports a hardware rate that differs from the rate the nodes
    /// were built with (e.g. JACK locked to 48 kHz while the graph was
    /// configured for 44.1 kHz), the nodes are re-initialised on the first tick
    /// so DSP (chip clocks, filter coefficients, …) matches the real rate.
    sample_rate: f32,
    /// Sample-accurate parameter changes awaiting application (shared with the actor handler).
    pending_params: PendingParams,
}

#[cfg(not(feature = "lang"))]
impl<T: Transcendental, const BUF_SIZE: usize> ProcessingState<T, BUF_SIZE> {
    /// Re-initialise every node for a new sample rate.
    ///
    /// Called from [`process_block`](Self::process_block) when the driving
    /// [`ClockTick`] reports a rate different from the one the graph is
    /// currently initialised for.
    #[allow(unsafe_code)]
    fn reinit_sample_rate(&mut self, sample_rate: f32) {
        unsafe {
            let nv = &mut *self.nodes.get();
            for node in nv.iter_mut() {
                node.init(sample_rate);
            }
        }
        self.sample_rate = sample_rate;
    }

    /// Process one block of signal data driven by an external [`ClockTick`].
    ///
    /// Processes all root nodes (recording + playback) — used when there's no
    /// split between input and output streams (single-stream backends).
    #[allow(unsafe_code)]
    pub fn process_block(&mut self, tick: &ClockTick) -> ProcessResult<()> {
        if tick.sample_rate > 0.0 && (tick.sample_rate - self.sample_rate).abs() > 0.5 {
            self.reinit_sample_rate(tick.sample_rate);
        }
        self.actor.drain();
        apply_due_params(
            &self.nodes,
            &self.pending_params,
            tick.sample_pos + tick.samples_since_last as u64,
        );
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
            for &root in self
                .recording_roots
                .iter()
                .chain(self.playback_roots.iter())
            {
                let _ = nv[root].process_block(&ctx, tick);
                for po in 0..nv[root].num_signal_outputs() {
                    if let Some(port) = nv[root].output_port(po) {
                        let _ = port.propagate(&ctx, tick);
                    }
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
                    if let NodeVariant::Source(src) = node {
                        src.set_capture(c.clone())
                    }
                }
                if let Some(ref p) = playback {
                    if let NodeVariant::Sink(sink) = node {
                        sink.set_playback(p.clone())
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
        let use_split = !self.recording_roots.is_empty() && !self.playback_roots.is_empty();
        if use_split {
            let mut actor = self.actor;
            let rec_ptrs = self.recording_ptrs;
            let sink = self.sink_ptr;
            let fb_ptrs = self.feedback_ptrs;
            let clock = self.system_clock;
            let parent = self.parent_ref;

            let clock_rec = clock.clone();
            driver.set_input_process_callback(Box::new(move |tick: &ClockTick| {
                actor.drain();
                if let Some(ref c) = clock_rec {
                    let ctx = RenderContext::with_tempo(
                        tick.sample_pos,
                        tick.samples_since_last,
                        tick.sample_rate,
                        c.bpm() as f32,
                    );
                    p_forward(&rec_ptrs, &ctx, tick);
                } else {
                    let ctx = RenderContext::new(
                        tick.sample_pos,
                        tick.samples_since_last,
                        tick.sample_rate,
                    );
                    p_forward(&rec_ptrs, &ctx, tick);
                }
            }));
            driver.set_process_callback(Box::new(move |tick: &ClockTick| {
                if let Some(ref c) = clock {
                    let mut ctx = RenderContext::with_tempo(
                        tick.sample_pos,
                        tick.samples_since_last,
                        tick.sample_rate,
                        c.bpm() as f32,
                    );
                    ctx.speed_ratio = tick.speed_ratio;
                    p_pull(sink, &ctx, tick);
                    p_process_branch(&fb_ptrs, &ctx, tick);
                } else {
                    let mut ctx = RenderContext::new(
                        tick.sample_pos,
                        tick.samples_since_last,
                        tick.sample_rate,
                    );
                    ctx.speed_ratio = tick.speed_ratio;
                    p_pull(sink, &ctx, tick);
                    p_process_branch(&fb_ptrs, &ctx, tick);
                }
                if tick.is_final {
                    if let Some(ref p) = parent {
                        p.send(CommandEnum::ClockTick(tick.clone()));
                    }
                }
            }));
            driver.run(running.clone())?;
        } else {
            driver.set_process_callback(Box::new(move |tick: &ClockTick| {
                let _ = self.process_block(tick);
                self.send_clock_tick(tick);
            }));
            driver.run(running.clone())?;
        }
        while running.load(std::sync::atomic::Ordering::Acquire) {
            std::thread::park();
        }
        let _ = driver.stop();
        Ok(())
    }
}

// ============================================================================
// Pointer-based chain processing (used by split-chain run_with_driver closures)
// ============================================================================

/// Forward propagate from recording roots.
#[cfg(not(feature = "lang"))]
#[allow(unsafe_code)]
fn p_forward<T: Transcendental, const BUF_SIZE: usize>(
    roots: &[*mut NodeVariant<T, BUF_SIZE>],
    ctx: &RenderContext,
    tick: &ClockTick,
) {
    for &root in roots {
        unsafe {
            let nv = &mut *root;
            let _ = nv.process_block(ctx, tick);
            for po in 0..nv.num_signal_outputs() {
                if let Some(port) = nv.output_port(po) {
                    let _ = port.propagate(ctx, tick);
                }
            }
        }
    }
}

/// Pull-model playback chain: start from sink, recursively process upstream.
#[cfg(not(feature = "lang"))]
#[allow(unsafe_code)]
fn p_pull<T: Transcendental, const BUF_SIZE: usize>(
    sink: *mut NodeVariant<T, BUF_SIZE>,
    ctx: &RenderContext,
    tick: &ClockTick,
) {
    if sink.is_null() {
        return;
    }
    unsafe {
        p_pull_recurse(&mut *sink, ctx, tick);
    }
}

#[cfg(not(feature = "lang"))]
#[allow(unsafe_code)]
fn p_pull_recurse<T: Transcendental, const BUF_SIZE: usize>(
    node: &mut NodeVariant<T, BUF_SIZE>,
    ctx: &RenderContext,
    tick: &ClockTick,
) {
    for pi in 0..node.num_signal_inputs() {
        if let Some(p) = node.input_port_mut(pi) {
            p.pre_process();
        }
    }
    for pi in 0..node.num_signal_inputs() {
        if let Some(p) = node.input_port(pi) {
            let src = p.upstream_node();
            if !src.is_null() {
                unsafe {
                    p_pull_recurse(&mut *src, ctx, tick);
                }
            }
        }
    }
    let _ = node.process_block(ctx, tick);
    for po in 0..node.num_signal_outputs() {
        if let Some(p) = node.output_port_mut(po) {
            p.snapshot_feedback();
        }
    }
    for po in 0..node.num_signal_outputs() {
        if let Some(port) = node.output_port(po) {
            let buf = port.buffer();
            for &in_ptr in port.downstream_input_ptrs() {
                unsafe {
                    let ip = &mut *in_ptr;
                    if !ip.is_zero_copy() {
                        let _ = ip.run_action(Some(buf.as_array()));
                    }
                    ip.set_data_received(true);
                }
            }
        }
    }
}

/// Process a topologically-ordered list of feedback-branch nodes.
///
/// These are side-branch nodes (e.g. effects inside a tape feedback loop) that
/// feed a feedback edge but do not reach the sink, so the playback pull never
/// processes them. Their inputs were already filled by the pull (their upstream
/// producers are on the sink path); here each is processed in order so its
/// output is produced and `snapshot_feedback` captures it into the downstream
/// feedback buffer for the next block.
#[cfg(not(feature = "lang"))]
#[allow(unsafe_code)]
fn p_process_branch<T: Transcendental, const BUF_SIZE: usize>(
    branch: &[*mut NodeVariant<T, BUF_SIZE>],
    ctx: &RenderContext,
    tick: &ClockTick,
) {
    for &np in branch {
        unsafe {
            let node = &mut *np;
            for pi in 0..node.num_signal_inputs() {
                if let Some(p) = node.input_port_mut(pi) {
                    p.pre_process();
                }
            }
            let _ = node.process_block(ctx, tick);
            for po in 0..node.num_signal_outputs() {
                if let Some(p) = node.output_port_mut(po) {
                    p.snapshot_feedback();
                }
            }
            for po in 0..node.num_signal_outputs() {
                if let Some(port) = node.output_port(po) {
                    let buf = port.buffer();
                    for &in_ptr in port.downstream_input_ptrs() {
                        let ip = &mut *in_ptr;
                        if !ip.is_zero_copy() {
                            let _ = ip.run_action(Some(buf.as_array()));
                        }
                        ip.set_data_received(true);
                    }
                }
            }
        }
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
    /// 3. Calls `process_block` on each root node and recursively
    ///    propagates through the DAG via [`Port::propagate`].
    ///
    /// The graph is `!Send + !Sync` — it stays on the I/O callback thread.
    #[cfg(not(feature = "lang"))]
    #[allow(unsafe_code)]
    pub fn process_block(&mut self, tick: &ClockTick) -> ProcessResult<()> {
        if let Some(ref mut actor) = self.actor {
            actor.drain();
        }
        apply_due_params(
            &self.nodes,
            &self.pending_params,
            tick.sample_pos + tick.samples_since_last as u64,
        );
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
            for &root in self
                .recording_roots
                .iter()
                .chain(self.playback_roots.iter())
            {
                let _ = nv[root].process_block(&ctx, tick);
                for po in 0..nv[root].num_signal_outputs() {
                    if let Some(port) = nv[root].output_port(po) {
                        let _ = port.propagate(&ctx, tick);
                    }
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
    #[cfg(not(feature = "lang"))]
    pub fn into_processing_state(mut self) -> ProcessingState<T, BUF_SIZE> {
        let actor = self.actor.take().expect("graph actor missing");
        ProcessingState {
            actor,
            nodes: self.nodes,
            recording_roots: self.recording_roots,
            playback_roots: self.playback_roots,
            recording_ptrs: self.recording_ptrs,
            sink_ptr: self.sink_ptr,
            feedback_ptrs: self.feedback_ptrs,
            parent_ref: self.parent_ref,
            system_clock: self.system_clock,
            sample_rate: self.current_tick.sample_rate,
            pending_params: self.pending_params,
        }
    }

    /// Obtain an [`ActorRef`] for sending commands to this graph.
    pub fn handle(&self) -> ActorRef<CommandEnum> {
        self.actor_ref.clone()
    }

    /// Consume the graph and return its owned parts (test only).
    #[cfg(test)]
    pub fn into_parts(self) -> GraphParts<T, BUF_SIZE> {
        let Self {
            nodes,
            topo_order,
            current_tick,
            resources: _,
            recording_roots: _,
            playback_roots: _,
            recording_ptrs: _,
            sink_ptr: _,
            feedback_ptrs: _,
            actor,
            actor_ref: _,
            parent_ref: _,
            system_clock: _,
            pending_params: _,
        } = self;
        drop(actor);
        let nodes = Rc::try_unwrap(nodes).unwrap().into_inner();
        (nodes, topo_order, current_tick)
    }
}

#[cfg(all(test, not(feature = "lang")))]
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
            let output = Port::output(id, 0, "out");
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
        fn num_signal_outputs(&self) -> usize {
            1
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
            self.output.write().fill(self.value);
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
        fn num_signal_inputs(&self) -> usize {
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
            let src = self.input.read();
            let buf = self.output.write();
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
        fn num_signal_inputs(&self) -> usize {
            1
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
    fn test_fanout_branches_are_independent_not_zero_copy() {
        // One source output feeding two consumers is a fan-out: each branch
        // must own an independent buffer so downstream processing is isolated.
        let factory = test_factory::<BUF>();
        let mut builder = test_builder::<BUF>(&factory);
        let system = test_system();

        let src = builder.add_node("test/const", &test_params(44100.0));
        let a = builder.add_node("test/gain", &test_params(44100.0));
        let b = builder.add_node("test/gain", &test_params(44100.0));
        builder.connect_signal(src, 0, a, 0);
        builder.connect_signal(src, 0, b, 0);

        let graph = builder.build(&system).unwrap();
        let nodes = graph.nodes();
        assert!(
            !nodes[a].input_port(0).unwrap().is_zero_copy(),
            "fan-out branch A must not alias the shared source buffer"
        );
        assert!(
            !nodes[b].input_port(0).unwrap().is_zero_copy(),
            "fan-out branch B must not alias the shared source buffer"
        );
        assert!(!nodes[a].input_port(0).unwrap().has_upstream_buffer());
        assert!(!nodes[b].input_port(0).unwrap().has_upstream_buffer());
    }

    #[test]
    fn test_linear_chain_edge_is_zero_copy() {
        // An exclusive 1:1 edge is safe to alias (single consumer).
        let factory = test_factory::<BUF>();
        let mut builder = test_builder::<BUF>(&factory);
        let system = test_system();

        let src = builder.add_node("test/const", &test_params(44100.0));
        let g = builder.add_node("test/gain", &test_params(44100.0));
        builder.connect_signal(src, 0, g, 0);

        let graph = builder.build(&system).unwrap();
        let nodes = graph.nodes();
        assert!(
            nodes[g].input_port(0).unwrap().is_zero_copy(),
            "exclusive 1:1 edge should be zero-copy"
        );
    }

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
        let source_idx = graph
            .recording_roots
            .first()
            .or(graph.playback_roots.first())
            .copied()
            .unwrap_or(0);

        let ctx = RenderContext::new(0, BUF as u32, 44100.0);
        let tick = ClockTick::new(0, BUF as u32, 44100.0, String::new());
        let nodes = graph.nodes.clone();
        unsafe {
            let nv = &mut *nodes.get();
            nv[source_idx].process_block(&ctx, &tick).unwrap();
            if let Some(port) = nv[source_idx].output_port(0) {
                port.propagate(&ctx, &tick).unwrap();
            }
        }
        unsafe {
            let nv = &*nodes.get();
            let val = nv[snk_idx]
                .input_port(0)
                .unwrap()
                .signal_buffer()
                .as_array()[0];
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
        let source_idx = graph
            .recording_roots
            .first()
            .or(graph.playback_roots.first())
            .copied()
            .unwrap_or(0);

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
            let src_val = nv[source_idx].output_port(0).unwrap().read()[0];
            eprintln!("source output: {src_val}");

            let out_port = nv[source_idx].output_port(0).unwrap();
            eprintln!(
                "source output port downstream_nodes: {}",
                out_port.downstream_nodes().len()
            );
            eprintln!(
                "source output port downstream_input_ptrs: {}",
                out_port.downstream_input_ptrs().len()
            );

            // Check processor output port connections BEFORE propagate
            {
                let proc_port = nv[proc_idx].output_port(0).unwrap();
                eprintln!(
                    "PROC OUT port downstream_nodes: {}",
                    proc_port.downstream_nodes().len()
                );
                eprintln!(
                    "PROC OUT port downstream_input_ptrs: {}",
                    proc_port.downstream_input_ptrs().len()
                );
                for (i, &dn) in proc_port.downstream().iter().enumerate() {
                    eprintln!("  downstream[{}]: (node={}, port={})", i, dn.0, dn.1);
                }
            }

            // --- BUFFER ADDRESS DEBUG ---
            let src_out = nv[source_idx].output_port(0).unwrap();
            let proc_in = nv[proc_idx].input_port(0).unwrap();
            let proc_out = nv[proc_idx].output_port(0).unwrap();
            let snk_in = nv[snk_idx].input_port(0).unwrap();
            eprintln!("BUFFER ADDRESSES:");
            eprintln!("  src output buf:  {:p}", src_out.read().as_ptr());
            eprintln!("  proc input buf:  {:p}", proc_in.read().as_ptr());
            eprintln!("  proc output buf: {:p}", proc_out.read().as_ptr());
            eprintln!("  snk input buf:   {:p}", snk_in.read().as_ptr());
            eprintln!(
                "  proc_in.has_upstream_buffer(): {}",
                proc_in.has_upstream_buffer()
            );
            eprintln!(
                "  snk_in.has_upstream_buffer(): {}",
                snk_in.has_upstream_buffer()
            );
            // --- END DEBUG ---

            out_port.propagate(&ctx, &tick).unwrap();

            // --- AFTER PROPAGATE: debug buffer values ---
            {
                let nv = &*nodes.get();
                let snk_in = nv[snk_idx].input_port(0).unwrap();
                eprintln!("AFTER propagate - snk input buf[0]: {}", snk_in.read()[0]);
            }

            let sink_buf = nv[snk_idx]
                .input_port(0)
                .unwrap()
                .signal_buffer()
                .as_array();
            eprintln!("SINK input port buffer first sample: {}", sink_buf[0]);

            // Check processor output port propagation
            let proc_out_port = nv[proc_idx].output_port(0).unwrap();
            eprintln!(
                "proc output port downstream_nodes: {}",
                proc_out_port.downstream_nodes().len()
            );
            eprintln!(
                "proc output port downstream_input_ptrs: {}",
                proc_out_port.downstream_input_ptrs().len()
            );

            // Sink
            let sink_val = nv[snk_idx]
                .input_port(0)
                .unwrap()
                .signal_buffer()
                .as_array()[0];
            eprintln!("sink input AFTER propagate: {sink_val}");

            assert!(
                (sink_val - 15.0).abs() < 1e-4,
                "expected 15.0, got {}",
                sink_val
            );
        }
    }

    /// A feedback-branch node (feeds a feedback edge but does not reach the
    /// sink) must be processed every block in split mode via `p_process_branch`.
    /// Before the fix these side-branch nodes were never run, so their
    /// `snapshot_feedback` never fired and feedback loops stayed silent.
    #[test]
    #[allow(unsafe_code)]
    fn test_split_processes_feedback_branch() {
        let mut f = NodeFactory::<f32, BUF>::new();
        f.register_fn("rill/input", |id, params| {
            let mut n = ConstantSource::<f32, BUF>::new(id, 0.0, params.sample_rate);
            n.init(params.sample_rate);
            NodeVariant::Source(Box::new(n))
        });
        f.register_fn("test/const", |id, params| {
            let v = params.get_f32("value", 1.0);
            let mut n = ConstantSource::<f32, BUF>::new(id, v, params.sample_rate);
            n.init(params.sample_rate);
            NodeVariant::Source(Box::new(n))
        });
        f.register_fn("test/gain", |id, params| {
            let g = params.get_f32("gain", 1.0);
            let mut n = GainProcessor::<f32, BUF>::new(id, params.sample_rate, g);
            n.init(params.sample_rate);
            NodeVariant::Processor(Box::new(n))
        });
        f.register_fn("test/capture", |id, params| {
            let mut n = CaptureSink::<f32, BUF>::new(id, params.sample_rate);
            n.init(params.sample_rate);
            NodeVariant::Sink(Box::new(n))
        });
        let factory = Arc::new(f);
        let mut builder = test_builder::<BUF>(&factory);
        let system = test_system();

        let rec_in = builder.add_node("rill/input", &test_params(44100.0)); // 0 (recording root)
        let mut cparams = test_params(44100.0);
        cparams.insert("value", ParamValue::Float(2.0));
        let play = builder.add_node("test/const", &cparams); // 1 (playback root)
        let sink = builder.add_node("test/capture", &test_params(44100.0)); // 2
        let branch = builder.add_node("test/gain", &test_params(44100.0)); // 3 (feedback branch)
        let rec_proc = builder.add_node("test/gain", &test_params(44100.0)); // 4 (recording)

        builder.connect_signal(play, 0, sink, 0); // playback: const -> sink
        builder.connect_signal(play, 0, branch, 0); // branch: const -> gain(branch)
        builder.connect_signal(rec_in, 0, rec_proc, 0); // recording: input -> gain
        builder.connect_feedback(branch, 0, rec_proc, 0); // branch -> recording (feedback)

        let graph = builder.build(&system).unwrap();
        assert_eq!(
            graph.feedback_ptrs.len(),
            1,
            "the feedback-branch node must be detected at build time"
        );

        let ctx = RenderContext::new(0, BUF as u32, 44100.0);
        for i in 0..3u64 {
            let tick = ClockTick::new(i * BUF as u64, BUF as u32, 44100.0, String::new());
            p_forward(&graph.recording_ptrs, &ctx, &tick);
            p_pull(graph.sink_ptr, &ctx, &tick);
            p_process_branch(&graph.feedback_ptrs, &ctx, &tick);
        }

        unsafe {
            let nv = &*graph.nodes.get();
            let branch_out = nv[branch].output_port(0).unwrap().read()[0];
            assert!(
                (branch_out - 2.0).abs() < 1e-4,
                "feedback-branch node was not processed (out={branch_out}, expected 2.0)"
            );
        }
    }
}
