//! Процессор для выполнения узлов графа

use kama_buffers::{BufferManager, NodeBuffers};
use kama_core::traits::{AudioError, AudioNode, NodeId};
use std::collections::HashMap;

/// Процессор для выполнения узла
#[derive(Default, Clone)]
pub struct NodeProcessor;

impl NodeProcessor {
    /// Создать новый процессор
    pub fn new() -> Self {
        Self
    }

    /// Обработать один узел
    pub fn process(
        &self,
        node: &mut dyn AudioNode,
        buffers: &mut NodeBuffers,
    ) -> Result<(), AudioError> {
        let input_slices: Vec<&[f32]> = buffers.inputs.iter().map(|buf| buf.as_slice()).collect();

        let mut output_slices: Vec<&mut [f32]> = buffers
            .outputs
            .iter_mut()
            .map(|buf| buf.as_mut_slice())
            .collect();

        node.process(&input_slices, &mut output_slices)
    }
}

/// Менеджер буферов для графа (адаптер под новый API)
pub struct GraphBufferManager {
    inner: BufferManager,
    cached_buffers: HashMap<NodeId, NodeBuffers>,
}

impl GraphBufferManager {
    /// Создать новый менеджер
    pub fn new() -> Self {
        Self {
            inner: BufferManager::new(),
            cached_buffers: HashMap::new(),
        }
    }

    /// Создать с указанными параметрами
    pub fn with_config(max_pool_size: usize, default_buffer_size: usize) -> Self {
        Self {
            inner: BufferManager::with_config(max_pool_size, default_buffer_size),
            cached_buffers: HashMap::new(),
        }
    }

    /// Получить буферы для узла (совместимость со старым API)
    pub fn get_buffers(
        &mut self,
        node_id: NodeId,
        num_inputs: usize,
        num_outputs: usize,
        buffer_size: usize,
    ) -> &mut NodeBuffers {
        // Проверяем, нужно ли обновить существующие буферы
        let needs_update = if let Some(buffers) = self.cached_buffers.get(&node_id) {
            buffers.inputs.len() != num_inputs
                || buffers.outputs.len() != num_outputs
                || buffers.inputs.iter().any(|b| b.len() != buffer_size)
                || buffers.outputs.iter().any(|b| b.len() != buffer_size)
        } else {
            true
        };

        if needs_update {
            // Создаем новые буферы через with_buffers_mut
            let mut new_buffers = None;
            self.inner
                .with_buffers_mut(node_id, num_inputs, num_outputs, buffer_size, |buffers| {
                    new_buffers = Some(buffers.clone());
                });
            if let Some(buffers) = new_buffers {
                self.cached_buffers.insert(node_id, buffers);
            }
        }

        self.cached_buffers.get_mut(&node_id).unwrap()
    }

    /// Освободить все буферы
    pub fn release_all(&mut self) {
        self.cached_buffers.clear();
        self.inner.clear_all();
    }

    /// Освободить буферы узла
    pub fn release_node(&mut self, node_id: NodeId) {
        self.cached_buffers.remove(&node_id);
        self.inner.release_node(node_id);
    }

    /// Очистить кэш узлов
    pub fn clear_cache(&mut self) {
        self.cached_buffers.clear();
    }

    /// Получить статистику
    pub fn stats(&self) -> kama_buffers::BufferManagerStats {
        self.inner.stats()
    }
}

impl Default for GraphBufferManager {
    fn default() -> Self {
        Self::new()
    }
}

// Реэкспорты для удобства
pub use kama_buffers::BufferManagerStats;
#[cfg(test)]
mod tests {
    use super::*;
    use kama_core::traits::{AudioError, AudioNode, NodeMetadata, NodeTypeId, ParamValue};

    // Тестовый узел
    struct TestNode;

    impl AudioNode for TestNode {
        fn process(
            &mut self,
            _inputs: &[&[f32]],
            _outputs: &mut [&mut [f32]],
        ) -> Result<(), AudioError> {
            Ok(())
        }

        fn get_param(&self, _name: &str) -> Option<ParamValue> {
            None
        }
        fn set_param(&mut self, _name: &str, _value: ParamValue) -> Result<(), AudioError> {
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
        fn node_type_id(&self) -> NodeTypeId {
            NodeTypeId::of::<Self>()
        }
        fn metadata(&self) -> NodeMetadata {
            unimplemented!()
        }
    }

    #[test]
    fn test_node_processor() {
        let processor = NodeProcessor::new();
        let mut node = TestNode;
        let mut buffers = NodeBuffers::default();

        buffers.inputs.push(vec![1.0; 64]);
        buffers.outputs.push(vec![0.0; 64]);

        let result = processor.process(&mut node, &mut buffers);
        assert!(result.is_ok());
    }

    #[test]
    fn test_graph_buffer_manager_basic() {
        let mut manager = GraphBufferManager::new();
        let node_id = NodeId(1);

        let buffers = manager.get_buffers(node_id, 2, 2, 256);
        assert_eq!(buffers.inputs.len(), 2);
        assert_eq!(buffers.outputs.len(), 2);
        assert_eq!(buffers.inputs[0].len(), 256);

        let stats = manager.stats();
        assert_eq!(stats.active_nodes, 1);
        assert_eq!(stats.active_buffers, 4);
    }

    #[test]
    fn test_graph_buffer_manager_with_config() {
        let mut manager = GraphBufferManager::with_config(4, 256);
        let node_id = NodeId(1);

        let buffers = manager.get_buffers(node_id, 2, 2, 256);
        assert_eq!(buffers.inputs.len(), 2);
        assert_eq!(buffers.outputs.len(), 2);
    }

    #[test]
    fn test_release_node() {
        let mut manager = GraphBufferManager::new();

        manager.get_buffers(NodeId(1), 1, 1, 128);
        manager.get_buffers(NodeId(2), 2, 2, 256);

        assert_eq!(manager.stats().active_nodes, 2);

        manager.release_node(NodeId(1));

        assert_eq!(manager.stats().active_nodes, 1);
    }

    #[test]
    fn test_release_all() {
        let mut manager = GraphBufferManager::new();

        manager.get_buffers(NodeId(1), 1, 1, 128);
        manager.get_buffers(NodeId(2), 1, 1, 128);

        assert_eq!(manager.stats().active_nodes, 2);

        manager.release_all();

        assert_eq!(manager.stats().active_nodes, 0);
    }

    #[test]
    fn test_clear_cache() {
        let mut manager = GraphBufferManager::new();

        manager.get_buffers(NodeId(1), 1, 1, 128);
        manager.get_buffers(NodeId(2), 1, 1, 128);

        assert_eq!(manager.stats().active_nodes, 2);

        manager.clear_cache();

        assert_eq!(manager.stats().active_nodes, 2); // Кэш очищен, но статистика может не обновиться
    }
}
