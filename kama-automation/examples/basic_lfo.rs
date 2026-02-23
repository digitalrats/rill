//! Базовый пример использования LFO для автоматизации параметра
//!
//! Запуск: cargo run --example basic_lfo

use kama_automation::{automaton::FunctionAutomaton, AutomationManager, TestSignalSender};
use kama_core_traits::time::{Clock, SystemClock};
use kama_oscillators::control::LfoWaveform;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

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

    // Вариант 1: Через специализированный метод
    manager.add_lfo_with_waveform(
        "volume_lfo_1",
        0.5, // частота
        0.3, // амплитуда
        0.5, // смещение
        LfoWaveform::Sine,
        "mixer_channel_1",
        "volume",
    );

    // Вариант 2: Через FunctionAutomaton напрямую
    let lfo_automaton = FunctionAutomaton::lfo_with_waveform(
        0.7,
        0.2,
        0.6,
        LfoWaveform::Triangle,
        "mixer_channel_2",
        "pan",
    );

    manager.add_servo(kama_automation::Servo::new(
        "volume_lfo_2".to_string(),
        Arc::new(lfo_automaton),
        "mixer_channel_2".to_string(),
        "pan".to_string(),
        kama_automation::ParameterMapping::Linear,
        kama_automation::AutomationContext::new(clock.clone()),
    ));

    println!("LFO добавлены. Начинаем автоматизацию...\n");
    println!("Время(s)\tVolume\t\tPan");
    println!("--------\t------\t\t---");

    // Симулируем 10 секунд работы с шагом 0.5 секунды
    for i in 0..20 {
        let time = i as f64 * 0.5;

        // Продвигаем время
        Clock::advance(clock.as_ref(), 22050); // 0.5 секунды при 44.1kHz
        manager.update(22050);

        // Получаем последние отправленные значения
        let volume = signal_sender
            .get_signals_for_param("mixer_channel_1", "volume")
            .last()
            .copied()
            .unwrap_or(0.5);
        let pan = signal_sender
            .get_signals_for_param("mixer_channel_2", "pan")
            .last()
            .copied()
            .unwrap_or(0.0);

        println!("{:.1}\t\t{:.3}\t\t{:.3}", time, volume, pan);

        thread::sleep(Duration::from_millis(100));
    }

    println!("\n✅ Пример завершён");
    Ok(())
}
