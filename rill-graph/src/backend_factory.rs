//! # BackendFactory — constructor registry for I/O backends

use std::collections::HashMap;

use rill_core::io::IoBackend;
use rill_core::traits::ParamValue;

/// Constructor signature.
///
/// Backend constructors receive a parameter map with string keys and
/// [`ParamValue`] entries. Typical keys: `"sample_rate"`, `"buffer_size"`,
/// `"channels"`. Each constructor parses the values it needs.
pub type BackendCtor =
    fn(params: &HashMap<String, ParamValue>) -> Result<Box<dyn IoBackend>, String>;

/// Registry of named backend constructors (analogue of `NodeFactory`).
#[derive(Clone)]
pub struct BackendFactory {
    ctors: HashMap<&'static str, BackendCtor>,
}

impl BackendFactory {
    /// Create an empty backend factory.
    pub fn new() -> Self {
        Self {
            ctors: HashMap::new(),
        }
    }

    /// Register a named backend constructor.
    pub fn register(&mut self, name: &'static str, ctor: BackendCtor) {
        self.ctors.insert(name, ctor);
    }

    /// Create a backend by name with the given parameters.
    pub fn create(
        &self,
        name: &str,
        params: &HashMap<String, ParamValue>,
    ) -> Result<Box<dyn IoBackend>, String> {
        match self.ctors.get(name) {
            Some(ctor) => ctor(params),
            None => Err(format!("unknown backend: {name}")),
        }
    }

    /// Returns `true` if a backend with the given name is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.ctors.contains_key(name)
    }
}

impl Default for BackendFactory {
    fn default() -> Self {
        Self::new()
    }
}
