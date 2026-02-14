//! Пример полной интеграции kama-io с AudioGraph
//! 
//! Сценарий:
//! 1. Входной сигнал с микрофона (2 канала)
//! 2. Обработка через граф узлов:
//!    - Фильтр нижних частот
//!    - Задержка с обратной связью
//!    - Усилитель
//! 3. Вывод на динамики (2 канала)

use kama_core::{
    AudioGraph, AudioNode,
    dsp::{BiquadFilter, BiquadType, DelayLine},
    node::GainNode,
    graph::PortId,
    param::ParamValue,
};
use kama_io::{
    AudioConfig, AudioEngine, AudioProcessor,
    AudioBackend, CpalBackend, NullBackend,
};
use std::sync::Arc;
use parking_lot::RwLock;
use std::f32::consts::PI;

// Процессор, который связывает AudioGraph с kama-io
struct GraphProcessor {
    graph: AudioGraph,
    input_node_id: Option<kama_core::graph::NodeId>,
    output_node_id: Option<kama_core::graph::NodeId>,
    filter_id: Option<kama_core::graph::NodeId>,
    delay_id: Option<kama_core::graph::NodeId>,
    feedback_id: Option<kama_core::graph::NodeId>,
    gain_id: Option<kama_core::graph::NodeId>,
    temp_input: Vec<f32>,
    temp_output: Vec<f32>,
    sample_rate: f32,
}

impl GraphProcessor {
    fn new(sample_rate: f32) -> Self {
        let mut graph = AudioGraph::new(sample_rate);
        
        // Создаем узлы обработки
        // 1. Фильтр нижних частот (срез 1000 Гц)
        let filter = BiquadFilter::new(BiquadType::LowPass, 1000.0, 0.707);
        let filter_id = graph.add_node(Box::new(filter));
        
        // 2. Линия задержки (0.3 секунды)
        let delay = DelayLine::new(1.0, sample_rate);
        let delay_id = graph.add_node(Box::new(delay));
        
        // 3. Усилитель (0.8)
        let gain = GainNode::new(0.8);
        let gain_id = graph.add_node(Box::new(gain));
        
        // 4. Входной узел
        let input_gain = GainNode::new(1.0);
        let input_id = graph.add_node(Box::new(input_gain));
        
        // 5. Выходной узел
        let output_gain = GainNode::new(1.0);
        let output_id = graph.add_node(Box::new(output_gain));
        
        // 6. Feedback узел
        let feedback_gain = GainNode::new(0.3);
        let feedback_id = graph.add_node(Box::new(feedback_gain));
        
        // Строим цепочку: Input -> Filter -> Delay -> Gain -> Output
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
        
        // Feedback: Delay -> Feedback Gain -> Filter
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
        
        println!("Создан граф с {} узлами", graph.get_processing_order().len());
        println!("Порядок обработки: {:?}", graph.get_processing_order());
        
        Self {
            graph,
            input_node_id: Some(input_id),
            output_node_id: Some(output_id),
            filter_id: Some(filter_id),
            delay_id: Some(delay_id),
            feedback_id: Some(feedback_id),
            gain_id: Some(gain_id),
            temp_input: Vec::new(),
            temp_output: Vec::new(),
            sample_rate,
        }
    }
    
    // Изменение параметров в реальном времени
    fn set_filter_cutoff(&mut self, cutoff: f32) {
        if let Some(id) = self.filter_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                let _ = node.set_param("cutoff", ParamValue::Float(cutoff));
                println!("  Filter cutoff = {} Hz", cutoff);
            }
        }
    }
    
    fn set_delay_time(&mut self, time_seconds: f32) {
        if let Some(id) = self.delay_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                let _ = node.set_param("delay", ParamValue::Float(time_seconds));
                println!("  Delay time = {} s", time_seconds);
            }
        }
    }
    
    fn set_feedback(&mut self, amount: f32) {
        if let Some(id) = self.feedback_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                let _ = node.set_param("gain", ParamValue::Float(amount));
                println!("  Feedback gain = {}", amount);
            }
        }
    }
    
    fn set_output_gain(&mut self, gain: f32) {
        if let Some(id) = self.gain_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                let _ = node.set_param("gain", ParamValue::Float(gain));
                println!("  Output gain = {}", gain);
            }
        }
    }
}

impl AudioProcessor for GraphProcessor {
    fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let num_samples = input.len();
        
        // Подготавливаем временные буферы
        if self.temp_input.len() != num_samples {
            self.temp_input.resize(num_samples, 0.0);
            self.temp_output.resize(num_samples, 0.0);
        }
        
        // Копируем входной сигнал
        self.temp_input.copy_from_slice(input);
        
