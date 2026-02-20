//! Реестр именованных буферов

use std::collections::HashMap;
use std::sync::Arc;
use std::fmt;
use parking_lot::RwLock;

use crate::{RingBuffer, MultiHeadBuffer};

/// Тип зарегистрированного буфера
pub enum RegisteredBuffer {
    /// Кольцевой буфер
    Ring(Arc<RwLock<RingBuffer>>),
    
    /// Многоголовый буфер
    MultiHead(Arc<RwLock<MultiHeadBuffer>>),
    
    /// Простой вектор
    Vector(Arc<RwLock<Vec<f32>>>),
    
    /// Пользовательский буфер (для расширяемости)
    Custom(Arc<dyn std::any::Any + Send + Sync>),
}

// Ручная реализация Debug для RegisteredBuffer
impl fmt::Debug for RegisteredBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegisteredBuffer::Ring(_) => f.debug_tuple("Ring").finish(),
            RegisteredBuffer::MultiHead(_) => f.debug_tuple("MultiHead").finish(),
            RegisteredBuffer::Vector(_) => f.debug_tuple("Vector").finish(),
            RegisteredBuffer::Custom(_) => f.debug_tuple("Custom").finish(),
        }
    }
}

impl Clone for RegisteredBuffer {
    fn clone(&self) -> Self {
        match self {
            RegisteredBuffer::Ring(r) => RegisteredBuffer::Ring(r.clone()),
            RegisteredBuffer::MultiHead(m) => RegisteredBuffer::MultiHead(m.clone()),
            RegisteredBuffer::Vector(v) => RegisteredBuffer::Vector(v.clone()),
            RegisteredBuffer::Custom(c) => RegisteredBuffer::Custom(c.clone()),
        }
    }
}

impl RegisteredBuffer {
    /// Получить как кольцевой буфер
    pub fn as_ring(&self) -> Option<Arc<RwLock<RingBuffer>>> {
        match self {
            RegisteredBuffer::Ring(r) => Some(r.clone()),
            _ => None,
        }
    }
    
    /// Получить как многоголовый буфер
    pub fn as_multi_head(&self) -> Option<Arc<RwLock<MultiHeadBuffer>>> {
        match self {
            RegisteredBuffer::MultiHead(m) => Some(m.clone()),
            _ => None,
        }
    }
    
    /// Получить как вектор
    pub fn as_vector(&self) -> Option<Arc<RwLock<Vec<f32>>>> {
        match self {
            RegisteredBuffer::Vector(v) => Some(v.clone()),
            _ => None,
        }
    }
    
    /// Получить как пользовательский тип
    pub fn as_custom<T: 'static + Send + Sync>(&self) -> Option<Arc<T>> {
        match self {
            RegisteredBuffer::Custom(c) => c.clone().downcast::<T>().ok(),
            _ => None,
        }
    }
}

/// Реестр именованных буферов
#[derive(Default, Clone)]
pub struct BufferRegistry {
    buffers: Arc<RwLock<HashMap<String, RegisteredBuffer>>>,
}

