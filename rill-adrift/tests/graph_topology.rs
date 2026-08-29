//! Graph topology tests via the thin frontend (`GraphBuilder.compile_def`),
//! which delegates IR formation to rill-lang. Node names are direct builtin
//! names (no `rill/` prefix, no aliases).
use rill_adrift::lang_builtins::full_registry_f32;
use rill_core::traits::{MultichannelAlgorithm, ParamValue, Params};
use rill_graph::GraphBuilder;

#[test]
fn fan_in_graph_compiles_and_runs() {
    let reg = full_registry_f32();
    let mut b: GraphBuilder<f32, 256> = GraphBuilder::new();

    let mut s1 = Params::new(44100.0);
    s1.insert("freq", ParamValue::Float(440.0));
    s1.insert("amp", ParamValue::Float(0.5));
    b.add_node("sine", &s1);

    let mut s2 = Params::new(44100.0);
    s2.insert("freq", ParamValue::Float(880.0));
    s2.insert("amp", ParamValue::Float(0.3));
    b.add_node("sine", &s2);

    let m = Params::new(44100.0);
    b.add_node("mixer", &m);

    b.connect_signal(0, 0, 2, 0);
    b.connect_signal(1, 0, 2, 1);

    let mut engine = b.compile_def(&reg, 44100.0).unwrap();
    let mut out = vec![0.0f32; 64];
    MultichannelAlgorithm::process(&mut engine, &[], &mut [&mut out[..]]).unwrap();
    assert!(
        out.iter().all(|v| v.is_finite()),
        "fan-in graph must run finite"
    );
}
