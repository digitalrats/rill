//! Пример использования LFO с envelope для автоматизации
//!
//! Запуск: cargo run --example lfo_with_envelope

use std::sync::Arc;
use std::time::Duration;
use std::thread;
use kama_core_traits::time::{SystemClock, Clock};
use kama_automation::{
    AutomationContext, AutomationManager,
    automaton::LfoWithEnvelopeAutomaton,  // Исправленный импорт
    Servo, ParameterMapping, TestSignalSender,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== LFO with Envelope Example ===\n");

    let clock = Arc::new(SystemClock::new(44100.0, 120.0));
    let system_clock = SystemClock::new(44100.0, 120.0);
    let signal_sender = Arc::new(TestSignalSender::new());
    
    let mut manager = AutomationManager::new(clock.clone(), system_clock)
        .with_signal_sender(signal_sender.clone());
    
    println!("Создаём LFO с envelope (attack=2s, release=2s)...");
    
    let lfo = Arc::new(LfoWithEnvelopeAutomaton::new(
        0.5,   // частота
        0.8,   // амплитуда
        0.5,   // смещение
        2.0,   // attack time
        2.0,   // release time
    ));
    
    let context = AutomationContext::new(clock.clone());
    
    let servo = Servo::new(
        "envelope_lfo".to_string(),
        lfo.clone(),
        "synth".to_string(),
        "filter_cutoff".to_string(),
        ParameterMapping::Linear,
        context,
    );
    
    manager.add_servo(servo);
    
    println!("LFO с envelope добавлен");
    println!("\nВремя(s)\tЗначение\tСостояние");
    println!("--------\t--------\t---------");
    
    for i in 0..20 {
        let time = i as f64 * 0.5;
        
        Clock::advance(clock.as_ref(), 22050);
        manager.update(22050);
        
        let signals = signal_sender.get_signals_for_param("synth", "filter_cutoff");
        if let Some(&value) = signals.last() {
            let state = if time < 2.0 { "Attack" }
                else if time < 4.0 { "Sustain" }
                else { "Release" };
            
            println!("{:.1}\t\t{:.3}\t{}", time, value, state);
        }
        
        thread::sleep(Duration::from_millis(100));
    }
    
    println!("\n✅ Пример с envelope завершён");
    Ok(())
}