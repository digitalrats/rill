use rill_lang::graph_build::build_graph_ir;
use rill_lang::graph_ir::EdgeKind;

fn compile(str: &str) -> rill_lang::graph_ir::GraphIr {
    let tokens = rill_lang::lexer::tokenize(str).unwrap();
    let program = rill_lang::parser::parse(&tokens).unwrap();
    let typed = rill_lang::types::infer::infer_program(&program).unwrap();
    build_graph_ir(&typed).unwrap()
}

#[test]
fn plain_process_no_params_is_empty_graph() {
    let ir = compile("process = _ * 0.5;");
    assert_eq!(
        ir.nodes.len(),
        0,
        "plain process with no param defs produces empty graph"
    );
}

#[test]
fn param_def_becomes_graph_node() {
    let ir = compile("param filt = sin(440.0); process = filt : _;");
    assert_eq!(ir.nodes.len(), 1);
    assert!(ir.nodes.contains_key("filt"));
}

#[test]
fn signal_edges_between_param_nodes() {
    let ir = compile("param osc = sin(440.0); param gain = _ * 0.5; process = osc : gain : _;");
    assert_eq!(ir.nodes.len(), 2);
    let signal_edges: Vec<_> = ir
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Signal)
        .collect();
    assert!(
        !signal_edges.is_empty(),
        "seq combinator creates signal edges"
    );
}

#[test]
fn feedback_edge_created() {
    let ir = compile("param del = _ * 0.5; process = _ : del ~ _;");
    let fb_edges: Vec<_> = ir
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Feedback)
        .collect();
    assert!(!fb_edges.is_empty(), "~ operator creates feedback edge");
}

#[test]
fn keep_param_has_keep_flag() {
    let ir = compile("keep param kf = _ * 0.5; process = kf : _;");
    assert!(ir.nodes["kf"].keep);
}

#[test]
fn inline_param_has_force_inline_flag() {
    let ir = compile("inline param iff = _ * 0.5; process = iff : _;");
    assert!(ir.nodes["iff"].force_inline);
}

#[test]
fn def_functions_are_not_graph_nodes() {
    let ir = compile("scale(x) = x * 0.5; param filt = _ : scale; process = filt : _;");
    assert_eq!(ir.nodes.len(), 1, "def is inlined, only param is a node");
}
