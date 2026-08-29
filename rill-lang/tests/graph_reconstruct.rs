//! GraphSpec reconstruction tests: channel-aware compilation of signal graphs.
//!
//! A [`GraphSpec`] with direct builtin names is reconstructed into a single
//! rill-lang program. The channel-aware build must select/drop producer output
//! channels (`<:`) and expose unconnected consumer inputs as free wires (`_`).

use std::collections::HashMap;

use rill_core::math::Transcendental;
use rill_core::traits::MultichannelAlgorithm;
use rill_lang::builtin::Registry;
use rill_lang::graph::{compile, spec::*, CompiledStream};

fn test_registry<T: Transcendental + 'static>() -> Registry<T> {
    let mut reg = Registry::new();
    rill_core_dsp::lang::register::register_lang_builtins(&mut reg);
    rill_lang::register::register_core_builtins(&mut reg);
    rill_router::register::register_lang_builtins(&mut reg);
    reg
}

fn node(type_name: &str, params: &[(&str, f64)]) -> GraphSpecNode {
    GraphSpecNode {
        type_name: type_name.to_string(),
        params: params
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect::<HashMap<_, _>>(),
    }
}

fn sig(from: usize, from_port: usize, to: usize, to_port: usize) -> GraphSpecEdge {
    GraphSpecEdge {
        from,
        from_port,
        to,
        to_port,
        kind: GraphEdgeKind::Signal,
    }
}

/// A stereo (two `sine`) → `mixer` → `biquad` graph compiles and runs finite.
/// The mixer has 2 signal outputs; the mono `biquad` consumer takes channel 0,
/// so the reconstruction must emit a `<:` Split/Cut selection (2 → 1).
#[test]
fn stereo_to_mono_compiles_and_runs_finite() {
    let spec = GraphSpec {
        nodes: vec![
            node("sine", &[("freq", 440.0), ("amp", 0.5)]),
            node("sine", &[("freq", 880.0), ("amp", 0.3)]),
            node("mixer", &[("buses", 0.0)]),
            node("biquad", &[("cutoff", 1800.0), ("q", 0.7)]),
        ],
        edges: vec![sig(0, 0, 2, 0), sig(1, 0, 2, 1), sig(2, 0, 3, 0)],
        resources: vec![],
        sample_rate: 44100.0,
        backends: vec![],
    };
    let registry = test_registry::<f32>();
    let mut engine = match compile(&spec, &registry, 44100.0).unwrap() {
        CompiledStream::Single(e) => e,
        _ => panic!("plain graph must compile to Single, got Duplex"),
    };
    // The two sines expose `phase` and the biquad exposes `gain_db` as dynamic
    // main parameters (program input channels); the graph itself has no free
    // signal wires.
    assert_eq!(engine.num_inputs(), 3);
    assert_eq!(engine.num_outputs(), 1, "mono biquad sink");

    let mut out = vec![0.0f32; 64];
    for _ in 0..16 {
        let mut outs: Vec<&mut [f32]> = vec![&mut out];
        MultichannelAlgorithm::process(&mut engine, &[], &mut outs).unwrap();
        assert!(
            out.iter().all(|v| v.is_finite()),
            "output must stay finite, got {:?}",
            &out[..4]
        );
    }
}

/// A consumer with an unconnected input port compiles: the free channel
/// becomes a `_` wire (a program input) instead of a type error.
#[test]
fn unconnected_consumer_input_compiles_as_free_wire() {
    let spec = GraphSpec {
        nodes: vec![
            node("sine", &[("freq", 440.0), ("amp", 0.5)]),
            node("dry_wet", &[("mix", 0.5)]),
        ],
        edges: vec![sig(0, 0, 1, 0)],
        resources: vec![],
        sample_rate: 44100.0,
        backends: vec![],
    };
    let registry = test_registry::<f32>();
    let mut engine = match compile(&spec, &registry, 44100.0).unwrap() {
        CompiledStream::Single(e) => e,
        _ => panic!("plain graph must compile to Single, got Duplex"),
    };
    // Channel 0 = the sine's `phase` param; channel 1 = the free wet wire.
    assert_eq!(engine.num_inputs(), 2);
    assert_eq!(engine.num_outputs(), 2, "dry_wet is 2-out");

    let input = vec![0.0f32; 64];
    let free = vec![0.1f32; 64];
    let mut out = [vec![0.0f32; 64], vec![0.0f32; 64]];
    for _ in 0..8 {
        let mut outs: Vec<&mut [f32]> = out.iter_mut().map(|v| v.as_mut_slice()).collect();
        MultichannelAlgorithm::process(&mut engine, &[&input, &free], &mut outs).unwrap();
        assert!(
            out.iter().flatten().all(|v| v.is_finite()),
            "output must stay finite"
        );
    }
}
