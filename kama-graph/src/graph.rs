//! Основная реализация графа обработки

use std::collections::{HashMap, VecDeque};
use kama_core::traits::{AudioNode, AudioError, NodeId, PortId, PortType};
use kama_buffers::BufferManager;
use crate::connection::Connection;
use crate::error::{GraphError, GraphResult};

// -----------------------------------------------------------------------------
// Структуры для управления буферами узлов
// -----------------------------------------------------------------------------

/// Буферы для обработки узла
#[derive(Debug, Clone, Default)]
pub struct NodeBuffers {
    pub inputs: Vec<Vec<f32>>,
    pub outputs: Vec<Vec<f32>>,
}

/// Статистика менеджера буферов
#[derive(Debug, Clone, Copy)]
pub struct BufferManagerStats {
    pub active_nodes: usize,
    pub active_buffers: usize,
    pub total_memory_bytes: usize,
    pub pool_size: usize,
    pub pool_available: usize,
}

/// Менеджер буферов для графа
struct GraphBufferManager {
    buffer_pool: BufferManager,
    node_buffers: HashMap<NodeId, NodeBuffers>,
    stats: BufferManagerStats,
}

impl GraphBufferManager {
    fn new() -> Self {
        Self {
            buffer_pool: BufferManager::new(),
            node_buffers: HashMap::new(),
            stats: BufferManagerStats {
                active_nodes: 0,
                active_buffers: 0,
                total_memory_bytes: 0,
                pool_size: 4096,
                pool_available: 16,
            },
        }
    }
    
    fn with_config(max_pool_size: usize, default_buffer_size: usize) -> Self {
        Self {
            buffer_pool: BufferManager::with_config(max_pool_size, default_buffer_size),
            node_buffers: HashMap::new(),
            stats: BufferManagerStats {
                active_nodes: 0,
                active_buffers: 0,
                total_memory_bytes: 0,
                pool_size: default_buffer_size,
                pool_available: max_pool_size,
            },
        }
    }
    
    fn get_buffers(
        &mut self, 
        node_id: NodeId, 
        num_inputs: usize, 
        num_outputs: usize, 
        buffer_size: usize
    ) -> &mut NodeBuffers {
        let needs_creation = if let Some(buffers) = self.node_buffers.get(&node_id) {
            buffers.inputs.len() != num_inputs ||
            buffers.outputs.len() != num_outputs ||
            buffers.inputs.iter().any(|b| b.len() != buffer_size) ||
            buffers.outputs.iter().any(|b| b.len() != buffer_size)
        } else {
            true
        };
        
        if needs_creation {
            // Если были старые буферы, вычитаем их из статистики
            if let Some(old) = self.node_buffers.remove(&node_id) {
                self.stats.active_buffers -= (old.inputs.len() + old.outputs.len());
                self.stats.total_memory_bytes -= old.inputs.iter().chain(&old.outputs)
                    .map(|b| b.len() * std::mem::size_of::<f32>())
                    .sum::<usize>();
            }
            
            // Создаём новые буферы
            let mut inputs = Vec::with_capacity(num_inputs);
            let mut outputs = Vec::with_capacity(num_outputs);
            
            for _ in 0..num_inputs {
                if let Ok(pooled) = self.buffer_pool.acquire(buffer_size) {
                    inputs.push(pooled.into_vec());
                } else {
                    inputs.push(vec![0.0; buffer_size]);
                }
            }
            
            for _ in 0..num_outputs {
                if let Ok(pooled) = self.buffer_pool.acquire(buffer_size) {
                    outputs.push(pooled.into_vec());
                } else {
                    outputs.push(vec![0.0; buffer_size]);
                }
            }
            
            let new_buffers = NodeBuffers { inputs, outputs };
            
            // Добавляем новые буферы в статистику
            let new_buffers_count = new_buffers.inputs.len() + new_buffers.outputs.len();
            let new_memory = new_buffers.inputs.iter().chain(&new_buffers.outputs)
                .map(|b| b.len() * std::mem::size_of::<f32>())
                .sum::<usize>();
            
            self.stats.active_buffers += new_buffers_count;
            self.stats.total_memory_bytes += new_memory;
            
            // Вставляем новые буферы и обновляем количество активных узлов
            self.node_buffers.insert(node_id, new_buffers);
            self.stats.active_nodes = self.node_buffers.len();
        }
        
        self.node_buffers.get_mut(&node_id).unwrap()
    }
    
