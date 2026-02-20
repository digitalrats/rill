//! Базовый пример использования SignalBus
//!
//! Запуск: cargo run --example basic_signal_bus

use kama_signal::{Signal, SignalBus, BusConfig};
use std::thread;
use std::time::Duration;

// Собственный тип сигнала
#[derive(Debug, Clone)]
struct NoteOn {
    note: u8,
    velocity: u8,
    channel: u8,
}

impl Signal for NoteOn {}

#[derive(Debug, Clone)]
struct NoteOff {
    note: u8,
    channel: u8,
}

impl Signal for NoteOff {}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Basic SignalBus Example ===\n");

    // Создаём шину для NoteOn сигналов
    let note_bus = SignalBus::<NoteOn>::new(BusConfig::Unbounded);
    let receiver = note_bus.receiver();

    println!("Отправляем MIDI-ноты...");

    // Отправляем несколько нот
    let notes = vec![60, 62, 64, 65, 67]; // До-Ре-Ми-Фа-Соль
    for (i, &note) in notes.iter().enumerate() {
        let signal = NoteOn {
            note,
            velocity: 100,
            channel: 1,
        };
        
        note_bus.send(signal)?;
        println!("  Отправлена нота: {} (velocity=100)", note);
        
        // Небольшая задержка между нотами
        thread::sleep(Duration::from_millis(200));
    }

    println!("\nПолучаем ноты из шины:");

    // Читаем все отправленные ноты
    while let Ok(signal) = receiver.try_recv() {
        println!("  Получена нота: {} на канале {}", signal.note, signal.channel);
    }

    // Пример с bounded очередью
    println!("\n=== Bounded Queue Example ===");
    
    let bounded_bus = SignalBus::<NoteOn>::new(
        BusConfig::Bounded(3, kama_signal::OverflowPolicy::DropOldest)
    );
    let bounded_rx = bounded_bus.receiver();

    println!("Отправляем 5 нот в очередь размером 3 (DropOldest)...");
    
    for i in 0..5 {
        let note = 60 + i;
        let signal = NoteOn {
            note,
            velocity: 100,
            channel: 1,
        };
        
        match bounded_bus.send(signal) {
            Ok(()) => println!("  Нота {} отправлена", note),
            Err(e) => println!("  Нота {} не отправлена: {:?}", note, e),
        }
    }

    println!("\nПолучаем оставшиеся ноты (должны быть 3 последние):");
    while let Ok(signal) = bounded_rx.try_recv() {
        println!("  Получена нота: {}", signal.note);
    }

    Ok(())
}