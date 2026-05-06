#[cfg(feature = "serialization")]
use rill_adrift::registration;
#[cfg(feature = "serialization")]
use rill_adrift::rill_core::time::SystemClock;
#[cfg(feature = "serialization")]
use rill_adrift::rill_core::traits::SignalNode;
#[cfg(feature = "serialization")]
use rill_adrift::rill_graph::backend_factory::{BackendConfig, BackendFactory};

#[cfg(feature = "serialization")]
const RATE: f32 = 48000.0;

#[cfg(feature = "serialization")]
#[test]
fn test_deserialize_input_biquad_output() {
    const B: usize = 256;

    let json = r#"{
        "format_version": "rill/1",
        "sample_rate": 48000.0,
        "block_size": 256,
        "resources": [],
        "nodes": [
            {
                "id": 0,
                "type_name": "rill/input",
                "name": "input",
                "parameters": {}
            },
            {
                "id": 1,
                "type_name": "rill/biquad",
                "name": "filter",
                "parameters": {
                    "cutoff": 600.0,
                    "q": 1.5,
                    "filter": 1.0
                }
            },
            {
                "id": 2,
                "type_name": "rill/output",
                "name": "output",
                "parameters": {}
            }
        ],
        "connections": [
            {"kind": "Signal", "from_node": 0, "from_port": 0, "to_node": 1, "to_port": 0},
            {"kind": "Signal", "from_node": 0, "from_port": 1, "to_node": 1, "to_port": 1},
            {"kind": "Signal", "from_node": 1, "from_port": 0, "to_node": 2, "to_port": 0},
            {"kind": "Signal", "from_node": 1, "from_port": 1, "to_node": 2, "to_port": 1}
        ],
        "description": null
    }"#;

    let builder = registration::load_graph_json::<B>(json).expect("load_graph_json should succeed");

    let mut backend_factory = BackendFactory::<f32>::new();
    registration::register_backends(&mut backend_factory);

    let backend_cfg = BackendConfig {
        factory: &backend_factory,
        name: "null",
        sample_rate: RATE as u32,
        buffer_size: B as u32,
        channels: 2,
    };

    let clock = Box::new(SystemClock::with_sample_rate(RATE));
    let graph = builder
        .build(clock, Some(&backend_cfg))
        .expect("graph build should succeed");

    // Treat SignalGraph as a black box — read metadata only.
    assert_eq!(graph.node_count(), 3, "should have 3 nodes");
    assert_eq!(
        graph.topo_order().len(),
        3,
        "topo order should cover all nodes"
    );

    let names: Vec<String> = graph
        .nodes()
        .iter()
        .map(|n| n.metadata().name.clone())
        .collect();
    assert_eq!(names, ["AudioInput", "BiquadProcessor", "AudioOutput"]);
}
