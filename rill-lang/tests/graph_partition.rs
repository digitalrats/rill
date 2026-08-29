//! Generalized partition tests: each subgraph has exactly one active (rill-io)
//! backend; the other ends are NullBackends.

use std::collections::HashMap;

use rill_lang::builtin::Registry;
use rill_lang::graph::partition::partition;
use rill_lang::graph::spec::*;
use rill_lang::graph::{compile, CompiledStream};

fn test_registry() -> Registry<f32> {
    let mut reg = Registry::new();
    rill_core_dsp::lang::register::register_lang_builtins(&mut reg);
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

fn edge(
    from: usize,
    from_port: usize,
    to: usize,
    to_port: usize,
    kind: GraphEdgeKind,
) -> GraphSpecEdge {
    GraphSpecEdge {
        from,
        from_port,
        to,
        to_port,
        kind,
    }
}

fn attach(input: bool, node: usize) -> BackendAttachment {
    BackendAttachment {
        input,
        backend_name: "pipewire".to_string(),
        node,
        port: 0,
    }
}

/// sine(passive) -> lowpass -> [active output on lowpass]. One subgraph; the
/// input end is a NullBackend (no active input), the output is rill-io.
#[test]
fn sine_graph_is_one_subprogram_with_nullbackend_input() {
    let s = GraphSpec {
        nodes: vec![
            node(
                "sine",
                &[("freq", 440.0), ("amp", 0.5)],
                Some(NodeBackendKind::Passive),
            ),
            node("lowpass", &[], None),
        ],
        edges: vec![edge(0, 0, 1, 0, GraphEdgeKind::Signal)],
        resources: Vec::new(),
        sample_rate: 44100.0,
        backends: vec![attach(true, 1)],
        boundary_out: Vec::new(),
        input_ports: Vec::new(),
    };
    let parts = partition(&s);
    assert_eq!(parts.len(), 1, "sine graph partitions to one subgraph");
    let stream = compile(&s, &test_registry(), 44100.0).unwrap();
    assert!(
        matches!(stream, CompiledStream::Single(_)),
        "sine graph compiles to a Single program"
    );
}

/// moonlight tape echo: ActiveInput(capture) on stereo_sum, Passive write_head
/// and read_head, ActiveOutput(playback) on mixL. Two subgraphs; recording has
/// an active input + NullBackend output, playback a NullBackend input + active
/// output.
#[test]
fn tape_echo_splits_into_two_subprograms() {
    let s = GraphSpec {
        nodes: vec![
            node("mixer", &[], None), // 0 stereo_sum
            node(
                "write_head",
                &[("delay_time", 0.5), ("feedback", 0.35)],
                Some(NodeBackendKind::Passive),
            ), // 1 write_head
            node(
                "read_head",
                &[("delay", 0.1)],
                Some(NodeBackendKind::Passive),
            ), // 2 read_head
            node("mixer", &[], None), // 3 tap_mixer
            node("dry_wet", &[], None), // 4 mixL
            node("biquad", &[], None), // 5 fb_lp
        ],
        edges: vec![
            edge(0, 0, 1, 0, GraphEdgeKind::Signal), // stereo_sum -> write_head.dry
            edge(2, 0, 3, 0, GraphEdgeKind::Signal), // read_head -> tap_mixer
            edge(3, 0, 4, 0, GraphEdgeKind::Signal), // tap_mixer.L -> mixL
            edge(3, 1, 5, 0, GraphEdgeKind::Signal), // tap_mixer.R -> fb_lp
            edge(5, 0, 1, 1, GraphEdgeKind::Feedback), // fb_lp -> write_head.feedback
            edge(0, 1, 4, 0, GraphEdgeKind::Signal), // stereo_sum.R -> mixL.dry (cross: dry)
        ],
        resources: vec![GraphResourceSpec {
            name: "tape_0".into(),
            kind: "tape".into(),
            capacity: 96000,
        }],
        sample_rate: 44100.0,
        backends: vec![attach(true, 0), attach(false, 4)],
        boundary_out: Vec::new(),
        input_ports: Vec::new(),
    };
    let parts = partition(&s);
    assert_eq!(parts.len(), 2, "tape echo partitions into two subgraphs");
    // recording owns stereo_sum + write_head; playback owns the read side.
    let rec = parts
        .iter()
        .find(|p| p.nodes.contains(&1))
        .expect("recording region");
    let pb = parts
        .iter()
        .find(|p| p.nodes.contains(&3))
        .expect("playback region");
    assert!(rec.nodes.contains(&0) && rec.nodes.contains(&1));
    assert!(
        pb.nodes.contains(&2)
            && pb.nodes.contains(&3)
            && pb.nodes.contains(&4)
            && pb.nodes.contains(&5)
    );

    let stream = compile(&s, &test_registry(), 44100.0).unwrap();
    match stream {
        CompiledStream::Duplex {
            recording: _,
            playback: _,
            resources,
            tape,
        } => {
            assert_eq!(tape.capacity, 96000);
            assert_eq!(tape.read_delays, vec![0.1]);
            assert!((tape.write_feedback - 0.35).abs() < 1e-9);
            assert!(!resources.is_empty(), "shared tape must be registered");
        }
        other => panic!("expected duplex, got Single"),
    }
}
