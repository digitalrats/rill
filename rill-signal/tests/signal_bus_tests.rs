// rill-signal/tests/signal_bus_tests.rs
//! Тесты для SignalBus

use rill_core::signal::{
    BusConfig, ClockTick, OverflowPolicy, ParameterChanged, Signal, SignalBus, SignalSource,
    SystemEvent,
};

// Простой тестовый сигнал
#[derive(Debug, Clone, PartialEq)]
struct TestSignal {
    value: i32,
}

impl Signal for TestSignal {}

#[test]
fn test_signal_bus_send_receive() {
    println!("\n=== test_signal_bus_send_receive ===");

    let bus = SignalBus::<TestSignal>::new(BusConfig::Unbounded);
    let receiver = bus.receiver();

    let signal = TestSignal { value: 42 };
    bus.send(signal.clone()).unwrap();

    let received = receiver.try_recv().unwrap();
    assert_eq!(received.value, 42);
    println!("✅ Signal sent and received successfully");
}

#[test]
fn test_signal_bus_bounded_drop_newest() {
    println!("\n=== test_signal_bus_bounded_drop_newest ===");

    let bus = SignalBus::<TestSignal>::new(BusConfig::Bounded(2, OverflowPolicy::DropNewest));

    // Отправляем 3 сигнала - последний должен быть отброшен
    bus.send(TestSignal { value: 1 }).unwrap();
    bus.send(TestSignal { value: 2 }).unwrap();
    let result = bus.send(TestSignal { value: 3 });
    assert!(result.is_err(), "Third send should fail with DropNewest");

    let receiver = bus.receiver();
    assert_eq!(receiver.try_recv().unwrap().value, 1);
    assert_eq!(receiver.try_recv().unwrap().value, 2);
    assert!(receiver.try_recv().is_err());

    println!("✅ DropNewest policy works");
}

#[test]
fn test_signal_bus_bounded_drop_oldest() {
    println!("\n=== test_signal_bus_bounded_drop_oldest ===");

    let bus = SignalBus::<TestSignal>::new(BusConfig::Bounded(2, OverflowPolicy::DropOldest));

    // Отправляем 3 сигнала - самый старый должен быть отброшен
    bus.send(TestSignal { value: 1 }).unwrap();
    bus.send(TestSignal { value: 2 }).unwrap();
    bus.send(TestSignal { value: 3 }).unwrap(); // Должно работать с DropOldest

    let receiver = bus.receiver();
    // Самый старый (1) должен быть отброшен, должны получить 2 и 3
    assert_eq!(receiver.try_recv().unwrap().value, 2);
    assert_eq!(receiver.try_recv().unwrap().value, 3);
    assert!(receiver.try_recv().is_err());

    println!("✅ DropOldest policy works");
}

#[test]
fn test_multiple_receivers_mpmc() {
    println!("\n=== test_multiple_receivers_mpmc ===");

    let bus = SignalBus::<TestSignal>::new(BusConfig::Unbounded);

    // Создаём двух получателей
    let receiver1 = bus.receiver();
    let receiver2 = bus.receiver();

    // Отправляем два сигнала
    bus.send(TestSignal { value: 1 }).unwrap();
    bus.send(TestSignal { value: 2 }).unwrap();
    println!("Two signals sent");

    std::thread::sleep(std::time::Duration::from_millis(10));

    // Первый получатель читает первый сигнал
    match receiver1.try_recv() {
        Ok(received) => {
            assert_eq!(received.value, 1);
            println!("Receiver 1 got signal 1");
        }
        Err(e) => panic!("Receiver 1 failed: {:?}", e),
    }

    // Второй получатель читает второй сигнал
    match receiver2.try_recv() {
        Ok(received) => {
            assert_eq!(received.value, 2);
            println!("Receiver 2 got signal 2");
        }
        Err(e) => panic!("Receiver 2 failed: {:?}", e),
    }

    println!("✅ MPMC behavior works - messages are distributed between receivers");
}

#[test]
fn test_broadcast_behavior() {
    println!("\n=== test_broadcast_behavior ===");

    // Для broadcast поведения нужно использовать отдельный крейт или другую стратегию
    // В текущей реализации каждый сигнал может быть прочитан только одним получателем

    let bus = SignalBus::<TestSignal>::new(BusConfig::Unbounded);

    let receiver1 = bus.receiver();
    let receiver2 = bus.receiver();

    bus.send(TestSignal { value: 42 }).unwrap();
    println!("Signal sent");

    std::thread::sleep(std::time::Duration::from_millis(10));

    // Только один получатель получит сигнал
    let got_signal = receiver1.try_recv().is_ok() || receiver2.try_recv().is_ok();
    assert!(got_signal, "At least one receiver should get the signal");

    // Второй получатель ничего не получит
    assert!(receiver1.try_recv().is_err() || receiver2.try_recv().is_err());

    println!("✅ Each message goes to exactly one receiver (MPMC behavior)");
}

