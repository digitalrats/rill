//! # Signal I/O — generic multi‑channel real‑time I/O abstraction

use std::collections::HashMap;

use crate::math::Scalar;

/// Result alias for signal I/O operations.
pub type IoResult<T> = Result<T, String>;

/// Generic multi‑channel real‑time signal I/O backend.
pub trait IoBackend<T: Scalar>: Send {
    fn set_process_callback(&self, cb: Box<dyn Fn()>);
    fn read(&self, channels: &mut [&mut [T]]) -> usize;
    fn write(&self, channels: &[&[T]]) -> usize;
    fn start(&self) -> IoResult<()>;
    fn stop(&self) -> IoResult<()>;
}

// ============================================================================
// BackendRegistry — safe, used during graph assembly
// ============================================================================

/// Registry of named `IoBackend` backends.
pub struct BackendRegistry<T: Scalar> {
    backends: HashMap<String, Box<dyn IoBackend<T>>>,
}

impl<T: Scalar> BackendRegistry<T> {
    pub fn new() -> Self {
        Self {
            backends: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: impl Into<String>, backend: Box<dyn IoBackend<T>>) {
        self.backends.insert(name.into(), backend);
    }

    pub fn take(&mut self, name: &str) -> Option<Box<dyn IoBackend<T>>> {
        self.backends.remove(name)
    }

    pub fn get_ref(&self, name: &str) -> Option<&dyn IoBackend<T>> {
        self.backends.get(name).map(|b| &**b)
    }

    pub fn len(&self) -> usize {
        self.backends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

impl<T: Scalar> Default for BackendRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}
