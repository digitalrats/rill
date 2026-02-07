// FILE: /home/mikek/Projects/kama/kama-audio/kama-core/src/graph/mod.rs
use std::collections::{HashMap, VecDeque};
use crate::node::AudioNode;
use crate::AudioError;

mod processor;
use processor::{BufferManager, NodeProcessor};

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
    
    fn update_processing_order(&mut self) {
        let mut dependencies: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut dependents: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        
        for conn in &self.connections {
            dependencies.entry(conn.to.node)
                .or_default()
                .push(conn.from.node);
                
            dependents.entry(conn.from.node)
                .or_default()
                .push(conn.to.node);
        }
        
        let mut queue: VecDeque<NodeId> = self.nodes.keys()
            .filter(|&id| dependencies.get(id).map_or(true, |d| d.is_empty()))
            .copied()
            .collect();
            
        let mut order = Vec::new();
        let mut visited = HashMap::new();
        
        while let Some(node) = queue.pop_front() {
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
    
    /// Простой метод обработки - без сложной маршрутизации пока
    pub fn process_simple(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }
        
        let buffer_size = outputs[0].len();
        
        // Просто обрабатываем каждый узел в правильном порядке
        for &node_id in &self.processing_order {
            if let Some(node) = self.nodes.get_mut(&node_id) {
                let num_inputs = node.num_inputs();
                let num_outputs = node.num_outputs();
                
                // Создаём временные буферы
                let mut input_buffers: Vec<Vec<f32>> = (0..num_inputs)
                    .map(|_| vec![0.0; buffer_size])
                    .collect();
                
                let mut output_buffers: Vec<Vec<f32>> = (0..num_outputs)
                    .map(|_| vec![0.0; buffer_size])
                    .collect();
                
                // Преобразуем в срезы
                let input_slices: Vec<&[f32]> = input_buffers.iter()
                    .map(|buf| buf.as_slice())
                    .collect();
                
                let mut output_slices: Vec<&mut [f32]> = output_buffers.iter_mut()
                    .map(|buf| buf.as_mut_slice())
                    .collect();
                
                // Обрабатываем узел
                node.process(&input_slices, &mut output_slices)?;
            }
        }
        
        // Очистить выходы (заглушка)
        for output in outputs {
            output.fill(0.0);
        }
        
        Ok(())
    }
    
    pub fn get_node(&self, id: NodeId) -> Option<&dyn AudioNode> {
        self.nodes.get(&id).map(|n| n.as_ref())
    }
    
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut dyn AudioNode> {
        self.nodes.get_mut(&id).map(|n| n.as_mut())
    }
    
    pub fn get_processing_order(&self) -> &[NodeId] {
        &self.processing_order
    }
}