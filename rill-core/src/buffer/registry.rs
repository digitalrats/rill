//! # Buffer registry — named buffers
//!
//! [`BufferRegistry`] — a temporary registry used during graph assembly.
//! Each node that requires a resource buffer receives a pointer through
//! the registry during `GraphBuilder::build()`.
//! After assembly the registry is retained in `Graph` to manage
//! buffer lifetimes.

use std::collections::HashMap;

use super::Buffer;

/// Registry of named buffers.
///
/// Used in `GraphBuilder::build()` to allocate resources and distribute
/// pointers to graph nodes.
pub struct BufferRegistry<T> {
    buffers: HashMap<String, Box<dyn Buffer<T>>>,
}

impl<T> BufferRegistry<T> {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
        }
    }

    /// Register a named buffer.
    pub fn register(&mut self, name: impl Into<String>, buffer: Box<dyn Buffer<T>>) {
        self.buffers.insert(name.into(), buffer);
    }

    /// Get a raw pointer to a buffer by name.
    ///
    /// Used to distribute pointers to graph nodes during build.
    pub fn get_ptr(&self, name: &str) -> Option<*const dyn Buffer<T>> {
        self.buffers.get(name).map(|b| &**b as *const dyn Buffer<T>)
    }

    /// Take ownership of a buffer by name, removing it from the registry.
    pub fn take(&mut self, name: &str) -> Option<Box<dyn Buffer<T>>> {
        self.buffers.remove(name)
    }

    /// Leak a buffer by name, returning a raw mutable pointer.
    /// The leaked buffer will live for the remainder of the program
    /// (or until manually re‑boxed and dropped).
    pub fn leak(&mut self, name: &str) -> Option<*mut dyn Buffer<T>> {
        self.buffers.remove(name).map(Box::into_raw)
    }

    /// Consume the registry and return all owned buffers.
    pub fn into_inner(self) -> Vec<Box<dyn Buffer<T>>> {
        self.buffers.into_values().collect()
    }

    /// Number of registered buffers.
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    /// Whether no buffers are registered.
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
