//! Демонстрация аналоговой эмуляции с WDF

use rill_wdf::*;
use rill_core::{AudioGraph, node::GainNode};
use std::f32::consts::PI;
use std::sync::Arc;
use parking_lot::RwLock;
use num_complex::Complex64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Analog Circuit Emulation with WDF ===\n");
    
    // 1. Создаем аудиограф
    let mut graph = AudioGraph::new(44100.0);
    
    // 2. Создаем различные аналоговые модели
    println!("Creating analog emulations:");
    
    // Moog ladder filter
    let moog_filter = WdfMoogFilterNode::new(44100.0, 1000.0, 0.7);
    let moog_id = graph.add_node(Box::new(moog_filter));
    println!("  - Moog Ladder Filter: ID {:?}", moog_id);
    
    // Cassette deck
    let cassette_deck = CassetteDeckNode::new(44100.0);
    let cassette_id = graph.add_node(Box::new(cassette_deck));
    println!("  - Cassette Deck (Sony TC-260): ID {:?}", cassette_id);
    
    // 3. Создаем тестовый сигнал (синус + белый шум)
    let mut test_signal = Vec::with_capacity(44100);
    let frequency = 440.0;
    
    for i in 0..44100 {
        let t = i as f32 / 44100.0;
        let sine = (2.0 * PI * frequency * t).sin() * 0.3;
        let noise = (rand::random::<f32>() - 0.5) * 0.05;
        test_signal.push(sine + noise);
    }
    
    println!("\nGenerated test signal: 440Hz sine + noise");
    
    // 4. Демонстрация параметров Moog фильтра
    println!("\nMoog Filter Parameters:");
    
    if let Some(moog_node) = graph.get_node(moog_id) {
        let metadata = moog_node.metadata();
        
        for param in &metadata.parameters {
            println!("  - {}: {:?} (min: {:?}, max: {:?})", 
                     param.name, param.typ, param.min, param.max);
        }
        
        // Изменяем параметры
        if let Some(mut node) = graph.get_node_mut(moog_id) {
            node.set_param("cutoff", rill_core::param::ParamValue::Float(2000.0))
                .expect("Failed to set cutoff");
                
            node.set_param("resonance", rill_core::param::ParamValue::Float(0.8))
                .expect("Failed to set resonance");
                
            node.set_param("drive", rill_core::param::ParamValue::Float(2.0))
                .expect("Failed to set drive");
                
            println!("Set Moog filter: cutoff=2kHz, resonance=0.8, drive=2.0");
        }
    }
    
    // 5. Демонстрация параметров кассетной деки
    println!("\nCassette Deck Parameters:");
    
    if let Some(cassette_node) = graph.get_node(cassette_id) {
        let metadata = cassette_node.metadata();
        
        for param in &metadata.parameters {
            if let Some(choices) = &param.choices {
                println!("  - {}: {:?}", param.name, param.typ);
                for (name, value) in choices {
                    println!("      {} = {}", name, value);
                }
            } else {
                println!("  - {}: {:?} (default: {:?})", 
                         param.name, param.typ, param.default);
            }
        }
        
        // Изменяем скорость ленты
        if let Some(mut node) = graph.get_node_mut(cassette_id) {
            node.set_param("tape_speed", rill_core::param::ParamValue::Float(9.52))
                .expect("Failed to set tape speed");
                
            node.set_param("bias_level", rill_core::param::ParamValue::Float(0.9))
                .expect("Failed to set bias level");
                
            println!("Set cassette deck: speed=9.52cm/s (double), bias=0.9");
        }
    }
    
    // 6. Демонстрация базовых WDF элементов
    println!("\nBasic WDF Elements:");
    
    let sample_rate = 44100.0;
    
    // Резистор
    let resistor = Resistor::new(1000.0);
    println!("  - Resistor: R = {:.1}Ω, port R = {:.1}Ω", 
             resistor.resistance(), resistor.port_resistance());
    
    // Конденсатор
    let capacitor = Capacitor::new(1e-6, sample_rate);
    println!("  - Capacitor: C = {:.2}μF, port R = {:.1}Ω", 
             capacitor.capacitance() * 1e6, capacitor.port_resistance());
    
    // Диод (1N4148 style)
    let diode = Diode::new(1e-9, 1.0, 300.0);
    println!("  - Diode: Is = {:.1}nA, Vt = {:.2}mV", 
             diode.saturation_current() * 1e9, diode.thermal_voltage() * 1000.0);
    
    // 7. Анализ схемы
    println!("\nCircuit Analysis:");
    
    // Простая RC цепь - ИСПРАВЛЕНО: явное указание типов
    let rc_resistor: Arc<RwLock<dyn WdfElement>> = Arc::new(RwLock::new(Resistor::new(1000.0)));
    let rc_capacitor: Arc<RwLock<dyn WdfElement>> = Arc::new(RwLock::new(Capacitor::new(1e-6, sample_rate)));
    
    let rc_elements = vec![rc_resistor.clone(), rc_capacitor.clone()];
    
    // Анализ частотной характеристики
    let frequencies = vec![20.0, 100.0, 1000.0, 5000.0, 20000.0];
    let response = analysis::frequency_response(&rc_elements, &frequencies, sample_rate);
    
    println!("  RC Circuit Frequency Response:");
    for (freq, complex) in response {
        let magnitude = complex.norm();
        let phase = complex.arg().to_degrees();
        println!("    {:.0}Hz: |H| = {:.3}, ∠ = {:6.1}°", freq, magnitude, phase);
    }
    
    // 8. Интеграция с lo-fi (если доступно)
    #[cfg(feature = "lofi")]
    {
        use rill_wdf::lofi_integration::VintageAnalogSystem;
        
        println!("\nVintage Analog + Digital System:");
        
        let mut vintage_system = VintageAnalogSystem::new(44100.0);
        
        // Обрабатываем тестовый семпл
        let test_input = 0.5;
        let vintage_output = vintage_system.process(test_input);
        
        println!("  Input: {:.3}, Output: {:.3}", test_input, vintage_output);
        println!("  Combines analog WDF filter with 12-bit digital emulation");
    }
    
    // 9. Сохранение preset'ов
    println!("\nExporting Analog Presets...");
    
    // Можно сохранить конфигурации фильтров
    let moog_config = serde_json::json!({
        "type": "moog_ladder",
        "cutoff": 1000.0,
        "resonance": 0.7,
        "drive": 1.0,
        "description": "Moog-style ladder filter emulation"
    });
    
    let cassette_config = serde_json::json!({
        "type": "cassette_deck",
        "tape_speed": 4.76,
        "bias_level": 0.8,
        "wow_flutter": 0.002,
        "description": "Sony TC-260 style cassette deck"
    });
    
    std::fs::write("moog_preset.json", serde_json::to_string_pretty(&moog_config)?)?;
    std::fs::write("cassette_preset.json", serde_json::to_string_pretty(&cassette_config)?)?;
    
    println!("  Saved: moog_preset.json, cassette_preset.json");
    
    println!("\n=== WDF Emulation Demo Complete ===");
    println!("\nKey Features:");
    println!("  1. Physical modeling of analog components");
    println!("  2. Nonlinear elements (diodes, tape saturation)");
    println!("  3. Accurate frequency response");
    println!("  4. Real-time parameter control");
    println!("  5. Integration with digital effects");
    
    Ok(())
}