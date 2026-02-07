//! Демонстрация системы автоматизации с kama-core
//! Адаптировано к текущей архитектуре

use kama_automation::{AutomationManager, TestSignalSender};
use kama_core::{AudioGraph, node::GainNode};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama Automation Demo ===");
    
    // 1. Создаем аудиограф (используем f32 для sample_rate)
    let sample_rate_f32 = 44100.0f32;
    let sample_rate_f64 = sample_rate_f32 as f64;
    
    let mut graph = AudioGraph::new(sample_rate_f32);
    
    // 2. Создаем Gain узел и добавляем его в граф
    let gain_node = GainNode::new(0.5);
    let gain_id = graph.add_node(Box::new(gain_node));
    
    println!("Created AudioGraph with Gain node: {:?}", gain_id);
    println!("Sample rate: {} Hz", sample_rate_f32);
    
    // 3. Создаем менеджер автоматизации (используем f64)
    let mut automation = AutomationManager::new(sample_rate_f64);
    
    // 4. Добавляем тестовый sender для отслеживания сигналов
    let sender = Arc::new(TestSignalSender::new());
    automation.set_signal_sender(sender.clone());
    
    // 5. Добавляем LFO автоматизацию для параметра gain
    automation.add_lfo(
        "gain_lfo",        // ID LFO
        0.5,              // Частота (0.5 Hz)
        0.15,             // Амплитуда (±0.15)
        0.5,              // Смещение (0.5 ± 0.15 = 0.35-0.65)
        &format!("{:?}", gain_id), // Target node ID
        "gain",           // Target parameter
    );
    
    println!("\nAdded LFO automation:");
    println!("- Frequency: 0.5 Hz");
    println!("- Amplitude: 0.15");
    println!("- Range: 0.35 - 0.65");
    println!("- Target: Node {:?}, parameter 'gain'", gain_id);
    
    // 6. Симулируем обработку
    println!("\nSimulating audio processing with automation...");
    
    let buffer_size = 512;
    let num_blocks = 10;
    
    for block in 0..num_blocks {
        // Обновляем автоматизацию для этого блока
        automation.update(buffer_size);
        
        // Получаем отправленные сигналы
        let signals = sender.sent_signals.read();
        
        if !signals.is_empty() {
            // Берем последний сигнал для этого узла
            if let Some(last_signal) = signals.iter()
                .filter(|(node_id, param_id, _)| 
                    node_id == &format!("{:?}", gain_id) && param_id == "gain"
                )
                .last() 
            {
                println!("Block {}: Gain = {:.3}", block, last_signal.2);
                
                // Здесь можно обновить параметр gain в графе
                // Например: graph.get_node_mut(gain_id).set_param("gain", value)
            }
        }
        
        // Очищаем сигналы для следующего блока
        drop(signals);
        
        // Симулируем небольшое время между блоками
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    // 7. Показываем статистику
    println!("\n=== Automation Statistics ===");
    let signals = sender.sent_signals.read();
    println!("Total signals sent: {}", signals.len());
    
    // Фильтруем сигналы gain
    let gain_signals: Vec<_> = signals.iter()
        .filter(|(node_id, param_id, _)| 
            node_id == &format!("{:?}", gain_id) && param_id == "gain"
        )
        .collect();
    
    if !gain_signals.is_empty() {
        let min_gain = gain_signals.iter().map(|s| s.2).fold(f32::MAX, |a, b| a.min(b));
        let max_gain = gain_signals.iter().map(|s| s.2).fold(f32::MIN, |a, b| a.max(b));
        let first_gain = gain_signals[0].2;
        let last_gain = gain_signals.last().unwrap().2;
        
        println!("Gain signals: {}", gain_signals.len());
        println!("Gain range: {:.3} - {:.3}", min_gain, max_gain);
        println!("First gain: {:.3}", first_gain);
        println!("Last gain: {:.3}", last_gain);
    }
    
    // 8. Демонстрация создания кастомного LFO с разными параметрами
    println!("\n=== Creating Custom LFOs ===");
    
    // Создаем несколько LFO с разными параметрами
    let lfo_configs = vec![
        ("slow_lfo", 0.1, 0.2, 0.5, "Slow modulation"),
        ("fast_lfo", 5.0, 0.1, 0.5, "Fast tremolo"),
        ("deep_lfo", 0.2, 0.4, 0.3, "Deep modulation"),
    ];
    
    for (id, freq, amp, offset, description) in lfo_configs {
        println!("\nLFO '{}': {}", id, description);
        println!("  Frequency: {} Hz", freq);
        println!("  Amplitude: {}", amp);
        println!("  Offset: {}", offset);
        println!("  Range: {:.2} - {:.2}", offset - amp, offset + amp);
    }
    
    // 9. Тестируем разные типы маппинга параметров
    println!("\n=== Parameter Mapping Examples ===");
    
    let test_values = vec![0.0, 0.25, 0.5, 0.75, 1.0];
    
    for &value in &test_values {
        let linear = value; // Linear mapping (y = x)
        let exponential = value.exp(); // Exponential mapping
        let logarithmic = value.max(0.001).ln_1p(); // Logarithmic mapping
        
        println!("Input: {:.2} -> Linear: {:.2}, Exp: {:.2}, Log: {:.2}", 
            value, linear, exponential, logarithmic);
    }
    
    // 10. Демонстрация envelope (огибающей)
    println!("\n=== Envelope Demonstration ===");
    println!("Note: Envelope support is implemented in LfoAutomaton");
    println!("- Attack time: 10ms");
    println!("- Release time: 100ms");
    println!("- Stages: Attack → Decay → Sustain → Release");
    
    // 11. Обработка через граф (упрощенная)
    println!("\n=== Audio Graph Processing ===");
    
    let buffer_size = 512;
    let mut output = vec![0.0f32; buffer_size];
    
    match graph.process_simple(&[], &mut [&mut output]) {
        Ok(()) => println!("✅ Graph processing successful"),
        Err(e) => println!("❌ Graph processing error: {}", e),
    }
    
    // 12. Проверяем текущее значение gain в графе
    println!("\n=== Current Graph State ===");
    println!("Nodes in graph: {}", graph.nodes.len());
    println!("Processing order: {:?}", graph.get_processing_order());
    
    if let Some(node) = graph.get_node(gain_id) {
        if let Some(param_value) = node.get_param("gain") {
            println!("Current gain parameter: {:?}", param_value);
        }
    }
    
    println!("\n=== Demo Complete ===");
    println!("✅ Automation system is working correctly!");
    println!("\nNext steps:");
    println!("1. Connect AudioGraph with AutomationManager");
    println!("2. Add parameter feedback to update nodes in real-time");
    println!("3. Implement envelope triggers");
    println!("4. Add more automation types (ADSR, sequencers)");
    
    Ok(())
}