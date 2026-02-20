//! Единый менеджер буферов
//!
//! Пул буферов владеет всеми буферами. Буферы выдаются через acquire и должны
//! возвращаться через release. Реестр хранит ссылки на буферы, которые были
//! выданы и зарегистрированы под именами.

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use parking_lot::{RwLock, RwLockReadGuard};

use kama_core_traits::NodeId;

use crate::{
    BufferError, BufferResult, PoolStrategy,
    RingBuffer, MultiHeadBuffer,
    BufferView, BufferViewMut,
};

// -----------------------------------------------------------------------------
// BufferPool - владеет буферами
// -----------------------------------------------------------------------------

/// Пул буферов для повторного использования
#[derive(Debug)]
struct BufferPool {
    buffers: Vec<Vec<f32>>,
    size: usize,
    strategy: PoolStrategy,
    max_size: usize,
}

impl BufferPool {
    fn new(max_size: usize, buffer_size: usize, strategy: PoolStrategy) -> Self {
        let mut buffers = Vec::with_capacity(max_size);
        for _ in 0..max_size {
            buffers.push(vec![0.0; buffer_size]);
        }
        
        Self {
            buffers,
            size: buffer_size,
            strategy,
            max_size,
        }
    }
    
    /// Получить буфер из пула
    fn acquire(&mut self) -> BufferResult<Vec<f32>> {
        self.buffers.pop().ok_or(BufferError::PoolEmpty)
    }
    
    /// Получить буфер указанного размера
    fn acquire_with_size(&mut self, size: usize) -> BufferResult<Vec<f32>> {
        if size == self.size {
            return self.acquire();
        }
        
        match self.strategy {
            PoolStrategy::Error => Err(BufferError::SizeMismatch {
                expected: self.size,
                got: size,
            }),
            
            PoolStrategy::Resize => {
                match self.acquire() {
                    Ok(mut buffer) => {
                        buffer.resize(size, 0.0);
                        Ok(buffer)
                    }
                    Err(e) => Err(e),
                }
            }
            
            PoolStrategy::CreateNew => {
                Ok(vec![0.0; size])
            }
            
            PoolStrategy::ResizePool => {
                self.size = size;
                self.buffers.clear();
                Ok(vec![0.0; size])
            }
        }
    }
    
    /// Вернуть буфер в пул
    fn release(&mut self, mut buffer: Vec<f32>) -> BufferResult<()> {
        if self.buffers.len() >= self.max_size {
            return Ok(());
        }
        
        if buffer.len() != self.size {
            match self.strategy {
                PoolStrategy::Error => Err(BufferError::SizeMismatch {
                    expected: self.size,
                    got: buffer.len(),
                }),
                
                PoolStrategy::Resize => {
                    buffer.resize(self.size, 0.0);
                    buffer.fill(0.0);
                    self.buffers.push(buffer);
                    Ok(())
                }
                
                PoolStrategy::CreateNew => Ok(()),
                
                PoolStrategy::ResizePool => {
                    self.size = buffer.len();
                    self.buffers.clear();
                    buffer.fill(0.0);
                    self.buffers.push(buffer);
                    Ok(())
                }
            }
        } else {
            buffer.fill(0.0);
            self.buffers.push(buffer);
            Ok(())
        }
    }
    
    fn available(&self) -> usize {
        self.buffers.len()
    }
    
    fn current_size(&self) -> usize {
        self.size
    }
    
    fn clear(&mut self) {
        self.buffers.clear();
    }
}

// -----------------------------------------------------------------------------
// BufferHandle - умный указатель на буфер из пула
// -----------------------------------------------------------------------------

/// Буфер, полученный из пула. При Drop автоматически возвращается в пул.
pub struct PooledBuffer {
    data: Vec<f32>,
    pool: Weak<RwLock<BufferPool>>,
}

impl PooledBuffer {
    /// Создать новый PooledBuffer
    fn new(data: Vec<f32>, pool: &Arc<RwLock<BufferPool>>) -> Self {
        Self {
            data,
            pool: Arc::downgrade(pool),
        }
    }
    
