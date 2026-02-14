//! Пример полной интеграции kama-io с AudioGraph через GraphProcessor

use kama_core::{
    AudioGraph, AudioNode,
    dsp::{BiquadFilter, BiquadType, DelayLine},
    node::GainNode,
    graph::PortId,
    param::ParamValue,
};
use kama_io::{
    AudioConfig, AudioEngine, AudioBackend,
    CpalBackend, NullBackend, GraphProcessor,
};

fn create_audio_graph(sample_rate: f32) -> (AudioGraph, kama_core::graph::NodeId, kama_core::graph::NodeId) {
    let mut graph = AudioGraph::new(sample_rate);
    
    // Создаем узлы обработки
    let filter = BiquadFilter::new(BiquadType::LowPass, 1000.0, 0.707);
    let filter_id = graph.add_node(Box::new(filter));
    
    let delay = DelayLine::new(1.0, sample_rate);
    let delay_id = graph.add_node(Box::new(delay));
    
    let gain = GainNode::new(0.8);
    let gain_id = graph.add_node(Box::new(gain));
    
    let input_gain = GainNode::new(1.0);
    let input_id = graph.add_node(Box::new(input_gain));
    
    let output_gain = GainNode::new(1.0);
    let output_id = graph.add_node(Box::new(output_gain));
    
    let feedback_gain = GainNode::new(0.3);
    let feedback_id = graph.add_node(Box::new(feedback_gain));
    
    // Строим цепочку
    graph.connect(
        PortId { node: input_id, index: 0, is_input: false },
        PortId { node: filter_id, index: 0, is_input: true },
        1.0,
    ).unwrap();
    
    graph.connect(
        PortId { node: filter_id, index: 0, is_input: false },
        PortId { node: delay_id, index: 0, is_input: true },
        1.0,
    ).unwrap();
    
    graph.connect(
        PortId { node: delay_id, index: 0, is_input: false },
        PortId { node: gain_id, index: 0, is_input: true },
        1.0,
    ).unwrap();
    
    graph.connect(
        PortId { node: gain_id, index: 0, is_input: false },
        PortId { node: output_id, index: 0, is_input: true },
        1.0,
    ).unwrap();
    
    // Feedback
    graph.connect(
        PortId { node: delay_id, index: 0, is_input: false },
        PortId { node: feedback_id, index: 0, is_input: true },
        1.0,
    ).unwrap();
    
    graph.connect(
        PortId { node: feedback_id, index: 0, is_input: false },
        PortId { node: filter_id, index: 0, is_input: true },
        0.3,
    ).unwrap();
    
    (graph, input_id, output_id)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama IO + AudioGraph Integration via GraphProcessor ===\n");
    
    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2);
    
    println!("Audio config: {} Hz, {} samples buffer",
             config.sample_rate, config.buffer_size);
    
    // Создаем граф
    let (graph, input_id, output_id) = create_audio_graph(config.sample_rate as f32);
    
    println!("\nГраф обработки:");
    println!("  Микрофон -> Фильтр (LowPass 1000Hz) -> Задержка (0.3s) -> Усилитель (0.8) -> Динамики");
    println!("  Задержка -> Фильтр (feedback 0.3)");
    
    // Создаем процессор на основе графа
    let processor = GraphProcessor::new(graph, Some(input_id), Some(output_id));
    
    // Создаем бэкенд
    #[cfg(feature = "cpal")]
    let backend = CpalBackend::new(config.clone())?;
    
    #[cfg(not(feature = "cpal"))]
    let backend = NullBackend::new(config.clone());
    
    println!("Using backend: {}", backend.name());
    
    // Создаем движок
    let mut engine = AudioEngine::new(backend, processor);
    
    println!("\nЗапуск аудио обработки...");
    engine.start()?;
    
    println!("\n=== Демонстрация изменения параметров в реальном времени ===");
    
    // Демонстрация изменения параметров
    for i in 0..6 {
        println!("\n--- Шаг {} ---", i + 1);
        
        // Используем update_processor для безопасного изменения
        match i {
            0 => {
                println!("Изменение частоты среза фильтра на 500 Hz");
                engine.update_processor(|proc: &mut GraphProcessor| {
                    let _ = proc.set_node_param::<BiquadFilter>("cutoff", ParamValue::Float(500.0));
                })?;
            }
            1 => {
                println!("Изменение времени задержки на 0.5 с");
                engine.update_processor(|proc: &mut GraphProcessor| {
                    let _ = proc.set_node_param::<DelayLine>("delay", ParamValue::Float(0.5));
                })?;
            }
            2 => {
                println!("Изменение обратной связи на 0.5");
                engine.update_processor(|proc: &mut GraphProcessor| {
                    proc.with_graph(|graph: &mut AudioGraph| {
                        // ИСПРАВЛЕНО: собираем ID узлов в вектор
                        let node_ids: Vec<kama_core::graph::NodeId> = graph.get_processing_order().to_vec();
                        for node_id in node_ids {
                            if let Some(node) = graph.get_node(node_id) {
                                if let Some(ParamValue::Float(val)) = node.get_param("gain") {
                                    if (val - 0.3).abs() < 0.1 {
                                        if let Some(node_mut) = graph.get_node_mut(node_id) {
                                            let _ = node_mut.set_param("gain", ParamValue::Float(0.5));
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    });
                })?;
            }
            3 => {
                println!("Изменение выходного усиления на 1.2");
                engine.update_processor(|proc: &mut GraphProcessor| {
                    proc.with_graph(|graph: &mut AudioGraph| {
                        // ИСПРАВЛЕНО: собираем ID узлов в вектор
                        let node_ids: Vec<kama_core::graph::NodeId> = graph.get_processing_order().to_vec();
                        for node_id in node_ids {
                            if let Some(node) = graph.get_node(node_id) {
                                if let Some(ParamValue::Float(val)) = node.get_param("gain") {
                                    if (val - 0.8).abs() < 0.1 {
                                        if let Some(node_mut) = graph.get_node_mut(node_id) {
                                            let _ = node_mut.set_param("gain", ParamValue::Float(1.2));
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    });
                })?;
            }
            4 => {
                println!("Возврат к исходным параметрам");
                engine.update_processor(|proc: &mut GraphProcessor| {
                    proc.with_graph(|graph: &mut AudioGraph| {
                        graph.reset();
                        // Восстанавливаем исходные значения
                        if let Some(filter_id) = proc.find_node_by_type::<BiquadFilter>() {
                            if let Some(filter) = graph.get_node_mut(filter_id) {
                                let _ = filter.set_param("cutoff", ParamValue::Float(1000.0));
                            }
                        }
                        if let Some(delay_id) = proc.find_node_by_type::<DelayLine>() {
                            if let Some(delay) = graph.get_node_mut(delay_id) {
                                let _ = delay.set_param("delay", ParamValue::Float(0.3));
                            }
                        }
                        // Сбрасываем gain узлы
                        // ИСПРАВЛЕНО: собираем ID узлов в вектор
                        let node_ids: Vec<kama_core::graph::NodeId> = graph.get_processing_order().to_vec();
                        for node_id in node_ids {
                            if let Some(node) = graph.get_node(node_id) {
                                if let Some(ParamValue::Float(val)) = node.get_param("gain") {
                                    if (val - 0.3).abs() < 0.1 {
                                        if let Some(node_mut) = graph.get_node_mut(node_id) {
                                            let _ = node_mut.set_param("gain", ParamValue::Float(0.3));
                                        }
                                    } else if (val - 0.8).abs() < 0.1 {
                                        if let Some(node_mut) = graph.get_node_mut(node_id) {
                                            let _ = node_mut.set_param("gain", ParamValue::Float(0.8));
                                        }
                                    } else if (val - 1.0).abs() < 0.1 {
                                        if let Some(node_mut) = graph.get_node_mut(node_id) {
                                            let _ = node_mut.set_param("gain", ParamValue::Float(1.0));
                                        }
                                    }
                                }
                            }
                        }
                    });
                })?;
            }
            5 => {
                println!("Экстремальные значения");
                engine.update_processor(|proc: &mut GraphProcessor| {
                    let _ = proc.set_node_param::<BiquadFilter>("cutoff", ParamValue::Float(200.0));
                    let _ = proc.set_node_param::<DelayLine>("delay", ParamValue::Float(0.8));
                    
                    proc.with_graph(|graph: &mut AudioGraph| {
                        // ИСПРАВЛЕНО: собираем ID узлов в вектор
                        let node_ids: Vec<kama_core::graph::NodeId> = graph.get_processing_order().to_vec();
                        for node_id in node_ids {
                            if let Some(node) = graph.get_node(node_id) {
                                if let Some(ParamValue::Float(val)) = node.get_param("gain") {
                                    if (val - 0.3).abs() < 0.1 {
                                        if let Some(node_mut) = graph.get_node_mut(node_id) {
                                            let _ = node_mut.set_param("gain", ParamValue::Float(0.8));
                                        }
                                    } else if (val - 0.8).abs() < 0.1 {
                                        if let Some(node_mut) = graph.get_node_mut(node_id) {
                                            let _ = node_mut.set_param("gain", ParamValue::Float(1.5));
                                        }
                                    }
                                }
                            }
                        }
                    });
                })?;
            }
            _ => {}
        }
        
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    
    println!("\nОстановка обработки...");
    engine.stop()?;
    
    println!("\nСтатистика:");
    println!("  Xruns: {}", engine.xruns());
    println!("  Задержка: {:?}", engine.latency());
    
    Ok(())
}