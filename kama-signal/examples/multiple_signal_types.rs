//! Пример использования разных типов сигналов
//!
//! Запуск: cargo run --example multiple_signal_types

use kama_signal::{
    Signal, SignalBus, BusConfig,
    ParameterChanged, SignalSource, ClockTick, SystemEvent,
};
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Multiple Signal Types Example ===\n");

    // Создаём шины для разных типов сигналов
    let param_bus = SignalBus::<ParameterChanged>::new(BusConfig::Unbounded);
    let clock_bus = SignalBus::<ClockTick>::new(BusConfig::Unbounded);
    let event_bus = SignalBus::<SystemEvent>::new(BusConfig::Unbounded);

    let param_rx = param_bus.receiver();
    let clock_rx = clock_bus.receiver();
    let event_rx = event_bus.receiver();

    println!("Отправляем сигналы разных типов...\n");

    // Параметрический сигнал
    let param_signal = ParameterChanged {
        node_id: "oscillator".to_string(),
        parameter_id: "frequency".to_string(),
        value: 440.0,
        normalized_value: 0.5,
        timestamp: 1234567890,
        source: SignalSource::UserInterface,
    };
    param_bus.send(param_signal)?;
    println!("[PARAM] Отправлено изменение частоты");

    // Тактовый сигнал
    let clock_signal = ClockTick {
        sample_pos: 44100,
        samples_since_last: 256,
    };
    clock_bus.send(clock_signal)?;
    println!("[CLOCK] Отправлен тактовый сигнал (позиция: 44100)");

    // Системное событие
    let event_signal = SystemEvent::GraphChanged;
    event_bus.send(event_signal)?;
    println!("[EVENT] Отправлено событие GraphChanged");

    thread::sleep(Duration::from_millis(100));

    println!("\nПолучаем сигналы:");

    // Получаем параметрический сигнал
    if let Ok(signal) = param_rx.try_recv() {
        println!("  [PARAM] {} = {} (источник: {:?})", 
                 signal.parameter_id, signal.value, signal.source);
    }

    // Получаем тактовый сигнал
    if let Ok(signal) = clock_rx.try_recv() {
        println!("  [CLOCK] Позиция: {} сэмплов", signal.sample_pos);
    }

    // Получаем системное событие
    if let Ok(signal) = event_rx.try_recv() {
        match signal {
            SystemEvent::GraphChanged => println!("  [EVENT] Граф изменён"),
            SystemEvent::TransportStarted => println!("  [EVENT] Транспорт запущен"),
            SystemEvent::TransportStopped => println!("  [EVENT] Транспорт остановлен"),
            SystemEvent::Error(msg) => println!("  [EVENT] Ошибка: {}", msg),
        }
    }

    Ok(())
}