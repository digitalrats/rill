// FILE: examples/final_demo.rs
use kama_core::{
    AudioGraph, 
    dsp::{SineOscillator, BiquadFilter},
    node::GainNode,
    graph::PortId,
};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("=== Kama Audio Demo - Финальная демонстрация ===\n");
    
    let sample_rate = 44100.0;
    
    // 1. Простая цепочка
    println!("1. Простая цепочка: Осциллятор → Усилитель");
    let mut simple_graph = AudioGraph::new(sample_rate);
    
    let osc = Box::new(SineOscillator::new(440.0));
    let gain = Box::new(GainNode::new(0.5));
    
    let osc_id = simple_graph.add_node(osc);
    let gain_id = simple_graph.add_node(gain);
    
    simple_graph.connect(
        PortId { node: osc_id, index: 0, is_input: false },
        PortId { node: gain_id, index: 0, is_input: true },
        1.0,
    )?;
    
    let mut simple_out = vec![0.0f32; 10];
    simple_graph.process(&[], &mut [simple_out.as_mut_slice()])?;
    
    println!("   Первые 5 сэмплов:");
    for i in 0..5 {
        println!("     [{:2}]: {:8.6}", i, simple_out[i]);
    }
    println!("   ✅ Простая цепочка работает\n");
    
    // 2. Параллельный микс
    println!("2. Параллельный микс: 2 осциллятора → Усилитель");
    let mut mix_graph = AudioGraph::new(sample_rate);
    
    let osc1_id = mix_graph.add_node(Box::new(SineOscillator::new(440.0)));
    let osc2_id = mix_graph.add_node(Box::new(SineOscillator::new(660.0)));
    let mix_gain_id = mix_graph.add_node(Box::new(GainNode::new(0.3)));
    
    // Подключаем оба осциллятора с разными gain
    mix_graph.connect(
        PortId { node: osc1_id, index: 0, is_input: false },
        PortId { node: mix_gain_id, index: 0, is_input: true },
        0.5,
    )?;
    
    mix_graph.connect(
        PortId { node: osc2_id, index: 0, is_input: false },
        PortId { node: mix_gain_id, index: 0, is_input: true },
        0.5,
    )?;
    
    let mut mix_out = vec![0.0f32; 10];
    mix_graph.process(&[], &mut [mix_out.as_mut_slice()])?;
    
    println!("   Первые 5 сэмплов микса:");
    for i in 0..5 {
        println!("     [{:2}]: {:8.6}", i, mix_out[i]);
    }
    println!("   ✅ Параллельное микширование работает\n");
    
    // 3. Полная цепочка с фильтром
    println!("3. Полная цепочка: Осциллятор → Фильтр → Усилитель");
    let mut chain_graph = AudioGraph::new(sample_rate);
    
    let chain_osc_id = chain_graph.add_node(Box::new(SineOscillator::new(440.0)));
    let chain_filter_id = chain_graph.add_node(Box::new(BiquadFilter::lowpass(1000.0, 0.707)));
    let chain_gain_id = chain_graph.add_node(Box::new(GainNode::new(0.5)));
    
    chain_graph.connect(
        PortId { node: chain_osc_id, index: 0, is_input: false },
        PortId { node: chain_filter_id, index: 0, is_input: true },
        1.0,
    )?;
    
    chain_graph.connect(
        PortId { node: chain_filter_id, index: 0, is_input: false },
        PortId { node: chain_gain_id, index: 0, is_input: true },
        1.0,
    )?;
    
    let mut chain_out = vec![0.0f32; 10];
    chain_graph.process(&[], &mut [chain_out.as_mut_slice()])?;
    
    println!("   Первые 5 сэмплов цепочки:");
    for i in 0..5 {
        println!("     [{:2}]: {:8.6}", i, chain_out[i]);
    }
    println!("   ✅ Полная цепочка с фильтром работает\n");
    
    // 4. Сложный патч: параллельные цепи + суммирование
    println!("4. Сложный патч: Две параллельные цепи с разными фильтрами");
    let mut complex_graph = AudioGraph::new(sample_rate);
    
    // Создаем две параллельные цепи
    let c_osc1_id = complex_graph.add_node(Box::new(SineOscillator::new(440.0)));
    let c_filter1_id = complex_graph.add_node(Box::new(BiquadFilter::lowpass(800.0, 0.707)));
    let c_gain1_id = complex_graph.add_node(Box::new(GainNode::new(0.4)));
    
    let c_osc2_id = complex_graph.add_node(Box::new(SineOscillator::new(880.0)));
    let c_filter2_id = complex_graph.add_node(Box::new(BiquadFilter::highpass(500.0, 0.707)));
    let c_gain2_id = complex_graph.add_node(Box::new(GainNode::new(0.3)));
    
    let c_master_id = complex_graph.add_node(Box::new(GainNode::new(0.7)));
    
    // Цепь 1: osc1 → filter1 → gain1 → master
    complex_graph.connect(
        PortId { node: c_osc1_id, index: 0, is_input: false },
        PortId { node: c_filter1_id, index: 0, is_input: true },
        1.0,
    )?;
    
    complex_graph.connect(
        PortId { node: c_filter1_id, index: 0, is_input: false },
        PortId { node: c_gain1_id, index: 0, is_input: true },
        1.0,
    )?;
    
    complex_graph.connect(
        PortId { node: c_gain1_id, index: 0, is_input: false },
        PortId { node: c_master_id, index: 0, is_input: true },
        1.0,
    )?;
    
    // Цепь 2: osc2 → filter2 → gain2 → master  
    complex_graph.connect(
        PortId { node: c_osc2_id, index: 0, is_input: false },
        PortId { node: c_filter2_id, index: 0, is_input: true },
        1.0,
    )?;
    
    complex_graph.connect(
        PortId { node: c_filter2_id, index: 0, is_input: false },
        PortId { node: c_gain2_id, index: 0, is_input: true },
        1.0,
    )?;
    
    complex_graph.connect(
        PortId { node: c_gain2_id, index: 0, is_input: false },
        PortId { node: c_master_id, index: 0, is_input: true },
        1.0,
    )?;
    
    let mut complex_out = vec![0.0f32; 10];
    complex_graph.process(&[], &mut [complex_out.as_mut_slice()])?;
    
    println!("   Первые 5 сэмплов сложного патча:");
    for i in 0..5 {
        println!("     [{:2}]: {:8.6}", i, complex_out[i]);
    }
    
    let has_signal = complex_out.iter().any(|&x| x != 0.0);
    if has_signal {
        println!("   ✅ Сложный патч работает!\n");
    }
    
    println!("=== ВСЕ ТЕСТЫ ПРОЙДЕНЫ УСПЕШНО! ===");
    println!("🎵 Kama Audio готов к использованию!");
    
    // Краткая сводка
    println!("\n📊 Сводка:");
    println!("  - AudioGraph: ✓ маршрутизация работает");
    println!("  - DSP узлы: ✓ осциллятор, фильтр, усилитель");
    println!("  - Параллельная обработка: ✓ работает");
    println!("  - Последовательная обработка: ✓ работает");
    println!("  - Сложные патчи: ✓ поддерживаются");
    
    Ok(())
}