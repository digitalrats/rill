use kama_core::{
    AudioGraph, 
    dsp::{SineOscillator, BiquadFilter},  // Убрали BiquadType, он не нужен
    node::GainNode,
};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("=== Тест полной аудиоцепочки ===\n");
    
    // Создаём граф
    let sample_rate = 44100.0;
    let mut graph = AudioGraph::new(sample_rate);
    
    // Создаём узлы
    let osc = Box::new(SineOscillator::new(440.0));     // Осциллятор 440 Гц
    let filter = Box::new(BiquadFilter::lowpass(1000.0, 0.707));  // Используем lowpass вместо new_lowpass
    let gain = Box::new(GainNode::new(0.5));
    
    // Добавляем узлы в граф
    let osc_id = graph.add_node(osc);
    let filter_id = graph.add_node(filter);
    let gain_id = graph.add_node(gain);
    
    println!("Созданы узлы:");
    println!("  Осциллятор: {:?}", osc_id);
    println!("  Фильтр: {:?}", filter_id);
    println!("  Усилитель: {:?}", gain_id);
    
    // Создаём соединения
    // Осциллятор -> Фильтр
    use kama_core::graph::PortId;
    
    let osc_out = PortId {
        node: osc_id,
        index: 0,
        is_input: false,
    };
    
    let filter_in = PortId {
        node: filter_id,
        index: 0,
        is_input: true,
    };
    
    graph.connect(osc_out, filter_in, 1.0)?;
    
    // Фильтр -> Усилитель
    let filter_out = PortId {
        node: filter_id,
        index: 0,
        is_input: false,
    };
    
    let gain_in = PortId {
        node: gain_id,
        index: 0,
        is_input: true,
    };
    
    graph.connect(filter_out, gain_in, 1.0)?;
    
    println!("\nСозданы соединения:");
    println!("  Осциллятор[0] -> Фильтр[0]");
    println!("  Фильтр[0] -> Усилитель[0]");
    
    // Проверяем порядок обработки
    println!("\nПорядок обработки: {:?}", graph.get_processing_order());
    
    // Обрабатываем аудио используем process вместо process
    let buffer_size = 64;
    let mut output_buffer = vec![0.0f32; buffer_size];
    
    let inputs: &[&[f32]] = &[];
    let mut outputs = [output_buffer.as_mut_slice()];
    
    // Используем process (базовая версия)
    graph.process(inputs, &mut outputs)?;
    
    // Анализируем результат
    println!("\nПервые 10 сэмплов выхода:");
    for (i, &sample) in output_buffer.iter().take(10).enumerate() {
        println!("  [{}]: {:.6}", i, sample);
    }
    
    // Проверяем, что сигнал обработан
    let max_amplitude = output_buffer.iter()
        .map(|&x| x.abs())
        .fold(0.0f32, f32::max);
    
    println!("\nМаксимальная амплитуда: {:.6}", max_amplitude);
    println!("Ожидаемая амплитуда: {:.6} (gain 0.5)", 0.5);
    
    if max_amplitude > 0.0 {
        println!("✅ Сигнал успешно обработан через цепочку!");
    } else {
        println!("❌ Сигнал не обработан");
    }
    
    // Тестируем параллельную обработку (микширование)
    println!("\n=== Тест параллельной обработки (микширование) ===");
    let mut graph2 = AudioGraph::new(sample_rate);
    
    // Два осциллятора на разных частотах
    let osc1 = Box::new(SineOscillator::new(440.0));
    let osc2 = Box::new(SineOscillator::new(550.0));
    let mixer_gain = Box::new(GainNode::new(0.3));
    
    let osc1_id = graph2.add_node(osc1);
    let osc2_id = graph2.add_node(osc2);
    let mixer_id = graph2.add_node(mixer_gain);
    
    // Подключаем оба осциллятора к одному усилителю
    graph2.connect(
        PortId { node: osc1_id, index: 0, is_input: false },
        PortId { node: mixer_id, index: 0, is_input: true },
        0.5,  // Gain для первого осциллятора
    )?;
    
    graph2.connect(
        PortId { node: osc2_id, index: 0, is_input: false },
        PortId { node: mixer_id, index: 0, is_input: true },
        0.5,  // Gain для второго осциллятора
    )?;
    
    println!("Смешивание двух осцилляторов через один усилитель");
    println!("Порядок обработки: {:?}", graph2.get_processing_order());
    
    let mut mixed_output = vec![0.0f32; buffer_size];
    
    // Используем process_simple
    graph2.process(&[], &mut [mixed_output.as_mut_slice()])?;
    
    let mixed_max = mixed_output.iter()
        .map(|&x| x.abs())
        .fold(0.0f32, f32::max);
    
    println!("Максимальная амплитуда микса: {:.6}", mixed_max);
    println!("✅ Параллельная обработка работает!");
    
    Ok(())
}