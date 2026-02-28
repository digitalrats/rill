use kama_automation::{
    automaton::{FunctionAutomaton, StatefulFunctionAutomaton},
    AutomationContext, AutomationManager, ParameterMapping, Servo, SignalSender, TestSignalSender,
};
use kama_core::traits::time::{Clock, SystemClock, TickInfo, TimeProvider};
use kama_core::traits::{NodeId, ParameterId, PortId};
use std::sync::Arc;

// Вспомогательная структура для тестового TimeProvider
#[derive(Debug, Clone)]
struct TestTimeProvider {
    clock: Arc<SystemClock>,
}

impl TestTimeProvider {
    fn new() -> Self {
        Self {
            clock: Arc::new(SystemClock::new(44100.0, 120.0)),
        }
    }

    fn advance(&self, samples: u64) {
        self.clock.advance(samples);
    }
}

impl Clock for TestTimeProvider {
    fn sample_rate(&self) -> f64 {
        self.clock.sample_rate()
    }

    fn position_samples(&self) -> u64 {
        self.clock.position_samples()
    }

    fn advance(&self, samples: u64) -> u64 {
        self.clock.advance(samples)
    }

    fn reset(&self) {
        self.clock.reset()
    }
}

impl TimeProvider for TestTimeProvider {
    fn bpm(&self) -> f64 {
        self.clock.bpm()
    }

    fn set_bpm(&self, bpm: f64) {
        self.clock.set_bpm(bpm)
    }

    fn tick_info(&self) -> TickInfo {
        self.clock.tick_info()
    }
}

// ==================== УСПЕШНЫЕ ТЕСТЫ ====================

#[test]
fn test_manager_creation() {
    let time_provider = Arc::new(TestTimeProvider::new());
    let clock = SystemClock::new(44100.0, 120.0);
    let manager = AutomationManager::new(time_provider, clock);

    assert_eq!(manager.servos().len(), 0);
    assert!(manager.context().time.sample_rate() > 0.0);
}

#[test]
fn test_add_lfo_servo() {
    let time_provider = Arc::new(TestTimeProvider::new());
    let clock = SystemClock::new(44100.0, 120.0);
    let mut manager = AutomationManager::new(time_provider.clone(), clock);

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let param = ParameterId::new("gain").unwrap();

    manager.add_lfo(
        "lfo1",
        2.0, // 2 Hz
        0.3, // amplitude
        0.5, // offset
        port,
        param,
    );

    assert_eq!(manager.servos().len(), 1);

    let servo = manager.get_servo("lfo1").unwrap();
    assert_eq!(servo.id(), "lfo1");
    assert_eq!(servo.target(), port);
}

#[test]
fn test_add_custom_automaton() {
    let time_provider = Arc::new(TestTimeProvider::new());
    let clock = SystemClock::new(44100.0, 120.0);
    let mut manager = AutomationManager::new(time_provider.clone(), clock);

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let param = ParameterId::new("param").unwrap();

    // Создаём кастомный автомат через замыкание
    let automaton = Arc::new(FunctionAutomaton::new(
        "Custom",
        |t| (t * 2.0).sin() * 0.5 + 0.5,
        port.to_string().as_str(),
        param.as_str(),
    ));

    let context = AutomationContext::new(time_provider.clone());
    let servo = Servo::new(
        "custom".to_string(),
        automaton,
        port,
        param,
        ParameterMapping::Linear,
        context,
    );

    manager.add_servo(servo);
    assert_eq!(manager.servos().len(), 1);
}

#[test]
fn test_remove_servo() {
    let time_provider = Arc::new(TestTimeProvider::new());
    let clock = SystemClock::new(44100.0, 120.0);
    let mut manager = AutomationManager::new(time_provider, clock);

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let param_gain = ParameterId::new("gain").unwrap();
    let param_pan = ParameterId::new("pan").unwrap();

    manager.add_lfo("lfo1", 1.0, 0.2, 0.5, port, param_gain);
    manager.add_lfo("lfo2", 2.0, 0.3, 0.3, port, param_pan);

    assert_eq!(manager.servos().len(), 2);

    let removed = manager.remove_servo("lfo1");
    assert!(removed);
    assert_eq!(manager.servos().len(), 1);

    let not_found = manager.remove_servo("nonexistent");
    assert!(!not_found);

    assert!(manager.get_servo("lfo2").is_some());
    assert!(manager.get_servo("lfo1").is_none());
}

