//! Процессор для выполнения узлов графа
//!
//! Этот модуль предоставляет процессор для выполнения отдельных узлов,
//! используя `BufferManager` из `kama-buffers` для управления буферами.

use kama_core_traits::{AudioNode, AudioError, NodeId};
use kama_buffers::{BufferManager as BufManager, NodeBuffers, PoolStrategy};

/// Процессор для выполнения узла
///
/// Отвечает за вызов метода `process` у узла с правильными срезами буферов.
#[derive(Default, Clone)]
pub struct NodeProcessor;

impl NodeProcessor {
    /// Создать новый процессор
    pub fn new() -> Self {
        Self
    }
    
    /// Обработать один узел
    ///
    /// # Arguments
    /// * `node` - узел для обработки
    /// * `buffers` - буферы узла (входные и выходные)
    ///
    /// # Returns
    /// * `Ok(())` если обработка успешна
    /// * `Err(AudioError)` если произошла ошибка
    pub fn process(
        &self,
        node: &mut dyn AudioNode,
        buffers: &mut NodeBuffers,
    ) -> Result<(), AudioError> {
        // Создаём срезы входных буферов
        let input_slices: Vec<&[f32]> = buffers.inputs.iter()
            .map(|buf| buf.as_slice())
            .collect();
        
        // Создаём мутабельные срезы выходных буферов
        let mut output_slices: Vec<&mut [f32]> = buffers.outputs.iter_mut()
            .map(|buf| buf.as_mut_slice())
            .collect();
        
        // Вызываем метод process узла
        node.process(&input_slices, &mut output_slices)
    }
}

/// Менеджер буферов для графа
///
/// Это обёртка над `kama_buffers::BufferManager` для удобства использования в графе.
pub struct BufferManager {
    inner: BufManager,
}

impl BufferManager {
    /// Создать новый менеджер буферов
    pub fn new() -> Self {
        Self {
            inner: BufManager::new(),
        }
    }
    
    /// Создать с указанным размером пула
    pub fn with_pool_size(pool_size: usize, buffer_size: usize) -> Self {
        Self {
            inner: BufManager::with_pool_size(pool_size, buffer_size),
        }
    }
    
    /// Получить или создать буферы для узла
    pub fn get_buffers(&mut self, node_id: NodeId, num_inputs: usize, num_outputs: usize, buffer_size: usize) -> &mut NodeBuffers {
        self.inner.get_buffers(node_id.0, num_inputs, num_outputs, buffer_size)
    }
    
    /// Освободить все буферы
    pub fn release_all(&mut self) {
        self.inner.release_all();
    }
    
    /// Освободить буферы конкретного узла
    pub fn release_node(&mut self, node_id: NodeId) {
        self.inner.release_node(node_id.0);
    }
    
    /// Очистить кэш узлов
    pub fn clear_cache(&mut self) {
        self.inner.clear_cache();
    }
    
    /// Получить размер пула
    pub fn pool_size(&self) -> usize {
        self.inner.pool_size()
    }
    
    /// Изменить размер пула
    pub fn set_pool_size(&mut self, new_size: usize) {
        self.inner.set_pool_size(new_size);
    }
    
    /// Получить статистику использования
    pub fn stats(&self) -> BufferManagerStats {
        let stats = self.inner.stats();
        BufferManagerStats {
            active_nodes: stats.active_nodes,
            active_buffers: stats.active_buffers,
            total_memory_bytes: stats.total_memory_bytes,
            pool_size: stats.pool_size,
            pool_available: stats.pool_available,
        }
    }
    
    /// Установить стратегию пула
    pub fn set_pool_strategy(&mut self, strategy: PoolStrategy) {
        self.inner.set_pool_strategy(strategy);
    }
    
    /// Получить текущую стратегию пула
    pub fn pool_strategy(&self) -> PoolStrategy {
        self.inner.pool_strategy()
    }
}

impl Default for BufferManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Статистика использования менеджера буферов
#[derive(Debug, Clone, Copy)]
pub struct BufferManagerStats {
    /// Количество активных узлов
    pub active_nodes: usize,
    /// Количество активных буферов
    pub active_buffers: usize,
    /// Общий объём памяти (в байтах)
    pub total_memory_bytes: usize,
    /// Размер пула
    pub pool_size: usize,
    /// Доступно буферов в пуле
    pub pool_available: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use kama_core_traits::{AudioNode, AudioError, ParamValue, NodeMetadata, NodeTypeId};
    
    // Тестовый узел
    struct TestNode;
    
    impl AudioNode for TestNode {
        fn process(&mut self, _inputs: &[&[f32]], _outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
            Ok(())
        }
        
        fn get_param(&self, _name: &str) -> Option<ParamValue> { None }
        fn set_param(&mut self, _name: &str, _value: ParamValue) -> Result<(), AudioError> { Ok(()) }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn num_inputs(&self) -> usize { 1 }
        fn num_outputs(&self) -> usize { 1 }
        fn node_type_id(&self) -> NodeTypeId { NodeTypeId::of::<Self>() }
        fn metadata(&self) -> NodeMetadata { unimplemented!() }
    }
    
    #[test]
    fn test_buffer_manager_basic() {
        let mut manager = BufferManager::new();
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
    fn test_buffer_reuse() {
        let mut manager = BufferManager::with_pool_size(4, 256);
        let node_id = NodeId(1);
        
        {
            let buffers = manager.get_buffers(node_id, 1, 1, 256);
            assert_eq!(buffers.inputs.len(), 1);
        }
        
        manager.clear_cache();
        
        let buffers = manager.get_buffers(node_id, 1, 1, 256);
        assert_eq!(buffers.inputs.len(), 1);
    }
    
    #[test]
    fn test_multiple_nodes() {
        let mut manager = BufferManager::new();
        
        manager.get_buffers(NodeId(1), 1, 1, 128);
        manager.get_buffers(NodeId(2), 2, 2, 256);
        
        let stats = manager.stats();
        assert_eq!(stats.active_nodes, 2);
    }
    
    #[test]
    fn test_release_node() {
        let mut manager = BufferManager::new();
        
        manager.get_buffers(NodeId(1), 1, 1, 128);
        manager.get_buffers(NodeId(2), 1, 1, 128);
        
        assert_eq!(manager.stats().active_nodes, 2);
        
        manager.release_node(NodeId(1));
        
        assert_eq!(manager.stats().active_nodes, 1);
    }
    
    #[test]
    fn test_node_processor() {
        let processor = NodeProcessor::new();
        let mut node = TestNode;
        let mut buffers = NodeBuffers::default();
        
        // Подготавливаем буферы
        buffers.inputs.push(vec![1.0; 64]);
        buffers.outputs.push(vec![0.0; 64]);
        
        let result = processor.process(&mut node, &mut buffers);
        assert!(result.is_ok());
    }
}