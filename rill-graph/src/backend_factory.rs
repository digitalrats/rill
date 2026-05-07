//! # BackendFactory — constructor registry for audio backends

use std::collections::HashMap;

use rill_core::io::IoBackend;
use rill_core::traits::ParamValue;

/// Constructor signature.
///
/// Backend constructors receive a parameter map with string keys and
/// [`ParamValue`] entries. Typical keys: `"sample_rate"`, `"buffer_size"`,
/// `"channels"`. Each constructor parses the values it needs.
pub type BackendCtor<T> =
    fn(params: &HashMap<String, ParamValue>) -> Result<Box<dyn IoBackend<T>>, String>;

/// Registry of named backend constructors (analogue of `NodeFactory`).
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
        params: &HashMap<String, ParamValue>,
    ) -> Result<Box<dyn IoBackend<T>>, String> {
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

impl<T> Default for BackendFactory<T> {
    fn default() -> Self {
        Self::new()
    }
}
