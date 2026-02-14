use crate::AudioError;  // ✅ Добавляем импорт

use std::collections::{HashMap, VecDeque};
use crate::node::AudioNode;

// Объявляем типы ДО их использования
mod processor;
pub use processor::{BufferManager, NodeProcessor};

/// Идентификатор узла
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

/// Идентификатор порта
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortId {
    pub node: NodeId,
    pub index: u8,
    pub is_input: bool,
}

/// Соединение
#[derive(Debug, Clone)]
pub struct Connection {
    pub from: PortId,
    pub to: PortId,
    pub gain: f32,
}

/// Аудиограф
pub struct AudioGraph {
    nodes: HashMap<NodeId, Box<dyn AudioNode>>,
    connections: Vec<Connection>,
    processing_order: Vec<NodeId>,
    sample_rate: f32,
    next_id: u32,
    buffer_manager: BufferManager,
    node_processor: NodeProcessor,
    
    // Таблицы соединений для быстрого доступа
    input_connections: HashMap<PortId, Vec<Connection>>,
    output_connections: HashMap<PortId, Vec<Connection>>,
}

// Теперь реализация
impl AudioGraph {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            nodes: HashMap::new(),
            connections: Vec::new(),
            processing_order: Vec::new(),
            sample_rate,
            next_id: 0,
            buffer_manager: BufferManager::new(),
            node_processor: NodeProcessor::new(),
            input_connections: HashMap::new(),
            output_connections: HashMap::new(),
        }
    }
    
    pub fn add_node(&mut self, mut node: Box<dyn AudioNode>) -> NodeId {
        node.init(self.sample_rate);
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.insert(id, node);
        self.update_processing_order();
        id
    }
    
    pub fn connect(&mut self, from: PortId, to: PortId, gain: f32) -> Result<(), AudioError> {
        if !self.nodes.contains_key(&from.node) || !self.nodes.contains_key(&to.node) {
            return Err(AudioError::Graph("Invalid node ID".to_string()));
        }
        
        if !from.is_input && to.is_input {
            let conn = Connection { from, to, gain };
            
            self.connections.push(conn.clone());
            
            self.input_connections
                .entry(to)
                .or_default()
                .push(conn.clone());
            
            self.output_connections
                .entry(from)
                .or_default()
                .push(conn.clone());
            
            self.update_processing_order();
            Ok(())
        } else {
            Err(AudioError::Graph("Invalid connection direction".to_string()))
        }
    }

    /// Инициализировать все узлы с новой частотой дискретизации
    pub fn init_all(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for node in self.nodes.values_mut() {
            node.init(sample_rate);
        }
    }

    /// Сбросить состояние всех узлов в графе
    /// 
    /// Полезно для:
    /// - Перезапуска обработки
    /// - Очистки состояний фильтров и задержек
    /// - Подготовки к новому семплу
    pub fn reset(&mut self) {
        for node in self.nodes.values_mut() {
            node.reset();
        }
    }

    /// Сбросить состояние конкретного узла
    /// 
    /// # Arguments
    /// * `id` - ID узла для сброса
    /// 
    /// # Returns
    /// * `true` если узел найден и сброшен, `false` в противном случае
    pub fn reset_node(&mut self, id: NodeId) -> bool {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.reset();
            true
        } else {
            false
        }
    }

    /// Сбросить все узлы определенного типа
    /// 
    /// # Type parameters
    /// * `T` - тип узла для сброса (должен реализовывать `AudioNode`)
    pub fn reset_nodes_by_type<T: AudioNode + 'static>(&mut self) {
        for node in self.nodes.values_mut() {
            // Проверяем тип через Any
            if node.as_ref().type_id() == std::any::TypeId::of::<T>() {
                node.reset();
            }
        }
    }

    /// Получить частоту дискретизации графа
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
    
    /// Получить мутабельную ссылку на частоту дискретизации (если нужно изменить)
    pub fn sample_rate_mut(&mut self) -> &mut f32 {
        &mut self.sample_rate
    }
    
    // В graph/mod.rs
    fn update_processing_order(&mut self) {
        let mut dependencies: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut dependents: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        
        // Инициализируем все узлы
        for &node_id in self.nodes.keys() {
            dependencies.entry(node_id).or_default();
            dependents.entry(node_id).or_default();
        }
        
        // Добавляем зависимости от соединений
        for conn in &self.connections {
            dependencies.entry(conn.to.node)
                .or_default()
                .push(conn.from.node);
                
            dependents.entry(conn.from.node)
                .or_default()
                .push(conn.to.node);
        }
        
        // Начинаем с узлов без зависимостей (источников)
        let mut queue: VecDeque<NodeId> = self.nodes.keys()
            .filter(|&id| dependencies.get(id).map_or(true, |d| d.is_empty()))
            .copied()
            .collect();
            
        let mut order = Vec::new();
        let mut visited = HashMap::new();
        
        
        while let Some(node) = queue.pop_front() {
            if visited.contains_key(&node) {
                continue;
            }
            
            order.push(node);
            visited.insert(node, true);
            
            
            if let Some(children) = dependents.get(&node) {
                for &child in children {
                    if let Some(parents) = dependencies.get_mut(&child) {
                        if let Some(idx) = parents.iter().position(|&n| n == node) {
                            parents.remove(idx);
                            if parents.is_empty() && !visited.contains_key(&child) {
                                queue.push_back(child);
                            }
                        }
                    }
                }
            }
        }
        
        // Добавляем оставшиеся узлы (если есть циклы или изолированные узлы)
        for &node_id in self.nodes.keys() {
            if !order.contains(&node_id) {
                order.push(node_id);
            }
        }
        
        self.processing_order = order;
    }
    
    pub fn get_node(&self, id: NodeId) -> Option<&dyn AudioNode> {
        self.nodes.get(&id).map(|n| n.as_ref())
    }
    
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut dyn AudioNode> {
        self.nodes.get_mut(&id).map(|n| n.as_mut())
    }
    
    pub fn get_connections(&self) -> &[Connection] {
        &self.connections
    }
    
    pub fn get_processing_order(&self) -> &[NodeId] {
        &self.processing_order
    }
    
    pub fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }
        
        let buffer_size = outputs[0].len();
        
        // Временное хранилище для выходов узлов
        let max_nodes = self.next_id as usize;
        let mut node_output_buffers: Vec<Option<Vec<Vec<f32>>>> = vec![None; max_nodes];
        
        // Проходим по узлам в порядке обработки
        for &node_id in &self.processing_order {
            if let Some(node) = self.nodes.get_mut(&node_id) {
                let num_inputs = node.num_inputs();
                let num_outputs = node.num_outputs();
                
                // Создаём входные буферы для этого узла
                let mut input_buffers = vec![vec![0.0; buffer_size]; num_inputs];
                
                // Собираем входные данные для этого узла
                for input_idx in 0..num_inputs {
                    let port_id = PortId {
                        node: node_id,
                        index: input_idx as u8,
                        is_input: true,
                    };
                    
                    // Получаем соединения для этого входа
                    if let Some(connections) = self.input_connections.get(&port_id) {
                        for conn in connections {
                            // Получаем выходы предыдущего узла
                            let src_node_id = conn.from.node;
                            let src_node_idx = src_node_id.0 as usize;
                            
                            if src_node_idx < node_output_buffers.len() {
                                if let Some(Some(src_outputs)) = node_output_buffers.get(src_node_idx) {
                                    let src_idx = conn.from.index as usize;
                                    if src_idx < src_outputs.len() {
                                        let src_buffer = &src_outputs[src_idx];
                                        
                                        // Суммируем с коэффициентом
                                        for i in 0..buffer_size {
                                            input_buffers[input_idx][i] += src_buffer[i] * conn.gain;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Обрабатываем узел
                let mut output_buffers = vec![vec![0.0; buffer_size]; num_outputs];
                
                // Создаём срезы для метода process
                let input_slices: Vec<&[f32]> = input_buffers.iter()
                    .map(|buf| buf.as_slice())
                    .collect();
                
                let mut output_slices: Vec<&mut [f32]> = output_buffers.iter_mut()
                    .map(|buf| buf.as_mut_slice())
                    .collect();
                
                node.process(&input_slices, &mut output_slices)?;
                
                // Сохраняем выходы узла
                let node_idx = node_id.0 as usize;
                if node_idx < node_output_buffers.len() {
                    node_output_buffers[node_idx] = Some(output_buffers);
                }
            }
        }
        
        // Находим выходные узлы (узлы, чьи выходы не подключены к другим узлам)
        let output_node_ids: Vec<NodeId> = self.processing_order.iter()
            .filter(|&&node_id| {
                let num_outputs = self.nodes.get(&node_id)
                    .map(|n| n.num_outputs())
                    .unwrap_or(0);
                
                (0..num_outputs).all(|output_idx| {
                    let port_id = PortId {
                        node: node_id,
                        index: output_idx as u8,
                        is_input: false,
                    };
                    
                    // Если нет соединений от этого выхода
                    self.output_connections.get(&port_id)
                        .map(|conns| conns.is_empty())
                        .unwrap_or(true)
                })
            })
            .copied()
            .collect();
        
        // Если указаны выходные узлы, используем первый
        if let Some(&output_node_id) = output_node_ids.first() {
            let node_idx = output_node_id.0 as usize;
            if node_idx < node_output_buffers.len() {
                if let Some(Some(outputs_to_copy)) = node_output_buffers.get(node_idx) {
                    for (i, output_channel) in outputs.iter_mut().enumerate() {
                        if i < outputs_to_copy.len() {
                            let copy_len = buffer_size.min(output_channel.len());
                            output_channel[..copy_len].copy_from_slice(&outputs_to_copy[i][..copy_len]);
                        }
                    }
                }
            }
        } else {
            // Если не нашли явных выходных узлов, используем последний в порядке обработки
            if let Some(&last_node_id) = self.processing_order.last() {
                let node_idx = last_node_id.0 as usize;
                if node_idx < node_output_buffers.len() {
                    if let Some(Some(outputs_to_copy)) = node_output_buffers.get(node_idx) {
                        for (i, output_channel) in outputs.iter_mut().enumerate() {
                            if i < outputs_to_copy.len() {
                                let copy_len = buffer_size.min(output_channel.len());
                                output_channel[..copy_len].copy_from_slice(&outputs_to_copy[i][..copy_len]);
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
}