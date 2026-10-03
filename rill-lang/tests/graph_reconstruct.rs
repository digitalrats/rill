//! GraphSpec reconstruction tests: channel-aware routing and heads-as-builtins.

use std::collections::HashMap;

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::builtin::Registry;
use rill_lang::graph::compile_spec;
use rill_lang::graph::spec::*;

fn test_registry() -> Registry<f32> {
    let mut reg = Registry::new();
    rill_lang::register::register_core_dsp_builtins(&mut reg);
    rill_lang::register::register_core_builtins(&mut reg);
    rill_router::register::register_lang_builtins(&mut reg);
    rill_sampler::tape::lang::register_tape_builtins(&mut reg);
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
    // two sine -> 3-in mixer (2-out) -> mono biquad via channel 0
    let s = spec(
        vec![
            node("sine", &[("freq", 440.0), ("amp", 0.5)], None),
            node("sine", &[("freq", 880.0), ("amp", 0.3)], None),
            node("mixer", &[], None),
            node(
                "biquad",
                &[("filter", 1.0), ("cutoff", 1800.0), ("q", 0.7)],
                None,
            ),
        ],
        vec![edge(0, 0, 2, 0), edge(1, 0, 2, 1), edge(2, 0, 3, 0)],
    );
    let mut engine = compile_spec::<f32, 256>(&s, &test_registry(), 44100.0).unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut engine, &[], &mut [&mut out[..]]).unwrap();
    assert!(
        out.iter().all(|v| v.is_finite()),
        "stereo->mono graph must run finite"
    );
}

#[test]
fn free_input_port_becomes_a_wire() {
    // a dry_wet node with only the wet input wired: the dry input is a free `_`
    // wire, so the program has one signal input.
    let s = spec(
        vec![
            node("sine", &[("freq", 440.0), ("amp", 0.5)], None),
            node("dry_wet", &[], None),
        ],
        vec![edge(0, 0, 1, 1)],
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

#[test]
fn heads_compile_as_builtins_with_shared_tape() {
    use rill_core::buffer::TapeLoop;
    let mut resources = rill_core::buffer::ResourceRegistry::<f32>::new();
    resources.register_buffer("tape_0", Box::new(TapeLoop::<f32>::new(1024).unwrap()));
    let reg = test_registry();

    let src = "tape_0 = TapeLoop 1024\nmain = (_, _) :> write_head tape_0 0.5 0.3";
    let prog = rill_lang::parser::parse(&rill_lang::lexer::tokenize(src).unwrap(), src.as_bytes())
        .unwrap();
    let mut weng =
        rill_lang::compile_program_with_resources::<f32, 256>(&prog, &reg, 44100.0, &mut resources)
            .unwrap();
    let dry = [1.0f32; 64];
    let fb = [0.0f32; 64];
    let mut wout = [0.0f32; 64];
    for _ in 0..40 {
        MultichannelAlgorithm::process(&mut weng, &[&dry, &fb], &mut [&mut wout[..]]).unwrap();
    }
    let src = "tape_0 = TapeLoop 1024\nmain = read_head tape_0 0.1";
    let prog = rill_lang::parser::parse(&rill_lang::lexer::tokenize(src).unwrap(), src.as_bytes())
        .unwrap();
    let mut reng =
        rill_lang::compile_program_with_resources::<f32, 256>(&prog, &reg, 44100.0, &mut resources)
            .unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut reng, &[], &mut [&mut out[..]]).unwrap();
    assert!(
        out.iter().any(|v| v.abs() > 1e-6),
        "read head must see the write head's samples"
    );
}
