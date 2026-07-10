//! Graph IR optimizer passes.

use crate::graph_ir::GraphIr;

/// Apply all optimization passes to a GraphIr in-place.
pub fn optimize(ir: &mut GraphIr) {
    eliminate_dead_edges(ir);
    inline_const_nodes(ir);
    merge_parallel_nodes(ir);
    reorder_lti(ir);
}

/// Remove edges whose source or destination node no longer exists.
fn eliminate_dead_edges(ir: &mut GraphIr) {
    ir.edges
        .retain(|e| ir.nodes.contains_key(&e.from_node) && ir.nodes.contains_key(&e.to_node));
}

/// Inline nodes that have no dynamic parameters and are not marked keep.
fn inline_const_nodes(ir: &mut GraphIr) {
    let mut to_remove = Vec::new();

    for (name, node) in &ir.nodes {
        if node.keep {
            continue;
        }
        if node.inline || node.params.is_empty() {
            to_remove.push(name.clone());
        }
    }

    for name in &to_remove {
        ir.nodes.shift_remove(name);
        ir.topo_order.retain(|n| n != name);
    }
}

/// Merge nodes with identical IR structure but different constant params.
fn merge_parallel_nodes(_ir: &mut GraphIr) {
    // Stub — to be implemented when IR comparison is available.
}

/// Reorder adjacent LTI nodes to reduce buffer pressure.
fn reorder_lti(_ir: &mut GraphIr) {
    // Stub — requires LTI detection on IR.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_ir::{EdgeKind, GraphEdge, GraphIr, GraphNode};
    use crate::ir::{Ir, StateLayout};
    use indexmap::IndexMap;

    fn empty_ir() -> Ir {
        Ir {
            instrs: Vec::new(),
            num_regs: 0,
            output_reg: 0,
            num_inputs: 1,
            num_outputs: 1,
            state: StateLayout {
                state_slots: 0,
                delay_lens: Vec::new(),
                num_outputs: 1,
            },
            builtins: Vec::new(),
            params: Vec::new(),
        }
    }

    #[test]
    fn eliminate_dead_edges_removes_dangling() {
        let mut ir = GraphIr {
            inputs: 1,
            outputs: 1,
            nodes: IndexMap::new(),
            edges: vec![
                GraphEdge {
                    from_node: "a".into(),
                    from_port: 0,
                    to_node: "b".into(),
                    to_port: 0,
                    kind: EdgeKind::Signal,
                },
                GraphEdge {
                    from_node: "b".into(),
                    from_port: 0,
                    to_node: "c".into(),
                    to_port: 0,
                    kind: EdgeKind::Signal,
                },
            ],
            topo_order: vec!["a".into(), "b".into()],
        };
        ir.nodes.insert(
            "a".into(),
            GraphNode {
                arity: (1, 1),
                ir: empty_ir(),
                params: Vec::new(),
                keep: false,
                inline: false,
                is_bridge: false,
                feedback_read: vec![],
                feedback_write: vec![],
            },
        );
        ir.nodes.insert(
            "b".into(),
            GraphNode {
                arity: (1, 1),
                ir: empty_ir(),
                params: Vec::new(),
                keep: false,
                inline: false,
                is_bridge: false,
                feedback_read: vec![],
                feedback_write: vec![],
            },
        );

        optimize(&mut ir);
        assert_eq!(ir.edges.len(), 1);
        assert_eq!(ir.nodes.len(), 0);
        assert!(ir.topo_order.is_empty());
    }

    #[test]
    fn keep_nodes_not_inlined() {
        let mut ir = GraphIr {
            inputs: 1,
            outputs: 1,
            nodes: IndexMap::new(),
            edges: Vec::new(),
            topo_order: vec!["a".into()],
        };
        ir.nodes.insert(
            "a".into(),
            GraphNode {
                arity: (1, 1),
                ir: empty_ir(),
                params: Vec::new(),
                keep: true,
                inline: false,
                is_bridge: false,
                feedback_read: vec![],
                feedback_write: vec![],
            },
        );

        optimize(&mut ir);
        assert_eq!(ir.nodes.len(), 1);
        assert!(ir.topo_order.contains(&"a".to_string()));
    }
}
