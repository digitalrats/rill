// FILE: kama-automation/examples/automation_demo.rs
use kama_core::{
    AudioGraph,
    dsp::SineOscillator,
    node::GainNode,
    graph::PortId,
    automation::{AutomatedParameter, LfoAutomaton},
    param::ParamValue,
};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("=== Kama Automation Demo ===\n");
    
    // Тест 1: Автоматизированный параметр
    println!("1. Тест автоматизированного параметра:");
    
    let mut param = AutomatedParameter::new(0.5);
    // Если метода set_range нет, установим min/max напрямую
    // param.set_range(0.0, 1.0); // Убрали если метод не существует
    
    // Вместо этого можем создать параметр с ограничениями через поля
    // или просто использовать как есть
    
    // Добавляем LFO автомат
    let lfo = LfoAutomaton::new(1.0, 0.2, 0.5); // 1 Hz, amplitude 0.2, offset 0.5
    param.set_automaton(Box::new(lfo));
    param.enable_automation();
    
    println!("   Начальное значение: {:.3}", param.update());
    println!("   Обновленное значение: {:.3}", param.update());
    println!("   Еще раз: {:.3}", param.update());
    
    // Тест 2: Граф с автоматизацией
    println!("\n2. Граф с автоматизацией:");
    
    let mut graph = AudioGraph::new(44100.0);
    
    let osc = Box::new(SineOscillator::new(440.0));
    let gain = Box::new(GainNode::new(0.5));
    
    let osc_id = graph.add_node(osc);
    let gain_id = graph.add_node(gain);
    
    // Соединяем
    graph.connect(
        PortId { node: osc_id, index: 0, is_input: false },
        PortId { node: gain_id, index: 0, is_input: true },
        1.0,
    )?;
    
    // Получаем доступ к узлу и изменяем параметр
    if let Some(gain_node) = graph.get_node_mut(gain_id) {
        // Устанавливаем параметр gain
        gain_node.set_param("gain", ParamValue::Float(0.3))?;
        
        // Читаем параметр обратно
        if let Some(ParamValue::Float(current_gain)) = gain_node.get_param("gain") {
            println!("   Усилитель gain установлен на: {:.2}", current_gain);
        }
    }
    
    // Обрабатываем
    let mut output = vec![0.0f32; 10];
    graph.process(&[], &mut [output.as_mut_slice()])?;
    
    println!("   Первые 3 сэмпла:");
    for i in 0..3 {
        println!("     [{:2}]: {:8.6}", i, output[i]);
    }
    
    // Тест 3: Динамическое изменение параметра
    println!("\n3. Динамическое изменение параметра:");
    
    let mut test_graph = AudioGraph::new(44100.0);
    let test_gain_id = test_graph.add_node(Box::new(GainNode::new(0.1)));
    
    // Симулируем входной сигнал (в реальности был бы осциллятор)
    println!("   Тестируем разные значения gain:");
    
    let test_values = vec![0.1f32, 0.3, 0.5, 0.8, 1.0];
    
    for value in &test_values {
        if let Some(node) = test_graph.get_node_mut(test_gain_id) {
            node.set_param("gain", ParamValue::Float(*value))?;
            
            // Тестовый входной сигнал
            let test_input = vec![0.5f32; 5];
            let mut test_output = vec![0.0f32; 5];
            
            node.process(&[&test_input], &mut [&mut test_output])?;
            
            println!("     gain={:.2}: output[0]={:.6}", value, test_output[0]);
        }
    }
    
    // Тест 4: Маппинг значений
    println!("\n4. Маппинг значений параметра:");
    
    // Линейный маппинг
    let linear_values = vec![0.0f32, 0.25, 0.5, 0.75, 1.0];
    println!("   Линейный маппинг (y = x):");
    for value in linear_values {
        println!("     {:.2} -> {:.2}", value, value);
    }
    
    // Экспоненциальный маппинг
    println!("\n   Экспоненциальный маппинг (y = e^x - 1):");
    for value in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        // Явно указываем тип и используем метод exp из f32
        let exp_value: f32 = (value * 4.0).exp() - 1.0; // Масштабируем для лучшей видимости
        println!("     {:.2} -> {:.4}", value, exp_value);
    }
    
    // Логарифмический маппинг
    println!("\n   Логарифмический маппинг (y = log(x + 1)):");
    for value in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        // Явно указываем тип
        let log_value: f32 = (value + 1.0).ln();
        println!("     {:.2} -> {:.4}", value, log_value);
    }
    
    Ok(())
}