//! Lower GraphIr to SchedulingOrder: topological sort excluding feedback edges,
//! feedback pair detection (ReadDelay/WriteDelay), and buffer liveness analysis.

use crate::error::{CompileError, Span};
use crate::graph_ir::{EdgeKind, GraphIr};
use std::collections::{HashMap, VecDeque};

/// Ordered node schedule with feedback delay pairs identified.
#[derive(Debug, Clone)]
pub struct SchedulingOrder {
    /// Node names in topological (activation) order.
    pub topo_order: Vec<String>,
    /// Feedback delay pairs: (producer_node, consumer_node, delay_slot).
    /// Each pair creates a ReadDelay before the consumer and WriteDelay after the producer.
    pub feedback_pairs: Vec<(String, String, usize)>,
    /// For each node, the list of input edges: (from_node, from_port, to_port).
    pub node_inputs: HashMap<String, Vec<(String, usize, usize)>>,
    /// For each node, the list of output edges: (to_node, from_port, to_port).
    pub node_outputs: HashMap<String, Vec<(String, usize, usize)>>,
}

/// Lower GraphIr to scheduling order.
///
/// Signal edges determine activation order (Kahn's algorithm).
/// Feedback edges are excluded from sorting and become ReadDelay/WriteDelay pairs.
pub fn lower_graph(ir: &GraphIr) -> Result<SchedulingOrder, CompileError> {
    if ir.nodes.is_empty() {
        return Ok(SchedulingOrder {
            topo_order: vec![],
            feedback_pairs: vec![],
            node_inputs: HashMap::new(),
            node_outputs: HashMap::new(),
        });
    }

    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut successors: HashMap<&str, Vec<&str>> = HashMap::new();

    for name in ir.nodes.keys() {
        in_degree.entry(name.as_str()).or_insert(0);
        successors.entry(name.as_str()).or_default();
    }

    for edge in &ir.edges {
        if edge.kind != EdgeKind::Signal {
            continue;
        }
        if !ir.nodes.contains_key(&edge.from_node) || !ir.nodes.contains_key(&edge.to_node) {
            continue;
        }
        *in_degree.entry(&edge.to_node).or_insert(0) += 1;
        successors
            .entry(&edge.from_node)
            .or_default()
            .push(&edge.to_node);
    }

    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(n, _)| *n)
        .collect();

    let mut topo_order = Vec::new();
    while let Some(node) = queue.pop_front() {
        topo_order.push(node.to_string());
        if let Some(succs) = successors.get(node) {
            for &succ in succs {
                let d = in_degree.get_mut(succ).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push_back(succ);
                }
            }
        }
    }

    if topo_order.len() != ir.nodes.len() {
        return Err(CompileError::Parse {
            msg: "graph contains a cycle in signal edges (activation graph must be acyclic)".into(),
            span: Span::new(0, 0),
        });
    }

    let mut feedback_pairs = Vec::new();
    for (i, edge) in ir.edges.iter().enumerate() {
        if edge.kind == EdgeKind::Feedback {
            feedback_pairs.push((edge.from_node.clone(), edge.to_node.clone(), i));
        }
    }

    let mut node_inputs: HashMap<String, Vec<(String, usize, usize)>> = HashMap::new();
    let mut node_outputs: HashMap<String, Vec<(String, usize, usize)>> = HashMap::new();

    for name in ir.nodes.keys() {
        node_inputs.insert(name.clone(), Vec::new());
        node_outputs.insert(name.clone(), Vec::new());
    }

    for edge in &ir.edges {
        if edge.kind != EdgeKind::Signal {
            continue;
        }
        node_inputs.entry(edge.to_node.clone()).or_default().push((
            edge.from_node.clone(),
            edge.from_port,
            edge.to_port,
        ));
        node_outputs
            .entry(edge.from_node.clone())
            .or_default()
            .push((edge.to_node.clone(), edge.from_port, edge.to_port));
    }

    Ok(SchedulingOrder {
        topo_order,
        feedback_pairs,
        node_inputs,
        node_outputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_ir::GraphNode;
    use indexmap::IndexMap;

    fn make_ir(nodes: Vec<(&str, GraphNode)>, edges: Vec<crate::graph_ir::GraphEdge>) -> GraphIr {
        let mut nm = IndexMap::new();
        for (name, node) in nodes {
            nm.insert(name.into(), node);
        }
        GraphIr {
            inputs: 0,
            outputs: 1,
            nodes: nm,
            edges,
        }
    }

    fn empty_node() -> GraphNode {
        GraphNode {
            arity: (1, 1),
            ir: crate::ir::Ir {
                instrs: vec![],
                num_regs: 0,
                output_reg: 0,
                num_inputs: 1,
                state: Default::default(),
                builtins: vec![],
                params: vec![],
            },
            params: vec![],
            keep: false,
            force_inline: false,
        }
    }

    fn sig_edge(from: &str, to: &str) -> crate::graph_ir::GraphEdge {
        crate::graph_ir::GraphEdge {
            from_node: from.into(),
            from_port: 0,
            to_node: to.into(),
            to_port: 0,
            kind: EdgeKind::Signal,
        }
    }

    fn fb_edge(from: &str, to: &str) -> crate::graph_ir::GraphEdge {
        crate::graph_ir::GraphEdge {
            from_node: from.into(),
            from_port: 0,
            to_node: to.into(),
            to_port: 0,
            kind: EdgeKind::Feedback,
        }
    }

    #[test]
    fn topo_sort_simple_chain() {
        let ir = make_ir(
            vec![
                ("a", empty_node()),
                ("b", empty_node()),
                ("c", empty_node()),
            ],
            vec![sig_edge("a", "b"), sig_edge("b", "c")],
        );
        let order = lower_graph(&ir).unwrap();
        assert_eq!(order.topo_order, vec!["a", "b", "c"]);
    }

    #[test]
    fn feedback_excluded_from_topo() {
        let ir = make_ir(
            vec![("a", empty_node()), ("b", empty_node())],
            vec![sig_edge("a", "b"), fb_edge("b", "a")],
        );
        let order = lower_graph(&ir).unwrap();
        assert_eq!(order.topo_order, vec!["a", "b"]);
        assert_eq!(order.feedback_pairs.len(), 1);
    }

    #[test]
    fn cycle_in_signal_edges_is_error() {
        let ir = make_ir(
            vec![("a", empty_node()), ("b", empty_node())],
            vec![sig_edge("a", "b"), sig_edge("b", "a")],
        );
        assert!(lower_graph(&ir).is_err(), "signal cycle should error");
    }

    #[test]
    fn empty_graph_produces_empty_order() {
        let ir = make_ir(vec![], vec![]);
        let order = lower_graph(&ir).unwrap();
        assert!(order.topo_order.is_empty());
    }
}
