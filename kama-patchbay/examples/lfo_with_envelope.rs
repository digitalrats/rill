//! Пример использования LFO с envelope для автоматизации

use kama_automation::{
    automaton::{LfoAutomaton, LfoWithEnvelopeAutomaton},
    AutomationContext,
    AutomationManager,
    Automaton,
    ParameterMapping,
    Servo,
    TestSignalSender,
    Waveform,
};
use kama_core::traits::time::{Clock, SystemClock};
use kama_core::traits::{NodeId, ParameterId, PortId};
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

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let filter_param = ParameterId::new("filter_cutoff")?;

    println!("Создаём LFO с envelope (attack=2s, release=2s)...");

    // LFO с огибающей
    manager.add_lfo_with_envelope(
        "envelope_lfo",
        0.5,  // частота
        0.8,  // амплитуда
        0.5,  // смещение
        2.0,  // attack
        2.0,  // release
        port,
        filter_param,
    );

    println!("\nВремя(s)\tFilter Cutoff\tСостояние");
    println!("--------\t-------------\t---------");

    for i in 0..20 {
        let time = i as f64 * 0.5;

        clock.advance(22050);
        manager.update(22050);

        let cutoff = signal_sender
            .get_signals_for_param(&ParameterId::new("filter_cutoff")?)
            .last()
            .map(|s| s.value)
            .unwrap_or(0.5);

        let state = if time < 2.0 {
            "Attack"
        } else if time < 4.0 {
            "Sustain"
        } else {
            "Release"
        };

        println!("{:.1}\t\t{:.3}\t\t{}", time, cutoff, state);

        thread::sleep(Duration::from_millis(100));
    }

    println!("\n✅ Пример с envelope завершён");
    Ok(())
}