        // Если есть входной узел, передаем ему сигнал
        if let Some(input_id) = self.input_node_id {
            if let Some(node) = self.graph.get_node_mut(input_id) {
                // Создаем входные срезы для узла
                let input_slices = [self.temp_input.as_slice()];
                let mut output_slices = [self.temp_output.as_mut_slice()];
                
                // Обрабатываем через входной узел
                let _ = node.process(&input_slices, &mut output_slices);
            }
        }
        
        // Обрабатываем весь граф
        // Вход для графа - выход входного узла
        let graph_input = [self.temp_output.as_slice()];
        let mut graph_output = [output];
        
        let _ = self.graph.process(&graph_input, &mut graph_output);
    }
    
    fn reset(&mut self) {
        // У AudioGraph нет метода reset, поэтому сбрасываем отдельные узлы
        if let Some(id) = self.filter_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                node.reset();
            }
        }
        if let Some(id) = self.delay_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                node.reset();
            }
        }
        if let Some(id) = self.gain_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                node.reset();
            }
        }
        if let Some(id) = self.feedback_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                node.reset();
            }
        }
        self.temp_input.clear();
        self.temp_output.clear();
    }
    
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        // Переинициализируем узлы с новой частотой
        if let Some(id) = self.filter_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                node.init(sample_rate);
            }
        }
        if let Some(id) = self.delay_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                node.init(sample_rate);
            }
        }
        if let Some(id) = self.gain_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                node.init(sample_rate);
            }
        }
        if let Some(id) = self.feedback_id {
            if let Some(node) = self.graph.get_node_mut(id) {
                node.init(sample_rate);
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama IO + AudioGraph Integration Demo ===\n");
    
    // Конфигурация аудио
    let config = AudioConfig::default()
        .with_sample_rate(44100)
        .with_buffer_size(256)
        .with_channels(2);
    
    println!("Audio config: {} Hz, {} samples buffer",
             config.sample_rate, config.buffer_size);
    
    // Создаем бэкенд (CPAL или Null)
    #[cfg(feature = "cpal")]
    let backend = CpalBackend::new(config.clone())?;
    
    #[cfg(not(feature = "cpal"))]
    let backend = NullBackend::new(config.clone());
    
    println!("Using backend: {}", backend.name());
    
    // Создаем процессор с графом
    let mut processor = GraphProcessor::new(config.sample_rate as f32);
    
    println!("\nГраф обработки:");
    println!("  Микрофон -> Фильтр (LowPass 1000Hz) -> Задержка (0.3s) -> Усилитель (0.8) -> Динамики");
    println!("  Задержка -> Фильтр (feedback 0.3)");
    
    // Создаем движок
    let mut engine = AudioEngine::new(backend, processor);
    
    println!("\nЗапуск аудио обработки...");
    engine.start()?;
    
    println!("\n=== Демонстрация изменения параметров в реальном времени ===");
    
    // Демонстрация изменения параметров
    for i in 0..6 {
        println!("\n--- Шаг {} ---", i + 1);
        
        // Получаем доступ к процессору (движок должен быть остановлен для прямого доступа)
        // В реальном приложении нужно использовать каналы для изменения параметров
        // Для демо мы просто останавливаем и запускаем движок
        
        engine.stop()?;
        
        match i {
            0 => {
                println!("Изменение частоты среза фильтра:");
                if let Some(proc) = engine.processor_mut() {
                    proc.set_filter_cutoff(500.0);
                }
            }
            1 => {
                println!("Изменение времени задержки:");
                if let Some(proc) = engine.processor_mut() {
                    proc.set_delay_time(0.5);
                }
            }
            2 => {
                println!("Изменение обратной связи:");
                if let Some(proc) = engine.processor_mut() {
                    proc.set_feedback(0.5);
                }
            }
            3 => {
                println!("Изменение выходного усиления:");
                if let Some(proc) = engine.processor_mut() {
                    proc.set_output_gain(1.2);
                }
            }
            4 => {
                println!("Возврат к исходным параметрам:");
                if let Some(proc) = engine.processor_mut() {
                    proc.set_filter_cutoff(1000.0);
                    proc.set_delay_time(0.3);
                    proc.set_feedback(0.3);
                    proc.set_output_gain(0.8);
                }
            }
            5 => {
                println!("Экстремальные значения:");
                if let Some(proc) = engine.processor_mut() {
                    proc.set_filter_cutoff(200.0);
                    proc.set_delay_time(0.8);
                    proc.set_feedback(0.8);
                    proc.set_output_gain(1.5);
                }
            }
            _ => {}
        }
        
        engine.start()?;
        
        // Ждем 2 секунды между изменениями
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    
    println!("\nОстановка обработки...");
    engine.stop()?;
    
    println!("\nСтатистика:");
    println!("  Xruns: {}", engine.xruns());
    println!("  Задержка: {:?}", engine.latency());
    
    Ok(())
}