    /// Получить доступ к данным
    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }
    
    /// Получить мутабельный доступ к данным
    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        &mut self.data
    }
    
    /// Получить длину буфера
    pub fn len(&self) -> usize {
        self.data.len()
    }
    
    /// Преобразовать в Vec (забирает владение, не возвращает в пул)
    pub fn into_vec(mut self) -> Vec<f32> {
        std::mem::take(&mut self.data)
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.upgrade() {
            let mut pool = pool.write();
            let mut data = std::mem::take(&mut self.data);
            data.fill(0.0);
            let _ = pool.release(data);
        }
    }
}

/// Тип зарегистрированного буфера (пул все еще владеет данными)
#[derive(Clone)]
pub enum RegisteredBuffer {
    /// Простой вектор (ссылка на буфер из пула)
    Vector(Arc<RwLock<Vec<f32>>>),
    
    /// Кольцевой буфер (содержит свой буфер, но может быть адаптирован)
    Ring(Arc<RwLock<RingBuffer>>),
    
    /// Многоголовый буфер
    MultiHead(Arc<RwLock<MultiHeadBuffer>>),
}

impl RegisteredBuffer {
    pub fn size(&self) -> usize {
        match self {
            RegisteredBuffer::Vector(v) => v.read().len(),
            RegisteredBuffer::Ring(r) => r.read().size(),
            RegisteredBuffer::MultiHead(m) => m.read().buffer_size(),
        }
    }
}

// -----------------------------------------------------------------------------
// Основные типы
// -----------------------------------------------------------------------------

/// Временные буферы для обработки узла
#[derive(Debug, Default, Clone)]
pub struct NodeBuffers {
    pub inputs: Vec<Vec<f32>>,
    pub outputs: Vec<Vec<f32>>,
}

/// Статистика использования менеджера буферов
#[derive(Debug, Clone, Copy)]
pub struct BufferManagerStats {
    pub active_nodes: usize,
    pub active_buffers: usize,
    pub total_memory_bytes: usize,
    pub pool_size: usize,
    pub pool_available: usize,
    pub registered_buffers: usize,
}

// -----------------------------------------------------------------------------
// Основной менеджер
// -----------------------------------------------------------------------------

/// Единый менеджер буферов - пул владеет буферами, выдача через acquire
pub struct BufferManager {
    // Пул буферов (владелец всех данных)
    pool: Arc<RwLock<BufferPool>>,
    
    // Реестр именованных буферов (ссылки на буферы из пула)
    registry: Arc<RwLock<HashMap<String, RegisteredBuffer>>>,
    
    // Кэш буферов для узлов графа
    node_buffers: Arc<RwLock<HashMap<NodeId, NodeBuffers>>>,
    
    // Конфигурация
    max_pool_size: usize,
    default_size: usize,
}

impl BufferManager {
    /// Создать новый менеджер
    pub fn new() -> Self {
        Self::with_config(16, 4096)
    }
    
    /// Создать с указанными параметрами
    pub fn with_config(max_pool_size: usize, default_buffer_size: usize) -> Self {
        let pool = BufferPool::new(max_pool_size, default_buffer_size, PoolStrategy::Resize);
        
        Self {
            pool: Arc::new(RwLock::new(pool)),
            registry: Arc::new(RwLock::new(HashMap::new())),
            node_buffers: Arc::new(RwLock::new(HashMap::new())),
            max_pool_size,
            default_size: default_buffer_size,
        }
    }
    
    // -------------------------------------------------------------------------
    // Core acquire-release API
    // -------------------------------------------------------------------------
    
    /// Получить буфер из пула
    pub fn acquire(&self, size: usize) -> BufferResult<PooledBuffer> {
        let data = self.pool.write().acquire_with_size(size)?;
        Ok(PooledBuffer::new(data, &self.pool))
    }
    
