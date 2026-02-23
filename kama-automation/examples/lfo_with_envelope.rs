//! Пример использования LFO с envelope для автоматизации
//!
//! Запуск: cargo run --example lfo_with_envelope

use kama_automation::{
    automaton::{LfoAutomaton, LfoWithEnvelopeAutomaton}, // <-- импортируем оба
    AutomationContext,
    AutomationManager,
    Automaton, // <-- Automaton трейт
    ParameterMapping,
    Servo,
    TestSignalSender,
};
use kama_core::traits::time::{Clock, SystemClock};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== LFO with Envelope Example ===\n");

    let clock = Arc::new(SystemClock::new(44100.0, 120.0));
    let system_clock = SystemClock::new(44100.0, 120.0);
    let signal_sender = Arc::new(TestSignalSender::new());

    let mut manager = AutomationManager::new(clock.clone(), system_clock)
        .with_signal_sender(signal_sender.clone());

    println!("Создаём LFO с envelope (attack=2s, release=2s)...");

    // Вариант 1: Специализированный конструктор
    let lfo_with_env = LfoWithEnvelopeAutomaton::lfo_with_envelope(
        0.5, // частота
        0.8, // амплитуда
        0.5, // смещение
        2.0, // attack
        2.0, // release
        "synth",
        "filter_cutoff",
    );

    let context = AutomationContext::new(clock.clone());
    let servo = Servo::new(
        "envelope_lfo_1".to_string(),
        Arc::new(lfo_with_env),
        "synth".to_string(),
        "filter_cutoff".to_string(),
        ParameterMapping::Linear,
        context,
    );

    manager.add_servo(servo);

    // Вариант 2: Комбинация через замыкание
    println!("\nСоздаём второй LFO с envelope через замыкание...");

    use kama_oscillators::control::{Envelope, Lfo, LfoWaveform};

    let lfo = Lfo::new(0.3, 0.5, 0.5).with_waveform(LfoWaveform::Saw);
    let env = Envelope::new(1.0, 0.5, 0.8, 3.0);

    // Для использования с состоянием нужен Mutex или другой механизм
    // В реальном приложении нужно использовать потокобезопасную обёртку

    println!("LFO с envelope добавлены");
    println!("\nВремя(s)\tFilter Cutoff\tResonance\tСостояние");
    println!("--------\t-------------\t---------\t---------");

    for i in 0..20 {
        let time = i as f64 * 0.5;

        Clock::advance(clock.as_ref(), 22050);
        manager.update(22050);

        let cutoff = signal_sender
            .get_signals_for_param("synth", "filter_cutoff")
            .last()
            .copied()
            .unwrap_or(0.5);

        let state = if time < 2.0 {
            "Attack"
        } else if time < 4.0 {
            "Sustain"
        } else {
            "Release"
        };

        println!("{:.1}\t\t{:.3}\t\t-\t\t{}", time, cutoff, state);

        thread::sleep(Duration::from_millis(100));
    }

    println!("\n✅ Пример с envelope завершён");
    Ok(())
}
