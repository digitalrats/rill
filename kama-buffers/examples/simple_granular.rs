use kama_buffers::{MultiHeadBuffer, ReadMode};
use kama_core_traits::AudioNode;
use std::f32::consts::PI;

fn main() {
    println!("=== Granular Synthesis Demo ===");
    let sample_rate = 44100.0;
    
    // Создаём буфер с гранулярным режимом
    let mut buffer = MultiHeadBuffer::new(8192, sample_rate);
    
    // Записываем тестовый сигнал (синусоида)
    let mut test_signal = vec![0.0f32; 1024];
    for i in 0..1024 {
        let freq = 440.0;
        test_signal[i] = (2.0 * PI * freq * i as f32 / sample_rate).sin() * 0.5;
    }
    
    buffer.write(&test_signal);
    println!("Written {} samples to buffer", test_signal.len());
    
    // Добавляем гранулярную головку
    let head_id = buffer.add_head();
    
    if let Some(head) = buffer.get_head_mut(head_id) {
        head.read_mode = ReadMode::Granular {
            grain_size: 256,
            grain_spacing: 512,
            randomization: 0.3,
        };
        head.state.speed = 0.7;
        head.state.pan = -0.5;
        
        println!("Added granular head:");
        println!("  Grain size: 256 samples");
        println!("  Grain spacing: 512 samples");
        println!("  Randomization: 30%");
        println!("  Speed: 0.7x");
        println!("  Pan: left (-0.5)");
    }
    
    // Обрабатываем несколько блоков
    const BUFFER_SIZE: usize = 512;
    const NUM_CHANNELS: usize = 2;
    
    // Предварительно аллоцированный буфер
    let mut output_storage = vec![0.0f32; BUFFER_SIZE * NUM_CHANNELS];
    
    println!("\nProcessing granular synthesis...");
    
    for block in 0..3 {
        // Разделяем буфер на каналы
        let (left_buf, right_buf) = output_storage.split_at_mut(BUFFER_SIZE);
        
        let mut outputs = [&mut left_buf[..BUFFER_SIZE], &mut right_buf[..BUFFER_SIZE]];
        
        if let Err(e) = buffer.process(&[&[]], &mut outputs) {
            eprintln!("Error: {}", e);
            break;
        }
        
        // Вычисляем статистику
        let max_amp = output_storage.iter()
            .map(|&x| x.abs())
            .fold(0.0f32, |a, b| a.max(b));
        
        println!("  Block {}: max amplitude = {:.4}", block, max_amp);
        
        // Очищаем буферы
        output_storage.fill(0.0);
    }
    
    println!("\n=== Granular Synthesis Concepts ===");
    println!("• Small audio segments (grains)");
    println!("• Randomized playback positions");
    println!("• Overlap and spacing control");
    println!("• Windowed playback (Hann window)");
    println!("• Creates cloud-like textures");
}