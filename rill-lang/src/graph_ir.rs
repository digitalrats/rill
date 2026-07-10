//! Graph intermediate representation for multi-node compilation.
//!
//! `GraphIr` is the bridge between `GraphBuilder` (programmatic API)
//! and the rill-lang optimizer/lowerer/engine pipeline.

use crate::ir::{Ir, ParamDef};
use indexmap::IndexMap;

/// A multi-node signal processing graph in IR form.
pub struct GraphIr {
    /// Number of graph input channels.
    pub inputs: usize,
    /// Number of graph output channels.
    pub outputs: usize,
    /// Named graph nodes (ordered by insertion, or topo order after build).
    pub nodes: IndexMap<String, GraphNode>,
    /// Signal and feedback edges between node ports.
    pub edges: Vec<GraphEdge>,
    /// Topological order of node names (signal edges only).
    pub topo_order: Vec<String>,
}

/// A single node in the graph — a compiled built-in with its IR.
pub struct GraphNode {
    /// (signal_ins, signal_outs) arity.
    pub arity: (usize, usize),
    /// Pre-compiled IR for this node's built-in function.
    pub ir: Ir,
    /// Parameter definitions exposed for automation.
    pub params: Vec<ParamDef>,
    /// If true, this node must remain independent (dynamic params).
    pub keep: bool,
    /// If true, this node should be inlined even with dynamic params.
    pub inline: bool,
}

/// A directed edge between two node ports.
pub struct GraphEdge {
    /// Source node name.
    pub from_node: String,
    /// Output port index on the source node.
    pub from_port: usize,
    /// Destination node name.
    pub to_node: String,
    /// Input port index on the destination node.
    pub to_port: usize,
    /// Edge kind.
    pub kind: EdgeKind,
}

/// Type of graph edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// Forward signal flow edge — participates in topological sort.
    Signal,
    /// Feedback edge with implicit 1-sample delay — excluded from topo sort.
    Feedback,
}
