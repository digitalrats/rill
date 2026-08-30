//! Generalized partition: split a [`GraphSpec`] into subgraphs, each driven by
//! exactly one active (rill-io) backend, with passive backends (generators and
//! tape heads) as the boundaries.
//!
//! rill is clocked exclusively by hardware: only rill-io backends are active.
//! Active attachments are grouped per backend (an output backend may attach at
//! several channels). From each active input backend the graph is walked forward
//! over signal edges until a `Passive` boundary node or a node claimed by
//! another region (recording). From each active output backend the graph is
//! walked backward until a `Passive` node, then extended forward from the
//! passive sources (playback). Passive nodes belong to the region that reaches
//! them and are compiled as builtins inside it. Cross-region edges become ports.

use crate::graph::spec::{GraphEdgeKind, GraphSpec, NodeBackendKind};

/// One subgraph of a partition.
#[derive(Debug, Clone)]
pub struct SubGraph {
    /// Node indices (into `GraphSpec.nodes`) owned by this subgraph.
    pub nodes: Vec<usize>,
    /// Indices into `GraphSpec.edges` of edges crossing OUT of this subgraph.
    pub out_edges: Vec<usize>,
    /// Indices into `GraphSpec.edges` of edges crossing INTO this subgraph.
    pub in_edges: Vec<usize>,
}

/// Partition a graph into subgraphs driven by active backends.
pub fn partition(spec: &GraphSpec) -> Vec<SubGraph> {
    let n = spec.nodes.len();
    let passive: Vec<bool> = (0..n)
        .map(|i| spec.nodes[i].backend == Some(NodeBackendKind::Passive))
        .collect();

    // Group active attachments by (backend, direction): one subgraph per active
    // backend (a rill-io input or output may attach at several nodes — e.g. the
    // two playback channels of one output backend).
    let mut input_groups: Vec<Vec<usize>> = Vec::new();
    let mut output_groups: Vec<Vec<usize>> = Vec::new();
    {
        use std::collections::HashMap;
        let mut by_name: HashMap<(&str, bool), Vec<usize>> = HashMap::new();
        for b in &spec.backends {
            by_name
                .entry((b.backend_name.as_str(), b.input))
                .or_default()
                .push(b.node);
        }
        for ((_name, is_input), nodes) in by_name {
            if is_input {
                input_groups.push(nodes);
            } else {
                output_groups.push(nodes);
            }
        }
    }

    // Forward/backward adjacency over signal edges.
    let mut fwd = vec![Vec::<usize>::new(); n];
    let mut bwd = vec![Vec::<usize>::new(); n];
    for e in &spec.edges {
        if e.kind == GraphEdgeKind::Signal {
            fwd[e.from].push(e.to);
            bwd[e.to].push(e.from);
        }
    }

    let mut region_of: Vec<Option<usize>> = vec![None; n];
    let mut regions: Vec<SubGraph> = Vec::new();

    // Recording regions: forward walk from each active input backend's anchors.
    for seeds in input_groups {
        let mut seen = vec![false; n];
        let mut stack = seeds;
        for &s in &stack {
            seen[s] = true;
        }
        while let Some(u) = stack.pop() {
            for &v in &fwd[u] {
                if seen[v] || is_active_out(v, spec) || region_of[v].is_some() {
                    continue;
                }
                seen[v] = true;
                if !passive[v] {
                    stack.push(v);
                }
            }
        }
        let rid = regions.len();
        regions.push(claim(&mut region_of, &seen, rid));
    }

    // Playback regions: backward walk from each active output backend's anchors,
    // then a forward extension from the passive sources (read heads) the region
    // reached — nodes downstream of the tape reads belong to playback.
    for seeds in output_groups {
        let mut seen = vec![false; n];
        let mut stack = seeds;
        for &s in &stack {
            seen[s] = true;
        }
        while let Some(u) = stack.pop() {
            for &v in &bwd[u] {
                if seen[v] || is_active_in(v, spec) || region_of[v].is_some() {
                    continue;
                }
                seen[v] = true;
                if !passive[v] {
                    stack.push(v);
                }
            }
        }
        let mut queue: Vec<usize> = (0..n)
            .filter(|&u| seen[u] && passive[u])
            .flat_map(|u| fwd[u].iter().copied())
            .collect();
        while let Some(u) = queue.pop() {
            for &v in &fwd[u] {
                if !seen[v] && !passive[v] && !is_active_in(v, spec) && region_of[v].is_none() {
                    seen[v] = true;
                    queue.push(v);
                }
            }
        }
        let rid = regions.len();
        regions.push(claim(&mut region_of, &seen, rid));
    }

    // Classify cross-region edges (signal and feedback) as cross-ports.
    for (i, e) in spec.edges.iter().enumerate() {
        if let (Some(rf), Some(rt)) = (region_of[e.from], region_of[e.to]) {
            if rf != rt {
                regions[rf].out_edges.push(i);
                regions[rt].in_edges.push(i);
            }
        }
    }
    for r in &mut regions {
        r.out_edges.sort_unstable();
        r.in_edges.sort_unstable();
    }
    regions
}

/// Whether the node has an active output (playback) attachment.
fn is_active_out(node: usize, spec: &GraphSpec) -> bool {
    spec.backends.iter().any(|b| b.node == node && !b.input)
}

/// Whether the node has an active input (capture) attachment.
fn is_active_in(node: usize, spec: &GraphSpec) -> bool {
    spec.backends.iter().any(|b| b.node == node && b.input)
}

/// Register all `seen` nodes into the new region `rid` and return its
/// [`SubGraph`].
fn claim(region_of: &mut [Option<usize>], seen: &[bool], rid: usize) -> SubGraph {
    let nodes: Vec<usize> = (0..seen.len()).filter(|&u| seen[u]).collect();
    for &u in &nodes {
        region_of[u] = Some(rid);
    }
    SubGraph {
        nodes,
        out_edges: Vec::new(),
        in_edges: Vec::new(),
    }
}
