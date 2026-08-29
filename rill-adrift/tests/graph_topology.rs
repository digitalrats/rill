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
