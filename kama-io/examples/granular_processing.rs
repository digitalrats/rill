//! Пример использования kama-io с гранулярным буфером
//! 
//! Сценарий:
//! 1. Загружаем сэмпл (или генерируем тестовый сигнал)
//! 2. Создаем многоголовый гранулярный буфер
//! 3. Обрабатываем через граф с эффектами
//! 4. Выводим через kama-io

use kama_core::{
    AudioGraph, AudioNode,
    dsp::{BiquadFilter, BiquadType, DelayLine},
    node::GainNode,
    graph::PortId,
    param::ParamValue,
};
use kama_buffers::{MultiHeadBuffer, ReadMode, BufferHead};
use kama_io::{
    AudioConfig, AudioEngine, AudioBackend,
    CpalBackend, NullBackend, GraphProcessor,
};
use std::f32::consts::PI;
use std::sync::Arc;
use parking_lot::RwLock;

/// Генерация тестового сэмпла (колокольчик)
fn generate_bell_sample(duration_seconds: f32, sample_rate: f32) -> Vec<f32> {
    let num_samples = (duration_seconds * sample_rate) as usize;
    let mut sample = Vec::with_capacity(num_samples);
    
    // Параметры колокольчика: основная частота и обертоны
    let base_freq = 440.0; // A4
    let harmonics = vec![
        (1.0, 1.0),   // фундаментальная
        (2.0, 0.5),   // октава
        (3.0, 0.3),   // квинта через октаву
        (4.2, 0.2),   // диссонансный обертон
        (5.5, 0.15),  // еще один обертон
    ];
    
    for i in 0..num_samples {
        let t = i as f32 / sample_rate;
        let envelope = (-t * 3.0).exp(); // затухание
        
        let mut val = 0.0;
        for (harmonic, amp) in &harmonics {
            val += (2.0 * PI * base_freq * harmonic * t).sin() * amp;
        }
        
        sample.push(val * envelope * 0.5);
    }
    
    sample
}

/// Генерация дроун-сэмпла (эмбиент)
fn generate_drone_sample(duration_seconds: f32, sample_rate: f32) -> Vec<f32> {
    let num_samples = (duration_seconds * sample_rate) as usize;
    let mut sample = Vec::with_capacity(num_samples);
    
    // Медленно меняющийся дрон
    let base_freq = 110.0; // A2
    let modulation_freq = 0.2;
    
    for i in 0..num_samples {
        let t = i as f32 / sample_rate;
        
        // Модуляция частоты
        let mod_amt = (2.0 * PI * modulation_freq * t).sin() * 0.05;
        let freq1 = base_freq * (1.0 + mod_amt);
        let freq2 = base_freq * 2.0 * (1.0 - mod_amt * 0.5);
        
        // Сложный waveform
        let val1 = (2.0 * PI * freq1 * t).sin();
        let val2 = (2.0 * PI * freq2 * t).sin() * 0.3;
        let val3 = (2.0 * PI * (freq1 * 3.0) * t).sin() * 0.1;
        
        sample.push((val1 + val2 + val3) * 0.3);
    }
    
    sample
}

/// Создание гранулярного буфера с несколькими головками
fn create_granular_buffer(sample: Vec<f32>, sample_rate: f32) -> MultiHeadBuffer {
    let mut buffer = MultiHeadBuffer::new(4096, sample_rate);
    
    // Записываем сэмпл в буфер (с циклическим заполнением)
    buffer.write(&sample);
    
    // Головка 1: нормальное воспроизведение
    let head1_id = buffer.add_head();
    if let Some(head) = buffer.get_head_mut(head1_id) {
        head.state.speed = 1.0;
        head.state.pan = -0.5; // левый канал
        head.state.volume = 0.7;
        head.read_mode = ReadMode::Simple;
    }
    
    // Головка 2: гранулярный режим
    let head2_id = buffer.add_head();
    if let Some(head) = buffer.get_head_mut(head2_id) {
        head.state.speed = 0.8;
        head.state.pan = 0.5; // правый канал
        head.state.volume = 0.6;
        head.read_mode = ReadMode::Granular {
            grain_size: 256,
            grain_spacing: 512,
            randomization: 0.3,
        };
    }
    
    // Головка 3: реверс + гранулы
    let head3_id = buffer.add_head();
    if let Some(head) = buffer.get_head_mut(head3_id) {
        head.state.speed = -0.5; // обратное воспроизведение
        head.state.pan = 0.0; // центр
        head.state.volume = 0.4;
        head.read_mode = ReadMode::Granular {
            grain_size: 128,
            grain_spacing: 384,
            randomization: 0.5,
        };
    }
    
    // Головка 4: пинг-понг
    let head4_id = buffer.add_head();
    if let Some(head) = buffer.get_head_mut(head4_id) {
        head.state.speed = 1.2;
        head.state.pan = -0.8; // лево
        head.state.volume = 0.3;
        head.read_mode = ReadMode::PingPong {
            segment_size: 512,
        };
    }
    
    println!("Создан гранулярный буфер с {} головками", 4);
    
    buffer
}

