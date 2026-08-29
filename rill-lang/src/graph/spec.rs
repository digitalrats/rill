//! Graph specification data types.
//!
//! A [`GraphSpec`] is a data-level description of a signal graph: nodes with
//! direct builtin names, signal/feedback edges, tape resources, and backend
//! attachments that mark where active/passive backends connect. It is the
//! input to IR formation ([`crate::graph::reconstruct`]) and the generalized
//! partition ([`crate::graph::partition`]).

use std::collections::HashMap;

/// A complete signal graph ready for IR formation.
#[derive(Debug, Clone, Default)]
pub struct GraphSpec {
    /// Graph nodes in index order.
    pub nodes: Vec<GraphSpecNode>,
    /// Signal and feedback edges between nodes.
    pub edges: Vec<GraphSpecEdge>,
    /// Named resources (tape loops) shared by the backends.
    pub resources: Vec<GraphResourceSpec>,
    /// Sample rate the graph is compiled for.
    pub sample_rate: f32,
    /// Where active/passive backends attach to the graph.
    pub backends: Vec<BackendAttachment>,
}

/// A graph node: a direct builtin name and its parameter bag.
#[derive(Debug, Clone)]
pub struct GraphSpecNode {
    /// Direct builtin name (no prefix, no alias).
    pub type_name: String,
    /// Named parameters in `BuiltinSig::param_names` order (default 0.0).
    pub params: HashMap<String, f64>,
}

/// Edge kind: an ordinary signal edge or a feedback edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphEdgeKind {
    /// Signal edge — participates in topological order and channel routing.
    Signal,
    /// Feedback edge — reconstructed as a free program input at its target port.
    Feedback,
}

/// A graph edge between node output `from_port` and node input `to_port`.
#[derive(Debug, Clone)]
pub struct GraphSpecEdge {
    /// Source node index.
    pub from: usize,
    /// Source output channel.
    pub from_port: usize,
    /// Target node index.
    pub to: usize,
    /// Target input channel.
    pub to_port: usize,
    /// Whether this is a signal or feedback edge.
    pub kind: GraphEdgeKind,
}

/// A named resource (tape loop) with a capacity in samples.
#[derive(Debug, Clone)]
pub struct GraphResourceSpec {
    /// Resource name, e.g. `"tape_0"`.
    pub name: String,
    /// Resource kind string, e.g. `"tape"`.
    pub kind: String,
    /// Capacity in samples.
    pub capacity: usize,
}

/// Backend kind: an active input (capture), an active output (playback), or a
/// passive backend (tape heads) that forms a subgraph boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// Active input backend — drives a recording subgraph.
    ActiveInput,
    /// Active output backend — drives a playback subgraph.
    ActiveOutput,
    /// Passive backend (tape heads) — a boundary between subgraphs.
    Passive,
}

/// A backend attached to a graph node port.
#[derive(Debug, Clone)]
pub struct BackendAttachment {
    /// Whether this backend is an active input, an active output, or passive.
    pub kind: BackendKind,
    /// Backend name, e.g. `"pipewire"`, `"tape"`.
    pub backend_name: String,
    /// Node the backend attaches to.
    pub node: usize,
    /// Port on that node.
    pub port: usize,
    /// Backend configuration (tape capacity, head delays, feedback).
    pub params: HashMap<String, f64>,
}