#[test]
fn test_clear_servos() {
    let time_provider = Arc::new(TestTimeProvider::new());
    let clock = SystemClock::new(44100.0, 120.0);
    let mut manager = AutomationManager::new(time_provider, clock);

    let node = NodeId(1);
    let port1 = PortId::control_in(node, 0);
    let port2 = PortId::control_in(node, 1);
    let param = ParameterId::new("gain").unwrap();

    manager.add_lfo("lfo1", 1.0, 0.2, 0.5, port1, param.clone());
    manager.add_lfo("lfo2", 2.0, 0.3, 0.3, port2, param.clone());
    manager.add_lfo("lfo3", 0.5, 0.1, 0.7, port1, param);

    assert_eq!(manager.servos().len(), 3);

    manager.clear();
    assert_eq!(manager.servos().len(), 0);
}

#[test]
fn test_servo_updates() {
    println!("\n=== test_servo_updates ===");

    let time_provider = Arc::new(TestTimeProvider::new());
    let signal_sender = Arc::new(TestSignalSender::new());
    let clock = SystemClock::new(44100.0, 120.0);

    let mut manager = AutomationManager::new(time_provider.clone(), clock)
        .with_signal_sender(signal_sender.clone());

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let param = ParameterId::new("gain").unwrap();

    println!("Adding LFO: id='lfo1', port={}, param='gain'", port);
    manager.add_lfo("lfo1", 1.0, 0.2, 0.5, port, param);

    for i in 1..=3 {
        println!("\n--- Update {} ---", i);
        time_provider.advance(4410);
        manager.update(4410);

        let signals = signal_sender.get_signals_for_port(port);
        println!("Signals for {}: {:?}", port, signals);
    }

    let signals = signal_sender.get_signals_for_port(port);
    assert!(!signals.is_empty(), "No signals were sent");

    for signal in &signals {
        assert!(
            signal.value >= 0.0 && signal.value <= 1.0,
            "Value {} out of range [0,1]",
            signal.value
        );
    }
}

#[test]
fn test_multiple_servos() {
    let time_provider = Arc::new(TestTimeProvider::new());
    let signal_sender = Arc::new(TestSignalSender::new());
    let clock = SystemClock::new(44100.0, 120.0);

    let mut manager = AutomationManager::new(time_provider.clone(), clock)
        .with_signal_sender(signal_sender.clone());

    let node = NodeId(1);
    let port1 = PortId::control_in(node, 0);
    let port2 = PortId::control_in(node, 1);
    let port3 = PortId::control_in(node, 2);

    let param_gain = ParameterId::new("gain").unwrap();
    let param_pan = ParameterId::new("pan").unwrap();
    let param_cutoff = ParameterId::new("cutoff").unwrap();

    manager.add_lfo("lfo1", 1.0, 0.2, 0.5, port1, param_gain);
    manager.add_lfo("lfo2", 2.0, 0.3, 0.3, port2, param_pan);
    manager.add_lfo("lfo3", 0.5, 0.1, 0.7, port3, param_cutoff);

    for _ in 0..10 {
        time_provider.advance(4410);
        manager.update(4410);
    }

    let signals1 = signal_sender.get_signals_for_port(port1);
    let signals2 = signal_sender.get_signals_for_port(port2);
    let signals3 = signal_sender.get_signals_for_port(port3);

    println!("test_multiple_servos - port1 (gain): {:?}", signals1);
    println!("test_multiple_servos - port2 (pan): {:?}", signals2);
    println!("test_multiple_servos - port3 (cutoff): {:?}", signals3);

    assert!(!signals1.is_empty(), "No gain signals");
    assert!(!signals2.is_empty(), "No pan signals");
    assert!(!signals3.is_empty(), "No cutoff signals");
}

#[test]
fn test_servo_range() {
    let time_provider = Arc::new(TestTimeProvider::new());
    let signal_sender = Arc::new(TestSignalSender::new());
    let clock = SystemClock::new(44100.0, 120.0);

    let mut manager = AutomationManager::new(time_provider.clone(), clock)
        .with_signal_sender(signal_sender.clone());

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let param = ParameterId::new("gain").unwrap();

    let automaton = Arc::new(FunctionAutomaton::lfo(1.0, 0.5, 0.0, port.to_string().as_str(), param.as_str()));

    let context = AutomationContext::new(time_provider.clone());

    let mut servo = Servo::new(
        "range_lfo".to_string(),
        automaton,
        port,
        param,
        ParameterMapping::Linear,
        context,
    );

    servo.set_range(0.2, 0.8);
    manager.add_servo(servo);

    for _ in 0..10 {
        time_provider.advance(4410);
        manager.update(4410);
    }

    let signals = signal_sender.get_signals_for_port(port);
    println!("test_servo_range - signals: {:?}", signals);
    assert!(!signals.is_empty(), "No range signals");

    for signal in &signals {
        assert!(
            signal.value >= 0.2 && signal.value <= 0.8,
            "Value {} out of range [0.2, 0.8]",
            signal.value
        );
    }
}