    /// Получить буфер и сразу зарегистрировать его под именем
    pub fn acquire_named(&self, name: &str, size: usize) -> BufferResult<Arc<RwLock<Vec<f32>>>> {
        let data = self.pool.write().acquire_with_size(size)?;
        let arc_buffer = Arc::new(RwLock::new(data));
        
        let mut registry = self.registry.write();
        registry.insert(name.to_string(), RegisteredBuffer::Vector(arc_buffer.clone()));
        
        Ok(arc_buffer)
    }
    
    /// Создать кольцевой буфер (использует свой внутренний пул)
    pub fn create_ring(&self, name: &str, size: usize) -> Arc<RwLock<RingBuffer>> {
        let buffer = RingBuffer::new(size);
        let arc_buffer = Arc::new(RwLock::new(buffer));
        
        let mut registry = self.registry.write();
        registry.insert(name.to_string(), RegisteredBuffer::Ring(arc_buffer.clone()));
        
        arc_buffer
    }
    
    /// Создать многоголовый буфер
    pub fn create_multi_head(&self, name: &str, size: usize, sample_rate: f32) -> Arc<RwLock<MultiHeadBuffer>> {
        let buffer = MultiHeadBuffer::new(size, sample_rate);
        let arc_buffer = Arc::new(RwLock::new(buffer));
        
        let mut registry = self.registry.write();
        registry.insert(name.to_string(), RegisteredBuffer::MultiHead(arc_buffer.clone()));
        
        arc_buffer
    }
    
    // -------------------------------------------------------------------------
    // Доступ к зарегистрированным буферам
    // -------------------------------------------------------------------------
    
    /// Получить зарегистрированный буфер по имени
    pub fn get(&self, name: &str) -> Option<RegisteredBuffer> {
        let registry = self.registry.read();
        registry.get(name).cloned()
    }
    
    /// Получить вектор по имени
    pub fn get_vector(&self, name: &str) -> Option<Arc<RwLock<Vec<f32>>>> {
        match self.get(name) {
            Some(RegisteredBuffer::Vector(v)) => Some(v),
            _ => None,
        }
    }
    
    /// Получить кольцевой буфер по имени
    pub fn get_ring(&self, name: &str) -> Option<Arc<RwLock<RingBuffer>>> {
        match self.get(name) {
            Some(RegisteredBuffer::Ring(r)) => Some(r),
            _ => None,
        }
    }
    
    /// Получить многоголовый буфер по имени
    pub fn get_multi_head(&self, name: &str) -> Option<Arc<RwLock<MultiHeadBuffer>>> {
        match self.get(name) {
            Some(RegisteredBuffer::MultiHead(m)) => Some(m),
            _ => None,
        }
    }
    
    /// Проверить наличие буфера в реестре
    pub fn contains(&self, name: &str) -> bool {
        self.registry.read().contains_key(name)
    }
    
    /// Удалить буфер из реестра (НЕ возвращает в пул - буфер может быть еще использован)
    pub fn unregister(&self, name: &str) -> bool {
        self.registry.write().remove(name).is_some()
    }
    

/// Удалить буфер из реестра и вернуть в пул (даже если есть другие ссылки)
pub fn unregister_and_release(&self, name: &str) -> bool {
    let mut registry = self.registry.write();
    if let Some(RegisteredBuffer::Vector(arc)) = registry.remove(name) {
        // Пытаемся получить уникальное владение
        match Arc::try_unwrap(arc) {
            Ok(vec_lock) => {
                let vec = vec_lock.into_inner();
                let _ = self.pool.write().release(vec);
                true
            }
            Err(arc) => {
                // Есть другие ссылки - не можем вернуть в пул
                // Возвращаем false, чтобы показать, что не удалось
                false
            }
        }
    } else {
        false
    }
}
    
    // -------------------------------------------------------------------------
    // API для работы с буферами узлов графа
    // -------------------------------------------------------------------------
    
