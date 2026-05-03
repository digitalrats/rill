//! # Buffer registry — поименованные буферы
//!
//! [`BufferRegistry`] — временный реестр на этапе сборки графа.
//! Каждый узел, использующий ресурсный буфер, получает указатель
//! на него через реестр во время `GraphBuilder::build()`.
//! После сборки реестр сохраняется в `SignalGraph` для управления
//! временем жизни буферов.

use std::collections::HashMap;

use super::Buffer;

/// Реестр поименованных буферов.
///
/// Используется в `GraphBuilder::build()` для аллокации ресурсов
/// и раздачи указателей узлам графа.
pub struct BufferRegistry<T> {
    buffers: HashMap<String, Box<dyn Buffer<T>>>,
}

impl<T> BufferRegistry<T> {
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
        }
    }

    /// Зарегистрировать именованный буфер.
    pub fn register(&mut self, name: impl Into<String>, buffer: Box<dyn Buffer<T>>) {
        self.buffers.insert(name.into(), buffer);
    }

    /// Получить сырой указатель на буфер по имени.
    /// Используется для раздачи указателей узлам.
    pub fn get_ptr(&self, name: &str) -> Option<*const dyn Buffer<T>> {
        self.buffers.get(name).map(|b| &**b as *const dyn Buffer<T>)
    }

    /// Количество зарегистрированных буферов.
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }
}

impl<T> Default for BufferRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::HeapBuffer;

    #[test]
    fn test_registry() {
        let mut reg = BufferRegistry::<f32>::new();
        reg.register("tape_0", Box::new(HeapBuffer::new(1024)));
        assert_eq!(reg.len(), 1);
        assert!(reg.get_ptr("tape_0").is_some());
        assert!(reg.get_ptr("nonexistent").is_none());
    }
}
