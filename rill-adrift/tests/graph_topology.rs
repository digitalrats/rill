//! Graph topology reconstruction tests: fan-in/fan-out via DSL combinators.
use rill_adrift::lang_builtins::full_registry_f32;
use rill_core::traits::{ParamValue, Params};
use rill_graph::GraphBuilder;
use rill_lang::ast::{BinOp, Expr};

fn has_merge(e: &Expr) -> bool {
    match e {
        Expr::Bin { op, lhs, rhs, .. } => {
            matches!(op, BinOp::Merge) || has_merge(lhs) || has_merge(rhs)
        }
        _ => false,
    }
}

#[test]
fn fan_in_reconstructs_to_merge() {
    let reg = full_registry_f32();
    let mut b: GraphBuilder<f32, 256> = GraphBuilder::new();

    let mut s1 = Params::new(44100.0);
    s1.insert("freq", ParamValue::Float(440.0));
    s1.insert("amp", ParamValue::Float(0.5));
    b.add_node("rill/sine", &s1);

    let mut s2 = Params::new(44100.0);
    s2.insert("freq", ParamValue::Float(880.0));
    s2.insert("amp", ParamValue::Float(0.3));
    b.add_node("rill/sine", &s2);

    let mut m = Params::new(44100.0);
    m.insert("buses", ParamValue::Float(0.0));
    b.add_node("rill/mixer", &m);

    b.connect_signal(0, 0, 2, 0);
    b.connect_signal(1, 0, 2, 1);

    let ast = b.ast_from_def(&reg).unwrap();
    let body = ast.main_def().unwrap().body();
    assert!(
        has_merge(body),
        "fan-in graph should reconstruct to a Merge combinator"
    );
}

#[test]
fn graph_read_head_with_tape_resource_compiles() {
    use rill_core::traits::MultichannelAlgorithm;
    let reg = full_registry_f32();
    let mut b: GraphBuilder<f32, 256> = GraphBuilder::new();

    b.add_resource(rill_graph::GraphResource {
        name: "tape_0".to_string(),
        kind: "tape".to_string(),
        capacity: 96000,
    });

    let mut rh = Params::new(44100.0);
    rh.insert("delay", ParamValue::Float(0.1));
    b.add_node("rill/read_head", &rh);

    let mut engine = b.compile_def(&reg, 44100.0).unwrap();
    let mut out = vec![0.0f32; 64];
    let mut outs: [&mut [f32]; 1] = [&mut out];
    MultichannelAlgorithm::process(&mut engine, &[], &mut outs).unwrap();
    assert!(out.iter().all(|v| v.is_finite()));
}

fn has_feedback(e: &Expr) -> bool {
    match e {
        Expr::Bin { op, lhs, rhs, .. } => {
            matches!(op, BinOp::Feedback) || has_feedback(lhs) || has_feedback(rhs)
        }
        _ => false,
    }
}

#[test]
fn feedback_edge_reconstructs_to_feedback_tap() {
    let reg = full_registry_f32();
    let mut b: GraphBuilder<f32, 256> = GraphBuilder::new();

    b.add_resource(rill_graph::GraphResource {
        name: "tape_0".to_string(),
        kind: "tape".to_string(),
        capacity: 96000,
    });

    // write_head (2 in: dry + feedback), read_head (0 in)
    let mut wh = Params::new(44100.0);
    wh.insert("delay_time", ParamValue::Float(0.5));
    wh.insert("feedback", ParamValue::Float(0.35));
    b.add_node("rill/write_head", &wh);

    let mut rh = Params::new(44100.0);
    rh.insert("delay", ParamValue::Float(0.33));
    b.add_node("rill/read_head", &rh);

    // feedback edge: read_head -> write_head (feedback input, port 1)
    b.connect_feedback(1, 0, 0, 1);

    let ast = b.ast_from_def(&reg).unwrap();
    let body = ast.main_def().unwrap().body();
    assert!(
        has_feedback(body),
        "feedback edge should reconstruct to a Feedback combinator"
    );
}
