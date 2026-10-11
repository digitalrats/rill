//! Graph topology tests via the thin frontend (`GraphBuilder.compile_def`),
//! which delegates IR formation to rill-lang. Node names are direct builtin
//! names (no `rill/` prefix, no aliases).
use rill_adrift::lang_builtins::full_registry_f32;
use rill_core::traits::{MultichannelAlgorithm, ParamValue, Params};
use rill_graph::GraphBuilder;

#[test]
fn graph_compiles_and_runs() {
    let reg = full_registry_f32();
    let mut b: GraphBuilder<f32, 256> = GraphBuilder::new();

    let mut s1 = Params::new(44100.0);
    s1.insert("freq", ParamValue::Float(440.0));
    s1.insert("amp", ParamValue::Float(0.5));
    b.add_node("sine", &s1);

    let mut f = Params::new(44100.0);
    f.insert("cutoff", ParamValue::Float(200.0));
    f.insert("q", ParamValue::Float(0.7));
    b.add_node("onepole", &f);

    b.connect_signal(0, 0, 1, 0);

    let mut engine = b.compile_def(&reg, 44100.0).unwrap();
    let mut out = vec![0.0f32; 64];
    MultichannelAlgorithm::process(&mut engine, &[], &mut [&mut out[..]]).unwrap();
    assert!(out.iter().all(|v| v.is_finite()), "graph must run finite");
}
