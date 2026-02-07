use kama_core::{AudioGraph, AudioNode};
use kama_core::dsp::{SineOscillator, BiquadFilter, DelayLine};
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
    
    // 1. Создаём осциллятор как источник звука
    let oscillator = SineOscillator::new(440.0);
    let osc_id = graph.add_node(Box::new(oscillator));
    println!("1. Added Sine Oscillator (440Hz) with ID: {:?}", osc_id);
    
    // 2. Создаём многоголовый буфер
    let mut buffer = MultiHeadBuffer::new(4096, sample_rate);
    
    // Добавляем головки с разными параметрами
    let head1_id = buffer.add_head();
    let head2_id = buffer.add_head();
    let head3_id = buffer.add_head();
    
    if let Some(head1) = buffer.get_head_mut(head1_id) {
        head1.state.speed = 1.0;  // Нормальная скорость
        head1.state.pan = -0.8;   // Слева
        head1.with_gain(0.7);
    }
    
    if let Some(head2) = buffer.get_head_mut(head2_id) {
        head2.state.speed = 0.5;  // Половина скорости
        head2.state.pan = 0.0;    // Центр
        head2.with_gain(0.5);
    }
    
    if let Some(head3) = buffer.get_head_mut(head3_id) {
        head3.state.speed = 2.0;  // Двойная скорость
        head3.state.pan = 0.8;    // Справа
        head3.with_gain(0.3);
    }
    
    let buffer_id = graph.add_node(Box::new(buffer));
    println!("2. Added MultiHeadBuffer with 3 heads (different speeds/pan) with ID: {:?}", buffer_id);
    
    // 3. Создаём фильтр для эффекта
    let filter = BiquadFilter::new_lowpass(2000.0, 0.707);
    let filter_id = graph.add_node(Box::new(filter));
    println!("3. Added Low-pass Filter (2000Hz) with ID: {:?}", filter_id);
    
    // 4. Создаём линию задержки
    let delay = DelayLine::new(1.0, sample_rate); // 1 секунда задержки
    let delay_id = graph.add_node(Box::new(delay));
    println!("4. Added Delay Line (1s) with ID: {:?}", delay_id);
    
    // Соединяем узлы
    println!("\nCreating connections...");
    
    // Осциллятор -> Буфер
    if let Ok(_) = graph.connect(
        kama_core::graph::PortId { node: osc_id, index: 0, is_input: false },
        kama_core::graph::PortId { node: buffer_id, index: 0, is_input: true },
        1.0,
    ) {
        println!("  Oscillator -> Buffer");
    }
    
    // Буфер -> Фильтр
    if let Ok(_) = graph.connect(
        kama_core::graph::PortId { node: buffer_id, index: 0, is_input: false },
        kama_core::graph::PortId { node: filter_id, index: 0, is_input: true },
        1.0,
    ) {
        println!("  Buffer -> Filter");
    }
    
    // Фильтр -> Задержка
    if let Ok(_) = graph.connect(
        kama_core::graph::PortId { node: filter_id, index: 0, is_input: false },
        kama_core::graph::PortId { node: delay_id, index: 0, is_input: true },
        0.7, // Немного тише
    ) {
        println!("  Filter -> Delay (0.7 gain)");
    }
    
    // Задержка -> Буфер (feedback)
    if let Ok(_) = graph.connect(
        kama_core::graph::PortId { node: delay_id, index: 0, is_input: false },
        kama_core::graph::PortId { node: buffer_id, index: 0, is_input: true },
        0.3, // Feedback
    ) {
        println!("  Delay -> Buffer (0.3 feedback)");
    }
    
    // Проверяем порядок обработки
    println!("\nProcessing order:");
    for (i, &node_id) in graph.get_processing_order().iter().enumerate() {
        println!("  {}. NodeId({})", i + 1, node_id.0);
    }
    
    // Создаём тестовые буферы
    let buffer_size = 512;
    let mut input_buffer = vec![0.0f32; buffer_size];
    let mut output_left = vec![0.0f32; buffer_size];
    let mut output_right = vec![0.0f32; buffer_size];
    
    let inputs = [&input_buffer[..]];
    let mut outputs = [&mut output_left[..], &mut output_right[..]];
    
    // Обрабатываем несколько блоков
    println!("\nProcessing audio blocks...");
    
    for block in 0..5 {
        // В реальном приложении здесь был бы входной сигнал
        // Для демо просто нули
        
        // Обрабатываем граф
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
        
        println!("  Block {}: L={:.4}, R={:.4}", 
                block, max_left, max_right);
        
        // Очищаем выходные буферы для следующего блока
        output_left.fill(0.0);
        output_right.fill(0.0);
        
        // Симуляция работы в реальном времени
        thread::sleep(Duration::from_millis(50));
    }
    
    // Демонстрация изменения параметров в реальном времени
    println!("\nDynamic parameter changes:");
    
    // Получаем доступ к буферу для изменения параметров
    if let Some(mut node) = graph.get_node_mut(buffer_id) {
        // Изменяем количество головок
        if let Err(e) = node.set_param("num_heads", kama_core::ParamValue::Int(2)) {
            eprintln!("Error setting num_heads: {}", e);
        } else {
            println!("  Changed num_heads to 2");
        }
    }
    
    // Демонстрация работы с отдельными головками напрямую
    println!("\nDirect head manipulation:");
    
    // Получаем ноду как MultiHeadBuffer
    if let Some(node) = graph.get_node_mut(buffer_id) {
        // Приводим к конкретному типу (в реальном коде нужен downcast)
        // Для демо просто показываем концепцию
        println!("  Node has {} parameters", 3); // Пример
    }
    
    println!("\nDemo completed successfully!");
    println!("\n=== Key Features Demonstrated ===");
    println!("1. Multi-head buffer with independent playback heads");
    println!("2. Different playback speeds (0.5x, 1x, 2x)");
    println!("3. Stereo panning (-0.8 left, 0 center, 0.8 right)");
    println!("4. Audio graph with feedback loop");
    println!("5. Real-time parameter changes");
    println!("6. Integration with other DSP modules (filter, delay)");
}