use kama_core::signal::*;
use kama_core::traits::{NodeId, ParameterId, PortId};

#[test]
fn test_signal_bus_basic() {
    let bus = SignalBus::<ParameterChanged>::new(BusConfig::Unbounded);
    let receiver = bus.receiver();
    
    let node = NodeId(42);
    let port = PortId::node(node);
    let param = ParameterId::new("test").unwrap();
    
    let signal = ParameterChanged::new(
        port,
        param,
        1.0,
        1.0,
        SignalSource::Automation,
    );
    
    bus.send(signal).unwrap();
    
    let received = receiver.try_recv().unwrap();
    assert_eq!(received.port.node_id(), node);
    assert_eq!(received.parameter.as_str(), "test");
    assert_eq!(received.value, 1.0);
}