use rill_core::math::Transcendental;
use rill_core::queues::CommandEnum;
use rill_core::traits::Params;
use rill_core_actor::ActorRef;

use rill_lang::builtin::SignatureSource;
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
    pub fn ast_from_def(
        &self,
        registry: &rill_lang::builtin::Registry<T>,
    ) -> Result<rill_lang::ast::Program, BuildError> {
        use rill_lang::ast::{BinOp, Def, Expr, Param, Program};
        use rill_lang::error::Span;

        let dummy = Span::new(0, 0);

        // Build id-to-index mapping
        let mut id_to_idx: HashMap<u32, usize> = HashMap::new();
        for (i, r) in self.recipes.iter().enumerate() {
            id_to_idx.insert(r.id, i);
        }

        // Resolve builtin names and parameter order for each recipe
        struct NodeMeta {
            builtin_name: String,
            param_values: Vec<f64>,
            param_names: Vec<String>,
            has_resource: bool,
        }

        let mut node_metas: Vec<NodeMeta> = Vec::with_capacity(self.recipes.len());

        for recipe in &self.recipes {
            let builtin_name = Self::resolve_builtin_name(&recipe.type_name, registry)
                .ok_or_else(|| BuildError::UnknownNodeType(recipe.type_name.clone()))?;

            let sig = registry.builtin_sig(&builtin_name).unwrap();

            // Build parameter values in builtin param_names order
            let param_names: Vec<String> = sig.param_names.iter().map(|n| n.to_string()).collect();
            let mut param_values = Vec::with_capacity(param_names.len());

            // Build a lookup from recipe param name to f64 value
            let recipe_defaults: HashMap<&str, f64> = recipe
                .params
                .parameters
                .iter()
                .filter_map(|(k, v)| v.as_f32().map(|f| (k.as_str(), f as f64)))
                .collect();

            for name in &param_names {
                let val = recipe_defaults.get(name.as_str()).copied().unwrap_or(0.0);
                param_values.push(val);
            }

            // Task 5 placeholder: resource-backed built-ins were removed — the
            // tape is a passive backend (rill-sampler), not a compile-time
            // resource, so no node carries a symbolic resource reference. This
            // `ast_from_def` reconstruction is removed entirely in Task 5.
            let has_resource = false;

            node_metas.push(NodeMeta {
                builtin_name,
                param_values,
                param_names,
                has_resource,
            });
        }

        // Topological sort
        let mut in_degree: Vec<usize> = vec![0; self.recipes.len()];
        let mut adj: Vec<Vec<usize>> = vec![vec![]; self.recipes.len()];

        for (from_idx, _from_port, to_idx, _to_port) in &self.signal_edges {
            if *from_idx < self.recipes.len() && *to_idx < self.recipes.len() {
                adj[*from_idx].push(*to_idx);
                in_degree[*to_idx] += 1;
            }
        }

        let mut queue: Vec<usize> = (0..self.recipes.len())
            .filter(|i| in_degree[*i] == 0)
            .collect();
        let mut order: Vec<usize> = Vec::new();

        while let Some(u) = queue.pop() {
            order.push(u);
            for &v in &adj[u] {
                in_degree[v] -= 1;
                if in_degree[v] == 0 {
                    queue.push(v);
                }
            }
        }

        if order.len() != self.recipes.len() {
            return Err(BuildError::CycleDetected);
        }

        // Build the block expression for each node (Apply with folded params).
        // The last param (convention) is exposed as a dynamic main parameter.
        // Resource-backed nodes get a symbolic `Ref` to the default tape loop.
        let default_tape = self
            .resources
            .iter()
            .find(|r| r.kind == "tape")
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "tape_0".to_string());

        let mut all_param_names: Vec<String> = Vec::new();
        let mut blocks: Vec<Expr> = Vec::with_capacity(self.recipes.len());
        for meta in &node_metas {
            let mut args: Vec<Expr> = Vec::new();
            if meta.has_resource {
                args.push(Expr::Ref(default_tape.clone(), dummy));
            }
            let n = meta.param_names.len();
            for (i, (&val, name)) in meta
                .param_values
                .iter()
                .zip(meta.param_names.iter())
                .enumerate()
            {
                if i < n.saturating_sub(1) {
                    args.push(Expr::Float(val, dummy));
                } else {
                    all_param_names.push(name.clone());
                    args.push(Expr::Ref(name.clone(), dummy));
                }
            }
            blocks.push(Expr::Apply {
                name: meta.builtin_name.clone(),
                args,
                span: dummy,
            });
        }

        // Build per-node input/output edge lists.
        let n = self.recipes.len();
        let mut in_edges: Vec<Vec<(usize, usize, usize)>> = vec![vec![]; n];
        let mut out_edges: Vec<Vec<(usize, usize, usize)>> = vec![vec![]; n];
        for (from, from_port, to, to_port) in &self.signal_edges {
            if *from < n && *to < n {
                in_edges[*to].push((*from, *from_port, *to_port));
                out_edges[*from].push((*to, *from_port, *to_port));
            }
        }
        for e in &mut in_edges {
            e.sort_by_key(|&(_, _, tp)| tp);
        }
        for e in &mut out_edges {
            e.sort_by_key(|&(_, fp, _)| fp);
        }

        // Feedback edges: `feedback_by_target[to]` lists source nodes feeding
        // `to`'s feedback input; `feedback_sources[from]` lists targets.
        let mut feedback_by_target: Vec<Vec<usize>> = vec![vec![]; n];
        let mut feedback_sources: Vec<Vec<usize>> = vec![vec![]; n];
        for (from, _fp, to, _tp) in &self.feedback_edges {
            if *from < n && *to < n {
                feedback_by_target[*to].push(*from);
                feedback_sources[*from].push(*to);
            }
        }

        // Reconstruct the graph into a single expression tree using the DSL
        // combinators. Fan-in uses `Par` + `:>` (merge); fan-out duplicates the
        // (stateless) source expression via memoization — stateful fan-out is a
        // known limitation and requires `<:` (Split) support. Feedback edges are
        // reconstructed as `A <~ B` (feedback tap).
        let mut memo: Vec<Option<Expr>> = vec![None; n];

        fn build(
            idx: usize,
            blocks: &[Expr],
            in_edges: &[Vec<(usize, usize, usize)>],
            feedback_by_target: &[Vec<usize>],
            memo: &mut Vec<Option<Expr>>,
            dummy: Span,
        ) -> Expr {
            if let Some(e) = &memo[idx] {
                return e.clone();
            }
            let block = blocks[idx].clone();
            let mut expr = if in_edges[idx].is_empty() {
                block
            } else {
                let mut producers: Vec<usize> = Vec::new();
                for &(from, _, _) in &in_edges[idx] {
                    if !producers.contains(&from) {
                        producers.push(from);
                    }
                }
                if producers.len() == 1 {
                    Expr::Bin {
                        op: BinOp::Seq,
                        lhs: Box::new(build(
                            producers[0],
                            blocks,
                            in_edges,
                            feedback_by_target,
                            memo,
                            dummy,
                        )),
                        rhs: Box::new(block),
                        span: dummy,
                    }
                } else {
                    let mut par = build(
                        producers[0],
                        blocks,
                        in_edges,
                        feedback_by_target,
                        memo,
                        dummy,
                    );
                    for &p in &producers[1..] {
                        par = Expr::Bin {
                            op: BinOp::Par,
                            lhs: Box::new(par),
                            rhs: Box::new(build(
                                p,
                                blocks,
                                in_edges,
                                feedback_by_target,
                                memo,
                                dummy,
                            )),
                            span: dummy,
                        };
                    }
                    Expr::Bin {
                        op: BinOp::Merge,
                        lhs: Box::new(par),
                        rhs: Box::new(block),
                        span: dummy,
                    }
                }
            };
            // Wrap with feedback taps for each feedback source targeting `idx`.
            for &fb_src in &feedback_by_target[idx] {
                expr = Expr::Bin {
                    op: BinOp::Feedback,
                    lhs: Box::new(expr),
                    rhs: Box::new(build(
                        fb_src,
                        blocks,
                        in_edges,
                        feedback_by_target,
                        memo,
                        dummy,
                    )),
                    span: dummy,
                };
            }
            memo[idx] = Some(expr.clone());
            expr
        }

        // The graph output is the parallel composition of all sink nodes
        // (excluding feedback-only leaves).
        let sinks: Vec<usize> = (0..n)
            .filter(|&i| out_edges[i].is_empty() && feedback_sources[i].is_empty())
            .collect();
        let body = if sinks.is_empty() {
            Expr::Wire(dummy)
        } else {
            let mut body = build(
                sinks[0],
                &blocks,
                &in_edges,
                &feedback_by_target,
                &mut memo,
                dummy,
            );
            for &s in &sinks[1..] {
                body = Expr::Bin {
                    op: BinOp::Par,
                    lhs: Box::new(body),
                    rhs: Box::new(build(
                        s,
                        &blocks,
                        &in_edges,
                        &feedback_by_target,
                        &mut memo,
                        dummy,
                    )),
                    span: dummy,
                };
            }
            body
        };

        let params: Vec<Param> = all_param_names
            .into_iter()
            .map(|name| Param { name, span: dummy })
            .collect();

        // Task 5 placeholder: emit top-level tape resource declarations before
        // `main`. Dormant (no graph registers tape resources) — the tape is a
        // backend, not a resource; `ast_from_def` is removed in Task 5.
        let mut defs: Vec<Def> = self
            .resources
            .iter()
            .filter(|r| r.kind == "tape")
            .map(|r| Def::Local {
                name: r.name.clone(),
                body: Expr::Apply {
                    name: "TapeLoop".to_string(),
                    args: vec![Expr::Int(r.capacity as i64, dummy)],
                    span: dummy,
                },
                where_defs: vec![],
                span: dummy,
            })
            .collect();
        defs.push(Def::Anchor {
            name: "main".to_string(),
            params,
            body,
            span: dummy,
            where_defs: vec![],
        });

        Ok(Program { defs })
    }

    /// Compile directly from the graph definition to a [`ProgramEngine`](rill_lang::program_engine::ProgramEngine).
    ///
    /// Calls [`ast_from_def`](Self::ast_from_def) followed by rill-lang compilation.
    pub fn compile_def(
        &self,
        registry: &rill_lang::builtin::Registry<T>,
        sample_rate: f32,
    ) -> Result<rill_lang::program_engine::ProgramEngine<T>, BuildError> {
        let program = self.ast_from_def(registry)?;
        rill_lang::compile_program::<T>(&program, registry, sample_rate)
            .map_err(|e| BuildError::CompilationFailed(format!("{e}")))
    }

    fn resolve_builtin_name(
        type_name: &str,
        registry: &rill_lang::builtin::Registry<T>,
    ) -> Option<String> {
        if registry.builtin_sig(type_name).is_some() {
            return Some(type_name.to_string());
        }
        if let Some(rest) = type_name.strip_prefix("rill/") {
            if registry.builtin_sig(rest).is_some() {
                return Some(rest.to_string());
            }
        }
        let mapped = match type_name {
            "rill/dry_wet_mix" => "dry_wet",
            "rill/parametric_eq" => "eq_parametric",
            "rill/graphic_eq" => "graphic_eq",
            "rill/mono_to_stereo" => "mono_to_stereo",
            "rill/moog_ladder" => "moog",
            "rill/write_head" => "write_head",
            "rill/read_head" => "read_head",
            "rill/lofi_chip" => "ay38910",
            _ => "",
        };
        if !mapped.is_empty() && registry.builtin_sig(mapped).is_some() {
            return Some(mapped.to_string());
        }
        None
    }
}