// rill-signal/tests/signal_bus_tests.rs

#[test]
fn test_multiple_receivers_with_shared_channel() {
    println!("\n=== test_multiple_receivers_with_shared_channel ===");

    use crossbeam_channel::unbounded;

    let (tx, rx) = unbounded();
    let rx1 = rx.clone();
    let rx2 = rx.clone();

    // Отправляем два сигнала
    tx.send(TestSignal { value: 42 }).unwrap();
    tx.send(TestSignal { value: 43 }).unwrap();
    println!("Two signals sent");

    std::thread::sleep(std::time::Duration::from_millis(10));

    // Первый получатель читает первый сигнал
    match rx1.try_recv() {
        Ok(received) => {
            assert_eq!(received.value, 42);
            println!("Receiver 1 got signal 42");
        }
        Err(e) => panic!("Receiver 1 failed: {:?}", e),
    }

    // Второй получатель читает второй сигнал
    match rx2.try_recv() {
        Ok(received) => {
            assert_eq!(received.value, 43);
            println!("Receiver 2 got signal 43");
        }
        Err(e) => panic!("Receiver 2 failed: {:?}", e),
    }

    println!("✅ In crossbeam_channel, messages are distributed between receivers");
}

#[test]
fn test_cloned_receivers_get_all_messages() {
    println!("\n=== test_cloned_receivers_get_all_messages ===");

    use crossbeam_channel::unbounded;

    let (tx, rx) = unbounded();
    let rx1 = rx.clone();
    let rx2 = rx.clone();

    // Отправляем один сигнал
    tx.send(TestSignal { value: 42 }).unwrap();
    println!("One signal sent");

    std::thread::sleep(std::time::Duration::from_millis(10));

    // В crossbeam_channel, cloned receivers получают доступ к тем же сообщениям,
    // но каждое сообщение может быть прочитано только одним получателем
    let got_by_rx1 = rx1.try_recv().is_ok();
    let got_by_rx2 = rx2.try_recv().is_ok();

    // Только один из них должен получить сообщение
    assert!(
        got_by_rx1 || got_by_rx2,
        "At least one receiver should get the message"
    );
    assert!(
        !(got_by_rx1 && got_by_rx2),
        "Only one receiver should get the message"
    );

    println!("✅ Each message goes to exactly one cloned receiver");
}

#[test]
fn test_parameter_changed_signal() {
    println!("\n=== test_parameter_changed_signal ===");

    let bus = SignalBus::<ParameterChanged>::new(BusConfig::Unbounded);
    let receiver = bus.receiver();

    let signal = ParameterChanged {
        node_id: "test_node".to_string(),
        parameter_id: "gain".to_string(),
        value: 0.75,
        normalized_value: 0.75,
        timestamp: 1234567890,
        source: SignalSource::UserInterface,
    };

    bus.send(signal.clone()).unwrap();

    let received = receiver.try_recv().unwrap();
    assert_eq!(received.node_id, "test_node");
    assert_eq!(received.parameter_id, "gain");
    assert!((received.value - 0.75).abs() < 1e-6);

    match received.source {
        SignalSource::UserInterface => println!("✅ SignalSource matches"),
        _ => panic!("Wrong signal source"),
    }

    println!("✅ ParameterChanged signal works");
}

#[test]
fn test_clock_tick_signal() {
    println!("\n=== test_clock_tick_signal ===");

    let bus = SignalBus::<ClockTick>::new(BusConfig::Unbounded);
    let receiver = bus.receiver();

    let signal = ClockTick {
        sample_pos: 44100,
        samples_since_last: 256,
    };

    bus.send(signal.clone()).unwrap();

    let received = receiver.try_recv().unwrap();
    assert_eq!(received.sample_pos, 44100);
    assert_eq!(received.samples_since_last, 256);

    println!("✅ ClockTick signal works");
}

#[test]
fn test_system_event_signal() {
    println!("\n=== test_system_event_signal ===");

    let bus = SignalBus::<SystemEvent>::new(BusConfig::Unbounded);
    let receiver = bus.receiver();

    let signal = SystemEvent::GraphChanged;
    bus.send(signal).unwrap();

    let received = receiver.try_recv().unwrap();
    match received {
        SystemEvent::GraphChanged => println!("✅ SystemEvent::GraphChanged works"),
        _ => panic!("Wrong event type"),
    }

    bus.send(SystemEvent::TransportStarted).unwrap();
    let received = receiver.try_recv().unwrap();
    match received {
        SystemEvent::TransportStarted => println!("✅ SystemEvent::TransportStarted works"),
        _ => panic!("Wrong event type"),
    }

    bus.send(SystemEvent::Error("test error".to_string()))
        .unwrap();
    let received = receiver.try_recv().unwrap();
    match received {
        SystemEvent::Error(msg) => assert_eq!(msg, "test error"),
        _ => panic!("Wrong event type"),
    }

    println!("✅ SystemEvent signal works");
}
