//! Generalized partition tests: each subgraph has exactly one active (rill-io)
//! backend; the other ends are NullBackends.

use std::collections::HashMap;

use rill_lang::builtin::Registry;
use rill_lang::graph::partition::partition;
use rill_lang::graph::spec::*;
use rill_lang::graph::{compile, CompiledStream};

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
    let stream = compile::<f32, 256>(&s, &test_registry(), 44100.0).unwrap();
    assert!(
        matches!(stream, CompiledStream::Single(_)),
        "sine graph compiles to a Single program"
    );
}
