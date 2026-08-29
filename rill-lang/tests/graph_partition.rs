//! Generalized partition tests: active → passive → active traversal splits a
//! graph into recording/playback subgraphs with cross-port edges.

use std::collections::HashMap;

use rill_core::math::Transcendental;
use rill_lang::builtin::Registry;
use rill_lang::graph::partition::partition;
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

fn backend(kind: BackendKind, node: usize) -> BackendAttachment {
    BackendAttachment {
        kind,
        backend_name: match kind {
            BackendKind::Passive => "tape".to_string(),
            _ => "pipewire".to_string(),
        },
        node,
        port: 0,
        params: HashMap::new(),
    }
}

/// The moonlight tape-echo shape: a stereo sum records to the tape
/// (write_head) and feeds the dry path; read heads drive a tap mixer that
/// feeds the output (mixL) and a filtered feedback loop back into the tape.
#[test]
fn moonlight_shape_partitions_into_recording_and_playback() {
    let spec = GraphSpec {
        nodes: vec![
            node("mixer", &[("buses", 0.0)]),                  // 0 stereo_sum
            node("write_head", &[("feedback", 0.35)]),         // 1 passive boundary
            node("read_head", &[("delay", 0.33)]),             // 2 passive boundary
            node("mixer", &[("buses", 0.0)]),                  // 3 tap_mixer
            node("dry_wet", &[("mix", 0.5)]),                  // 4 mixL (sink)
            node("biquad", &[("cutoff", 1800.0), ("q", 0.7)]), // 5 fb_lp
        ],
        edges: vec![
            edge(0, 0, 1, 0, GraphEdgeKind::Signal),   // record
            edge(0, 0, 4, 0, GraphEdgeKind::Signal),   // dry (cross-port)
            edge(2, 0, 3, 0, GraphEdgeKind::Signal),   // tap
            edge(3, 0, 4, 1, GraphEdgeKind::Signal),   // wet
            edge(3, 0, 5, 0, GraphEdgeKind::Signal),   // feedback path
            edge(5, 0, 1, 1, GraphEdgeKind::Feedback), // fb (cross-port)
        ],
        resources: vec![GraphResourceSpec {
            name: "tape_0".to_string(),
            kind: "tape".to_string(),
            capacity: 96000,
        }],
        sample_rate: 44100.0,
        backends: vec![
            backend(BackendKind::ActiveInput, 0),
            backend(BackendKind::Passive, 1),
            backend(BackendKind::Passive, 2),
            backend(BackendKind::ActiveOutput, 4),
        ],
    };

    let parts = partition(&spec);
    assert_eq!(
        parts.len(),
        2,
        "moonlight shape must partition into 2 regions"
    );

    let recording = &parts[0];
    let playback = &parts[1];
    assert_eq!(
        recording.nodes,
        vec![0, 1],
        "recording region is stereo_sum + write_head"
    );
    assert_eq!(
        playback.nodes,
        vec![2, 3, 4, 5],
        "playback region is read_head + tap_mixer + mixL + fb_lp"
    );

    let dry = spec
        .edges
        .iter()
        .position(|e| e.from == 0 && e.to == 4)
        .unwrap();
    let fb = spec
        .edges
        .iter()
        .position(|e| e.from == 5 && e.to == 1)
        .unwrap();
    assert!(
        recording.out_edges.contains(&dry),
        "the dry edge crosses OUT of recording"
    );
    assert!(
        playback.in_edges.contains(&dry),
        "the dry edge crosses INTO playback"
    );
    assert!(
        playback.out_edges.contains(&fb),
        "the fb edge crosses OUT of playback"
    );
    assert!(
        recording.in_edges.contains(&fb),
        "the fb edge crosses INTO recording"
    );

    let registry = test_registry::<f32>();
    match compile(&spec, &registry, 44100.0).unwrap() {
        CompiledStream::Duplex {
            recording: _,
            playback: _,
            tape,
        } => {
            assert_eq!(tape.name, "tape_0");
            assert_eq!(tape.capacity, 96000);
            assert!((tape.write_feedback - 0.35).abs() < 1e-9);
            assert_eq!(tape.read_delays.len(), 1);
        }
        _ => panic!("moonlight-shaped graph must compile to Duplex"),
    }
}

/// A plain graph with no passive backends compiles to a single program.
#[test]
fn plain_graph_without_passive_backends_compiles_single() {
    let spec = GraphSpec {
        nodes: vec![
            node("sine", &[("freq", 440.0), ("amp", 0.5)]),
            node("dry_wet", &[("mix", 0.5)]),
        ],
        edges: vec![edge(0, 0, 1, 0, GraphEdgeKind::Signal)],
        resources: vec![],
        sample_rate: 44100.0,
        backends: vec![
            backend(BackendKind::ActiveInput, 0),
            backend(BackendKind::ActiveOutput, 1),
        ],
    };
    let registry = test_registry::<f32>();
    assert!(
        matches!(
            compile(&spec, &registry, 44100.0).unwrap(),
            CompiledStream::Single(_)
        ),
        "no passive backends ⇒ a single compiled program"
    );
}