#[test]
fn test_servo_with_custom_mapping() {
    let time_provider = Arc::new(TestTimeProvider::new());
    let signal_sender = Arc::new(TestSignalSender::new());
    let clock = SystemClock::new(44100.0, 120.0);

    let mut manager = AutomationManager::new(time_provider.clone(), clock)
        .with_signal_sender(signal_sender.clone());

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let param = ParameterId::new("gain").unwrap();

    let automaton = Arc::new(FunctionAutomaton::lfo(1.0, 0.5, 0.0, port.to_string().as_str(), param.as_str()));

    let context = AutomationContext::new(time_provider.clone());

    let servo = Servo::new(
        "exp_lfo".to_string(),
        automaton,
        port,
        param,
        ParameterMapping::Exponential,
        context,
    );

    manager.add_servo(servo);

    for _ in 0..10 {
        time_provider.advance(4410);
        manager.update(4410);
    }

    let signals = signal_sender.get_signals_for_port(port);
    println!("test_servo_with_custom_mapping - signals: {:?}", signals);
    assert!(!signals.is_empty(), "No custom mapping signals");

    for signal in &signals {
        assert!(
            signal.value >= 0.0 && signal.value <= 1.0,
            "Value {} out of range [0,1]",
            signal.value
        );
    }
}

#[test]
fn test_servo_persistence() {
    println!("\n=== test_servo_persistence ===");

    let time_provider = Arc::new(TestTimeProvider::new());
    let signal_sender = Arc::new(TestSignalSender::new());
    let clock = SystemClock::new(44100.0, 120.0);

    let mut manager = AutomationManager::new(time_provider.clone(), clock)
        .with_signal_sender(signal_sender.clone());

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let param = ParameterId::new("gain").unwrap();

    manager.add_lfo("lfo1", 0.25, 0.3, 0.5, port, param);

    let mut values = Vec::new();
    let mut last_value = None;

    for i in 0..20 {
        time_provider.advance(44100);
        manager.update(44100);

        let signals = signal_sender.get_signals_for_port(port);
        if let Some(signal) = signals.last() {
            println!("Iteration {}: value = {:.6}", i, signal.value);

            if last_value != Some(signal.value) {
                values.push(signal.value);
                last_value = Some(signal.value);
                println!("  -> NEW VALUE at iteration {}", i);
            }
        }
    }

    println!(
        "test_servo_persistence - collected {} unique values: {:?}",
        values.len(),
        values
    );
    assert!(
        values.len() >= 2,
        "Should have at least 2 different values, got {}",
        values.len()
    );
}

#[test]
fn test_disable_servo() {
    println!("\n=== test_disable_servo ===");

    let time_provider = Arc::new(TestTimeProvider::new());
    let signal_sender = Arc::new(TestSignalSender::new());
    let clock = SystemClock::new(44100.0, 120.0);

    let mut manager = AutomationManager::new(time_provider.clone(), clock)
        .with_signal_sender(signal_sender.clone());

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let param = ParameterId::new("value").unwrap();

    // Используем автомат, который гарантированно меняет значения
    let counter = Arc::new(StatefulFunctionAutomaton::new(
        "Counter",
        |_time, count| {
            *count += 1;
            *count as f64
        },
        0,
        port.to_string().as_str(),
        param.as_str(),
    ));

    let context = AutomationContext::new(time_provider.clone());
    let mut servo = Servo::new(
        "counter".to_string(),
        counter,
        port,
        param,
        ParameterMapping::Linear,
        context,
    );

    servo.set_range(f64::NEG_INFINITY, f64::INFINITY);
    manager.add_servo(servo);

    // Первое обновление
    time_provider.advance(44100);
    manager.update(44100);

    let initial_signals = signal_sender.get_signals_for_port(port);
    println!("test_disable_servo - initial: {:?}", initial_signals);
    assert!(!initial_signals.is_empty(), "No initial signals");
    let initial_count = initial_signals.len();
    let initial_value = initial_signals.last().unwrap().value;

    // Отключаем сервопривод
    if let Some(servo) = manager.get_servo_mut("counter") {
        servo.set_enabled(false);
    }

    // Обновляем несколько раз с отключённым сервоприводом
    for _ in 0..3 {
        time_provider.advance(44100);
        manager.update(44100);
    }

    // Проверяем, что количество сигналов не увеличилось
    let signals_after_disable = signal_sender.get_signals_for_port(port);
    println!(
        "test_disable_servo - after disable: {:?}",
        signals_after_disable
    );
    assert_eq!(
        signals_after_disable.len(),
        initial_count,
        "Signals were sent while disabled"
    );

    // Включаем обратно
    if let Some(servo) = manager.get_servo_mut("counter") {
        servo.set_enabled(true);
    }

    // Ждём изменения значения
    let mut new_value_found = false;
    for attempt in 0..5 {
        time_provider.advance(44100);
        manager.update(44100);

        let current_signals = signal_sender.get_signals_for_port(port);
        if let Some(signal) = current_signals.last() {
            if (signal.value - initial_value).abs() > 0.5 {
                new_value_found = true;
                println!(
                    "New value {:.6} found after {} attempts",
                    signal.value,
                    attempt + 1
                );
                break;
            }
        }
    }

    assert!(new_value_found, "No new signals after re-enabling");
}

