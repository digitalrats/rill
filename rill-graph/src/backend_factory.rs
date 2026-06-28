//! # BackendFactory — constructor registry for I/O backends

use std::collections::HashMap;
use std::sync::Arc;

use rill_core::io::{IoCapture, IoDriver, IoPlayback};
use rill_core::traits::ParamValue;

/// Constructor signature. Returns `(driver, capture?, playback?)`.
pub type BackendCtor = fn(
    params: &HashMap<String, ParamValue>,
) -> Result<(Arc<dyn IoDriver>, Option<Arc<dyn IoCapture>>, Option<Arc<dyn IoPlayback>>), String>;

/// Output-only backend bundle.
pub struct OutputBundle {
    pub driver: Arc<dyn IoDriver>,
    pub playback: Arc<dyn IoPlayback>,
}

/// Input-only backend bundle.
pub struct InputBundle {
    pub driver: Arc<dyn IoDriver>,
    pub capture: Arc<dyn IoCapture>,
}

/// Full-duplex backend bundle.
pub struct DuplexBundle {
    pub driver: Arc<dyn IoDriver>,
    pub capture: Arc<dyn IoCapture>,
    pub playback: Arc<dyn IoPlayback>,
}

/// Registry of named backend constructors with caching.
#[derive(Clone)]
pub struct BackendFactory {
    ctors: HashMap<&'static str, BackendCtor>,
    cache: HashMap<String, (Arc<dyn IoDriver>, Option<Arc<dyn IoCapture>>, Option<Arc<dyn IoPlayback>>)>,
}

impl BackendFactory {
    /// Create an empty backend factory.
    pub fn new() -> Self {
        Self {
            ctors: HashMap::new(),
            cache: HashMap::new(),
        }
    }

    /// Register a named backend constructor.
    pub fn register(&mut self, name: &'static str, ctor: BackendCtor) {
        self.ctors.insert(name, ctor);
    }

    /// Create or retrieve a cached backend by name.
    fn get_or_create(
        &mut self,
        name: &str,
        params: &HashMap<String, ParamValue>,
    ) -> Result<(Arc<dyn IoDriver>, Option<Arc<dyn IoCapture>>, Option<Arc<dyn IoPlayback>>), String> {
        if let Some(cached) = self.cache.get(name) {
            return Ok(cached.clone());
        }
        let ctor = self
            .ctors
            .get(name)
            .ok_or_else(|| format!("unknown backend: {name}"))?;
        let result = ctor(params)?;
        self.cache.insert(name.to_string(), result.clone());
        Ok(result)
    }

    /// Create a backend returning whatever capabilities it provides.
    /// Use this when the graph determines what's needed (launch path).
    pub fn create_any(
        &mut self,
        name: &str,
        params: &HashMap<String, ParamValue>,
    ) -> Result<(Arc<dyn IoDriver>, Option<Arc<dyn IoCapture>>, Option<Arc<dyn IoPlayback>>), String> {
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
            playback: playback.ok_or_else(|| format!("backend '{name}' does not support output"))?,
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

    /// Create a full-duplex backend.
    pub fn create_duplex(
        &mut self,
        name: &str,
        params: &HashMap<String, ParamValue>,
    ) -> Result<DuplexBundle, String> {
        let (driver, capture, playback) = self.get_or_create(name, params)?;
        Ok(DuplexBundle {
            driver,
            capture: capture
                .ok_or_else(|| format!("backend '{name}' does not support input"))?,
            playback: playback
                .ok_or_else(|| format!("backend '{name}' does not support output"))?,
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