    /// Получить буферы для узла (автоматически берутся из пула)
    pub fn with_buffers_mut<F, R>(&self, node_id: NodeId, num_inputs: usize, num_outputs: usize, buffer_size: usize, f: F) -> R
    where
        F: FnOnce(&mut NodeBuffers) -> R,
    {
        let mut guard = self.node_buffers.write();
        
        let needs_update = if let Some(buffers) = guard.get(&node_id) {
            buffers.inputs.len() != num_inputs ||
            buffers.outputs.len() != num_outputs ||
            buffers.inputs.iter().any(|b| b.len() != buffer_size) ||
            buffers.outputs.iter().any(|b| b.len() != buffer_size)
        } else {
            true
        };
        
        if needs_update {
            let buffers = self.create_node_buffers(num_inputs, num_outputs, buffer_size);
            guard.insert(node_id, buffers);
        }
        
        f(guard.get_mut(&node_id).unwrap())
    }
    
    /// Создать буферы для узла (берутся из пула)
    fn create_node_buffers(&self, num_inputs: usize, num_outputs: usize, buffer_size: usize) -> NodeBuffers {
        let mut inputs = Vec::with_capacity(num_inputs);
        let mut outputs = Vec::with_capacity(num_outputs);
        
        let mut pool = self.pool.write();
        
        for _ in 0..num_inputs {
            inputs.push(pool.acquire_with_size(buffer_size).unwrap_or_else(|_| vec![0.0; buffer_size]));
        }
        for _ in 0..num_outputs {
            outputs.push(pool.acquire_with_size(buffer_size).unwrap_or_else(|_| vec![0.0; buffer_size]));
        }
        
        NodeBuffers { inputs, outputs }
    }
    
    /// Освободить буферы узла (возвращаются в пул)
    pub fn release_node(&self, node_id: NodeId) {
        let mut guard = self.node_buffers.write();
        if let Some(buffers) = guard.remove(&node_id) {
            let mut pool = self.pool.write();
            for buf in buffers.inputs {
                let _ = pool.release(buf);
            }
            for buf in buffers.outputs {
                let _ = pool.release(buf);
            }
        }
    }
    
    // -------------------------------------------------------------------------
    // Управление и статистика
    // -------------------------------------------------------------------------
    
    /// Получить статистику
    pub fn stats(&self) -> BufferManagerStats {
        let node_buffers = self.node_buffers.read();
        let pool = self.pool.read();
        let registry = self.registry.read();
        
        let mut total_buffers = 0;
        let mut total_memory = 0;
        
        for buffers in node_buffers.values() {
            total_buffers += buffers.inputs.len() + buffers.outputs.len();
            for buf in &buffers.inputs {
                total_memory += buf.len() * std::mem::size_of::<f32>();
            }
            for buf in &buffers.outputs {
                total_memory += buf.len() * std::mem::size_of::<f32>();
            }
        }
        
        BufferManagerStats {
            active_nodes: node_buffers.len(),
            active_buffers: total_buffers,
            total_memory_bytes: total_memory,
            pool_size: pool.current_size(),
            pool_available: pool.available(),
            registered_buffers: registry.len(),
        }
    }
    
    /// Очистить всё (возвращает все буферы в пул)
    pub fn clear_all(&self) {
        // Очищаем кэш узлов (буферы возвращаются в пул)
        let mut node_guard = self.node_buffers.write();
        let mut pool = self.pool.write();
        
        for (_, buffers) in node_guard.drain() {
            for buf in buffers.inputs {
                let _ = pool.release(buf);
            }
            for buf in buffers.outputs {
                let _ = pool.release(buf);
            }
        }
        
        // Очищаем реестр (пытаемся вернуть векторы в пул)
        let mut registry = self.registry.write();
        for (_, handle) in registry.drain() {
            if let RegisteredBuffer::Vector(arc) = handle {
                if let Ok(vec) = Arc::try_unwrap(arc) {
                    let vec = vec.into_inner();
                    let _ = pool.release(vec);
                }
            }
        }
        
        // Переинициализируем пул до начального состояния
        pool.clear();
        for _ in 0..self.max_pool_size {
            let _ = pool.release(vec![0.0; self.default_size]);
        }
    }

