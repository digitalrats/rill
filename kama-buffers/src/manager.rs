use std::collections::HashMap;
use crate::{BufferPool, AudioBuffer, BufferPoolError, PoolStrategy};

/// Идентификатор узла (для совместимости с графом)
pub type NodeId = u32;

/// Временные буферы для обработки узла
#[derive(Default)]
pub struct NodeBuffers {
    pub inputs: Vec<AudioBuffer>,
    pub outputs: Vec<AudioBuffer>,
}

/// Менеджер буферов для графа
pub struct BufferManager {
    node_buffers: HashMap<NodeId, NodeBuffers>,
    buffer_pool: BufferPool,
    pool_size: usize,
}

impl BufferManager {
    pub fn new() -> Self {
        Self {
            node_buffers: HashMap::new(),
            buffer_pool: BufferPool::with_strategy(16, 4096, PoolStrategy::Resize),
            pool_size: 4096,
        }
    }
    
    pub fn with_pool_size(pool_size: usize, buffer_size: usize) -> Self {
        Self {
            node_buffers: HashMap::new(),
            buffer_pool: BufferPool::with_strategy(pool_size, buffer_size, PoolStrategy::Resize),
            pool_size: buffer_size,
        }
    }
    
    pub fn get_buffers(&mut self, node_id: NodeId, num_inputs: usize, num_outputs: usize, buffer_size: usize) -> &mut NodeBuffers {
        // Обновляем размер пула если нужно
        if buffer_size > self.pool_size {
            self.pool_size = buffer_size;
            self.buffer_pool.resize(buffer_size);
        }
        
        // Извлекаем буферы из node_buffers, полностью убирая запись
        let (mut inputs, mut outputs) = if let Some(mut entry) = self.node_buffers.remove(&node_id) {
            (entry.inputs, entry.outputs)
        } else {
            (Vec::new(), Vec::new())
        };
        
        // Обновляем буферы (здесь self заимствуется, но node_buffers временно не содержит эту запись)
        self.ensure_buffers(&mut inputs, num_inputs, buffer_size);
        self.ensure_buffers(&mut outputs, num_outputs, buffer_size);
        
        // Создаём новую запись с обновлёнными буферами
        let node_entry = self.node_buffers.entry(node_id).or_insert_with(NodeBuffers::default);
        node_entry.inputs = inputs;
        node_entry.outputs = outputs;
        
        node_entry
    }
    
    fn ensure_buffers(&mut self, buffers: &mut Vec<AudioBuffer>, count: usize, size: usize) {
        // Освобождаем лишние буферы
        while buffers.len() > count {
            if let Some(buffer) = buffers.pop() {
                let _ = self.buffer_pool.release(buffer);
            }
        }
        
        // Добавляем недостающие буферы
        while buffers.len() < count {
            match self.buffer_pool.acquire_with_size(size) {
                Ok(mut buffer) => {
                    if buffer.len() != size {
                        buffer.resize(size, 0.0);
                    }
                    buffers.push(buffer);
                }
                Err(BufferPoolError::PoolEmpty) => {
                    // Если пул пуст, создаём новый буфер
                    buffers.push(vec![0.0; size]);
                }
                Err(e) => {
                    // Для других ошибок создаём новый буфер
                    eprintln!("Buffer pool warning: {}, creating new buffer", e);
                    buffers.push(vec![0.0; size]);
                }
            }
        }
        
        // Обнуляем буферы
        for buffer in buffers.iter_mut() {
            if buffer.len() != size {
                buffer.resize(size, 0.0);
            } else {
                buffer.fill(0.0);
            }
        }
    }
    
