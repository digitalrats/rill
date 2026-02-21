//! Демонстрация chiptune и lo-fi эмуляции

use kama_lofi::{
    emulators::NesEmulator,
    LofiConfig, LofiProcessor, ClassicSystem, HardwareEmulation,
    AudioNode, dsp  // <-- импортируем трейт и dsp модуль
};
use std::f32::consts::PI;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Chiptune & Lo-Fi Emulation Demo ===\n");
    
    // 1. Создаем различные lo-fi процессоры
    println!("Creating classic system emulators:");
    
    // NES эмулятор
    let mut nes = NesEmulator::new(44100.0);
    nes.init(44100.0);
    println!("  - NES Emulator");
    
    // 8-bit процессор (Commodore 64 style)
    let c64_config = LofiConfig::for_system(ClassicSystem::Commodore64);
    let mut c64_processor = LofiProcessor::new(c64_config);
    c64_processor.init(44100.0);
    println!("  - Commodore 64 SID");
    
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
    let mut akai_processor = LofiProcessor::new(akai_config);
    akai_processor.init(44100.0);
    println!("  - Akai S900");
    
    // Fairlight CMI (первый семплер)
    let fairlight_config = LofiConfig::for_system(ClassicSystem::FairlightCMI);
    let mut fairlight_processor = LofiProcessor::new(fairlight_config);
    fairlight_processor.init(44100.0);
    println!("  - Fairlight CMI");
    
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
        let quantized = dsp::quantization::bitcrush(test_sample, bits, false);  // <-- используем правильный путь
        let error = (test_sample - quantized).abs();
        let snr_db = 20.0 * (test_sample / error.max(1e-6)).log10();
        
        println!("  {:2}-bit: {:.4} (error: {:.4}, SNR: {:5.1} dB)", 
                 bits, quantized, error, snr_db);
    }
    
    // 5. Создаем тестовый сигнал для утилит
    println!("\nLo-Fi Utilities Demo:");
    
    let test_signal: Vec<f32> = (0..4410) // 0.1 секунда
        .map(|i| {
            let t = i as f32 / 44100.0;
            (2.0 * PI * 440.0 * t).sin() * 0.7
        })
        .collect();
    
    // Используем прямые DSP функции вместо утилит
    let bitcrushed: Vec<f32> = test_signal.iter()
        .map(|&s| dsp::quantization::bitcrush(s, 8, true))
        .collect();
    println!("  - 8-bit bitcrushing applied");
    
    let with_noise: Vec<f32> = bitcrushed.iter()
        .map(|&s| s + dsp::noise::white_noise(0.02))
        .collect();
    println!("  - Vintage noise added");
    
    println!("  - Tape degradation and radio effects available in dsp::vintage module");
    
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
    
    println!("\n=== Demo Complete ===");
    
    Ok(())
}