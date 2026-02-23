use kama_buffers::{BufferHead, MultiHeadBuffer, ReadMode};
use kama_core::traits::AudioNode;
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
            spacing: 512,
            randomization: 0.3,
        };
        head.set_speed(0.7);
        head.set_pan(-0.5);

        println!("Added granular head:");
        println!("  Grain size: 256 samples");
        println!("  Spacing: 512 samples");
        println!("  Randomization: 30%");
        println!("  Speed: 0.7x");
        println!("  Pan: left (-0.5)");
    }

    // Обрабатываем несколько блоков
    const BUFFER_SIZE: usize = 512;

    println!("\nProcessing granular synthesis...");

    for block in 0..3 {
        let mut output_left = vec![0.0f32; BUFFER_SIZE];
        let mut output_right = vec![0.0f32; BUFFER_SIZE];

        let mut outputs = [&mut output_left[..], &mut output_right[..]];

        if let Err(e) = buffer.process(&[], &mut outputs) {
            eprintln!("Error: {}", e);
            break;
        }

        // Вычисляем статистику
        let max_amp = output_left
            .iter()
            .chain(output_right.iter())
            .map(|&x| x.abs())
            .fold(0.0f32, |a, b| a.max(b));

        println!("  Block {}: max amplitude = {:.4}", block, max_amp);
    }

    println!("\n=== Granular Synthesis Concepts ===");
    println!("• Small audio segments (grains)");
    println!("• Randomized playback positions");
    println!("• Overlap and spacing control");
    println!("• Windowed playback (Hann window)");
    println!("• Creates cloud-like textures");
}
