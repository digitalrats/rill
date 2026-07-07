use indexmap::IndexMap;
use rill_lang::graph_ir::{EdgeKind, GraphIr, GraphNode};
use rill_lang::graph_optimize::optimize;

fn make_node(keep: bool, force_inline: bool) -> GraphNode {
    GraphNode {
        arity: (1, 1),
        ir: rill_lang::ir::Ir {
            instrs: vec![],
            num_regs: 0,
            output_reg: 0,
            num_inputs: 1,
            state: Default::default(),
            builtins: vec![],
            params: vec![],
        },
        params: vec![],
        keep,
        force_inline,
    }
}

fn edge(from: &str, to: &str, kind: EdgeKind) -> rill_lang::graph_ir::GraphEdge {
    rill_lang::graph_ir::GraphEdge {
        from_node: from.into(),
        from_port: 0,
        to_node: to.into(),
        to_port: 0,
        kind,
    }
}

#[test]
fn inline_param_is_removed() {
    let mut nodes = IndexMap::new();
    nodes.insert("a".into(), make_node(false, true));
    nodes.insert("b".into(), make_node(false, false));
    let mut ir = GraphIr {
        inputs: 0,
        outputs: 1,
        nodes,
        edges: vec![edge("a", "b", EdgeKind::Signal)],
    };
    optimize(&mut ir);
    assert!(
        !ir.nodes.contains_key("a"),
        "inline param should be removed"
    );
    assert!(ir.nodes.contains_key("b"));
}

#[test]
fn keep_param_not_removed() {
    let mut nodes = IndexMap::new();
    nodes.insert("k".into(), make_node(true, false));
    let mut ir = GraphIr {
        inputs: 0,
        outputs: 1,
        nodes,
        edges: vec![],
    };
    optimize(&mut ir);
    assert!(
        ir.nodes.contains_key("k"),
        "keep param should survive inlining"
    );
}

#[test]
fn dead_edge_removed() {
    let mut nodes = IndexMap::new();
    nodes.insert("src".into(), make_node(false, false));
    nodes.insert("sink".into(), make_node(false, false));
    let mut ir = GraphIr {
        inputs: 0,
        outputs: 1,
        nodes,
        edges: vec![
            edge("src", "orphan", EdgeKind::Signal),
            edge("src", "sink", EdgeKind::Signal),
        ],
    };
    optimize(&mut ir);
    let signal_edges: Vec<_> = ir
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Signal)
        .collect();
    assert_eq!(signal_edges.len(), 1, "orphan edge should be removed");
    assert_eq!(signal_edges[0].to_node, "sink");
}

#[test]
fn feedback_edges_survive_dce() {
    let mut nodes = IndexMap::new();
    nodes.insert("a".into(), make_node(false, false));
    nodes.insert("b".into(), make_node(false, false));
    let mut ir = GraphIr {
        inputs: 0,
        outputs: 1,
        nodes,
        edges: vec![
            edge("a", "b", EdgeKind::Signal),
            edge("b", "a", EdgeKind::Feedback),
        ],
    };
    optimize(&mut ir);
    let fb: Vec<_> = ir
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Feedback)
        .collect();
    assert_eq!(fb.len(), 1, "feedback edges survive DCE");
}
