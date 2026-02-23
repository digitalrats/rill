//! Пример использования нескольких сервоприводов одновременно
//!
//! Запуск: cargo run --example multiple_servos

use kama_automation::{automaton::FunctionAutomaton, AutomationManager, TestSignalSender};
use kama_core_traits::time::{Clock, SystemClock};
use kama_oscillators::control::LfoWaveform;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Multiple Servos Example ===\n");

    let clock = Arc::new(SystemClock::new(44100.0, 120.0));
    let system_clock = SystemClock::new(44100.0, 120.0);
    let signal_sender = Arc::new(TestSignalSender::new());

    let mut manager = AutomationManager::new(clock.clone(), system_clock)
        .with_signal_sender(signal_sender.clone());

    println!("Добавляем несколько автоматов для разных параметров:\n");

    // 1. Sine LFO для громкости
    manager.add_servo(kama_automation::Servo::new(
        "volume_lfo".to_string(),
        Arc::new(FunctionAutomaton::lfo_with_waveform(
            0.2,
            0.3,
            0.5,
            LfoWaveform::Sine,
            "mixer",
            "volume",
        )),
        "mixer".to_string(),
        "volume".to_string(),
        kama_automation::ParameterMapping::Linear,
        kama_automation::AutomationContext::new(clock.clone()),
    ));
    println!("  Volume LFO: 0.2 Hz Sine, range [0.2, 0.8]");

    // 2. Triangle LFO для панорамы
    manager.add_servo(kama_automation::Servo::new(
        "pan_lfo".to_string(),
        Arc::new(FunctionAutomaton::lfo_with_waveform(
            2.0,
            0.8,
            0.0,
            LfoWaveform::Triangle,
            "mixer",
            "pan",
        )),
        "mixer".to_string(),
        "pan".to_string(),
        kama_automation::ParameterMapping::Linear,
        kama_automation::AutomationContext::new(clock.clone()),
    ));
    println!("  Pan LFO: 2.0 Hz Triangle, range [-0.8, 0.8]");

    // 3. Square LFO для фильтра
    manager.add_servo(kama_automation::Servo::new(
        "filter_lfo".to_string(),
        Arc::new(FunctionAutomaton::lfo_with_waveform(
            1.0,
            0.4,
            0.5,
            LfoWaveform::Square,
            "synth",
            "cutoff",
        )),
        "synth".to_string(),
        "cutoff".to_string(),
        kama_automation::ParameterMapping::Linear,
        kama_automation::AutomationContext::new(clock.clone()),
    ));
    println!("  Filter LFO: 1.0 Hz Square, range [0.1, 0.9]");

    // 4. Random Walk через состояние
    let random_automaton = kama_automation::automaton::StatefulFunctionAutomaton::new(
        "Random Walk",
        |_time, state| {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            *state += (rng.gen::<f64>() - 0.5) * 0.1;
            state.clamp(0.2, 0.8)
        },
        0.5,
        "effect",
        "parameter",
    );

    manager.add_servo(kama_automation::Servo::new(
        "random_walk".to_string(),
        Arc::new(random_automaton),
        "effect".to_string(),
        "parameter".to_string(),
        kama_automation::ParameterMapping::Linear,
        kama_automation::AutomationContext::new(clock.clone()),
    ));
    println!("  Random Walk: smooth random changes");

    println!("\nВремя(s)\tVolume\t\tPan\t\tCutoff\t\tRandom");
    println!("--------\t------\t\t---\t\t------\t\t------");

    for i in 0..20 {
        let time = i as f64 * 0.25;

        Clock::advance(clock.as_ref(), 11025);
        manager.update(11025);

        let volume = signal_sender
            .get_signals_for_param("mixer", "volume")
            .last()
            .copied()
            .unwrap_or(0.5);
        let pan = signal_sender
            .get_signals_for_param("mixer", "pan")
            .last()
            .copied()
            .unwrap_or(0.0);
        let cutoff = signal_sender
            .get_signals_for_param("synth", "cutoff")
            .last()
            .copied()
            .unwrap_or(0.5);
        let random = signal_sender
            .get_signals_for_param("effect", "parameter")
            .last()
            .copied()
            .unwrap_or(0.5);

        println!(
            "{:.2}\t\t{:.3}\t\t{:.3}\t\t{:.3}\t\t{:.3}",
            time, volume, pan, cutoff, random
        );

        thread::sleep(Duration::from_millis(50));
    }

    println!("\n✅ Все сервоприводы работают одновременно");
    Ok(())
}
