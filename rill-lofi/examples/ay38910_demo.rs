//! Демонстрация AY-3-8910 эмулятора

use rill_lofi::emulators::Ay38910Emulator;
use rill_lofi::AudioNode; // <-- импортируем трейт

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== AY-3-8910 (ZX Spectrum 128) Demo ===\n");

    let sample_rate = 44100.0;
    let mut ay = Ay38910Emulator::new(sample_rate);

    // Программируем простую мелодию

    // Канал A: нота (период = 256)
    ay.write_register(0, 0x00);
    ay.write_register(1, 0x01);

    // Канал B: нота (период = 128)
    ay.write_register(2, 0x80);
    ay.write_register(3, 0x00);

    // Канал C: шум
    ay.write_register(6, 0x10); // Период шума

    // Громкость: канал A = 15, канал B = 10, канал C = 8
    ay.write_register(8, 0x0F);
    ay.write_register(9, 0x0A);
    ay.write_register(10, 0x08);

    // Микшер: канал A - тон, канал B - тон, канал C - шум
    ay.write_register(7, 0x38); // 0011 1000

    // Огибающая
    ay.write_register(11, 0x00); // Период огибающей (младшие)
    ay.write_register(12, 0x01); // Период огибающей (старшие)
    ay.write_register(13, 0x0C); // Режим: затухающая

    println!("Генерируем 1 секунду звука...\n");

    let mut output = vec![0.0f32; sample_rate as usize];
    let mut outputs = [&mut output[..]];

    ay.process(&[], &mut outputs)?; // <-- теперь работает, трейт в области видимости

    // Анализ
    let max_amp = output
        .iter()
        .map(|&x| x.abs())
        .fold(0.0f32, |a, b| a.max(b));

    let rms = (output.iter().map(|&x| x * x).sum::<f32>() / output.len() as f32).sqrt();

    println!("Статистика:");
    println!("  Максимальная амплитуда: {:.3}", max_amp);
    println!("  RMS: {:.3}", rms);
    println!(
        "  Уникальных значений: {}",
        output
            .iter()
            .map(|&x| (x * 100.0) as i32)
            .collect::<std::collections::HashSet<_>>()
            .len()
    );

    Ok(())
}
