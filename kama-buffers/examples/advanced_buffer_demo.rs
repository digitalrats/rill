use kama_buffers::{BufferManager, MultiHeadBuffer};
use kama_core::traits::AudioNode;
use std::thread;
use std::time::Duration;

fn main() {
    let sample_rate = 44100.0;

    println!("=== Kama Buffers Advanced Demo ===");
    println!("Sample rate: {} Hz", sample_rate);
    println!();

    // Создаём менеджер буферов
    let buffer_manager = BufferManager::new();

    // Создаём многоголовый буфер через менеджер
    let buffer = buffer_manager.create_multi_head("granular", 4096, sample_rate);
    let mut buffer_guard = buffer.write();

    // Добавляем головки с разными параметрами
    let head1_id = buffer_guard.add_head();
    let head2_id = buffer_guard.add_head();
    let head3_id = buffer_guard.add_head();

    if let Some(head1) = buffer_guard.get_head_mut(head1_id) {
        head1.set_speed(1.0);
        head1.set_pan(-0.8);
        head1.set_volume(0.7);
        println!("  Head 1: speed=1.0, pan=-0.8, volume=0.7");
    }

    if let Some(head2) = buffer_guard.get_head_mut(head2_id) {
        head2.set_speed(0.5);
        head2.set_pan(0.0);
        head2.set_volume(0.5);
        println!("  Head 2: speed=0.5, pan=0.0, volume=0.5");
    }

    if let Some(head3) = buffer_guard.get_head_mut(head3_id) {
        head3.set_speed(2.0); // было head2, исправлено на head3
        head3.set_pan(0.8); // было head2, исправлено на head3
        head3.set_volume(0.3); // было head2, исправлено на head3
        println!("  Head 3: speed=2.0, pan=0.8, volume=0.3");
    }

    println!("\nProcessing audio...");

    // Параметры обработки
    let buffer_size = 512;
    let num_blocks = 5;

    println!(
        "\nProcessing {} audio blocks of size {}...",
        num_blocks, buffer_size
    );

    // Создаём тестовый сигнал через acquire
    let mut test_buffer = buffer_manager.acquire(buffer_size).unwrap();
    for i in 0..buffer_size {
        test_buffer.as_mut_slice()[i] =
            (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sample_rate).sin() * 0.5;
    }
    buffer_guard.write(test_buffer.as_slice());

    // Освобождаем тестовый буфер (он автоматически вернется в пул при drop)
    drop(test_buffer);

    for block in 0..num_blocks {
        // Создаём выходные буферы через acquire
        let mut output_left = buffer_manager.acquire(buffer_size).unwrap();
        let mut output_right = buffer_manager.acquire(buffer_size).unwrap();

        let mut outputs = [output_left.as_mut_slice(), output_right.as_mut_slice()];

        if let Err(e) = buffer_guard.process(&[], &mut outputs) {
            eprintln!("Error processing block {}: {}", block, e);
            break;
        }

        // Вычисляем статистику выхода
        let max_left = output_left
            .as_slice()
            .iter()
            .map(|&x| x.abs())
            .fold(0.0f32, |a, b| a.max(b));
        let max_right = output_right
            .as_slice()
            .iter()
            .map(|&x| x.abs())
            .fold(0.0f32, |a, b| a.max(b));

        println!("  Block {}: L={:.4}, R={:.4}", block, max_left, max_right);

        // Выходные буферы автоматически вернутся в пул при выходе из блока

        // Симуляция работы в реальном времени
        thread::sleep(Duration::from_millis(50));
    }

    // Буферы автоматически вернутся в пул при выходе из scope

    println!("\nDemo completed successfully!");
    println!("\n=== Key Features Demonstrated ===");
    println!("1. Multi-head buffer with independent playback heads");
    println!("2. Different playback speeds (0.5x, 1x, 2x)");
    println!("3. Stereo panning (-0.8 left, 0 center, 0.8 right)");
    println!("4. BufferManager with acquire/release pattern");
    println!("5. Automatic buffer pooling");
}
