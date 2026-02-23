use kama_core::signal::*;
use kama_core::traits::{NodeId, ParameterId};

#[test]
fn test_signal_bus_basic() {
    let bus = SignalBus::<ParameterChanged>::new(BusConfig::Unbounded);
    let receiver = bus.receiver();
    
    let signal = ParameterChanged {
        node_id: NodeId(42),
        parameter_id: ParameterId::from_name("test"),  // используем from_name
        value: 1.0,
        normalized_value: 1.0,
        timestamp: 12345,
        source: SignalSource::Automation,
    };
    
    bus.send(signal).unwrap();
    
    let received = receiver.try_recv().unwrap();
    assert_eq!(received.node_id, NodeId(42));
    assert_eq!(received.parameter_id, ParameterId::from_name("test"));  // сравниваем с ParameterId
    assert_eq!(received.value, 1.0);
}