/// Создание графа обработки для гранулярного синтеза
fn create_processing_graph(sample_rate: f32) -> (AudioGraph, kama_core::graph::NodeId, kama_core::graph::NodeId) {
    let mut graph = AudioGraph::new(sample_rate);
    
    // 1. Входной узел (принимает сигнал от буфера)
    let input_gain = GainNode::new(1.0);
    let input_id = graph.add_node(Box::new(input_gain));
    
    // 2. Фильтр низких частот (сглаживание гранул)
    let filter = BiquadFilter::new(BiquadType::LowPass, 3000.0, 0.5);
    let filter_id = graph.add_node(Box::new(filter));
    
    // 3. Хорус/фленджер через линию задержки с модуляцией
    let delay = DelayLine::new(0.5, sample_rate);
    let delay_id = graph.add_node(Box::new(delay));
    
    // 4. Усилитель
    let gain = GainNode::new(0.8);
    let gain_id = graph.add_node(Box::new(gain));
    
    // 5. Выходной узел
    let output_gain = GainNode::new(1.0);
    let output_id = graph.add_node(Box::new(output_gain));
    
    // Соединения
    graph.connect(
        PortId { node: input_id, index: 0, is_input: false },
        PortId { node: filter_id, index: 0, is_input: true },
        1.0,
    ).unwrap();
    
    graph.connect(
        PortId { node: filter_id, index: 0, is_input: false },
        PortId { node: delay_id, index: 0, is_input: true },
        0.7,
    ).unwrap();
    
    graph.connect(
        PortId { node: filter_id, index: 0, is_input: false },
        PortId { node: gain_id, index: 0, is_input: true },
        0.3, // dry signal
    ).unwrap();
    
    graph.connect(
        PortId { node: delay_id, index: 0, is_input: false },
        PortId { node: gain_id, index: 0, is_input: true },
        0.5, // wet signal
    ).unwrap();
    
    graph.connect(
        PortId { node: gain_id, index: 0, is_input: false },
        PortId { node: output_id, index: 0, is_input: true },
        1.0,
    ).unwrap();
    
    (graph, input_id, output_id)
}

/// Структура для хранения состояния гранулярного процессора
struct GranularState {
    buffer: MultiHeadBuffer,
    head_ids: Vec<usize>,
    current_head: usize,
}

impl GranularState {
    fn new(sample: Vec<f32>, sample_rate: f32) -> Self {
        let buffer = create_granular_buffer(sample, sample_rate);
        let head_ids = (1..=4).collect(); // головки с 1 по 4
        Self {
            buffer,
            head_ids,
            current_head: 0,
        }
    }
    
    fn cycle_head(&mut self) {
        self.current_head = (self.current_head + 1) % self.head_ids.len();
        println!("Активная головка: {}", self.head_ids[self.current_head]);
    }
    
