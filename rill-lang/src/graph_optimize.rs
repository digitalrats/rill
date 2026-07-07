//! Graph IR optimizer — applies passes to reduce node count and buffer pressure.

use crate::graph_ir::{EdgeKind, GraphEdge, GraphIr};
use std::collections::HashSet;

/// Run all optimization passes on the graph IR.
pub fn optimize(ir: &mut GraphIr) {
    dead_edge_elimination(ir);
    inline_nodes(ir);
}

/// Remove signal edges whose destination node doesn't exist.
/// Feedback edges are always preserved (they represent delayed data paths).
fn dead_edge_elimination(ir: &mut GraphIr) {
    let node_names: HashSet<&str> = ir.nodes.keys().map(|s| s.as_str()).collect();
    ir.edges.retain(|e| {
        e.kind == EdgeKind::Feedback
            || (node_names.contains(e.from_node.as_str())
                && node_names.contains(e.to_node.as_str()))
    });
}

/// Inline nodes marked `force_inline` (unless `keep`).
///
/// When a node is inlined, incoming edges P→M and outgoing edges M→N
/// are rewired as P→N and the inlined node is removed.
fn inline_nodes(ir: &mut GraphIr) {
    let to_inline: HashSet<String> = ir
        .nodes
        .iter()
        .filter(|(_, node)| node.force_inline && !node.keep)
        .map(|(name, _)| name.clone())
        .collect();

    if to_inline.is_empty() {
        return;
    }

    let mut new_edges = Vec::new();
    for edge in &ir.edges {
        if to_inline.contains(&edge.to_node) && !to_inline.contains(&edge.from_node) {
            let mid = &edge.to_node;
            for out_edge in &ir.edges {
                if &out_edge.from_node == mid && out_edge.to_node != edge.from_node {
                    new_edges.push(GraphEdge {
                        from_node: edge.from_node.clone(),
                        from_port: edge.from_port,
                        to_node: out_edge.to_node.clone(),
                        to_port: out_edge.to_port,
                        kind: out_edge.kind,
                    });
                }
            }
        } else if !to_inline.contains(&edge.from_node) && !to_inline.contains(&edge.to_node) {
            new_edges.push(edge.clone());
        }
    }

    ir.nodes.retain(|name, _| !to_inline.contains(name));
    ir.edges = new_edges;
}
