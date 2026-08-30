//! GraphSpec reconstruction: turn a [`GraphSpec`] into a single rill-lang AST
//! program with channel-aware routing.
//!
//! Each node becomes an [`Expr::Apply`] with parameters ordered by the builtin
//! signature's `param_names`. Nodes are composed with `Par`/`Merge`, producers
//! are channel-selected with `<:` (Split + `_`/`!` selectors), and unconnected
//! consumer inputs become free `_` wires (program inputs). Feedback edges are
//! free inputs (no `<~` wrapper) and the tape is a backend, so no tape resource
//! declarations are emitted.

use rill_core::math::Transcendental;

use crate::ast::{BinOp, Def, Expr, Program};
use crate::builtin::{Registry, SignatureSource};
use crate::error::{CompileError, Span};
use crate::graph::spec::{GraphEdgeKind, GraphSpec};

/// Compile a [`GraphSpec`] into a runnable [`crate::program_engine::ProgramEngine`].
pub fn compile_spec<T: Transcendental + 'static>(
    spec: &GraphSpec,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<crate::program_engine::ProgramEngine<T>, CompileError> {
    let program = reconstruct(spec, registry)?;
    crate::compile_program(&program, registry, sample_rate)
}

/// Reconstruct a [`GraphSpec`] into an rill-lang AST `Program`.
pub fn reconstruct<T: Transcendental + 'static>(
    spec: &GraphSpec,
    registry: &Registry<T>,
) -> Result<Program, CompileError> {
    let n = spec.nodes.len();
    let dummy = Span::new(0, 0);

    // --- Name resolution (direct) + parameter ordering -----------------------
    struct NodeMeta {
        builtin_name: String,
        param_values: Vec<f64>,
        param_names: Vec<String>,
        has_resource: bool,
        signal_ins: usize,
        signal_outs: usize,
    }

    let mut metas = Vec::with_capacity(n);
    for node in &spec.nodes {
        let sig = registry.builtin_sig(&node.type_name).ok_or_else(|| {
            CompileError::Unsupported(format!("unknown builtin '{}'", node.type_name))
        })?;
        let param_names: Vec<String> = sig.param_names.iter().map(|s| s.to_string()).collect();
        let param_values: Vec<f64> = param_names
            .iter()
            .map(|name| node.params.get(name).copied().unwrap_or(0.0))
            .collect();
        let has_resource = sig
            .params
            .iter()
            .any(|p| matches!(p, crate::builtin::ParamType::Resource));
        metas.push(NodeMeta {
            builtin_name: node.type_name.clone(),
            param_values,
            param_names,
            has_resource,
            signal_ins: sig.signal_ins(),
            signal_outs: sig.signal_outs,
        });
    }

    // --- Topological sort over signal edges (feedback edges excluded) --------
    let mut in_degree = vec![0usize; n];
    let mut adj = vec![Vec::<usize>::new(); n];
    for e in &spec.edges {
        if e.kind == GraphEdgeKind::Signal {
            adj[e.from].push(e.to);
            in_degree[e.to] += 1;
        }
    }
    let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut visited = 0usize;
    while let Some(u) = queue.pop() {
        visited += 1;
        for &v in &adj[u] {
            in_degree[v] -= 1;
            if in_degree[v] == 0 {
                queue.push(v);
            }
        }
    }
    if visited != n {
        return Err(CompileError::Unsupported(
            "graph contains a cycle".to_string(),
        ));
    }

    // --- Per-node block expression (Apply with ordered params) ---------------
    // The last param of each node is exposed as a per-node actor parameter
    // `?{node}_{name}={default}` (0-in, late-binding slot addressable by name
    // from the control thread, unique per node so same-named params with
    // different defaults do not conflict) — NOT a `main` λ-parameter, which
    // would add a signal input. Resource-backed nodes (tape heads) get a
    // symbolic `Ref` to the shared tape as their first arg.
    let default_tape = spec
        .resources
        .iter()
        .find(|r| r.kind == "tape")
        .map(|r| r.name.clone())
        .unwrap_or_else(|| "tape_0".to_string());

    // Per-node actor-param names, collision-aware: a name used with a SINGLE
    // consistent default keeps its plain form (addressable externally, e.g. the
    // STC player's "regs"); a name used with different defaults across nodes is
    // namespaced per node (`delay_0`, `delay_1`, ...) so the redeclaration is
    // not ambiguous.
    let last_param: Vec<Option<(String, f64)>> = metas
        .iter()
        .map(|m| {
            let last = m.param_names.len().saturating_sub(1);
            if m.param_names.is_empty() {
                None
            } else {
                Some((m.param_names[last].clone(), m.param_values[last]))
            }
        })
        .collect();
    let ambiguous: std::collections::HashSet<String> = {
        use std::collections::HashMap as M;
        let mut defaults: M<String, Vec<f64>> = M::new();
        for (name, val) in last_param.iter().flatten() {
            defaults.entry(name.clone()).or_default().push(*val);
        }
        defaults
            .iter()
            .filter(|(_, vals)| vals.iter().any(|&v| (v - vals[0]).abs() > f64::EPSILON))
            .map(|(name, _)| name.clone())
            .collect()
    };
    let actor_name = |idx: usize| -> String {
        match &last_param[idx] {
            Some((name, _)) if ambiguous.contains(name) => format!("{name}_{idx}"),
            Some((name, _)) => name.clone(),
            None => String::new(),
        }
    };

    let mut blocks: Vec<Expr> = Vec::with_capacity(n);
    for (idx, meta) in metas.iter().enumerate() {
        let mut args: Vec<Expr> = Vec::with_capacity(meta.param_names.len() + 1);
        if meta.has_resource {
            args.push(Expr::Ref(default_tape.clone(), dummy));
        }
        let last = meta.param_names.len().saturating_sub(1);
        for (i, (&val, _name)) in meta
            .param_values
            .iter()
            .zip(meta.param_names.iter())
            .enumerate()
        {
            if i < last {
                args.push(Expr::Float(val, dummy));
            } else {
                args.push(Expr::ActorParam {
                    name: actor_name(idx),
                    default: Some(Box::new(Expr::Float(val, dummy))),
                    span: dummy,
                });
            }
        }
        blocks.push(Expr::Apply {
            name: meta.builtin_name.clone(),
            args,
            span: dummy,
        });
    }

    // --- Signal input edges grouped by target port ---------------------------
    let mut in_edges: Vec<Vec<(usize, usize, usize)>> = vec![Vec::new(); n];
    for e in &spec.edges {
        if e.kind == GraphEdgeKind::Signal {
            in_edges[e.to].push((e.from, e.from_port, e.to_port));
        }
    }
    for e in &mut in_edges {
        e.sort_by_key(|&(_, _, tp)| tp);
    }

    let sig_ins: Vec<usize> = metas.iter().map(|m| m.signal_ins).collect();
    let sig_outs: Vec<usize> = metas.iter().map(|m| m.signal_outs).collect();

    // Free input ports per node (from active input attachments and cross-in
    // boundary edges) — each emits a `_` wire so the program exposes it.
    let mut node_input_ports: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(node, port) in &spec.input_ports {
        if node < n && !node_input_ports[node].contains(&port) {
            node_input_ports[node].push(port);
        }
    }

    // --- Channel-aware build (memoized for fan-out) ---------------------------
    let mut memo: Vec<Option<Expr>> = vec![None; n];

    #[allow(clippy::too_many_arguments)]
    fn build(
        idx: usize,
        blocks: &[Expr],
        in_edges: &[Vec<(usize, usize, usize)>],
        sig_ins: &[usize],
        sig_outs: &[usize],
        node_input_ports: &[Vec<usize>],
        memo: &mut Vec<Option<Expr>>,
        dummy: Span,
    ) -> Expr {
        if let Some(e) = &memo[idx] {
            return e.clone();
        }
        let block = blocks[idx].clone();
        let max_port = in_edges[idx].iter().map(|&(_, _, tp)| tp).max();
        let max_input_port = node_input_ports[idx].iter().copied().max();
        let n_in = max_port
            .map(|p| p + 1)
            .unwrap_or(0)
            .max(sig_ins[idx])
            .max(max_input_port.map(|p| p + 1).unwrap_or(0));

        // One channel-source per input port: the producer's selected channel
        // (`<:` with `_`/`!` selectors), or a free `_` wire (program input).
        let mut channels: Vec<Expr> = Vec::with_capacity(n_in);
        for j in 0..n_in {
            match in_edges[idx].iter().find(|&&(_, _, tp)| tp == j) {
                Some(&(from, from_port, _)) => {
                    let producer = build(
                        from,
                        blocks,
                        in_edges,
                        sig_ins,
                        sig_outs,
                        node_input_ports,
                        memo,
                        dummy,
                    );
                    channels.push(select_channels(
                        producer,
                        sig_outs[from],
                        &[from_port],
                        dummy,
                    ));
                }
                None => channels.push(Expr::Wire(dummy)),
            }
        }

        let expr = if channels.is_empty() {
            block
        } else {
            let mut par = channels.pop().unwrap();
            while let Some(c) = channels.pop() {
                par = Expr::Bin {
                    op: BinOp::Par,
                    lhs: Box::new(c),
                    rhs: Box::new(par),
                    span: dummy,
                };
            }
            Expr::Bin {
                op: BinOp::Merge,
                lhs: Box::new(par),
                rhs: Box::new(block),
                span: dummy,
            }
        };
        memo[idx] = Some(expr.clone());
        expr
    }

    // --- Program body: parallel composition of the sink nodes ---------------
    let mut out_edges_count = vec![0usize; n];
    for e in &spec.edges {
        if e.kind == GraphEdgeKind::Signal {
            out_edges_count[e.from] += 1;
        }
    }
    let sinks: Vec<usize> = (0..n).filter(|&i| out_edges_count[i] == 0).collect();
    let mut body = if sinks.is_empty() {
        Expr::Wire(dummy)
    } else {
        let mut body = build(
            sinks[0],
            &blocks,
            &in_edges,
            &sig_ins,
            &sig_outs,
            &node_input_ports,
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
                    &sig_ins,
                    &sig_outs,
                    &node_input_ports,
                    &mut memo,
                    dummy,
                )),
                span: dummy,
            };
        }
        body
    };
    // Cross-boundary outputs (dry, fb): expose the selected channel of the
    // producing node as an additional program output.
    for &(node, channel) in &spec.boundary_out {
        let ch = build(
            node,
            &blocks,
            &in_edges,
            &sig_ins,
            &sig_outs,
            &node_input_ports,
            &mut memo,
            dummy,
        );
        body = Expr::Bin {
            op: BinOp::Par,
            lhs: Box::new(body),
            rhs: Box::new(select_channels(ch, sig_outs[node], &[channel], dummy)),
            span: dummy,
        };
    }

    let defs = vec![Def::Anchor {
        name: "main".to_string(),
        params: Vec::new(),
        body,
        span: dummy,
        where_defs: vec![],
    }];

    Ok(Program { defs })
}

/// `expr <: (w_0, ..., w_{Ao-1})` where `w_k = _` when channel `k` is kept and
/// `!` (cut) otherwise. Returns an expression whose outputs are the kept
/// channels in ascending order. Identity when all channels are kept.
fn select_channels(expr: Expr, out_arity: usize, keep: &[usize], dummy: Span) -> Expr {
    use crate::ast::Expr as E;
    if keep.len() == out_arity && (0..out_arity).all(|k| keep.contains(&k)) {
        return expr;
    }
    let mut parts: Vec<E> = Vec::with_capacity(out_arity);
    for k in 0..out_arity {
        parts.push(if keep.contains(&k) {
            E::Wire(dummy)
        } else {
            E::Cut(dummy)
        });
    }
    let mut rhs = parts.pop().unwrap();
    while let Some(p) = parts.pop() {
        rhs = E::Bin {
            op: BinOp::Par,
            lhs: Box::new(p),
            rhs: Box::new(rhs),
            span: dummy,
        };
    }
    E::Bin {
        op: BinOp::Split,
        lhs: Box::new(expr),
        rhs: Box::new(rhs),
        span: dummy,
    }
}
