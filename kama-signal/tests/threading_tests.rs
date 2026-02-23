// kama-signal/tests/threading_tests.rs
//! Тесты многопоточности для SignalBus

use kama_core::signal::{BusConfig, OverflowPolicy, Signal, SignalBus};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
struct ThreadSignal {
    id: usize,
    value: i32,
}

impl Signal for ThreadSignal {}

#[test]
fn test_multiple_producers() {
    println!("\n=== test_multiple_producers ===");

    let bus = Arc::new(SignalBus::<ThreadSignal>::new(BusConfig::Unbounded));
    let receiver = bus.receiver();

    let mut handles = vec![];

    // Запускаем 5 потоков-производителей
    for i in 0..5 {
        let bus_clone = bus.clone();
        handles.push(thread::spawn(move || {
            for j in 0..10 {
                let signal = ThreadSignal {
                    id: i,
                    value: j as i32,
                };
                bus_clone.send(signal).unwrap();
                thread::sleep(Duration::from_micros(100));
            }
            println!("Producer {} finished", i);
        }));
    }

    // Собираем результаты в основном потоке
    let mut received_count = 0;
    let mut received_by_id = vec![0; 5];

    while received_count < 50 {
        // 5 потоков * 10 сигналов = 50
        if let Ok(signal) = receiver.try_recv() {
            received_count += 1;
            received_by_id[signal.id] += 1;
            println!("Received: id={}, value={}", signal.id, signal.value);
        }
        thread::sleep(Duration::from_micros(10));
    }

    // Проверяем, что все потоки отправили свои сигналы
    for i in 0..5 {
        assert_eq!(
            received_by_id[i], 10,
            "Thread {} sent only {} signals",
            i, received_by_id[i]
        );
    }

    // Ждём завершения всех производителей
    for handle in handles {
        handle.join().unwrap();
    }

    println!("✅ Multiple producers work correctly");
}

#[test]
fn test_multiple_consumers() {
    println!("\n=== test_multiple_consumers ===");

    let bus = Arc::new(SignalBus::<ThreadSignal>::new(BusConfig::Unbounded));

    // Запускаем 3 потока-потребителя
    let mut consumer_handles = vec![];
    let consumer_counts = Arc::new(std::sync::Mutex::new(vec![0; 3]));

    for i in 0..3 {
        let bus_clone = bus.clone();
        let counts_clone = consumer_counts.clone();
        consumer_handles.push(thread::spawn(move || {
            let receiver = bus_clone.receiver();
            let mut local_count = 0;
            while local_count < 10 {
                if let Ok(signal) = receiver.try_recv() {
                    local_count += 1;
                    println!(
                        "Consumer {} received: id={}, value={}",
                        i, signal.id, signal.value
                    );
                }
                thread::sleep(Duration::from_micros(10));
            }
            let mut counts = counts_clone.lock().unwrap();
            counts[i] = local_count;
            println!("Consumer {} finished with {} signals", i, local_count);
        }));
    }

    // Отправляем сигналы из основного потока
    for i in 0..30 {
        let signal = ThreadSignal {
            id: i,
            value: i as i32,
        };
        bus.send(signal).unwrap();
        println!("Sent signal {}", i);
        thread::sleep(Duration::from_micros(50));
    }

    // Ждём завершения всех потребителей
    for handle in consumer_handles {
        handle.join().unwrap();
    }

    // Проверяем, что все сигналы были получены
    let counts = consumer_counts.lock().unwrap();
    let total: i32 = counts.iter().sum();
    assert_eq!(total, 30, "Total received signals: {}", total);

    println!("✅ Multiple consumers work correctly");
}

#[test]
fn test_producer_consumer_sync() {
    println!("\n=== test_producer_consumer_sync ===");

    let bus = Arc::new(SignalBus::<ThreadSignal>::new(
        BusConfig::Bounded(5, OverflowPolicy::Block), // Уменьшил размер для теста
    ));

    let producer_bus = bus.clone();
    let consumer_bus = bus.clone();

    let producer = thread::spawn(move || {
        for i in 0..20 {
            let signal = ThreadSignal { id: 0, value: i };
            // Должно блокироваться, когда буфер полон
            match producer_bus.send(signal) {
                Ok(()) => println!("Produced: {}", i),
                Err(e) => println!("Producer error at {}: {:?}", i, e),
            }
            thread::sleep(Duration::from_micros(100));
        }
        println!("Producer finished");
    });

    let consumer = thread::spawn(move || {
        let receiver = consumer_bus.receiver();
        let mut received = 0;
        while received < 20 {
            if let Ok(signal) = receiver.try_recv() {
                received += 1;
                println!("Consumed: {}", signal.value);
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(received, 20);
        println!("Consumer finished");
    });

    producer.join().unwrap();
    consumer.join().unwrap();

    println!("✅ Producer-consumer sync works with bounded channel");
}
