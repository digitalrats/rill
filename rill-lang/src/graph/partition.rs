//! Generalized partition: split a [`GraphSpec`] into subgraphs, each driven by
//! exactly one active (rill-io) backend, with passive backends (generators and
//! tape heads) as the boundaries.
//!
//! rill is clocked exclusively by hardware: only rill-io backends are active.
//! From each `ActiveInput` attachment the graph is walked forward over signal
//! edges until a `Passive` boundary node or a node claimed by another region
//! (recording). From each `ActiveOutput` attachment the graph is walked
//! backward until a `Passive` node, then extended forward from the passive
//! sources (playback). Passive nodes belong to the region that reaches them and
//! are compiled as builtins inside it (the write head is the recording sink,
//! the read heads are the playback sources). Cross-region edges become ports.

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
///
/// Recording regions are claimed by walking forward from each active input
/// attachment; playback regions by walking backward from each active output
/// attachment and extending forward from the passive sources they reach. A
/// passive boundary node belongs to the region that reaches it. Edges whose
/// endpoints land in different regions become cross-ports.
pub fn partition(spec: &GraphSpec) -> Vec<SubGraph> {
    let n = spec.nodes.len();
    let passive: Vec<bool> = (0..n)
        .map(|i| spec.nodes[i].backend == Some(NodeBackendKind::Passive))
        .collect();
    let active_in = (0..n).filter(|&i| spec.backends.iter().any(|b| b.node == i && !b.input));
    let active_out = (0..n).filter(|&i| spec.backends.iter().any(|b| b.node == i && b.input));

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

    // Recording regions: forward walk from each active input anchor. Passive
    // boundary nodes are claimed as region sinks but not expanded through.
    for seed in active_in {
        let mut seen = vec![false; n];
        let mut stack = vec![seed];
        seen[seed] = true;
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

    // Playback regions: backward walk from each active output anchor, then a
    // forward extension from the passive sources (read heads) the region
    // reached — nodes downstream of the tape reads belong to playback.
    for seed in active_out {
        let mut seen = vec![false; n];
        let mut stack = vec![seed];
        seen[seed] = true;
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
    spec.backends.iter().any(|b| b.node == node && b.input)
}

/// Whether the node has an active input (capture) attachment.
fn is_active_in(node: usize, spec: &GraphSpec) -> bool {
    spec.backends.iter().any(|b| b.node == node && !b.input)
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
