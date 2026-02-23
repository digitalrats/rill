use kama_core::traits::*;

#[test]
fn test_node_id() {
    let id = NodeId(42);
    assert_eq!(id.0, 42);
    assert_eq!(format!("{}", id), "Node(42)");
}

#[test]
fn test_param_value() {
    let float_val = ParamValue::Float(0.5);
    match float_val {
        ParamValue::Float(f) => assert_eq!(f, 0.5),
        _ => panic!("Wrong variant"),
    }
}