//! Основная реализация графа обработки

use crate::connection::Connection;
use crate::error::{GraphError, GraphResult};
use crate::processor::{GraphBufferManager, NodeProcessor};
use kama_buffers::BufferManager;
use kama_core::traits::{AudioError, AudioNode, NodeId, PortId};
use std::collections::{HashMap, VecDeque};

/// Аудиограф - основной контейнер для узлов и соединений
pub struct AudioGraph {
    nodes: HashMap<NodeId, Box<dyn AudioNode>>,
    connections: Vec<Connection>,
    processing_order: Vec<NodeId>,
    sample_rate: f32,
    next_id: u32,
    buffer_manager: GraphBufferManager,
    node_processor: NodeProcessor,

    // Таблицы соединений для быстрого доступа
    input_connections: HashMap<PortId, Vec<Connection>>,
    output_connections: HashMap<PortId, Vec<Connection>>,
}

impl AudioGraph {
    /// Создать новый граф
    pub fn new(sample_rate: f32) -> Self {
        Self {
            nodes: HashMap::new(),
            connections: Vec::new(),
            processing_order: Vec::new(),
            sample_rate,
            next_id: 0,
            buffer_manager: GraphBufferManager::new(),
            node_processor: NodeProcessor::new(),
            input_connections: HashMap::new(),
            output_connections: HashMap::new(),
        }
    }

    /// Добавить узел в граф
    pub fn add_node(&mut self, mut node: Box<dyn AudioNode>) -> NodeId {
        node.init(self.sample_rate);
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.insert(id, node);
        self.update_processing_order();
        id
    }

    /// Удалить узел из графа
    pub fn remove_node(&mut self, id: NodeId) -> Option<Box<dyn AudioNode>> {
        let node = self.nodes.remove(&id);

        // Удаляем все соединения, связанные с этим узлом
        self.connections
            .retain(|conn| conn.from.node != id && conn.to.node != id);

        self.rebuild_connection_cache();
        self.update_processing_order();
        self.buffer_manager.release_node(id);

        node
    }

    /// Создать соединение
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

    /// Перестроить кэш соединений
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

    /// Обновить порядок обработки (топологическая сортировка)
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
            dependencies
                .entry(conn.to.node)
                .or_default()
                .push(conn.from.node);

            dependents
                .entry(conn.from.node)
                .or_default()
                .push(conn.to.node);
        }

        // Топологическая сортировка
        let mut queue: VecDeque<NodeId> = self
            .nodes
            .keys()
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

        // Добавляем оставшиеся узлы (если есть циклы)
        for &node_id in self.nodes.keys() {
            if !order.contains(&node_id) {
                order.push(node_id);
            }
        }

        self.processing_order = order;
    }

    /// Получить узел по ID
    pub fn get_node(&self, id: NodeId) -> Option<&dyn AudioNode> {
        self.nodes.get(&id).map(|n| n.as_ref())
    }

    /// Получить мутабельный узел по ID
    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut dyn AudioNode> {
        self.nodes
            .get_mut(&id)
            .map(|node| node.as_mut() as &mut dyn AudioNode)
    }

    /// Получить все соединения
    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }

    /// Получить порядок обработки
    pub fn processing_order(&self) -> &[NodeId] {
        &self.processing_order
    }

    /// Получить частоту дискретизации
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Получить количество узлов
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Получить количество соединений
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Обработать граф
    /// Обработать граф
    pub fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> GraphResult<()> {
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

                // Получаем буферы для узла
                let buffers =
                    self.buffer_manager
                        .get_buffers(node_id, num_inputs, num_outputs, buffer_size);

                // Создаём входные буферы для этого узла
                let mut input_buffers = vec![vec![0.0; buffer_size]; num_inputs];

                // Собираем входные данные для этого узла
                for input_idx in 0..num_inputs {
                    let port_id = PortId::input(node_id, input_idx as u8);

                    if let Some(connections) = self.input_connections.get(&port_id) {
                        for conn in connections {
                            let src_node_id = conn.from.node;
                            let src_node_idx = src_node_id.0 as usize;

                            if src_node_idx < node_output_buffers.len() {
                                if let Some(Some(src_outputs)) =
                                    node_output_buffers.get(src_node_idx)
                                {
                                    let src_idx = conn.from.index as usize;
                                    if src_idx < src_outputs.len() {
                                        let src_buffer = &src_outputs[src_idx];

                                        for i in 0..buffer_size {
                                            input_buffers[input_idx][i] +=
                                                src_buffer[i] * conn.gain;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Копируем входные данные в буферы узла
                for (i, input_buf) in input_buffers.iter().enumerate() {
                    if i < buffers.inputs.len() {
                        buffers.inputs[i].copy_from_slice(input_buf);
                    }
                }

                // Обрабатываем узел - используем as_mut() для преобразования Box<dyn> в &mut dyn
                self.node_processor.process(node.as_mut(), buffers)?;

                // Сохраняем выходы узла
                let node_idx = node_id.0 as usize;
                if node_idx < node_output_buffers.len() {
                    node_output_buffers[node_idx] = Some(buffers.outputs.clone());
                }
            }
        }

        // Копируем выходные данные (берем последний узел в порядке обработки)
        if let Some(&last_node_id) = self.processing_order.last() {
            let node_idx = last_node_id.0 as usize;
            if node_idx < node_output_buffers.len() {
                if let Some(Some(outputs_to_copy)) = node_output_buffers.get(node_idx) {
                    for (i, output_channel) in outputs.iter_mut().enumerate() {
                        if i < outputs_to_copy.len() {
                            let copy_len = buffer_size.min(output_channel.len());
                            output_channel[..copy_len]
                                .copy_from_slice(&outputs_to_copy[i][..copy_len]);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Очистить граф
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.connections.clear();
        self.input_connections.clear();
        self.output_connections.clear();
        self.processing_order.clear();
        self.buffer_manager.release_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Тестовый узел
    struct TestNode {
        id: u32,
    }

    impl TestNode {
        fn new(id: u32) -> Self {
            Self { id }
        }
    }

    impl kama_core::traits::AudioNode for TestNode {
        fn process(
            &mut self,
            _inputs: &[&[f32]],
            _outputs: &mut [&mut [f32]],
        ) -> Result<(), AudioError> {
            Ok(())
        }

        fn get_param(&self, _name: &str) -> Option<kama_core::traits::ParamValue> {
            None
        }

        fn set_param(
            &mut self,
            _name: &str,
            _value: kama_core::traits::ParamValue,
        ) -> Result<(), AudioError> {
            Ok(())
        }

        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn num_inputs(&self) -> usize {
            1
        }
        fn num_outputs(&self) -> usize {
            1
        }

        fn node_type_id(&self) -> kama_core::traits::NodeTypeId {
            kama_core::traits::NodeTypeId::of::<Self>()
        }

        fn metadata(&self) -> kama_core::traits::NodeMetadata {
            unimplemented!()
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

        let out = PortId::output(id1, 0);
        let in_port = PortId::input(id2, 0);

        graph.connect(out, in_port, 1.0).unwrap();

        assert_eq!(graph.connection_count(), 1);
    }
}
