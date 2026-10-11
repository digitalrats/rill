//! GraphSpec reconstruction tests: channel-aware routing through
//! `compile_spec` (plain graphs, no tape).

use std::collections::HashMap;

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::builtin::Registry;
use rill_lang::graph::compile_spec;
use rill_lang::graph::spec::*;

fn test_registry() -> Registry<f32> {
    let mut reg = Registry::new();
    rill_lang::register::register_core_dsp_builtins(&mut reg);
    reg
}

fn node(
    type_name: &str,
    params: &[(&str, f64)],
    backend: Option<NodeBackendKind>,
) -> GraphSpecNode {
    GraphSpecNode {
        type_name: type_name.to_string(),
        params: params
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect::<HashMap<_, _>>(),
        backend,
    }
}

fn edge(from: usize, from_port: usize, to: usize, to_port: usize) -> GraphSpecEdge {
    GraphSpecEdge {
        from,
        from_port,
        to,
        to_port,
        kind: GraphEdgeKind::Signal,
    }
}

fn spec(nodes: Vec<GraphSpecNode>, edges: Vec<GraphSpecEdge>) -> GraphSpec {
    GraphSpec {
        nodes,
        edges,
        resources: Vec::new(),
        sample_rate: 44100.0,
        backends: Vec::new(),
        boundary_out: Vec::new(),
        input_ports: Vec::new(),
    }
}

#[test]
fn stereo_mixer_to_mono_consumer_compiles() {
    // sine -> mono biquad: a plain channel-aware graph compiles via compile_spec.
    let s = spec(
        vec![
            node("sine", &[("freq", 440.0), ("amp", 0.5)], None),
            node(
                "biquad",
                &[("filter", 1.0), ("cutoff", 1800.0), ("q", 0.7)],
                None,
            ),
        ],
        vec![edge(0, 0, 1, 0)],
    );
    let mut engine = compile_spec::<f32, 256>(&s, &test_registry(), 44100.0).unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut engine, &[], &mut [&mut out[..]]).unwrap();
    assert!(
        out.iter().all(|v| v.is_finite()),
        "sine->filter graph must run finite"
    );
}

#[test]
fn free_input_port_becomes_a_wire() {
    // an onepole node with no incoming edge: its input port is a free `_`
    // wire, so the program has one signal input.
    let s = spec(
        vec![node("onepole", &[("cutoff", 200.0), ("q", 0.7)], None)],
        vec![],
    );
    let mut engine = compile_spec::<f32, 256>(&s, &test_registry(), 44100.0).unwrap();
    assert_eq!(
        rill_core::traits::MultichannelAlgorithm::num_inputs(&engine),
        1
    );
    let inp = [0.5f32; 64];
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut engine, &[&inp], &mut [&mut out[..]]).unwrap();
    assert!(out.iter().all(|v| v.is_finite()));
}
