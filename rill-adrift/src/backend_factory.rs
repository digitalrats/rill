//! # BackendFactory — constructor registry for I/O backends
//!
//! The backend factory lives in rill-adrift (not rill-graph): rill-graph is
//! purely an alternative program representation, while backend construction is
//! an application-level concern (`ModularSystem` consumes it here).

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::io::{BackendMeta, IoCapture, IoDriver, IoPlayback};
use rill_core::traits::ParamValue;

/// Raw backend construction result: `(driver, capture?, playback?)`.
pub type BackendParts = (
    Arc<dyn IoDriver>,
    Option<Arc<dyn IoCapture>>,
    Option<Arc<dyn IoPlayback>>,
);

/// Constructor signature. Returns `(driver, capture?, playback?)`.
pub type BackendCtor = fn(params: &HashMap<String, ParamValue>) -> Result<BackendParts, String>;

/// Output-only backend bundle.
pub struct OutputBundle {
    /// The clock driver.
    pub driver: Arc<dyn IoDriver>,
    /// The playback (output) backend.
    pub playback: Arc<dyn IoPlayback>,
}

/// Input-only backend bundle.
pub struct InputBundle {
    /// The clock driver.
    pub driver: Arc<dyn IoDriver>,
    /// The capture (input) backend.
    pub capture: Arc<dyn IoCapture>,
}

/// A named backend constructor together with its static metadata.
#[derive(Clone)]
pub struct RegisteredBackend {
    /// The backend constructor.
    pub ctor: BackendCtor,
    /// Static metadata (active/passive) for the backend.
    pub meta: BackendMeta,
}

/// Registry of named backend constructors with caching.
#[derive(Clone)]
pub struct BackendFactory {
    ctors: HashMap<&'static str, RegisteredBackend>,
    cache: HashMap<String, BackendParts>,
}

impl BackendFactory {
    /// Create an empty backend factory.
    pub fn new() -> Self {
        Self {
            ctors: HashMap::new(),
            cache: HashMap::new(),
        }
    }

    /// Register a named backend constructor with its metadata.
    pub fn register(&mut self, name: &'static str, meta: BackendMeta, ctor: BackendCtor) {
        self.ctors.insert(name, RegisteredBackend { ctor, meta });
    }

    /// Whether the named backend is active (creates a callback).
    pub fn is_active(&self, name: &str) -> Option<bool> {
        self.ctors.get(name).map(|r| r.meta.active)
    }

    /// Create or retrieve a cached backend by name.
    fn get_or_create(
        &mut self,
        name: &str,
        params: &HashMap<String, ParamValue>,
    ) -> Result<BackendParts, String> {
        if let Some(cached) = self.cache.get(name) {
            return Ok(cached.clone());
        }
        let ctor = self
            .ctors
            .get(name)
            .ok_or_else(|| format!("unknown backend: {name}"))?;
        let result = (ctor.ctor)(params)?;
        self.cache.insert(name.to_string(), result.clone());
        Ok(result)
    }

    /// Create a backend returning whatever capabilities it provides.
    /// Use this when the graph determines what's needed (launch path).
    pub fn create_any(
        &mut self,
        name: &str,
        params: &HashMap<String, ParamValue>,
    ) -> Result<BackendParts, String> {
        self.get_or_create(name, params)
    }

    /// Create an output-only backend.
    pub fn create_output(
        &mut self,
        name: &str,
        params: &HashMap<String, ParamValue>,
    ) -> Result<OutputBundle, String> {
        let (driver, _capture, playback) = self.get_or_create(name, params)?;
        Ok(OutputBundle {
            driver,
            playback: playback
                .ok_or_else(|| format!("backend '{name}' does not support output"))?,
        })
    }

    /// Create an input-only backend.
    pub fn create_input(
        &mut self,
        name: &str,
        params: &HashMap<String, ParamValue>,
    ) -> Result<InputBundle, String> {
        let (driver, capture, _playback) = self.get_or_create(name, params)?;
        Ok(InputBundle {
            driver,
            capture: capture.ok_or_else(|| format!("backend '{name}' does not support input"))?,
        })
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
