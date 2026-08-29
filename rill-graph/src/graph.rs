use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::Params;
use rill_core_actor::ActorRef;

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
    /// The graph topology is not supported for conversion to a flat chain.
    UnsupportedTopology(String),
    /// AST compilation failed.
    CompilationFailed(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CycleDetected => write!(f, "graph cycle detected"),
            Self::Backend(msg) => write!(f, "backend error: {msg}"),
            Self::UnknownNodeType(msg) => write!(f, "unknown node type: {msg}"),
            Self::UnsupportedTopology(msg) => write!(f, "unsupported topology: {msg}"),
            Self::CompilationFailed(msg) => write!(f, "compilation failed: {msg}"),
        }
    }
}

// ============================================================================
// Node Storage
// ============================================================================

/// A deferred node recipe — constructed at `ast_from_def` time.
struct NodeRecipe<T: Transcendental, const BUF_SIZE: usize> {
    type_name: String,
    id: u32,
    // Node anchor name — consumed by `ast_from_def` (where-def anchors) once
    // full topology lowering lands; kept in the recipe for that purpose.
    #[allow(dead_code)]
    name: String,
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

impl<T: Transcendental, const BUF_SIZE: usize> Default for GraphBuilder<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
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
        let id = self.recipes.len() as u32;
        self.add_node_with_id(type_name, params, id)
    }

    /// Add a node with an explicit `NodeId`.
    pub fn add_node_with_id(&mut self, type_name: &str, params: &Params, id: u32) -> usize {
        self.add_node_with_name(type_name, params, id, String::new())
    }

    /// Add a node with an explicit `NodeId` and a human-readable name
    /// (typically sourced from the JSON `name` field). The name becomes the
    /// program/anchor name in the compiled graph, used by `SetParameter` routing.
    pub fn add_node_with_name(
        &mut self,
        type_name: &str,
        params: &Params,
        id: u32,
        name: String,
    ) -> usize {
        let idx = self.recipes.len();
        self.recipes.push(NodeRecipe {
            type_name: type_name.to_string(),
            id,
            name,
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

    /// Convert the graph to an rill-lang AST `Program`.
    ///
    /// Each graph node becomes an [`Expr::Apply`](rill_lang::ast::Expr::Apply) with parameters ordered
    /// according to the builtin's `BuiltinSig::param_names`. Nodes are
    /// chained via [`BinOp::Seq`](rill_lang::ast::BinOp::Seq) according to their signal connections.
    ///
    /// Only simple chain topologies are supported (fan-out/fan-in will
    /// return [`BuildError::UnsupportedTopology`]).
    /// Serialize this builder into rill-lang's plain [`GraphSpec`] (frontend only —
    /// no IR formation lives here). `type_name` is emitted verbatim: it must be a
    /// rill-lang builtin name directly (no `rill/` prefix, no aliases).
    ///
    /// Passive nodes (software generators and tape heads) are classified
    /// [`NodeBackendKind::Passive`]; active rill-io attachment points are
    /// populated by the caller via `spec.backends`.
    pub fn to_graph_spec(&self) -> rill_lang::graph::spec::GraphSpec {
        use rill_lang::graph::spec::{
            GraphEdgeKind, GraphResourceSpec, GraphSpec, GraphSpecEdge, GraphSpecNode,
            NodeBackendKind,
        };
        let is_passive = |t: &str| {
            matches!(
                t,
                "write_head"
                    | "read_head"
                    | "sine"
                    | "saw"
                    | "square"
                    | "triangle"
                    | "noise"
                    | "sampler"
            )
        };
        GraphSpec {
            nodes: self
                .recipes
                .iter()
                .map(|r| GraphSpecNode {
                    type_name: r.type_name.clone(),
                    params: r
                        .params
                        .parameters
                        .iter()
                        .filter_map(|(k, v)| v.as_f32().map(|f| (k.clone(), f as f64)))
                        .collect(),
                    backend: if is_passive(&r.type_name) {
                        Some(NodeBackendKind::Passive)
                    } else {
                        None
                    },
                })
                .collect(),
            edges: self
                .signal_edges
                .iter()
                .map(|&(f, fp, t, tp)| GraphSpecEdge {
                    from: f,
                    from_port: fp,
                    to: t,
                    to_port: tp,
                    kind: GraphEdgeKind::Signal,
                })
                .chain(
                    self.feedback_edges
                        .iter()
                        .map(|&(f, fp, t, tp)| GraphSpecEdge {
                            from: f,
                            from_port: fp,
                            to: t,
                            to_port: tp,
                            kind: GraphEdgeKind::Feedback,
                        }),
                )
                .collect(),
            resources: self
                .resources
                .iter()
                .map(|r| GraphResourceSpec {
                    name: r.name.clone(),
                    kind: r.kind.clone(),
                    capacity: r.capacity,
                })
                .collect(),
            sample_rate: self.sample_rate.unwrap_or(44100.0),
            backends: Vec::new(),
            boundary_out: Vec::new(),
        }
    }

    /// Compile this graph via rill-lang's IR formation.
    ///
    /// Returns the single-program engine for a plain graph. Tape-echo graphs
    /// partition into a duplex stream — use [`to_graph_spec`](Self::to_graph_spec)
    /// + [`rill_lang::graph::compile`] directly for those.
    pub fn compile_def(
        &self,
        registry: &rill_lang::builtin::Registry<T>,
        sample_rate: f32,
    ) -> Result<rill_lang::program_engine::ProgramEngine<T>, BuildError> {
        let spec = self.to_graph_spec();
        match rill_lang::graph::compile(&spec, registry, sample_rate) {
            Ok(rill_lang::graph::CompiledStream::Single(engine)) => Ok(engine),
            Ok(_) => Err(BuildError::CompilationFailed(
                "graph is a tape echo; use rill_lang::graph::compile for the duplex stream".into(),
            )),
            Err(e) => Err(BuildError::CompilationFailed(format!("{e}"))),
        }
    }
}
