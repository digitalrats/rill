//! Базовый пример использования LFO для автоматизации параметра

use kama_automation::{automaton::LfoAutomaton, AutomationManager, TestSignalSender};
use kama_core::traits::time::{Clock, SystemClock};
use kama_core::traits::{NodeId, ParameterId, PortId};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Basic LFO Automation Example ===\n");

    let clock = Arc::new(SystemClock::new(44100.0, 120.0));
    let system_clock = SystemClock::new(44100.0, 120.0);
    let signal_sender = Arc::new(TestSignalSender::new());

    let mut manager = AutomationManager::new(clock.clone(), system_clock)
        .with_signal_sender(signal_sender.clone());

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);
    let param = ParameterId::new("volume")?;

    println!("Добавляем LFO для автоматизации громкости...");

    manager.add_lfo(
        "volume_lfo",
        0.5,  // 0.5 Hz
        0.3,  // amplitude
        0.5,  // offset
        port,
        param,
    );

    println!("LFO добавлен. Начинаем автоматизацию...\n");
    println!("Время(s)\tVolume");
    println!("--------\t------");

    for i in 0..20 {
        let time = i as f64 * 0.5;
        clock.advance(22050);
        manager.update(22050);

        let signals = signal_sender.get_signals_for_param(&param);
        if let Some(signal) = signals.last() {
            println!("{:.1}\t\t{:.3}", time, signal.value);
        }

        thread::sleep(Duration::from_millis(100));
    }

    println!("\n✅ Пример завершён");
    Ok(())
}