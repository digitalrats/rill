//! # Signal I/O — generic multi‑channel real‑time I/O abstraction

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::math::Scalar;

/// Result alias for signal I/O operations.
pub type IoResult<T> = Result<T, String>;

/// Generic multi‑channel real‑time signal I/O backend.
///
/// Lifecycle (called by `rill-patchbay` or `rill-adrift` which own the audio thread):
///   1. `set_process_callback(cb)` — register the graph processing callback
///   2. `run(running)` — called on the pre‑created audio thread; blocks for
///      poll‑driven backends (ALSA, PipeWire) or returns immediately for
///      callback‑driven ones (JACK, CPAL).  Checks `running` to know when to exit.
///   3. `stop()` — signals shutdown; tears down resources.
pub trait IoBackend<T: Scalar>: Send + Sync {
    /// Register the process callback that the backend calls each block.
    fn set_process_callback(&self, cb: Box<dyn Fn()>);
    /// Read audio data from the backend into the provided channel slices.
    ///
    /// Returns the number of frames read.
    fn read(&self, channels: &mut [&mut [T]]) -> usize;
    /// Write audio data from the provided channel slices to the backend.
    ///
    /// Returns the number of frames written.
    fn write(&self, channels: &[&[T]]) -> usize;
    /// Enter the audio I/O lifecycle.  Called on the pre‑created audio thread.
    ///
    /// For poll‑driven backends (ALSA, PipeWire) this blocks inside the
    /// audio I/O loop and returns only after `running` becomes false.
    /// For callback‑driven backends (JACK, CPAL) it sets up the stream and
    /// returns immediately — the process callback fires on the audio API's
    /// own thread.
    fn run(&self, running: Arc<AtomicBool>) -> IoResult<()>;
    /// Signal the backend to shut down.  Called from the control thread.
    /// After this returns the backend must be safe to drop.
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
    /// Create an empty backend registry.
    pub fn new() -> Self {
        Self {
            backends: HashMap::new(),
        }
    }

    /// Register a named backend.
    pub fn register(&mut self, name: impl Into<String>, backend: Box<dyn IoBackend<T>>) {
        self.backends.insert(name.into(), backend);
    }

    /// Remove and return a registered backend by name.
    pub fn take(&mut self, name: &str) -> Option<Box<dyn IoBackend<T>>> {
        self.backends.remove(name)
    }

    /// Get a reference to a registered backend by name.
    pub fn get_ref(&self, name: &str) -> Option<&dyn IoBackend<T>> {
        self.backends.get(name).map(|b| &**b)
    }

    /// Number of registered backends.
    pub fn len(&self) -> usize {
        self.backends.len()
    }

    /// Returns `true` if no backends are registered.
    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

impl<T: Scalar> Default for BackendRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}
