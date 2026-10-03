//! # Shared-buffer resources — generic Reader/Writer capabilities
//!
//! A **resource** is a named shared buffer (e.g. a tape loop) handed to
//! resource-backed builtins at construction. Buffers are **single-threaded** —
//! they are never shared across threads. Interaction with a buffer goes only
//! through the [`Reader`] / [`Writer`] capability wrappers, which split the
//! full [`DelayBuffer`] into a read side (cloneable, many consumers) and a
//! write side (unique, one producer).
//!
//! The mechanism is generic: the registry holds any `Box<dyn DelayBuffer>`, and
//! builtins receive `SharedReader`/`SharedWriter` handles via the traits — it is
//! not tied to a particular buffer type.

use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::math::Transcendental;

// ============================================================================
// Capability traits
// ============================================================================

/// Read capability over a shared delay buffer (single-threaded).
///
/// Implemented by buffer-specific reader handles; consumers read only through
/// this trait, so they cannot mutate the buffer.
pub trait Reader<T: Transcendental> {
    /// Read a whole block at `delay` samples behind the write position.
    fn read_block(&self, delay: usize, out: &mut [T]);
    /// Read a single sample at a fractional `delay` with interpolation.
    fn read_interpolated(&self, delay: f64) -> T;
}

/// Write capability over a shared delay buffer (single-threaded).
///
/// Implemented by a unique writer handle; the single-writer invariant is
/// encoded by construction (one handle per named buffer).
pub trait Writer<T: Transcendental> {
    /// Write a single sample and advance the write position.
    fn write(&mut self, sample: T);
    /// Write a whole block and advance the write position.
    fn write_block(&mut self, block: &[T]);
}

/// Full capabilities of a delay buffer (read + write + lifecycle).
///
/// The concrete buffer (e.g. [`TapeLoop`](crate::buffer::TapeLoop)) lives
/// inside a shared cell; the registry splits it into a [`Reader`] and a
/// [`Writer`] for builtins.
pub trait DelayBuffer<T: Transcendental>: 'static {
    /// Maximum capacity in samples.
    fn capacity(&self) -> usize;
    /// Write a single sample and advance the write position.
    fn write(&mut self, sample: T);
    /// Write a whole block.
    fn write_block(&mut self, block: &[T]);
    /// Read a single sample at a fractional `delay` with interpolation.
    fn read_interpolated(&self, delay: f64) -> T;
    /// Read a whole block at `delay` samples behind the write position.
    fn read_block(&self, delay: usize, out: &mut [T]);
    /// Reset the buffer to zeros.
    fn clear(&mut self);
}

// ============================================================================
// Shared cell + capability wrappers
// ============================================================================

/// Shared, single-threaded cell holding a boxed [`DelayBuffer`].
///
/// Lives on the signal thread; the graph is single-threaded, so writer and
/// readers never overlap (nodes run sequentially in topological order).
///
/// A program owns one cell per tape (`Vec<SharedCell<T>>`); heads receive
/// [`SharedWriter`]/[`SharedReader`] handles cloned from the same cell, so a
/// write head and its read heads share one buffer. The caller is responsible
/// for the single-writer convention (at most one active writer per cell).
pub struct SharedCell<T: Transcendental> {
    inner: Rc<UnsafeCell<Box<dyn DelayBuffer<T>>>>,
}

impl<T: Transcendental> Clone for SharedCell<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T: Transcendental> SharedCell<T> {
    /// Wrap a buffer into a shared cell. Use [`shared_handles`] to split it
    /// into a writer/reader pair directly.
    pub fn new(buffer: Box<dyn DelayBuffer<T>>) -> Self {
        Self {
            inner: Rc::new(UnsafeCell::new(buffer)),
        }
    }

    /// A write handle over this cell. Multiple calls return independent
    /// handles to the same buffer; the single-writer convention is the
    /// caller's responsibility.
    pub fn writer(&self) -> SharedWriter<T> {
        SharedWriter { cell: self.clone() }
    }

    /// A read handle over this cell. Cloneable — one per reader.
    pub fn reader(&self) -> SharedReader<T> {
        SharedReader { cell: self.clone() }
    }

    /// Maximum capacity in samples.
    #[allow(unsafe_code)]
    pub fn capacity(&self) -> usize {
        unsafe { &*self.inner.get() }.capacity()
    }
}

/// Unique write handle over a shared [`DelayBuffer`]. Not `Clone`.
pub struct SharedWriter<T: Transcendental> {
    cell: SharedCell<T>,
}

/// Shared read handle over a [`DelayBuffer`]. `Clone` — one per reader.
pub struct SharedReader<T: Transcendental> {
    cell: SharedCell<T>,
}

impl<T: Transcendental> Clone for SharedReader<T> {
    fn clone(&self) -> Self {
        Self {
            cell: self.cell.clone(),
        }
    }
}

/// Wrap a buffer into a writer + reader handle pair sharing one cell.
///
/// The writer is unique; clone the reader for additional read taps. The buffer
/// stays alive while any handle exists.
pub fn shared_handles<T: Transcendental>(
    buffer: Box<dyn DelayBuffer<T>>,
) -> (SharedWriter<T>, SharedReader<T>) {
    let cell = SharedCell::new(buffer);
    (SharedWriter { cell: cell.clone() }, SharedReader { cell })
}

