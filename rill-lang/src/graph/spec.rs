//! Graph specification data types.
//!
//! A [`GraphSpec`] is a data-level description of a signal graph: nodes with
//! direct builtin names, signal/feedback edges, tape resources, and active
//! backend attachments. Each node carries a [`NodeBackendKind`] that classifies
//! it as an active callback attachment point, a passive boundary (generator or
//! tape head), or a pure transform. The [`crate::graph::partition`] uses that
//! classification to split the graph into subgraphs; [`crate::graph::reconstruct`]
//! turns each subgraph into a compiled program.

use std::collections::HashMap;

/// Backend kind of a graph node: active callback attachment, passive boundary,
/// or a pure transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeBackendKind {
    /// Attached to a rill-io callback backend (capture/playback attachment
    /// points). Seeds a subgraph in the partition.
    Active,
    /// A software generator (`sine`, `saw`, `sampler`) or a tape head
    /// (`write_head`/`read_head`). No callback; forms a subgraph boundary.
    Passive,
}

/// A complete signal graph ready for IR formation.
#[derive(Debug, Clone, Default)]
pub struct GraphSpec {
    /// Graph nodes in index order.
    pub nodes: Vec<GraphSpecNode>,
    /// Signal and feedback edges between nodes.
    pub edges: Vec<GraphSpecEdge>,
    /// Named resources (tape loops) shared by the subgraphs.
    pub resources: Vec<GraphResourceSpec>,
    /// Sample rate the graph is compiled for.
    pub sample_rate: f32,
    /// Active rill-io backend attachment points (capture/playback).
    pub backends: Vec<BackendAttachment>,
    /// Channels of nodes that cross the subgraph boundary and must be exposed
    /// as program outputs, in `(node index, output channel)` order. Empty for a
    /// full (non-sub) graph.
    pub boundary_out: Vec<(usize, usize)>,
    /// Free program input ports: `(node index, input channel)` that receive
    /// signal from an active input backend (capture) or a cross-in boundary.
    /// The reconstruction emits a `_` wire for each.
    pub input_ports: Vec<(usize, usize)>,
}

/// A graph node: a direct builtin name, its parameter bag, and its backend
/// classification.
#[derive(Debug, Clone)]
pub struct GraphSpecNode {
    /// Direct builtin name (no prefix, no alias).
    pub type_name: String,
    /// Named parameters in `BuiltinSig::param_names` order (default 0.0).
    pub params: HashMap<String, f64>,
    /// Backend classification: `Active`, `Passive`, or `None` (pure transform).
    pub backend: Option<NodeBackendKind>,
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

/// An active rill-io backend attached to a graph node port.
///
/// Only active (callback-driving) backends appear here — passive boundaries
/// are marked on the node itself via [`GraphSpecNode::backend`].
#[derive(Debug, Clone)]
pub struct BackendAttachment {
    /// Whether this is an input (capture) or an output (playback) attachment.
    pub input: bool,
    /// Backend name, e.g. `"pipewire"`.
    pub backend_name: String,
    /// Node the backend attaches to.
    pub node: usize,
    /// Port on that node.
    pub port: usize,
}
