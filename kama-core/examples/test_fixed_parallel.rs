// FILE: kama-core/examples/test_fixed_parallel.rs
use kama_core::{
    AudioGraph, 
    dsp::SineOscillator,
    node::GainNode,
    graph::PortId,
    AudioNode,  // Добавляем импорт трейта
};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("=== Исправленный тест параллельного микширования ===\n");
    
    let sample_rate = 44100.0;
    let mut graph = AudioGraph::new(sample_rate);
    
    // 1. Сначала создадим осцилляторы
    println!("1. Создаем узлы:");
    let osc1 = Box::new(SineOscillator::new(440.0));
    let osc2 = Box::new(SineOscillator::new(660.0));
    let gain = Box::new(GainNode::new(0.3)); // Меньший gain
    
    let osc1_id = graph.add_node(osc1);
    let osc2_id = graph.add_node(osc2);
    let gain_id = graph.add_node(gain);
    
    println!("   Осциллятор 1 (440 Гц): {:?}", osc1_id);
    println!("   Осциллятор 2 (660 Гц): {:?}", osc2_id);
    println!("   Усилитель (gain 0.3): {:?}", gain_id);
    
    // 2. Создаем соединения
    println!("\n2. Создаем соединения:");
    
    // Осциллятор 1 -> Усилитель с gain 0.5
    graph.connect(
        PortId { node: osc1_id, index: 0, is_input: false },
        PortId { node: gain_id, index: 0, is_input: true },
        0.5,
    )?;
    
    // Осциллятор 2 -> Усилитель с gain 0.5  
    graph.connect(
        PortId { node: osc2_id, index: 0, is_input: false },
        PortId { node: gain_id, index: 0, is_input: true },
        0.5,
    )?;
    
    println!("   Соединения созданы успешно");
    
    // 3. Проверим входные порты усилителя
    println!("\n3. Проверка усилителя:");
    if let Some(gain_node) = graph.get_node(gain_id) {
        println!("   Усилитель имеет {} входов, {} выходов", 
                gain_node.num_inputs(), gain_node.num_outputs());
    }
    
    // 4. Обработаем с большим буфером
    println!("\n4. Обработка (буфер 256 сэмплов):");
    let buffer_size = 256;
    let mut output = vec![0.0f32; buffer_size];
    
    graph.process(&[], &mut [output.as_mut_slice()])?;
    
    // 5. Анализируем результат
    println!("\n5. Анализ результата:");
    
    // Найдем максимальную амплитуду
    let max_amp = output.iter()
        .map(|&x| x.abs())
        .fold(0.0f32, f32::max);
    
    println!("   Максимальная амплитуда: {:.6}", max_amp);
    
    // Проверим есть ли сигнал
    if max_amp > 0.0 {
        println!("   ✅ Сигнал есть на выходе!");
        
        // Найдем где максимальная амплитуда
        let max_pos = output.iter()
            .position(|&x| x.abs() == max_amp)
            .unwrap_or(0);
        
        println!("   Максимальная амплитуда на сэмпле: {}", max_pos);
        
        // Покажем сэмплы вокруг максимума
        let start = max_pos.saturating_sub(2);
        let end = (max_pos + 3).min(buffer_size);
        
        println!("   Сэмплы вокруг максимума:");
        for i in start..end {
            println!("     [{:3}]: {:8.6}", i, output[i]);
        }
        
        // Также покажем начало
        println!("   Первые 10 сэмплов:");
        for i in 0..10.min(buffer_size) {
            println!("     [{:3}]: {:8.6}", i, output[i]);
        }
        
        // Вычислим среднюю амплитуду
        let avg_amp: f32 = output.iter()
            .map(|&x| x.abs())
            .sum::<f32>() / buffer_size as f32;
        
        println!("   Средняя амплитуда: {:.6}", avg_amp);
        
    } else {
        println!("   ❌ Нет сигнала на выходе!");
        
        // Диагностика: проверяем что в буфере
        println!("   Проверка всех сэмплов:");
        let mut all_zero = true;
        for (i, &sample) in output.iter().enumerate() {
            if sample != 0.0 {
                println!("     [{:3}]: {:8.6} ← НЕ НОЛЬ!", i, sample);
                all_zero = false;
                break;
            }
        }
        
        if all_zero {
            println!("   Все сэмплы точно равны 0.0");
        }
    }
    
    // 6. Тест: проверяем осцилляторы по отдельности
    println!("\n6. Тест осцилляторов отдельно:");
    
    let mut osc1_test = SineOscillator::new(440.0);
    osc1_test.init(sample_rate);
    
    let mut osc1_out = vec![0.0f32; 10];
    osc1_test.process(&[], &mut [osc1_out.as_mut_slice()])?;
    
    println!("   Осциллятор 440 Гц (первые 10):");
    for (i, &sample) in osc1_out.iter().enumerate() {
        println!("     [{:2}]: {:8.6}", i, sample);
    }
    
    let mut osc2_test = SineOscillator::new(660.0);
    osc2_test.init(sample_rate);
    
    let mut osc2_out = vec![0.0f32; 10];
    osc2_test.process(&[], &mut [osc2_out.as_mut_slice()])?;
    
    println!("   Осциллятор 660 Гц (первые 10):");
    for (i, &sample) in osc2_out.iter().enumerate() {
        println!("     [{:2}]: {:8.6}", i, sample);
    }
    
    // 7. Принудительная проверка маршрутизации:
    println!("\n7. Принудительная проверка маршрутизации:");

    // Создадим новый граф с явной диагностикой
    let mut test_graph = AudioGraph::new(sample_rate);

    // Добавим один осциллятор и один усилитель
    let test_osc = Box::new(SineOscillator::new(440.0));
    let test_gain = Box::new(GainNode::new(0.5));

    let test_osc_id = test_graph.add_node(test_osc);
    let test_gain_id = test_graph.add_node(test_gain);

    // Соединим
    test_graph.connect(
        PortId { node: test_osc_id, index: 0, is_input: false },
        PortId { node: test_gain_id, index: 0, is_input: true },
        1.0,
    )?;

    println!("   Простой граф: осциллятор -> усилитель");
    println!("   Порядок обработки: {:?}", test_graph.get_processing_order());

    let mut test_output = vec![0.0f32; 20];
    test_graph.process(&[], &mut [test_output.as_mut_slice()])?;

    println!("   Результат:");
    for (i, &sample) in test_output.iter().enumerate() {
        println!("     [{:2}]: {:8.6}", i, sample);
    }

    let test_has_signal = test_output.iter().any(|&x| x != 0.0);
    println!("   Есть сигнал: {}", test_has_signal);
    
    // 8. Проверим цепочку осциллятор -> фильтр -> усилитель
    println!("\n8. Проверка цепочки (осциллятор -> фильтр -> усилитель):");
    let mut chain_graph = AudioGraph::new(sample_rate);
    
    use kama_core::dsp::BiquadFilter;
    
    let chain_osc = Box::new(SineOscillator::new(440.0));
    let chain_filter = Box::new(BiquadFilter::lowpass(1000.0, 0.707));
    let chain_gain = Box::new(GainNode::new(0.5));
    
    let chain_osc_id = chain_graph.add_node(chain_osc);
    let chain_filter_id = chain_graph.add_node(chain_filter);
    let chain_gain_id = chain_graph.add_node(chain_gain);
    
    // Создаем цепочку
    chain_graph.connect(
        PortId { node: chain_osc_id, index: 0, is_input: false },
        PortId { node: chain_filter_id, index: 0, is_input: true },
        1.0,
    )?;
    
    chain_graph.connect(
        PortId { node: chain_filter_id, index: 0, is_input: false },
        PortId { node: chain_gain_id, index: 0, is_input: true },
        1.0,
    )?;
    
    println!("   Создана цепочка");
    println!("   Порядок обработки: {:?}", chain_graph.get_processing_order());
    
    let mut chain_output = vec![0.0f32; 20];
    chain_graph.process(&[], &mut [chain_output.as_mut_slice()])?;
    
    println!("   Результат цепочки:");
    for (i, &sample) in chain_output.iter().enumerate() {
        println!("     [{:2}]: {:8.6}", i, sample);
    }
    
    let chain_has_signal = chain_output.iter().any(|&x| x != 0.0);
    println!("   Есть сигнал в цепочке: {}", chain_has_signal);
    
    // Сравним амплитуды
    if test_has_signal && chain_has_signal {
        let test_max = test_output.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);
        let chain_max = chain_output.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);
        
        println!("\nСравнение амплитуд:");
        println!("   Простой граф (осциллятор -> усилитель): {:.6}", test_max);
        println!("   Цепочка (осциллятор -> фильтр -> усилитель): {:.6}", chain_max);
        println!("   Ожидаемое соотношение: примерно одинаково (фильтр ФНЧ не сильно ослабляет 440 Гц при срезе 1000 Гц)");
    }
    
    Ok(())
}