//! Пример использования разных политик переполнения
//!
//! Запуск: cargo run --example bus_with_policies

use kama_core::signal::{BusConfig, OverflowPolicy, Signal, SignalBus};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
struct ControlChange {
    controller: u8,
    value: u8,
}

impl Signal for ControlChange {}

fn demonstrate_policy(name: &str, policy: OverflowPolicy, values: &[u8]) {
    println!("\n--- Демонстрация: {} ---", name);

    let bus = SignalBus::<ControlChange>::new(BusConfig::Bounded(3, policy));
    let receiver = bus.receiver();

    println!("Отправляем значения: {:?}", values);

    for &value in values {
        let signal = ControlChange {
            controller: 7, // volume
            value,
        };

        match bus.send(signal) {
            Ok(()) => println!("  ✓ Отправлено: {}", value),
            Err(e) => println!("  ✗ Ошибка отправки {}: {:?}", value, e),
        }

        thread::sleep(Duration::from_millis(10));
    }

    println!("\nПолученные значения:");
    let mut received = Vec::new();
    while let Ok(signal) = receiver.try_recv() {
        received.push(signal.value);
        println!("  Получено: {}", signal.value);
    }

    println!("Итог: {:?}", received);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Signal Bus Overflow Policies ===\n");

    let values = vec![10, 20, 30, 40, 50, 60];

    // DropNewest - отбрасывает новые сообщения при переполнении
    demonstrate_policy(
        "DropNewest (отбрасывать новые)",
        OverflowPolicy::DropNewest,
        &values,
    );

    // DropOldest - отбрасывает старые сообщения
    demonstrate_policy(
        "DropOldest (отбрасывать старые)",
        OverflowPolicy::DropOldest,
        &values,
    );

    // Создадим отдельный поток для демонстрации Block
    println!("\n--- Демонстрация: Block (блокировка) ---");

    let blocking_bus =
        SignalBus::<ControlChange>::new(BusConfig::Bounded(2, OverflowPolicy::Block));

    let bus_clone = blocking_bus.clone();

    // Поток-получатель с задержкой
    let receiver = blocking_bus.receiver();
    let handle = thread::spawn(move || {
        println!("Получатель: начинаю чтение...");
        thread::sleep(Duration::from_millis(500));

        while let Ok(signal) = receiver.try_recv() {
            println!("Получатель: прочитано {}", signal.value);
            thread::sleep(Duration::from_millis(200));
        }
        println!("Получатель: закончил");
    });

    // Отправляем больше сообщений, чем вмещает очередь
    println!("Отправитель: отправляю сообщения...");
    for i in 1..=5 {
        println!("Отправитель: отправка {}", i * 10);
        let signal = ControlChange {
            controller: 7,
            value: i * 10,
        };
        // send будет блокироваться, пока есть место
        bus_clone.send(signal)?;
        println!("Отправитель: отправлено {}", i * 10);
    }

    handle.join().unwrap();

    Ok(())
}
