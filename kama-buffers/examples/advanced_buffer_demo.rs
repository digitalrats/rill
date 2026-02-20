use kama_graph::AudioGraph;
use kama_core_traits::{ParamValue, NodeTypeId, PortId};  // Добавляем PortId
use kama_buffers::MultiHeadBuffer;
use std::thread;
use std::time::Duration;

fn main() {
    let sample_rate = 44100.0;
    
    println!("=== Kama Buffers Advanced Demo ===");
    println!("Sample rate: {} Hz", sample_rate);
    println!();
    
    // Создаём граф
    let mut graph = AudioGraph::new(sample_rate);
    
    // Создаём многоголовый буфер
    let mut buffer = MultiHeadBuffer::new(4096, sample_rate);
    
    // Добавляем головки с разными параметрами
    let head1_id = buffer.add_head();
    let head2_id = buffer.add_head();
    let head3_id = buffer.add_head();
    
    if let Some(head1) = buffer.get_head_mut(head1_id) {
        head1.state.speed = 1.0;
        head1.state.pan = -0.8;
        head1.state.volume = 0.7;
        println!("  Head 1: speed=1.0, pan=-0.8, volume=0.7");
    }
    
    if let Some(head2) = buffer.get_head_mut(head2_id) {
        head2.state.speed = 0.5;
        head2.state.pan = 0.0;
        head2.state.volume = 0.5;
        println!("  Head 2: speed=0.5, pan=0.0, volume=0.5");
    }
    
    if let Some(head3) = buffer.get_head_mut(head3_id) {
        head3.state.speed = 2.0;
        head3.state.pan = 0.8;
        head3.state.volume = 0.3;
        println!("  Head 3: speed=2.0, pan=0.8, volume=0.3");
    }
    
    // Добавляем буфер в граф
    let buffer_id = graph.add_node(Box::new(buffer));
    println!("\nAdded MultiHeadBuffer to graph with ID: {:?}", buffer_id);
    
    // Создаём тестовый сигнал (синус) для записи в буфер (пока не используется)
    let _test_signal: Vec<f32> = (0..4096)
        .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sample_rate).sin() * 0.5)
        .collect();
    
    println!("\nProcessing audio...");
    
    // Проверяем порядок обработки
    println!("Processing order: {:?}", graph.processing_order());
    
    // Получаем информацию о буфере через параметры
    if let Some(node) = graph.get_node(buffer_id) {
        if node.node_type_id() == NodeTypeId::of::<MultiHeadBuffer>() {
            if let Some(ParamValue::Int(num_heads)) = node.get_param("num_heads") {
                println!("Number of heads: {}", num_heads);
            }
            if let Some(ParamValue::Int(buffer_size)) = node.get_param("buffer_size") {
                println!("Buffer size: {} samples", buffer_size);
            }
        }
    }
    
    // Параметры обработки
    let buffer_size = 512;
    let num_blocks = 5;
    
    println!("\nProcessing {} audio blocks of size {}...", num_blocks, buffer_size);
    
    for block in 0..num_blocks {
        // Создаём новые буферы для каждого блока
        let mut output_left = vec![0.0f32; buffer_size];
        let mut output_right = vec![0.0f32; buffer_size];
        let input_buffer = vec![0.0f32; buffer_size]; // Пустой входной буфер
        
        let inputs = [&input_buffer[..]];
        let mut outputs = [&mut output_left[..], &mut output_right[..]];
        
        if let Err(e) = graph.process(&inputs, &mut outputs) {
            eprintln!("Error processing block {}: {}", block, e);
            break;
        }
        
        // Вычисляем статистику выхода
        let max_left = output_left.iter()
            .map(|&x| x.abs())
            .fold(0.0f32, |a, b| a.max(b));
        let max_right = output_right.iter()
            .map(|&x| x.abs())
            .fold(0.0f32, |a, b| a.max(b));
        
        println!("  Block {}: L={:.4}, R={:.4}", block, max_left, max_right);
        
        // Симуляция работы в реальном времени
        thread::sleep(Duration::from_millis(50));
    }
    
    // Демонстрация изменения параметров в реальном времени
    println!("\nDynamic parameter changes:");
    
    // Изменяем количество головок через параметры
    if let Some(node) = graph.get_node_mut(buffer_id) {
        // Проверяем тип узла
        if node.node_type_id() == NodeTypeId::of::<MultiHeadBuffer>() {
            if let Err(e) = node.set_param("num_heads", ParamValue::Int(2)) {
                eprintln!("Error setting num_heads: {}", e);
            } else {
                println!("  Changed num_heads to 2");
                
                // Проверяем, что изменилось
                if let Some(ParamValue::Int(new_count)) = node.get_param("num_heads") {
                    println!("  Now num_heads = {}", new_count);
                }
            }
        }
    }
    
    println!("\nDemo completed successfully!");
    println!("\n=== Key Features Demonstrated ===");
    println!("1. Multi-head buffer with independent playback heads");
    println!("2. Different playback speeds (0.5x, 1x, 2x)");
    println!("3. Stereo panning (-0.8 left, 0 center, 0.8 right)");
    println!("4. Audio graph integration");
    println!("5. Type identification via NodeTypeId");
    println!("6. Parameter-based control (no downcasting)");
    println!("7. Real-time parameter changes");
}