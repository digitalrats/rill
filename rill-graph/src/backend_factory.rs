//! # BackendFactory — constructor registry for audio backends

use std::collections::HashMap;

use rill_core::io::IoBackend;

/// Constructor signature.
pub type BackendCtor<T> =
    fn(sample_rate: u32, buffer_size: u32, channels: u32) -> Result<Box<dyn IoBackend<T>>, String>;

/// Registry of named backend constructors (analogue of `NodeRegistry`).
pub struct BackendFactory<T> {
    ctors: HashMap<&'static str, BackendCtor<T>>,
}

impl<T> BackendFactory<T> {
    /// Create an empty backend factory.
    pub fn new() -> Self {
        Self {
            ctors: HashMap::new(),
        }
    }

    /// Register a named backend constructor.
    pub fn register(&mut self, name: &'static str, ctor: BackendCtor<T>) {
        self.ctors.insert(name, ctor);
    }

    /// Create a backend by name with the given parameters.
    pub fn create(
        &self,
        name: &str,
        sample_rate: u32,
        buffer_size: u32,
        channels: u32,
    ) -> Result<Box<dyn IoBackend<T>>, String> {
        match self.ctors.get(name) {
            Some(ctor) => ctor(sample_rate, buffer_size, channels),
            None => Err(format!("unknown backend: {name}")),
        }
    }

    /// Returns `true` if a backend with the given name is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.ctors.contains_key(name)
    }
}

impl<T> Default for BackendFactory<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Backend configuration passed to [`GraphBuilder::build`](crate::SignalGraph::build).
/// Backend configuration passed to [`GraphBuilder::build`](crate::SignalGraph::build).
pub struct BackendConfig<'a, T> {
    /// Backend factory to use for construction.
    pub factory: &'a BackendFactory<T>,
    /// Name of the backend to construct.
    pub name: &'a str,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Buffer size in frames.
    pub buffer_size: u32,
    /// Number of audio channels.
    pub channels: u32,
}
