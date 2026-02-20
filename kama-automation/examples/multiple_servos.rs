//! Пример использования нескольких сервоприводов одновременно
//!
//! Запуск: cargo run --example multiple_servos

use std::sync::Arc;
use std::time::Duration;
use std::thread;
use kama_core::time::{SystemClock, Clock};  // Добавили импорт Clock
use kama_automation::{
    AutomationManager, TestSignalSender,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Multiple Servos Example ===\n");

    let clock = Arc::new(SystemClock::new(44100.0, 120.0));
    let system_clock = SystemClock::new(44100.0, 120.0);
    let signal_sender = Arc::new(TestSignalSender::new());
    
    let mut manager = AutomationManager::new(clock.clone(), system_clock)
        .with_signal_sender(signal_sender.clone());
    
    println!("Добавляем несколько LFO для разных параметров:\n");
    
    manager.add_lfo(
        "volume_lfo",
        0.2, 0.3, 0.5, "mixer", "volume"
    );
    println!("  Volume LFO: 0.2 Hz, range [0.2, 0.8]");
    
    manager.add_lfo(
        "pan_lfo",
        2.0, 0.8, 0.0, "mixer", "pan"
    );
    println!("  Pan LFO: 2.0 Hz, range [-0.8, 0.8]");
    
    manager.add_lfo(
        "filter_lfo",
        1.0, 0.4, 0.5, "synth", "cutoff"
    );
    println!("  Filter LFO: 1.0 Hz, range [0.1, 0.9]");
    
    println!("\nВремя(s)\tVolume\t\tPan\t\tCutoff");
    println!("--------\t------\t\t---\t\t------");
    
    for i in 0..20 {
        let time = i as f64 * 0.25;
        
        Clock::advance(clock.as_ref(), 11025);
        manager.update(11025);
        
        let volume = signal_sender.get_signals_for_param("mixer", "volume")
            .last().copied().unwrap_or(0.5);
        let pan = signal_sender.get_signals_for_param("mixer", "pan")
            .last().copied().unwrap_or(0.0);
        let cutoff = signal_sender.get_signals_for_param("synth", "cutoff")
            .last().copied().unwrap_or(0.5);
        
        println!("{:.2}\t\t{:.3}\t\t{:.3}\t\t{:.3}", 
                 time, volume, pan, cutoff);
        
        thread::sleep(Duration::from_millis(50));
    }
    
    println!("\n✅ Все сервоприводы работают одновременно");
    Ok(())
}