#[test]
fn test_signal_sender_integration() {
    println!("\n=== test_signal_sender_integration ===");

    let time_provider = Arc::new(TestTimeProvider::new());
    let signal_sender = Arc::new(TestSignalSender::new());
    let clock = SystemClock::new(44100.0, 120.0);

    let mut manager = AutomationManager::new(time_provider.clone(), clock)
        .with_signal_sender(signal_sender.clone());

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let param = ParameterId::new("gain").unwrap();

    manager.add_lfo("lfo1", 0.25, 0.5, 0.5, port, param);

    // Первое обновление
    time_provider.advance(44100);
    manager.update(44100);

    let signals = signal_sender.get_signals_for_port(port);
    println!("test_signal_sender_integration - first: {:?}", signals);
    assert_eq!(signals.len(), 1, "Expected 1 signal after first update");

    // Второе обновление
    time_provider.advance(44100);
    manager.update(44100);

    let signals = signal_sender.get_signals_for_port(port);
    println!("test_signal_sender_integration - second: {:?}", signals);

    assert!(
        signals.len() >= 2,
        "Expected at least 2 signals, got {}",
        signals.len()
    );

    let diff = (signals[0].value - signals[1].value).abs();
    println!("Difference between signals: {:.6}", diff);
    assert!(
        diff > 0.00001,
        "Signals should be different: {:.6} vs {:.6}",
        signals[0].value,
        signals[1].value
    );
}

#[test]
fn test_stateful_automaton() {
    println!("\n=== test_stateful_automaton ===");

    let time_provider = Arc::new(TestTimeProvider::new());
    let signal_sender = Arc::new(TestSignalSender::new());
    let clock = SystemClock::new(44100.0, 120.0);

    let mut manager = AutomationManager::new(time_provider.clone(), clock)
        .with_signal_sender(signal_sender.clone());

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let param = ParameterId::new("value").unwrap();

    // Создаём автомат с состоянием (счётчик)
    let counter = Arc::new(StatefulFunctionAutomaton::new(
        "Counter",
        |_time, count| {
            *count += 1;
            *count as f64
        },
        0,
        port.to_string().as_str(),
        param.as_str(),
    ));

    let context = AutomationContext::new(time_provider.clone());
    let mut servo = Servo::new(
        "counter".to_string(),
        counter,
        port,
        param,
        ParameterMapping::Linear,
        context,
    );

    servo.set_range(f64::NEG_INFINITY, f64::INFINITY);
    manager.add_servo(servo);

    // Первое обновление должно дать значение 2.0 (initial_state дала 1)
    time_provider.advance(4410);
    manager.update(4410);

    let signals = signal_sender.get_signals_for_port(port);
    assert_eq!(signals.len(), 1, "Should have one signal");
    assert!(
        (signals[0].value - 2.0).abs() < 0.01,
        "Expected 2.0, got {}",
        signals[0].value
    );

    // Второе обновление должно дать значение 3.0
    time_provider.advance(4410);
    manager.update(4410);

    let signals = signal_sender.get_signals_for_port(port);
    assert_eq!(signals.len(), 2, "Should have two signals");
    assert!(
        (signals[1].value - 3.0).abs() < 0.01,
        "Expected 3.0, got {}",
        signals[1].value
    );
}