    /// Освободить все буферы (очистить кэш узлов и вернуть буферы в пул)
    pub fn release_all(&mut self) {
        let mut all_buffers = Vec::new();
        
        // Собираем все буферы из всех узлов
        for buffers in self.node_buffers.values_mut() {
            all_buffers.append(&mut buffers.inputs);
            all_buffers.append(&mut buffers.outputs);
        }
        
        // Возвращаем их в пул
        for buffer in all_buffers {
            let _ = self.buffer_pool.release(buffer);
        }
        
        self.node_buffers.clear();
    }
    
    /// Освободить буферы конкретного узла
    pub fn release_node(&mut self, node_id: NodeId) {
        if let Some(buffers) = self.node_buffers.remove(&node_id) {
            for buffer in buffers.inputs {
                let _ = self.buffer_pool.release(buffer);
            }
            for buffer in buffers.outputs {
                let _ = self.buffer_pool.release(buffer);
            }
        }
    }
    
    /// Очистить кэш узлов (без возврата в пул)
    pub fn clear_cache(&mut self) {
        self.node_buffers.clear();
    }
    
    /// Получить размер пула
    pub fn pool_size(&self) -> usize {
        self.pool_size
    }
    
    /// Изменить размер пула
    pub fn set_pool_size(&mut self, new_size: usize) {
        if new_size != self.pool_size {
            self.pool_size = new_size;
            self.buffer_pool.resize(new_size);
            self.clear_cache();
        }
    }
    
    /// Получить статистику использования
    pub fn stats(&self) -> BufferManagerStats {
        let mut total_buffers = 0;
        let mut total_memory = 0;
        
        for buffers in self.node_buffers.values() {
            total_buffers += buffers.inputs.len() + buffers.outputs.len();
            for buf in &buffers.inputs {
                total_memory += buf.len() * std::mem::size_of::<f32>();
            }
            for buf in &buffers.outputs {
                total_memory += buf.len() * std::mem::size_of::<f32>();
            }
        }
        
        BufferManagerStats {
            active_nodes: self.node_buffers.len(),
            active_buffers: total_buffers,
            total_memory_bytes: total_memory,
            pool_size: self.pool_size,
            pool_available: self.buffer_pool.available(),
        }
    }
    
    /// Установить стратегию пула
    pub fn set_pool_strategy(&mut self, strategy: PoolStrategy) {
        self.buffer_pool.set_strategy(strategy);
    }
    
    /// Получить текущую стратегию пула
    pub fn pool_strategy(&self) -> PoolStrategy {
        self.buffer_pool.strategy()
    }
}

/// Статистика использования менеджера буферов
#[derive(Debug, Clone, Copy)]
pub struct BufferManagerStats {
    pub active_nodes: usize,
    pub active_buffers: usize,
    pub total_memory_bytes: usize,
    pub pool_size: usize,
    pub pool_available: usize,
}

impl Default for BufferManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_buffer_manager_basic() {
        let mut manager = BufferManager::new();
        let node_id = 1;
        
        let buffers = manager.get_buffers(node_id, 2, 2, 256);
        assert_eq!(buffers.inputs.len(), 2);
        assert_eq!(buffers.outputs.len(), 2);
        assert_eq!(buffers.inputs[0].len(), 256);
        
        let stats = manager.stats();
        assert_eq!(stats.active_nodes, 1);
        assert_eq!(stats.active_buffers, 4);
    }
    
    #[test]
    fn test_release_all() {
        let mut manager = BufferManager::new();
        
        manager.get_buffers(1, 1, 1, 128);
        manager.get_buffers(2, 1, 1, 128);
        
        assert_eq!(manager.stats().active_nodes, 2);
        
        manager.release_all();
        
        assert_eq!(manager.stats().active_nodes, 0);
    }
    
    #[test]
    fn test_release_node() {
        let mut manager = BufferManager::new();
        
        manager.get_buffers(1, 1, 1, 128);
        manager.get_buffers(2, 1, 1, 128);
        
        assert_eq!(manager.stats().active_nodes, 2);
        
        manager.release_node(1);
        
        assert_eq!(manager.stats().active_nodes, 1);
    }
}