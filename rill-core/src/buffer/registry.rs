//! # Resource registry — named shared resources
//!
//! [`ResourceRegistry`] — a build-time registry that owns named resources
//! (currently [`TapeLoop`](super::TapeLoop)s) and hands out capability handles
//! to graph nodes during `GraphBuilder::build()`.
//!
//! Each registered tape yields a [`TapeWriter`] (unique) and a [`TapeReader`]
//! (shared, cloneable). Nodes acquire the capability matching their role. The
//! handles keep the underlying resource alive via reference counting, so the
//! registry itself is only needed during assembly — it is dropped once every
//! node has resolved its resources.

use std::collections::HashMap;

use super::tape::{tape_handles, TapeReader, TapeWriter};
use super::TapeLoop;

/// Registry of named shared resources.
///
/// Used in `GraphBuilder::build()` to allocate resources and distribute
/// capability handles to graph nodes.
pub struct ResourceRegistry<T> {
    /// Unique writer handles, removed on first acquisition (single-writer).
    writers: HashMap<String, TapeWriter<T>>,
    /// Reader handles, cloned on each acquisition (many read taps).
    readers: HashMap<String, TapeReader<T>>,
}

impl<T> ResourceRegistry<T> {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            writers: HashMap::new(),
            readers: HashMap::new(),
        }
    }

    /// Register a named tape loop, creating its writer/reader capability pair.
    pub fn register_tape(&mut self, name: impl Into<String>, tape: TapeLoop<T>) {
        let name = name.into();
        let (writer, reader) = tape_handles(tape);
        self.writers.insert(name.clone(), writer);
        self.readers.insert(name, reader);
    }

    /// Acquire a read capability for the named tape (cloneable, many taps).
    pub fn reader(&self, name: &str) -> Option<TapeReader<T>> {
        self.readers.get(name).cloned()
    }

    /// Acquire the unique write capability for the named tape.
    ///
    /// Returns `Some` only on the first call for a given name; subsequent
    /// calls return `None`. This enforces the single-writer invariant.
    pub fn writer(&mut self, name: &str) -> Option<TapeWriter<T>> {
        self.writers.remove(name)
    }

    /// Number of registered resources.
    pub fn len(&self) -> usize {
        self.readers.len()
    }

    /// Whether no resources are registered.
    pub fn is_empty(&self) -> bool {
        self.readers.is_empty()
    }
}

impl<T> Default for ResourceRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_writer_is_unique() {
        let mut reg = ResourceRegistry::<f32>::new();
        reg.register_tape("tape_0", TapeLoop::new(1024).unwrap());
        assert_eq!(reg.len(), 1);
        assert!(reg.reader("tape_0").is_some());
        assert!(reg.reader("nonexistent").is_none());

        // First writer succeeds, second is denied (single-writer invariant).
        assert!(reg.writer("tape_0").is_some());
        assert!(reg.writer("tape_0").is_none());
    }

    #[test]
    fn test_registry_reader_writer_share_tape() {
        let mut reg = ResourceRegistry::<f32>::new();
        reg.register_tape("t", TapeLoop::new(64).unwrap());
        let mut writer = reg.writer("t").unwrap();
        let reader = reg.reader("t").unwrap();
        writer.write(1.0);
        writer.write(2.0);
        assert_eq!(reader.read(0), 2.0);
        assert_eq!(reader.read(1), 1.0);
    }
}
