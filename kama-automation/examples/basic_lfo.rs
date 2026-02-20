//! Базовый пример использования LFO для автоматизации параметра
//!
//! Запуск: cargo run --example basic_lfo

use std::sync::Arc;
use std::time::Duration;
use std::thread;
use kama_core::time::{SystemClock, Clock};  // Добавили импорт Clock
use kama_automation::{
    AutomationManager, TestSignalSender,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Basic LFO Automation Example ===\n");

    // Создаём источник времени
    let clock = Arc::new(SystemClock::new(44100.0, 120.0));
    let system_clock = SystemClock::new(44100.0, 120.0);
    
    // Создаём отправитель сигналов для отслеживания изменений
    let signal_sender = Arc::new(TestSignalSender::new());
    
    // Создаём менеджер автоматизации
    let mut manager = AutomationManager::new(clock.clone(), system_clock)
        .with_signal_sender(signal_sender.clone());
    
    println!("Добавляем LFO для автоматизации громкости...");
    
    // Добавляем LFO: частота 0.5 Гц, амплитуда 0.3, смещение 0.5
    manager.add_lfo(
        "volume_lfo",
        0.5,   // частота
        0.3,   // амплитуда
        0.5,   // смещение
        "mixer_channel_1",
        "volume",
    );
    
    println!("LFO добавлен. Начинаем автоматизацию...\n");
    println!("Время(s)\tЗначение");
    println!("--------\t--------");
    
    // Симулируем 10 секунд работы с шагом 0.5 секунды
    for i in 0..20 {
        let time = i as f64 * 0.5;
        
        // Продвигаем время - используем трейт Clock
        Clock::advance(clock.as_ref(), 22050); // 0.5 секунды при 44.1kHz
        manager.update(22050);
        
        // Получаем последнее отправленное значение
        let signals = signal_sender.get_signals_for_param("mixer_channel_1", "volume");
        if let Some(&value) = signals.last() {
            println!("{:.1}\t\t{:.3}", time, value);
        }
        
        thread::sleep(Duration::from_millis(100));
    }
    
    println!("\n✅ Пример завершён");
    Ok(())
}