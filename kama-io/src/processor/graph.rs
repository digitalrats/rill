//! Интеграция с AudioGraph

use std::sync::Arc;
use parking_lot::RwLock;

use kama_core::{AudioGraph, AudioNode, param::ParamValue};

use crate::engine::AudioProcessor;
use crate::error::{IoResult, IoError};

/// Процессор, который обрабатывает аудио через AudioGraph
pub struct GraphProcessor {
    graph: Arc<RwLock<AudioGraph>>,
    input_node_id: Option<kama_core::graph::NodeId>,
    output_node_id: Option<kama_core::graph::NodeId>,
    temp_input: Vec<f32>,
    temp_output: Vec<f32>,
    sample_rate: f32,
}

impl GraphProcessor {
    /// Создать новый процессор на основе графа
    /// 
    /// # Arguments
    /// * `graph` - аудиограф для обработки
    /// * `input_node_id` - ID узла, который будет получать входной сигнал
    /// * `output_node_id` - ID узла, выход которого будет использоваться как выход процессора
    pub fn new(
        graph: AudioGraph,
        input_node_id: Option<kama_core::graph::NodeId>,
        output_node_id: Option<kama_core::graph::NodeId>,
    ) -> Self {
        let sample_rate = graph.sample_rate();
        
        Self {
            graph: Arc::new(RwLock::new(graph)),
            input_node_id,
            output_node_id,
            temp_input: Vec::new(),
            temp_output: Vec::new(),
            sample_rate,
        }
    }
    
    /// Получить доступ к графу для изменений
    pub fn with_graph<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut AudioGraph) -> R,
    {
        let mut graph = self.graph.write();
        f(&mut graph)
    }
    
    /// Получить доступ к графу для чтения
    pub fn with_graph_read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&AudioGraph) -> R,
    {
        let graph = self.graph.read();
        f(&graph)
    }
    
    /// Изменить входной узел
    pub fn set_input_node(&mut self, node_id: Option<kama_core::graph::NodeId>) {
        self.input_node_id = node_id;
    }
    
    /// Изменить выходной узел
    pub fn set_output_node(&mut self, node_id: Option<kama_core::graph::NodeId>) {
        self.output_node_id = node_id;
    }
    
    /// Получить частоту дискретизации
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
    
    /// Найти узел по типу
    pub fn find_node_by_type<T: AudioNode + 'static>(&self) -> Option<kama_core::graph::NodeId> {
        self.with_graph_read(|graph| {
            for &node_id in graph.get_processing_order() {
                if let Some(node) = graph.get_node(node_id) {
                    if node.type_id() == std::any::TypeId::of::<T>() {
                        return Some(node_id);
                    }
                }
            }
            None
        })
    }
    
    /// Изменить параметр узла
    pub fn set_node_param<T: AudioNode + 'static>(
        &self,
        param_name: &str,
        value: ParamValue,
    ) -> Result<(), kama_core::AudioError> {
        self.with_graph(|graph| {
            if let Some(node_id) = Self::find_node_by_type_static(graph, std::any::TypeId::of::<T>()) {
                if let Some(node) = graph.get_node_mut(node_id) {
                    return node.set_param(param_name, value);
                }
            }
            Ok(())
        })
    }
    
    // Статическая версия для использования внутри замыканий
    fn find_node_by_type_static(graph: &AudioGraph, type_id: std::any::TypeId) -> Option<kama_core::graph::NodeId> {
        for &node_id in graph.get_processing_order() {
            if let Some(node) = graph.get_node(node_id) {
                if node.type_id() == type_id {
                    return Some(node_id);
                }
            }
        }
        None
    }
    
    /// Сбросить граф
    pub fn reset_graph(&self) {
        self.with_graph(|graph| graph.reset());
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
        
        let mut graph = self.graph.write();
        
        // Если есть входной узел, передаем ему сигнал
        if let Some(input_id) = self.input_node_id {
            if let Some(node) = graph.get_node_mut(input_id) {
                let input_slices = [self.temp_input.as_slice()];
                let mut output_slices = [self.temp_output.as_mut_slice()];
                let _ = node.process(&input_slices, &mut output_slices);
            }
        }
        
        // Обрабатываем весь граф
        let graph_input = [self.temp_output.as_slice()];
        let mut graph_output = [output];
        
        let _ = graph.process(&graph_input, &mut graph_output);
    }
    
    fn reset(&mut self) {
        self.with_graph(|graph| graph.reset());
        self.temp_input.clear();
        self.temp_output.clear();
    }
    
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.with_graph(|graph| graph.init_all(sample_rate));
    }
}

impl Clone for GraphProcessor {
    fn clone(&self) -> Self {
        Self {
            graph: self.graph.clone(),
            input_node_id: self.input_node_id,
            output_node_id: self.output_node_id,
            temp_input: Vec::new(),
            temp_output: Vec::new(),
            sample_rate: self.sample_rate,
        }
    }
}