// Ручная реализация Debug для BufferRegistry
impl fmt::Debug for BufferRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let buffers = self.buffers.read();
        f.debug_struct("BufferRegistry")
            .field("buffer_count", &buffers.len())
            .field("buffer_names", &buffers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl BufferRegistry {
    /// Создать новый реестр
    pub fn new() -> Self {
        Self {
            buffers: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Зарегистрировать кольцевой буфер
    pub fn register_ring(&self, name: &str, buffer: RingBuffer) {
        let mut buffers = self.buffers.write();
        buffers.insert(name.to_string(), RegisteredBuffer::Ring(Arc::new(RwLock::new(buffer))));
    }
    
    /// Зарегистрировать многоголовый буфер
    pub fn register_multi_head(&self, name: &str, buffer: MultiHeadBuffer) {
        let mut buffers = self.buffers.write();
        buffers.insert(name.to_string(), RegisteredBuffer::MultiHead(Arc::new(RwLock::new(buffer))));
    }
    
    /// Зарегистрировать вектор
    pub fn register_vector(&self, name: &str, buffer: Vec<f32>) {
        let mut buffers = self.buffers.write();
        buffers.insert(name.to_string(), RegisteredBuffer::Vector(Arc::new(RwLock::new(buffer))));
    }
    
    /// Зарегистрировать пользовательский буфер
    pub fn register_custom<T: Send + Sync + 'static>(&self, name: &str, buffer: T) {
        let mut buffers = self.buffers.write();
        buffers.insert(name.to_string(), RegisteredBuffer::Custom(Arc::new(buffer)));
    }
    
    /// Получить буфер по имени
    pub fn get(&self, name: &str) -> Option<RegisteredBuffer> {
        let buffers = self.buffers.read();
        buffers.get(name).cloned()
    }
    
    /// Получить кольцевой буфер по имени
    pub fn get_ring(&self, name: &str) -> Option<Arc<RwLock<RingBuffer>>> {
        self.get(name)?.as_ring()
    }
    
    /// Получить многоголовый буфер по имени
    pub fn get_multi_head(&self, name: &str) -> Option<Arc<RwLock<MultiHeadBuffer>>> {
        self.get(name)?.as_multi_head()
    }
    
    /// Получить вектор по имени
    pub fn get_vector(&self, name: &str) -> Option<Arc<RwLock<Vec<f32>>>> {
        self.get(name)?.as_vector()
    }
    
    /// Получить пользовательский буфер по имени
    pub fn get_custom<T: 'static + Send + Sync>(&self, name: &str) -> Option<Arc<T>> {
        self.get(name)?.as_custom::<T>()
    }
    
    /// Удалить буфер
    pub fn remove(&self, name: &str) -> bool {
        let mut buffers = self.buffers.write();
        buffers.remove(name).is_some()
    }
    
    /// Проверить наличие буфера
    pub fn contains(&self, name: &str) -> bool {
        let buffers = self.buffers.read();
        buffers.contains_key(name)
    }
    
    /// Получить список всех имен
    pub fn names(&self) -> Vec<String> {
        let buffers = self.buffers.read();
        buffers.keys().cloned().collect()
    }
    
    /// Получить количество зарегистрированных буферов
    pub fn len(&self) -> usize {
        let buffers = self.buffers.read();
        buffers.len()
    }
    
    /// Очистить реестр
    pub fn clear(&self) {
        let mut buffers = self.buffers.write();
        buffers.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RingBuffer, MultiHeadBuffer};
    
    #[test]
    fn test_registry_basic() {
        let registry = BufferRegistry::new();
        
        let ring = RingBuffer::new(1024);
        registry.register_ring("test_ring", ring);
        
        assert!(registry.contains("test_ring"));
        assert_eq!(registry.names().len(), 1);
        
        let retrieved = registry.get_ring("test_ring");
        assert!(retrieved.is_some());
        
        registry.remove("test_ring");
        assert!(!registry.contains("test_ring"));
    }
    
    #[test]
    fn test_registry_multi_head() {
        let registry = BufferRegistry::new();
        
        let multi = MultiHeadBuffer::new(2048, 44100.0);
        registry.register_multi_head("test_multi", multi);
        
        let retrieved = registry.get_multi_head("test_multi");
        assert!(retrieved.is_some());
    }
    
    #[test]
    fn test_registry_vector() {
        let registry = BufferRegistry::new();
        
        let vec = vec![0.0; 512];
        registry.register_vector("test_vec", vec);
        
        let retrieved = registry.get_vector("test_vec");
        assert!(retrieved.is_some());
    }
    
    #[test]
    fn test_registry_clear() {
        let registry = BufferRegistry::new();
        
        registry.register_ring("ring1", RingBuffer::new(128));
        registry.register_ring("ring2", RingBuffer::new(256));
        
        assert_eq!(registry.len(), 2);
        
        registry.clear();
        assert_eq!(registry.len(), 0);
    }
}