//! Демонстрация chiptune и lo-fi эмуляции

use kama_lofi::{
    emulators::NesEmulator,
    LofiConfig, LofiProcessor, ClassicSystem, HardwareEmulation
};
use kama_core::{AudioGraph, node::GainNode};
use std::f32::consts::PI;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Chiptune & Lo-Fi Emulation Demo ===\n");
    
    // 1. Создаем аудиограф
    let mut graph = AudioGraph::new(44100.0);
    
    // 2. Создаем различные lo-fi процессоры
    println!("Creating classic system emulators:");
    
    // NES эмулятор
    let nes = NesEmulator::new(44100.0);
    let nes_id = graph.add_node(Box::new(nes));
    println!("  - NES Emulator: ID {:?}", nes_id);
    
    // 8-bit процессор (Commodore 64 style)
    let c64_config = LofiConfig::for_system(ClassicSystem::Commodore64);
    let c64_processor = LofiProcessor::new(c64_config);
    let c64_id = graph.add_node(Box::new(c64_processor));
    println!("  - Commodore 64 SID: ID {:?}", c64_id);
    
    // 12-bit семплер (Akai S900 style)
    let akai_config = LofiConfig {
        system: ClassicSystem::AkaiS900,
        enable_bitcrush: true,
        enable_sr_reduction: true,
        enable_noise: true,
        dry_wet: 1.0,
        output_gain: 0.8,
        hardware: HardwareEmulation::for_system(ClassicSystem::AkaiS900),
    };
    let akai_processor = LofiProcessor::new(akai_config);
    let akai_id = graph.add_node(Box::new(akai_processor));
    println!("  - Akai S900: ID {:?}", akai_id);
    
    // Fairlight CMI (первый семплер)
    let fairlight_config = LofiConfig::for_system(ClassicSystem::FairlightCMI);
    let fairlight_processor = LofiProcessor::new(fairlight_config);
    let fairlight_id = graph.add_node(Box::new(fairlight_processor));
    println!("  - Fairlight CMI: ID {:?}", fairlight_id);
    
    // 3. Создаем тестовый сигнал (чистый синус)
    let mut sine_wave = Vec::with_capacity(44100);
    let frequency = 440.0;
    let amplitude = 0.5;
    
    for i in 0..44100 {
        let t = i as f32 / 44100.0;
        sine_wave.push((2.0 * PI * frequency * t).sin() * amplitude);
    }
    
    println!("\nGenerated 440Hz sine wave for processing");
    
    // 4. Демонстрация различных битностей
    println!("\nBit depth comparison:");
    
    let test_sample = 0.75f32;
    
    for &bits in &[16, 12, 8, 4] {
        let quantized = kama_lofi::dsp::quantize(test_sample, bits, false);
        let error = (test_sample - quantized).abs();
        let snr_db = 20.0 * (test_sample / error.max(1e-6)).log10();
        
        println!("  {:2}-bit: {:.4} (error: {:.4}, SNR: {:5.1} dB)", 
                 bits, quantized, error, snr_db);
    }
    
    // 5. Демонстрация утилит
    println!("\nLo-Fi Utilities Demo:");
    
    // Создаем тестовый сигнал
    let test_signal: Vec<f32> = (0..4410) // 0.1 секунда
        .map(|i| {
            let t = i as f32 / 44100.0;
            (2.0 * PI * 440.0 * t).sin() * 0.7
        })
        .collect();
    
    let bitcrushed = kama_lofi::utils::create_8bit_sound(&test_signal, 8);
    println!("  - 8-bit bitcrushing applied");
    
    let with_noise = kama_lofi::utils::add_vintage_noise(&bitcrushed, 0.02);
    println!("  - Vintage noise added");
    
    let tape_degraded = kama_lofi::utils::add_tape_degradation(&with_noise, 0.3);
    println!("  - Tape degradation simulated");
    
    let radio_effect = kama_lofi::utils::create_radio_effect(&tape_degraded, 44100.0);
    println!("  - Old radio effect applied");
    
    // 6. Анализ результатов
    println!("\nSignal Analysis:");
    
    let analyze_signal = |name: &str, signal: &[f32]| {
        let max = signal.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        let unique_values = signal.iter()
            .map(|&x| (x * 256.0).round() as i32)
            .collect::<std::collections::HashSet<_>>()
            .len();
        
        println!("  {}: max={:.3}, unique≈{}", name, max, unique_values);
    };
    
    analyze_signal("Original", &test_signal);
    analyze_signal("8-bit", &bitcrushed);
    analyze_signal("Radio", &radio_effect);
    
    // 7. Сохранение preset'ов
    println!("\nExporting presets...");
    
    let presets = vec![
        ("nes_preset.json", ClassicSystem::Nes),
        ("c64_preset.json", ClassicSystem::Commodore64),
        ("akai_preset.json", ClassicSystem::AkaiS900),
        ("fairlight_preset.json", ClassicSystem::FairlightCMI),
    ];
    
    for (filename, system) in presets {
        let config = LofiConfig::for_system(system);
        let json = serde_json::to_string_pretty(&config)?;
        std::fs::write(filename, json)?;
        println!("  Saved: {}", filename);
    }
    
    println!("\n=== Demo Complete ===");
    
    Ok(())
}