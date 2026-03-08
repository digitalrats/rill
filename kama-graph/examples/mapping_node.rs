//! Пример создания узла маппинга для микро-контроля
//!
//! Этот пример показывает, как создать собственный узел,
//! который преобразует аудио в управляющие сигналы и
//! отправляет их в мир автоматов через очередь.
//!
//! Запуск: cargo run --example mapping_node

use kama_core::prelude::*;
use kama_core::macros::processor_node;
use kama_core::queue::{CommandEnum, SetParameter, SignalSource, Telemetry};
use kama_graph::prelude::*;
use std::collections::VecDeque;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// =====================================================================
// ШАГ 1: Создаем свой узел с помощью макроса
// =====================================================================

// Состояние для канала
#[derive(Clone, Default)]
struct ChannelState {
    rms_buffer: VecDeque<f32>,
    envelope_state: f32,
    last_value: f32,
}

// Создаем процессор с помощью макроса из kama-core
processor_node! {
    /// Узел маппинга — преобразует аудио в управляющие сигналы
    ///
    /// Этот узел создан в примере, но может быть использован
    /// в любом проекте, так как использует только публичные API.
    pub struct MyMappingNode {
        params: {
            /// Коэффициент сглаживания
            smoothing: f32 = 0.1,
            
            /// Порог для гейта
            threshold: f32 = 0.5,
        },
        control_inputs: {},
        state: {
            /// Состояние для каждого канала
            channel_states: Vec<ChannelState> = Vec::new(),
            
            /// Отправитель команд (в мир автоматов)
            command_tx: Option<crossbeam_channel::Sender<CommandEnum>> = None,
            
            /// Отправитель телеметрии (для отладки)
            telemetry_tx: Option<crossbeam_channel::Sender<Telemetry>> = None,
        },
        inputs: 2,
        outputs: 0,
        process: |this, channel, input, _output, _control| {
            // Инициализируем состояние если нужно
            if this.channel_states.len() <= channel {
                this.channel_states.resize(channel + 1, ChannelState::default());
            }
            
            let state = &mut this.channel_states[channel];
            
            // Простой RMS детектор
            for &sample in input {
                state.rms_buffer.push_back(sample * sample);
                if state.rms_buffer.len() > 256 {
                    state.rms_buffer.pop_front();
                }
            }
            
            let sum: f32 = state.rms_buffer.iter().sum();
            let rms = (sum / state.rms_buffer.len() as f32).sqrt();
            
            // Сглаживание
            let smoothed = if this.smoothing > 0.0 {
                state.last_value = state.last_value * (1.0 - this.smoothing) + rms * this.smoothing;
                state.last_value
            } else {
                rms
            };
            
            // Масштабируем в 0..1
            let output_val = smoothed.clamp(0.0, 1.0);
            
            // Отправляем команду в мир автоматов
            if let Some(tx) = &this.command_tx {
                let cmd = SetParameter::new(
                    PortId::control_in(NodeId(0), channel as u16),  // куда отправляем
                    ParameterId::new("amplitude").unwrap(),       // какой параметр
                    output_val,
                    SignalSource::Automaton(format!("mapping/ch_{}", channel)),
                );
                
                let _ = tx.send(CommandEnum::SetParameter(cmd));
            }
            
            // Отправляем телеметрию
            if let Some(tx) = &this.telemetry_tx {
                let _ = tx.send(Telemetry::event(
                    format!("mapping_ch_{}", channel),
                    "rms",
                    vec![output_val],
                ));
            }
        }
    }
}

// Дополнительные методы для удобства
impl MyMappingNode {
    pub fn with_command_queue(mut self, tx: crossbeam_channel::Sender<CommandEnum>) -> Self {
        self.command_tx = Some(tx);
        self
    }
    
    pub fn with_telemetry(mut self, tx: crossbeam_channel::Sender<Telemetry>) -> Self {
        self.telemetry_tx = Some(tx);
        self
    }
}

// =====================================================================
// ШАГ 2: Простой автомат (для примера)
// =====================================================================

struct DummyAutomaton {
    name: String,
    value: f32,
}

impl DummyAutomaton {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            value: 0.0,
        }
    }
    
    fn update(&mut self, command: &SetParameter) {
        println!("[{}] Получил команду: {} = {}", 
            self.name, command.parameter, command.value);
        self.value = command.value;
    }
}

// =====================================================================
// ШАГ 3: Приложение связывает всё вместе
// =====================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Пример создания узла маппинга ===\n");
    
    // 1. Создаем очереди (законы природы)
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (tel_tx, tel_rx) = crossbeam_channel::unbounded();
    
    // 2. Создаем наш узел и подключаем к очередям
    let mapping_node = MyMappingNode::new(0.1, 0.5)  // smoothing, threshold
        .with_command_queue(cmd_tx)
        .with_telemetry(tel_tx);
    
    // 3. Создаем граф и добавляем узел
    let mut graph = AudioGraph::new(44100.0);
    let node_id = graph.add_node(Box::new(mapping_node));
    
    println!("Узел добавлен в граф: {:?}", node_id);
    
    // 4. Создаем простой автомат (в другом потоке)
    let automaton = Arc::new(std::sync::Mutex::new(DummyAutomaton::new("LFO")));
    let automaton_clone = automaton.clone();
    
    let handle = thread::spawn(move || {
        println!("Автомат запущен и слушает команды...");
        
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                CommandEnum::SetParameter(sp) => {
                    automaton_clone.lock().unwrap().update(&sp);
                }
                _ => {}
            }
        }
    });
    
    // 5. Поток телеметрии
    let tel_handle = thread::spawn(move || {
        while let Ok(tel) = tel_rx.recv() {
            match tel {
                Telemetry::Event { source, data, .. } => {
                    println!("[Телеметрия] {}: {:?}", source, data);
                }
                _ => {}
            }
        }
    });
    
    // 6. Имитируем обработку аудио
    println!("\nИмитация обработки аудио...");
    
    let test_signal = vec![0.5; 512];
    let inputs = [test_signal.as_slice(), test_signal.as_slice()];
    let mut outputs: [&mut [f32]; 0] = [];
    
    for i in 0..10 {
        thread::sleep(Duration::from_millis(100));
        
        // Меняем входной сигнал для демонстрации
        let signal = if i % 2 == 0 {
            vec![0.8; 512]
        } else {
            vec![0.2; 512]
        };
        
        let inputs = [signal.as_slice(), signal.as_slice()];
        graph.process(&inputs, &mut outputs)?;
        
        println!("Обработан блок {}", i + 1);
    }
    
    println!("\n✅ Пример завершен");
    
    Ok(())
}