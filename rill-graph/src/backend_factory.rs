//! # BackendFactory — constructor registry for audio backends

use std::collections::HashMap;

use rill_core::io::{BackendRegistry, IoBackend};

/// Constructor signature.
pub type BackendCtor<T> =
    fn(sample_rate: u32, buffer_size: u32, channels: u32) -> Result<Box<dyn IoBackend<T>>, String>;

/// Registry of named backend constructors (analogue of `NodeRegistry`).
pub struct BackendFactory<T> {
    ctors: HashMap<&'static str, BackendCtor<T>>,
}

impl<T> BackendFactory<T> {
    pub fn new() -> Self {
        Self {
            ctors: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: &'static str, ctor: BackendCtor<T>) {
        self.ctors.insert(name, ctor);
    }

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
pub struct BackendConfig<'a, T> {
    pub factory: &'a BackendFactory<T>,
    pub name: &'a str,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub channels: u32,
}
