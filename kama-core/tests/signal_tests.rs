use kama_core::signal::*;

#[test]
fn test_signal_bus_basic() {
    let bus = SignalBus::<ParameterChanged>::new(BusConfig::Unbounded);
    let receiver = bus.receiver();
    
    let signal = ParameterChanged {
        node_id: "node_42".to_string(),  // используем String
        parameter_id: "test".to_string(),
        value: 1.0,
        normalized_value: 1.0,
        timestamp: 12345,
        source: SignalSource::Automation,
    };
    
    bus.send(signal).unwrap();
    
    let received = receiver.try_recv().unwrap();
    assert_eq!(received.node_id, "node_42");  // сравниваем String с &str
    assert_eq!(received.parameter_id, "test");
    assert_eq!(received.value, 1.0);
}