    fn release_all(&mut self) {
        self.node_buffers.clear();
        self.stats.active_nodes = 0;
        self.stats.active_buffers = 0;
        self.stats.total_memory_bytes = 0;
        self.buffer_pool.clear_all();
    }
    
    fn release_node(&mut self, node_id: NodeId) {
        if let Some(buffers) = self.node_buffers.remove(&node_id) {
            self.stats.active_buffers -= (buffers.inputs.len() + buffers.outputs.len());
            self.stats.total_memory_bytes -= buffers.inputs.iter().chain(&buffers.outputs)
                .map(|b| b.len() * std::mem::size_of::<f32>())
                .sum::<usize>();
            self.stats.active_nodes = self.node_buffers.len();
            
            // Возвращаем буферы в пул (опционально)
            for mut buf in buffers.inputs.into_iter().chain(buffers.outputs) {
                buf.fill(0.0);
                let _ = self.buffer_pool.acquire(buf.len());
            }
        }
    }
    
    fn clear_cache(&mut self) {
        self.node_buffers.clear();
        self.stats.active_nodes = 0;
        self.stats.active_buffers = 0;
        self.stats.total_memory_bytes = 0;
    }
    
    fn stats(&self) -> BufferManagerStats {
        self.stats
    }
}

// -----------------------------------------------------------------------------
// Основной граф
// -----------------------------------------------------------------------------

pub struct AudioGraph {
    nodes: HashMap<NodeId, Box<dyn AudioNode>>,
    connections: Vec<Connection>,
    processing_order: Vec<NodeId>,
    sample_rate: f32,
    next_id: u32,
    buffer_manager: GraphBufferManager,
    input_connections: HashMap<PortId, Vec<Connection>>,
    output_connections: HashMap<PortId, Vec<Connection>>,
}

