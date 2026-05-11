#[cfg(feature = "serialization")]
use rill_adrift::modular::{ModularConfig, ModularSystem};
#[cfg(feature = "serialization")]
use rill_adrift::registration;
#[cfg(feature = "serialization")]
use rill_adrift::rill_core::queues::{SetParameter, SignalOrigin};
#[cfg(feature = "serialization")]
use rill_adrift::rill_core::traits::Node;
#[cfg(feature = "serialization")]
use rill_adrift::rill_core::traits::{ParamValue, ParameterId, PortId};
#[cfg(feature = "serialization")]
use std::sync::atomic::AtomicBool;
#[cfg(feature = "serialization")]
use std::sync::Arc;
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
                "Source": {
                    "id": 0,
                    "type_name": "rill/input",
                    "name": "input",
                    "parameters": {}
                }
            },
            {
                "Processor": {
                    "id": 1,
                    "type_name": "rill/biquad",
                    "name": "filter",
                    "parameters": {
                        "cutoff": 600.0,
                        "q": 1.5,
                        "filter": 1.0
                    }
                }
            },
            {
                "Sink": {
                    "id": 2,
                    "type_name": "rill/output",
                    "name": "output",
                    "parameters": {}
                }
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

    let def = registration::load_graph_json(json).expect("load_graph_json");
    let mut system = ModularSystem::<B>::new(ModularConfig::default());
    let mut p = std::collections::HashMap::new();
    p.insert("sample_rate".into(), ParamValue::Int(RATE as i32));
    p.insert("buffer_size".into(), ParamValue::Int(B as i32));
    p.insert("channels".into(), ParamValue::Int(2));
    system.set_default_backend("null", p);

    let mut builder = system.create_builder();
    def.populate(&mut builder).expect("populate");
    let graph = builder.build().expect("graph build should succeed");

    // Treat Graph as a black box — read metadata only.
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
    assert_eq!(names, ["Input", "BiquadProcessor", "Output"]);
}

#[cfg(feature = "serialization")]
#[test]
fn test_send_parameter_via_queue() {
    const B: usize = 256;

    // Build a graph: sine → null output
    let def = registration::load_graph_json(
        r#"{
        "format_version": "rill/1",
        "sample_rate": 48000.0,
        "block_size": 256,
        "resources": [],
        "nodes": [
            {"Source": {"id": 0, "type_name": "rill/sine", "name": "osc", "parameters": {"freq": 440.0, "amp": 0.5}}},
            {"Sink": {"id": 1, "type_name": "rill/output", "name": "out", "parameters": {}}}
        ],
        "connections": [
            {"kind": "Signal", "from_node": 0, "from_port": 0, "to_node": 1, "to_port": 0},
            {"kind": "Signal", "from_node": 0, "from_port": 0, "to_node": 1, "to_port": 1}
        ],
        "description": null
    }"#,
    )
    .expect("load_graph_json");
    let mut system = ModularSystem::<B>::new(ModularConfig::default());
    let mut p = std::collections::HashMap::new();
    p.insert("sample_rate".into(), ParamValue::Int(RATE as i32));
    p.insert("buffer_size".into(), ParamValue::Int(B as i32));
    p.insert("channels".into(), ParamValue::Int(2));
    system.set_default_backend("null", p);

    let mut builder = system.create_builder();
    def.populate(&mut builder).expect("populate");
    let mut graph = builder.build().expect("graph build");

    // Send parameter via queue
    graph
        .handle()
        .expect("graph has a command queue")
        .send(SetParameter::new(
            PortId::param(rill_adrift::rill_core::NodeId(0), 0),
            ParameterId::new("frequency").unwrap(),
            ParamValue::Float(880.0),
            SignalOrigin::Manual,
        ));

    // Parameter is in the queue — not yet applied (no callback fired).
    // Run graph once — NullBackend fires the callback and returns.
    // running is false, so the park loop exits immediately.
    let running = Arc::new(AtomicBool::new(false));
    graph.run(running).expect("run should succeed");

    // After callback: queue drained, parameter applied.
    let val = graph.nodes()[0].get_parameter(&ParameterId::new("frequency").unwrap());
    assert_eq!(val, Some(ParamValue::Float(880.0)));
}
