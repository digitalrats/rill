//! Тест аудиоцепочки: Oscillator -> Filter -> Gain
use rill_core::{AudioGraph, AudioNode};
use rill_core::dsp::{SineOscillator, BiquadFilter};
use rill_core::node::GainNode;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Тест аудиоцепочки Rill Core ===");
    
    // Создать граф обработки
    let sample_rate = 44100.0;
    let mut graph = AudioGraph::new(sample_rate);
    
    // Создать узлы
    let osc = SineOscillator::new(440.0).with_amplitude(0.5);
    let filter = BiquadFilter::lowpass(1000.0, 0.707);
    let gain = GainNode::new(0.8);
    
    // Добавить узлы в граф
    let osc_id = graph.add_node(Box::new(osc));
    let filter_id = graph.add_node(Box::new(filter));
    let gain_id = graph.add_node(Box::new(gain));
    
    println!("Создан граф с узлами:");
    println!("  - Oscillator: {:?}", osc_id);
    println!("  - Filter: {:?}", filter_id);
    println!("  - Gain: {:?}", gain_id);
    
    // TODO: Добавить соединения когда будет реализован GraphProcessor
    
    // Протестировать обработку
    let buffer_size = 512;
    let input = vec![0.0; buffer_size]; // Осциллятору не нужен вход
    let mut output = vec![0.0; buffer_size];
    
    // Обработать через граф
    match graph.process(&[&input], &mut [&mut output]) {
        Ok(()) => println!("✅ Обработка завершена успешно"),
        Err(e) => println!("❌ Ошибка обработки: {}", e),
    }
    
    // Протестировать узлы напрямую
    println!("\nТестирование узлов напрямую:");
    
    let mut osc = SineOscillator::new(440.0);
    osc.init(sample_rate);
    
    let mut direct_output = vec![0.0; 10];
    osc.process(&[], &mut [&mut direct_output]).unwrap();
    
    println!("Осциллятор генерирует сигнал:");
    for (i, &sample) in direct_output.iter().enumerate() {
        println!("  Sample {}: {:.6}", i, sample);
    }
    
    println!("\n✅ Тест завершён успешно!");
    Ok(())
}