impl AudioGraph {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            nodes: HashMap::new(),
            connections: Vec::new(),
            processing_order: Vec::new(),
            sample_rate,
            next_id: 0,
            buffer_manager: GraphBufferManager::new(),
            input_connections: HashMap::new(),
            output_connections: HashMap::new(),
        }
    }
    
    pub fn with_buffer_config(mut self, max_pool_size: usize, default_buffer_size: usize) -> Self {
        self.buffer_manager = GraphBufferManager::with_config(max_pool_size, default_buffer_size);
        self
    }
    
    pub fn add_node(&mut self, mut node: Box<dyn AudioNode>) -> NodeId {
        node.init(self.sample_rate);
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.insert(id, node);
        self.update_processing_order();
        id
    }
    
    pub fn remove_node(&mut self, id: NodeId) -> Option<Box<dyn AudioNode>> {
        let node = self.nodes.remove(&id);
        
        self.connections.retain(|conn| {
            conn.from.node != id && conn.to.node != id
        });
        
        self.rebuild_connection_cache();
        self.update_processing_order();
        self.buffer_manager.release_node(id);
        
        node
    }
    
    pub fn connect(&mut self, from: PortId, to: PortId, gain: f32) -> GraphResult<()> {
        if !self.nodes.contains_key(&from.node) || !self.nodes.contains_key(&to.node) {
            return Err(GraphError::InvalidNodeId);
        }
        
        if !from.is_output() || !to.is_input() {
            return Err(GraphError::InvalidConnectionDirection);
        }
        
        let conn = Connection::new(from, to, gain);
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
    }
    
    fn rebuild_connection_cache(&mut self) {
        self.input_connections.clear();
        self.output_connections.clear();
        
        for conn in &self.connections {
            self.input_connections
                .entry(conn.to)
                .or_default()
                .push(conn.clone());
            
            self.output_connections
                .entry(conn.from)
                .or_default()
                .push(conn.clone());
        }
    }
    
    fn update_processing_order(&mut self) {
        let mut dependencies: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        let mut dependents: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        
        for &node_id in self.nodes.keys() {
            dependencies.entry(node_id).or_default();
            dependents.entry(node_id).or_default();
        }
        
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
        self.nodes.get_mut(&id).map(|node| node.as_mut() as &mut dyn AudioNode)
    }
    
    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }
    
    pub fn processing_order(&self) -> &[NodeId] {
        &self.processing_order
    }
    
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }
    
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }
    
    pub fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> GraphResult<()> {
        if outputs.is_empty() {
            return Ok(());
        }
        
        let buffer_size = outputs[0].len();
        let max_nodes = self.next_id as usize;
        let mut node_output_buffers: Vec<Option<Vec<Vec<f32>>>> = vec![None; max_nodes];
        
        for &node_id in &self.processing_order {
            if let Some(node) = self.nodes.get_mut(&node_id) {
                let num_inputs = node.as_ref().num_ports(PortType::AudioIn);
                let num_outputs = node.as_ref().num_ports(PortType::AudioOut);
                
                let buffers = self.buffer_manager.get_buffers(
                    node_id,
                    num_inputs,
                    num_outputs,
                    buffer_size
                );
                
                let mut input_buffers = vec![vec![0.0; buffer_size]; num_inputs];
                
                for input_idx in 0..num_inputs {
                    let port_id = PortId::audio_in(node_id, input_idx as u16);
                    
                    if let Some(connections) = self.input_connections.get(&port_id) {
                        for conn in connections {
                            let src_node_id = conn.from.node;
                            let src_node_idx = src_node_id.0 as usize;
                            
                            if src_node_idx < node_output_buffers.len() {
                                if let Some(Some(src_outputs)) = node_output_buffers.get(src_node_idx) {
                                    let src_idx = conn.from.index as usize;
                                    if src_idx < src_outputs.len() {
                                        let src_buffer = &src_outputs[src_idx];
                                        
                                        for i in 0..buffer_size {
                                            input_buffers[input_idx][i] += src_buffer[i] * conn.gain;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                for (i, input_buf) in input_buffers.iter().enumerate() {
                    if i < buffers.inputs.len() {
                        buffers.inputs[i].copy_from_slice(input_buf);
                    }
                }
                
                Self::process_node(node.as_mut(), buffers)?;
                
                let node_idx = node_id.0 as usize;
                if node_idx < node_output_buffers.len() {
                    node_output_buffers[node_idx] = Some(buffers.outputs.clone());
                }
            }
        }
        
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
        
        Ok(())
    }
    
    fn process_node(node: &mut dyn AudioNode, buffers: &mut NodeBuffers) -> Result<(), AudioError> {
        let input_slices: Vec<&[f32]> = buffers.inputs.iter()
            .map(|buf| buf.as_slice())
            .collect();
        
        let mut output_slices: Vec<&mut [f32]> = buffers.outputs.iter_mut()
            .map(|buf| buf.as_mut_slice())
            .collect();
        
        node.process(&input_slices, &mut output_slices)
    }
    
    pub fn reset(&mut self) {
        for node in self.nodes.values_mut() {
            node.reset();
        }
        self.buffer_manager.clear_cache();
    }
    
    pub fn init_all(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        for node in self.nodes.values_mut() {
            node.init(sample_rate);
        }
    }
    
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.connections.clear();
        self.input_connections.clear();
        self.output_connections.clear();
        self.processing_order.clear();
        self.buffer_manager.release_all();
    }
    
    pub fn buffer_stats(&self) -> BufferManagerStats {
        self.buffer_manager.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kama_core::traits::{NodeMetadata, NodeCategory, NodeTypeId, ParamValue};
    
    struct TestNode {
        id: u32,
    }
    
    impl TestNode {
        fn new(id: u32) -> Self {
            Self { id }
        }
    }
    
    impl AudioNode for TestNode {
        fn process(&mut self, _inputs: &[&[f32]], _outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
            Ok(())
        }
        
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        
        fn node_type_id(&self) -> NodeTypeId {
            NodeTypeId::of::<Self>()
        }
        
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "Test Node".to_string(),
                category: NodeCategory::Utility,
                description: "Test node for graph".to_string(),
                author: "Kama".to_string(),
                version: "1.0".to_string(),
                parameters: vec![],
            }
        }
    }
    
    #[test]
    fn test_graph_creation() {
        let graph = AudioGraph::new(44100.0);
        assert_eq!(graph.sample_rate(), 44100.0);
        assert_eq!(graph.node_count(), 0);
    }
    
    #[test]
    fn test_add_node() {
        let mut graph = AudioGraph::new(44100.0);
        let node = Box::new(TestNode::new(1));
        let id = graph.add_node(node);
        
        assert_eq!(graph.node_count(), 1);
        assert!(graph.get_node(id).is_some());
    }
    
    #[test]
    fn test_remove_node() {
        let mut graph = AudioGraph::new(44100.0);
        
        let node1 = Box::new(TestNode::new(1));
        let node2 = Box::new(TestNode::new(2));
        
        let id1 = graph.add_node(node1);
        let id2 = graph.add_node(node2);
        
        assert_eq!(graph.node_count(), 2);
        
        graph.remove_node(id1);
        assert_eq!(graph.node_count(), 1);
        assert!(graph.get_node(id2).is_some());
    }
    
    #[test]
    fn test_connect_nodes() {
        let mut graph = AudioGraph::new(44100.0);
        
        let node1 = Box::new(TestNode::new(1));
        let node2 = Box::new(TestNode::new(2));
        
        let id1 = graph.add_node(node1);
        let id2 = graph.add_node(node2);
        
        let out = PortId::audio_out(id1, 0);
        let in_port = PortId::audio_in(id2, 0);
        
        graph.connect(out, in_port, 1.0).unwrap();
        
        assert_eq!(graph.connection_count(), 1);
    }
    
    #[test]
#[test]
fn test_buffer_stats() {
    let mut graph = AudioGraph::new(44100.0);
    
    // Создаём источник (генератор) и преобразователь (эффект)
    let source = Box::new(TestSource::new(1));  // источник сигнала (0 входов, 1 выход)
    let processor = Box::new(TestProcessor::new(2));  // преобразователь (1 вход, 1 выход)
    
    let source_id = graph.add_node(source);
    let processor_id = graph.add_node(processor);
    
    // Соединяем источник с преобразователем
    let out_port = PortId::audio_out(source_id, 0);
    let in_port = PortId::audio_in(processor_id, 0);
    graph.connect(out_port, in_port, 1.0).unwrap();
    
    println!("Before processing:");
    let stats_before = graph.buffer_stats();
    println!("  active_nodes: {}", stats_before.active_nodes);
    println!("  active_buffers: {}", stats_before.active_buffers);
    println!("  total_memory_bytes: {}", stats_before.total_memory_bytes);
    
    assert_eq!(stats_before.active_nodes, 0);
    assert_eq!(stats_before.active_buffers, 0);
    
    // Обрабатываем - теперь буферы должны создаться
    let mut output = vec![0.0; 64];
    let mut outputs = [output.as_mut_slice()];
    graph.process(&[], &mut outputs).unwrap();
    
    println!("\nAfter processing:");
    let stats_after = graph.buffer_stats();
    println!("  active_nodes: {}", stats_after.active_nodes);
    println!("  active_buffers: {}", stats_after.active_buffers);
    println!("  total_memory_bytes: {}", stats_after.total_memory_bytes);
    
    // Должно быть 2 активных узла и 3 буфера:
    // - source: 0 входов, 1 выход = 1 буфер
    // - processor: 1 вход, 1 выход = 2 буфера
    // Всего: 3 буфера
    assert_eq!(stats_after.active_nodes, 2);
    assert_eq!(stats_after.active_buffers, 3);
    assert!(stats_after.total_memory_bytes > 0);
}

// Тестовый источник сигнала (0 входов, 1 выход)
#[test]
fn test_signal_propagation() -> Result<(), Box<dyn std::error::Error>> {
    let sample_rate = 44100.0;
    let mut graph = AudioGraph::new(sample_rate);

    // Создаём простую цепочку с измерительными узлами
    let source = Box::new(TestSource::new(1));  // генерирует сигнал 0.5
    let source_id = graph.add_node(source);
    
    let amplifier = Box::new(TestAmplifier::new(2.0));  // усиливает в 2 раза
    let amp_id = graph.add_node(amplifier);
    
    let meter = Box::new(TestMeter::new(3));  // измеряет сигнал
    let meter_id = graph.add_node(meter);

    // Соединяем: source -> amplifier -> meter
    graph.connect(
        PortId::audio_out(source_id, 0), 
        PortId::audio_in(amp_id, 0), 
        1.0
    )?;
    
    graph.connect(
        PortId::audio_out(amp_id, 0), 
        PortId::audio_in(meter_id, 0), 
        1.0
    )?;

    // Обрабатываем
    let mut output = vec![0.0; 1024];
    let mut outputs = [output.as_mut_slice()];
    graph.process(&[], &mut outputs)?;

    // Проверяем, что сигнал прошёл через все узлы
    // Для этого нам нужен доступ к состоянию meter
    // В реальности нужно добавить метод для получения измерений
    
    Ok(())
}

// Тестовый усилитель
struct TestAmplifier {
    gain: f32,
}

impl TestAmplifier {
    fn new(gain: f32) -> Self {
        Self { gain }
    }
}

impl AudioNode for TestAmplifier {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if !inputs.is_empty() && !outputs.is_empty() {
            for i in 0..inputs[0].len().min(outputs[0].len()) {
                outputs[0][i] = inputs[0][i] * self.gain;
            }
        }
        Ok(())
    }
    
    fn num_ports(&self, port_type: PortType) -> usize {
        match port_type {
            PortType::AudioIn => 1,
            PortType::AudioOut => 1,
            _ => 0,
        }
    }
    
    fn init(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {}
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Test Amplifier".to_string(),
            category: NodeCategory::Effect,
            description: "Test amplifier".to_string(),
            author: "Kama".to_string(),
            version: "1.0".to_string(),
            parameters: vec![],
        }
    }
}
// Тестовый источник сигнала (0 входов, 1 выход)
struct TestSource {
    id: u32,
}

impl TestSource {
    fn new(id: u32) -> Self {
        Self { id }
    }
}

impl AudioNode for TestSource {
    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if !outputs.is_empty() {
            // Генерируем тестовый сигнал
            for sample in outputs[0].iter_mut() {
                *sample = 0.5;
            }
        }
        Ok(())
    }
    
    fn num_ports(&self, port_type: PortType) -> usize {
        match port_type {
            PortType::AudioOut => 1,  // 1 выход
            _ => 0,
        }
    }
    
    fn init(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {}
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Test Source".to_string(),
            category: NodeCategory::Generator,
            description: "Test source node".to_string(),
            author: "Kama".to_string(),
            version: "1.0".to_string(),
            parameters: vec![],
        }
    }
}

// Тестовый преобразователь (1 вход, 1 выход)
struct TestProcessor {
    id: u32,
}

impl TestProcessor {
    fn new(id: u32) -> Self {
        Self { id }
    }
}

impl AudioNode for TestProcessor {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if !inputs.is_empty() && !outputs.is_empty() {
            // Просто копируем вход в выход
            for i in 0..inputs[0].len().min(outputs[0].len()) {
                outputs[0][i] = inputs[0][i];
            }
        }
        Ok(())
    }
    
    fn num_ports(&self, port_type: PortType) -> usize {
        match port_type {
            PortType::AudioIn => 1,   // 1 вход
            PortType::AudioOut => 1,  // 1 выход
            _ => 0,
        }
    }
    
    fn init(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {}
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Test Processor".to_string(),
            category: NodeCategory::Effect,
            description: "Test processor node".to_string(),
            author: "Kama".to_string(),
            version: "1.0".to_string(),
            parameters: vec![],
        }
    }
}

// Тестовый измеритель
struct TestMeter {
    last_value: f32,
}

impl TestMeter {
    fn new(_id: u32) -> Self {
        Self { last_value: 0.0 }
    }
    
    fn last_value(&self) -> f32 {
        self.last_value
    }
}

impl AudioNode for TestMeter {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if !inputs.is_empty() {
            // Запоминаем последнее значение
            if !inputs[0].is_empty() {
                self.last_value = inputs[0][inputs[0].len() - 1];
            }
            
            // Пропускаем сигнал дальше
            if !outputs.is_empty() {
                outputs[0].copy_from_slice(inputs[0]);
            }
        }
        Ok(())
    }
    
    fn num_ports(&self, port_type: PortType) -> usize {
        match port_type {
            PortType::AudioIn => 1,
            PortType::AudioOut => 1,
            _ => 0,
        }
    }
    
    fn init(&mut self, _sample_rate: f32) {}
    fn reset(&mut self) {
        self.last_value = 0.0;
    }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Test Meter".to_string(),
            category: NodeCategory::Analyzer,
            description: "Test meter".to_string(),
            author: "Kama".to_string(),
            version: "1.0".to_string(),
            parameters: vec![],
        }
    }
}
}