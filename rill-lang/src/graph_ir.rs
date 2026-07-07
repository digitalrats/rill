//! Graph-level IR: nodes, edges, and topology produced by the graph builder
//! before optimization and lowering.

use crate::ir::{Ir, ParamDef};
use indexmap::IndexMap;

/// A node in the graph IR.
#[derive(Debug, Clone)]
pub struct GraphNode {
    /// Signal arity: (inputs, outputs).
    pub arity: (usize, usize),
    /// Compiled IR for this node's signal-processing algorithm.
    pub ir: Ir,
    /// Parameter slots (name, default, range).
    pub params: Vec<ParamDef>,
    /// `keep param` — never inline.
    pub keep: bool,
    /// `inline param` — always inline.
    pub force_inline: bool,
}

/// Edge kind determines whether this edge participates in activation
/// propagation (Signal) or is a delayed data path (Feedback).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// Normal signal edge — determines activation order.
    Signal,
    /// Feedback edge (`~`) — delayed data, excluded from topological sort.
    Feedback,
}

/// A directed edge between two nodes and their ports.
#[derive(Debug, Clone)]
pub struct GraphEdge {
    /// Source node name.
    pub from_node: String,
    /// Source output port index.
    pub from_port: usize,
    /// Destination node name.
    pub to_node: String,
    /// Destination input port index.
    pub to_port: usize,
    /// Signal vs feedback.
    pub kind: EdgeKind,
}

/// The graph-level intermediate representation.
///
/// Produced by the graph builder from a typed AST, consumed by the optimizer,
/// then lowered to a `ScheduledGraph`.
#[derive(Debug, Clone)]
pub struct GraphIr {
    /// Number of external input channels (0 for Source, 1 for Processor).
    pub inputs: usize,
    /// Number of external output channels (1 for process).
    pub outputs: usize,
    /// Nodes in definition order, keyed by name.
    pub nodes: IndexMap<String, GraphNode>,
    /// All edges (signal + feedback).
    pub edges: Vec<GraphEdge>,
}
