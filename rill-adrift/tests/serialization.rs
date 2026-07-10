#[cfg(feature = "serialization")]
use rill_adrift::registration;

#[cfg(feature = "serialization")]
#[test]
fn test_deserialize_input_biquad_output() {
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
    assert_eq!(def.nodes.len(), 3, "should have 3 nodes");
    assert_eq!(def.connections.len(), 4, "should have 4 connections");
    assert_eq!(def.sample_rate, 48000.0, "should preserve sample_rate");
}

#[cfg(feature = "serialization")]
#[test]
fn test_sine_graph_deserialization() {
    let json = r#"{
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
    }"#;

    let def = registration::load_graph_json(json).expect("load_graph_json");
    assert_eq!(def.nodes.len(), 2, "should have 2 nodes");
    assert_eq!(def.connections.len(), 2, "should have 2 connections");
}
