//! Пример использования системы автоматизации с kama-core

use kama_automation::*;
use kama_core::{AudioGraph, node::GainNode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama Automation Demo ===");
    
    // 1. Создаем аудиограф
    let mut graph = AudioGraph::new(44100.0);
    
    // 2. Создаем автоматизированный узел Gain
    let gain_node = GainNode::new(0.5);
    let mut automated_gain = AutomatedNode::new(gain_node, 44100.0);
    
    // 3. Добавляем LFO автоматизацию для параметра gain
    automated_gain.add_automation("gain".to_string(), 0, 0.2, 0.8);
    
    // 4. Симулируем обработку
    println!("Processing with automated gain...");
    
    let input_buffer = vec![0.5f32; 512];
    let mut output_buffer = vec![0.0f32; 512];
    
    for i in 0..5 {
        let inputs = [&input_buffer[..]];
        let mut outputs = [&mut output_buffer[..]];
        
        automated_gain.process_with_automation(&inputs, &mut outputs)
            .expect("Processing failed");
        
        let gain_value = automated_gain.automated_params[0].current_value;
        println!("Block {}: Gain = {:.3}", i, gain_value);
    }
    
    println!("\n=== Automation System Ready ===");
    
    // 5. Демонстрация создания LFO автомата
    let lfo = LfoAutomaton {
        id: 0,
        frequency: 2.0,
        amplitude: 0.3,
        offset: 0.5,
        waveform: Waveform::Triangle,
        sync_to_clock: false,
    };
    
    println!("\nLFO Automaton created:");
    println!("- Frequency: {} Hz", lfo.frequency);
    println!("- Amplitude: {}", lfo.amplitude);
    println!("- Waveform: {:?}", lfo.waveform);
    
    Ok(())
}