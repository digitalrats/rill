use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::{NodeId, Params};
use rill_core_actor::ActorRef;

use indexmap::IndexMap;
use rill_lang::builtin::SignatureSource;
use rill_lang::graph_ir::{EdgeKind, GraphEdge, GraphIr, GraphNode};
use std::collections::HashMap;

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
    /// A node type is not registered in the built-in registry.
    UnknownNodeType(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CycleDetected => write!(f, "graph cycle detected"),
            Self::Backend(msg) => write!(f, "backend error: {msg}"),
            Self::UnknownNodeType(msg) => write!(f, "unknown node type: {msg}"),
        }
    }
}

// ============================================================================
// Node Storage
// ============================================================================

/// A deferred node recipe — constructed at build_ir time.
struct NodeRecipe<T: Transcendental, const BUF_SIZE: usize> {
    type_name: String,
    id: NodeId,
    params: Params,
    routing_entries: Vec<(usize, usize, f32)>,
    _phantom: std::marker::PhantomData<(T, [(); BUF_SIZE])>,
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
    recipes: Vec<NodeRecipe<T, BUF_SIZE>>,
    signal_edges: Vec<(usize, usize, usize, usize)>,
    control_edges: Vec<(usize, usize, usize, usize)>,
    clock_edges: Vec<(usize, usize, usize, usize)>,
    feedback_edges: Vec<(usize, usize, usize, usize)>,
    resources: Vec<GraphResource>,
    sample_rate: Option<f32>,
    parent_ref: Option<ActorRef<CommandEnum>>,
}

impl<T: Transcendental, const BUF_SIZE: usize> GraphBuilder<T, BUF_SIZE> {
    /// Create a new empty graph builder.
    pub fn new() -> Self {
        Self {
            recipes: Vec::new(),
            signal_edges: Vec::new(),
            control_edges: Vec::new(),
            clock_edges: Vec::new(),
            feedback_edges: Vec::new(),
            resources: Vec::new(),
            sample_rate: None,
            parent_ref: None,
        }
    }

    /// Add a node by type name.
    ///
    /// Returns the index of the newly added node.
    pub fn add_node(&mut self, type_name: &str, params: &Params) -> usize {
        let id = NodeId(self.recipes.len() as u32);
        self.add_node_with_id(type_name, params, id)
    }

    /// Add a node with an explicit [`NodeId`].
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

    /// Build a [`rill_lang::graph_ir::GraphIr`] using the built-in [`Registry`].
    ///
    /// This is the new execution path. It looks up each node type in the registry,
    /// constructs placeholder IRs, and performs topological sort. Actual compilation
    /// to executable programs happens in a future phase.
    pub fn build_ir(
        self,
        registry: &rill_lang::builtin::Registry<T>,
    ) -> Result<GraphIr, BuildError> {
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
