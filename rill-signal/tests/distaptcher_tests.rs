// rill-signal/tests/dispatcher_tests.rs
//! Тесты для SimpleSignalDispatcher

use rill_core::signal::{
    ClockTick, ParameterChanged, Signal, SignalHandler, SignalSource, SimpleSignalDispatcher,
};
use std::sync::{Arc, Mutex};

// Тестовый обработчик
struct TestHandler {
    received: Arc<Mutex<Vec<String>>>,
}

impl TestHandler {
    fn new() -> Self {
        Self {
            received: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl SignalHandler<ParameterChanged> for TestHandler {
    fn handle(&mut self, signal: &ParameterChanged) {
        let mut received = self.received.lock().unwrap();
        received.push(format!("param:{}={}", signal.parameter_id, signal.value));
    }
}

impl SignalHandler<ClockTick> for TestHandler {
    fn handle(&mut self, signal: &ClockTick) {
        let mut received = self.received.lock().unwrap();
        received.push(format!("clock:{}", signal.sample_pos));
    }
}

#[test]
fn test_dispatcher_register_and_emit() {
    println!("\n=== test_dispatcher_register_and_emit ===");

    let mut dispatcher = SimpleSignalDispatcher::new();
    let handler = TestHandler::new();
    let received = handler.received.clone();

    dispatcher.register::<ParameterChanged, _>(handler);

    let signal = ParameterChanged {
        node_id: "test".to_string(),
        parameter_id: "gain".to_string(),
        value: 0.5,
        normalized_value: 0.5,
        timestamp: 123,
        source: SignalSource::Automation,
    };

    dispatcher.emit(signal).unwrap();

    let msgs = received.lock().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0], "param:gain=0.5");

    println!("✅ Dispatcher works for registered type");
}

#[test]
fn test_dispatcher_multiple_handlers() {
    println!("\n=== test_dispatcher_multiple_handlers ===");

    let mut dispatcher = SimpleSignalDispatcher::new();

    let handler1 = TestHandler::new();
    let received1 = handler1.received.clone();

    let handler2 = TestHandler::new();
    let received2 = handler2.received.clone();

    dispatcher.register::<ParameterChanged, _>(handler1);
    dispatcher.register::<ParameterChanged, _>(handler2);

    let signal = ParameterChanged {
        node_id: "test".to_string(),
        parameter_id: "gain".to_string(),
        value: 0.5,
        normalized_value: 0.5,
        timestamp: 123,
        source: SignalSource::Automation,
    };

    dispatcher.emit(signal).unwrap();

    assert_eq!(received1.lock().unwrap().len(), 1);
    assert_eq!(received2.lock().unwrap().len(), 1);

    println!("✅ Multiple handlers receive the signal");
}

#[test]
fn test_dispatcher_different_types() {
    println!("\n=== test_dispatcher_different_types ===");

    let mut dispatcher = SimpleSignalDispatcher::new();

    let handler = TestHandler::new();
    let received = handler.received.clone();

    dispatcher.register::<ParameterChanged, _>(handler);

    // Пытаемся отправить сигнал другого типа
    let clock_signal = ClockTick {
        sample_pos: 44100,
        samples_since_last: 256,
    };

    let result = dispatcher.emit(clock_signal);
    assert!(result.is_err()); // Должно быть ReceiverNotFound

    let msgs = received.lock().unwrap();
    assert_eq!(msgs.len(), 0); // Ничего не должно быть получено

    println!("✅ Dispatcher correctly filters by type");
}

#[test]
fn test_dispatcher_unregistered_type() {
    println!("\n=== test_dispatcher_unregistered_type ===");

    let mut dispatcher = SimpleSignalDispatcher::new();

    let signal = ParameterChanged {
        node_id: "test".to_string(),
        parameter_id: "gain".to_string(),
        value: 0.5,
        normalized_value: 0.5,
        timestamp: 123,
        source: SignalSource::Automation,
    };

    let result = dispatcher.emit(signal);
    assert!(result.is_err()); // Должно быть ReceiverNotFound

    println!("✅ Dispatcher correctly handles unregistered types");
}

#[test]
fn test_dispatcher_multiple_types() {
    println!("\n=== test_dispatcher_multiple_types ===");

    let mut dispatcher = SimpleSignalDispatcher::new();

    let handler = TestHandler::new();
    let received = handler.received.clone();

    // Регистрируем обработчик для двух типов сигналов
    dispatcher.register::<ParameterChanged, _>(handler);

    // Отправляем ParameterChanged
    let param_signal = ParameterChanged {
        node_id: "test".to_string(),
        parameter_id: "gain".to_string(),
        value: 0.5,
        normalized_value: 0.5,
        timestamp: 123,
        source: SignalSource::Automation,
    };

    dispatcher.emit(param_signal).unwrap();

    // Отправляем ClockTick (не должен быть получен)
    let clock_signal = ClockTick {
        sample_pos: 44100,
        samples_since_last: 256,
    };

    assert!(dispatcher.emit(clock_signal).is_err());

    let msgs = received.lock().unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0], "param:gain=0.5");

    println!("✅ Dispatcher handles multiple types correctly");
}