    /// Получить временный буфер из пула с автоматическим возвратом
    pub fn acquire_pooled(&self, size: usize) -> BufferResult<PooledBuffer> {
        let data = self.pool.write().acquire_with_size(size)?;
        Ok(PooledBuffer::new(data, &self.pool))
    }

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
    fn test_acquire_release() {
        let manager = BufferManager::new();
        let initial_available = manager.stats().pool_available;
        
        // Acquire
        let buffer = manager.acquire(256).unwrap();
        assert_eq!(buffer.len(), 256);
        assert_eq!(manager.stats().pool_available, initial_available - 1);
        
        // Release (при drop)
        drop(buffer);
        assert_eq!(manager.stats().pool_available, initial_available);
    }
    
fn test_acquire_named() {
    let manager = BufferManager::new();
    let initial_available = manager.stats().pool_available;
    
    // Acquire and register
    let buffer = manager.acquire_named("test", 256).unwrap();
    assert_eq!(buffer.read().len(), 256);
    
    // После acquire_named пул должен уменьшиться на 1
    assert_eq!(manager.stats().pool_available, initial_available - 1);
    assert_eq!(manager.stats().registered_buffers, 1);
    
    // Get by name
    let retrieved = manager.get_vector("test").unwrap();
    assert_eq!(retrieved.read().len(), 256);
    
    // Unregister (буфер еще жив, потому что у нас есть buffer)
    assert!(manager.unregister("test"));
    assert!(!manager.contains("test"));
    assert_eq!(manager.stats().registered_buffers, 0);
    
    // После unregister пул все еще уменьшен на 1, потому что buffer еще жив
    assert_eq!(manager.stats().pool_available, initial_available - 1);
    
    // Release buffer
    drop(buffer);
    
    // После drop буфер должен вернуться в пул
    assert_eq!(manager.stats().pool_available, initial_available);
}

    #[test]
    fn test_node_buffers() {
        let manager = BufferManager::new();
        let initial_available = manager.stats().pool_available;
        let node_id = NodeId(42);
        
        // Берем буферы для узла
        manager.with_buffers_mut(node_id, 2, 2, 256, |buffers| {
            assert_eq!(buffers.inputs.len(), 2);
            assert_eq!(buffers.outputs.len(), 2);
            buffers.inputs[0][0] = 1.0;
        });
        
        assert_eq!(manager.stats().active_nodes, 1);
        assert_eq!(manager.stats().active_buffers, 4);
        assert_eq!(manager.stats().pool_available, initial_available - 4);
        
        // Освобождаем
        manager.release_node(node_id);
        assert_eq!(manager.stats().active_nodes, 0);
        assert_eq!(manager.stats().pool_available, initial_available);
    }
    
    #[test]
    fn test_stats() {
        let manager = BufferManager::new();
        let initial_available = manager.stats().pool_available;
        
        let _buf1 = manager.acquire_named("buf1", 256).unwrap();
        let _buf2 = manager.acquire_named("buf2", 256).unwrap();
        
        let stats = manager.stats();
        assert_eq!(stats.registered_buffers, 2);
        assert_eq!(stats.pool_available, initial_available - 2);
    }
    
    #[test]
    fn test_clear_all() {
        let manager = BufferManager::new();
        let initial_available = manager.stats().pool_available;
        
        let _buf1 = manager.acquire_named("buf1", 256).unwrap();
        let _buf2 = manager.acquire_named("buf2", 256).unwrap();
        
        let node_id = NodeId(1);
        manager.with_buffers_mut(node_id, 1, 1, 128, |_| {});
        
        assert_eq!(manager.stats().registered_buffers, 2);
        assert_eq!(manager.stats().active_nodes, 1);
        
        manager.clear_all();
        
        assert_eq!(manager.stats().registered_buffers, 0);
        assert_eq!(manager.stats().active_nodes, 0);
        assert_eq!(manager.stats().pool_available, initial_available);
    }
}