    fn set_head_param(&mut self, head_idx: usize, param_name: &str, value: ParamValue) -> Result<(), kama_core::AudioError> {
        if let Some(head_id) = self.head_ids.get(head_idx) {
            if let Some(head) = self.buffer.get_head_mut(*head_id) {
                match param_name {
                    "speed" => {
                        if let ParamValue::Float(v) = value {
                            head.state.speed = v;
                        }
                    }
                    "pan" => {
                        if let ParamValue::Float(v) = value {
                            head.state.pan = v;
                        }
                    }
                    "volume" => {
                        if let ParamValue::Float(v) = value {
                            head.state.volume = v;
                        }
                    }
                    "grain_size" => {
                        if let ParamValue::Int(v) = value {
                            if let ReadMode::Granular { grain_size, grain_spacing, randomization } = &mut head.read_mode {
                                *grain_size = v as usize;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama IO + Granular Buffer Demo ===\n");
    
    let sample_rate = 44100.0;
    let config = AudioConfig::default()
        .with_sample_rate(sample_rate as u32)
        .with_buffer_size(256)
        .with_channels(2);
    
    println!("Audio config: {} Hz, {} samples buffer",
             config.sample_rate, config.buffer_size);
    
    // Генерируем тестовые сэмплы
    println!("\nГенерация тестовых сэмплов...");
    let bell_sample = generate_bell_sample(2.0, sample_rate);
    let drone_sample = generate_drone_sample(4.0, sample_rate);
    
    println!("  Bell sample: {} samples", bell_sample.len());
    println!("  Drone sample: {} samples", drone_sample.len());
    
    // Создаем гранулярное состояние
    let granular_state = Arc::new(RwLock::new(GranularState::new(bell_sample, sample_rate)));
    
    // Создаем граф обработки
    let (graph, input_id, output_id) = create_processing_graph(sample_rate);
    
    // Создаем процессор, который будет использовать гранулярный буфер
    struct GranularProcessor {
        graph: AudioGraph,
        input_id: kama_core::graph::NodeId,
        output_id: kama_core::graph::NodeId,
        state: Arc<RwLock<GranularState>>,
        temp_input: Vec<f32>,
        temp_output: Vec<f32>,
    }
    
    impl GranularProcessor {
        fn new(
            graph: AudioGraph,
            input_id: kama_core::graph::NodeId,
            output_id: kama_core::graph::NodeId,
            state: Arc<RwLock<GranularState>>,
        ) -> Self {
            Self {
                graph,
                input_id,
                output_id,
                state,
                temp_input: Vec::new(),
                temp_output: Vec::new(),
            }
        }
    }
    
    impl kama_io::AudioProcessor for GranularProcessor {
        fn process(&mut self, input: &[f32], output: &mut [f32]) {
            let num_samples = input.len();
            
            // Подготавливаем временные буферы
            if self.temp_input.len() != num_samples {
                self.temp_input.resize(num_samples, 0.0);
                self.temp_output.resize(num_samples, 0.0);
            }
            
            // Получаем доступ к гранулярному буферу
            let mut state = self.state.write();
            
            // Обрабатываем через гранулярный буфер
            // Создаем входные срезы (пустые, так как буфер сам генерирует звук)
            let buffer_inputs: &[&[f32]] = &[];
            let mut buffer_outputs = [&mut self.temp_input[..]];
            
            // Генерируем звук из буфера
            let _ = state.buffer.process(buffer_inputs, &mut buffer_outputs);
            
            // Подаем сгенерированный звук на вход графа
            if let Some(node) = self.graph.get_node_mut(self.input_id) {
                let input_slices = [self.temp_input.as_slice()];
                let mut output_slices = [self.temp_output.as_mut_slice()];
                let _ = node.process(&input_slices, &mut output_slices);
            }
            
            // Обрабатываем через весь граф
            let graph_input = [self.temp_output.as_slice()];
            let mut graph_output = [output];
            let _ = self.graph.process(&graph_input, &mut graph_output);
        }
        
        fn reset(&mut self) {
            self.graph.reset();
            let mut state = self.state.write();
            state.buffer.reset();
        }
        
        fn set_sample_rate(&mut self, sample_rate: f32) {
            self.graph.init_all(sample_rate);
        }
    }
    
    let processor = GranularProcessor::new(graph, input_id, output_id, granular_state.clone());
    
    // Создаем бэкенд
    #[cfg(feature = "cpal")]
    let backend = CpalBackend::new(config.clone())?;
    
    #[cfg(not(feature = "cpal"))]
    let backend = NullBackend::new(config.clone());
    
    println!("\nUsing backend: {}", backend.name());
    
    // Создаем движок
    let mut engine = AudioEngine::new(backend, processor);
    
    println!("\nЗапуск гранулярного синтеза...");
    engine.start()?;
    
    println!("\n=== Демонстрация гранулярного синтеза ===");
    println!("Активная головка: 1 (нормальное воспроизведение)");
    
    // Демонстрация изменения параметров
    for i in 0..8 {
        println!("\n--- Шаг {} ---", i + 1);
        
        match i {
            0 => {
                println!("Смена сэмпла на дрон");
                let mut state = granular_state.write();
                let drone = generate_drone_sample(4.0, sample_rate);
                state.buffer.write(&drone);
            }
            1 => {
                println!("Скорость головки 1: 0.5x");
                let mut state = granular_state.write();
                let _ = state.set_head_param(0, "speed", ParamValue::Float(0.5));
            }
            2 => {
                println!("Активация гранулярной головки 2");
                let mut state = granular_state.write();
                state.cycle_head();
            }
            3 => {
                println!("Размер гранул: 64, рандомизация: 0.8");
                let mut state = granular_state.write();
                let _ = state.set_head_param(1, "grain_size", ParamValue::Int(64));
            }
            4 => {
                println!("Активация реверс-гранулярной головки 3");
                let mut state = granular_state.write();
                state.cycle_head();
            }
            5 => {
                println!("Панорамирование головки 3: правый канал");
                let mut state = granular_state.write();
                let _ = state.set_head_param(2, "pan", ParamValue::Float(0.8));
            }
            6 => {
                println!("Активация пинг-понг головки 4");
                let mut state = granular_state.write();
                state.cycle_head();
            }
            7 => {
                println!("Возврат к исходным настройкам");
                let mut state = granular_state.write();
                let bell = generate_bell_sample(2.0, sample_rate);
                state.buffer.write(&bell);
                state.current_head = 0;
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