impl<T: Transcendental> Writer<T> for SharedWriter<T> {
    /// SAFETY: the graph is single-threaded; at most one writer exists per cell
    /// and it never runs concurrently with a reader (sequential topo order).
    #[allow(unsafe_code)]
    fn write(&mut self, sample: T) {
        unsafe { &mut *self.cell.inner.get() }.write(sample);
    }
    #[allow(unsafe_code)]
    fn write_block(&mut self, block: &[T]) {
        unsafe { &mut *self.cell.inner.get() }.write_block(block);
    }
}

impl<T: Transcendental> SharedWriter<T> {
    /// Reset the underlying buffer to zeros.
    #[allow(unsafe_code)]
    pub fn clear(&mut self) {
        unsafe { &mut *self.cell.inner.get() }.clear();
    }
    /// Maximum capacity in samples.
    #[allow(unsafe_code)]
    pub fn capacity(&self) -> usize {
        unsafe { &*self.cell.inner.get() }.capacity()
    }
}

impl<T: Transcendental> Reader<T> for SharedReader<T> {
    /// SAFETY: see [`SharedWriter::write_block`] — no `&mut` is live while a
    /// reader is active.
    #[allow(unsafe_code)]
    fn read_block(&self, delay: usize, out: &mut [T]) {
        unsafe { &*self.cell.inner.get() }.read_block(delay, out);
    }
    #[allow(unsafe_code)]
    fn read_interpolated(&self, delay: f64) -> T {
        unsafe { &*self.cell.inner.get() }.read_interpolated(delay)
    }
}

impl<T: Transcendental> SharedReader<T> {
    /// Maximum capacity in samples.
    #[allow(unsafe_code)]
    pub fn capacity(&self) -> usize {
        unsafe { &*self.cell.inner.get() }.capacity()
    }
}

// ============================================================================
// Resource registry
// ============================================================================

/// Registry of named shared buffers.
///
/// Used at build time to allocate buffers and distribute capability handles to
/// resource-backed builtins. The registry itself is only needed during
/// assembly — the handles keep the buffer alive after it is dropped.
pub struct ResourceRegistry<T: Transcendental> {
    /// Unique writer handles, removed on first acquisition (single-writer).
    writers: HashMap<String, SharedWriter<T>>,
    /// Reader handles, cloned on each acquisition (many readers).
    readers: HashMap<String, SharedReader<T>>,
}

impl<T: Transcendental> ResourceRegistry<T> {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            writers: HashMap::new(),
            readers: HashMap::new(),
        }
    }

    /// Register a named shared buffer, creating its writer/reader handle pair.
    pub fn register_buffer(&mut self, name: impl Into<String>, buffer: Box<dyn DelayBuffer<T>>) {
        let name = name.into();
        let (writer, reader) = shared_handles(buffer);
        self.writers.insert(name.clone(), writer);
        self.readers.insert(name, reader);
    }

    /// Acquire a read handle for the named buffer (cloneable, many readers).
    pub fn reader(&self, name: &str) -> Option<SharedReader<T>> {
        self.readers.get(name).cloned()
    }

    /// Acquire the unique write handle for the named buffer.
    ///
    /// Returns `Some` only on the first call for a given name; subsequent calls
    /// return `None` (single-writer invariant).
    pub fn writer(&mut self, name: &str) -> Option<SharedWriter<T>> {
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

impl<T: Transcendental> Default for ResourceRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_writer_is_unique() {
        use crate::buffer::TapeLoop;
        let mut reg = ResourceRegistry::<f32>::new();
        reg.register_buffer(
            "tape_0",
            Box::new(TapeLoop::<f32>::new(1024).unwrap()) as Box<dyn DelayBuffer<f32>>,
        );
        assert_eq!(reg.len(), 1);
        assert!(reg.reader("tape_0").is_some());
        assert!(reg.reader("nonexistent").is_none());

        // First writer succeeds, second is denied (single-writer invariant).
        assert!(reg.writer("tape_0").is_some());
        assert!(reg.writer("tape_0").is_none());
    }

    #[test]
    fn test_registry_reader_writer_share_buffer() {
        use crate::buffer::TapeLoop;
        let mut reg = ResourceRegistry::<f32>::new();
        reg.register_buffer(
            "t",
            Box::new(TapeLoop::<f32>::new(64).unwrap()) as Box<dyn DelayBuffer<f32>>,
        );
        let mut writer = reg.writer("t").unwrap();
        let reader = reg.reader("t").unwrap();
        writer.write_block(&[1.0, 2.0]);
        let mut out = [0.0f32; 2];
        reader.read_block(0, &mut out);
        assert_eq!(out[0], 1.0);
        assert_eq!(out[1], 2.0);
    }

    #[test]
    fn shared_cell_writer_reader_share_buffer() {
        use crate::buffer::TapeLoop;
        let cell = SharedCell::new(Box::new(TapeLoop::<f32>::new(64).unwrap()));
        let mut writer = cell.writer();
        let reader = cell.reader();
        assert_eq!(cell.capacity(), 64);
        writer.write_block(&[1.0, 2.0, 3.0]);
        let mut out = [0.0f32; 3];
        reader.read_block(0, &mut out);
        assert_eq!(out, [1.0, 2.0, 3.0]);
    }
}
