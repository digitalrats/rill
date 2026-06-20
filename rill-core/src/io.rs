//! # Signal I/O — generic multi-channel real-time I/O abstraction

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::time::ClockTick;
use crate::traits::buffer_view::BufferView;

/// Result alias for signal I/O operations.
pub type IoResult<T> = Result<T, String>;

/// Control interface for backends that accept operational data
/// separate from the signal stream (e.g. chip register writes).
pub trait IoControl {
    /// Write control data. Interpretation is device-specific.
    fn write_data(&self, data: &[u8]) -> usize;
}

/// Generic multi-channel real-time signal I/O backend.
///
/// Lifecycle (called by the orchestrator which owns the I/O thread):
///   1. `create_view()` — obtain the backend's BufferView for graph nodes
///   2. `set_process_callback(cb)` — register the graph processing callback
///   3. `run(running)` — enter the I/O loop; blocks for poll-driven backends,
///      returns immediately for callback-driven ones.
///   4. `stop()` — signals shutdown; tears down resources.
pub trait IoBackend: Send {
    /// Create a BufferView for this backend.
    ///
    /// The view encapsulates backend-specific access rules (interleave/
    /// deinterleave semantics) for reading input and writing output
    /// through lock-free ring buffers.
    fn create_view(&self) -> Arc<dyn BufferView>;

    /// Register the process callback that the backend calls each block.
    ///
    /// The callback receives a [`ClockTick`] with timing information,
    /// source name, and the backend's BufferView reference.
    fn set_process_callback(&self, cb: Box<dyn FnMut(&ClockTick)>);

    /// Enter the I/O lifecycle. Called on the pre-created I/O thread.
    ///
    /// For poll-driven backends (ALSA, PipeWire) this blocks inside the
    /// I/O loop and returns only after `running` becomes false.
    /// For callback-driven backends (JACK, PortAudio) it sets up the
    /// stream and returns immediately — the process callback fires on
    /// the I/O API's own thread.
    fn run(&self, running: Arc<AtomicBool>) -> IoResult<()>;

    /// Signal the backend to shut down. Called from the control thread.
    /// After this returns the backend must be safe to drop.
    fn stop(&self) -> IoResult<()>;

    /// Returns a control interface if this backend supports runtime
    /// register/data writes. Returns `None` by default.
    fn as_control(&self) -> Option<&dyn IoControl> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::buffer_view::NullBufferView;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

    struct TestBackend {
        reg: AtomicU8,
    }

    impl IoBackend for TestBackend {
        fn create_view(&self) -> Arc<dyn BufferView> {
            Arc::new(NullBufferView::new(2, 2))
        }

        fn set_process_callback(&self, _cb: Box<dyn FnMut(&ClockTick)>) {}

        fn run(&self, _: Arc<AtomicBool>) -> IoResult<()> {
            Ok(())
        }

        fn stop(&self) -> IoResult<()> {
            Ok(())
        }

        fn as_control(&self) -> Option<&dyn IoControl> {
            Some(self)
        }
    }

    impl IoControl for TestBackend {
        fn write_data(&self, data: &[u8]) -> usize {
            if let Some(&v) = data.first() {
                self.reg.store(v, Ordering::Relaxed);
            }
            1
        }
    }

    #[test]
    fn test_iocontrol_write_data() {
        let b = TestBackend {
            reg: AtomicU8::new(0),
        };
        let ctrl = b.as_control().unwrap();
        ctrl.write_data(&[42]);
        assert_eq!(b.reg.load(Ordering::Relaxed), 42);
    }

    #[test]
    fn test_iocontrol_default_returns_none() {
        struct NoControl;
        impl IoBackend for NoControl {
            fn create_view(&self) -> Arc<dyn BufferView> {
                Arc::new(NullBufferView::new(0, 0))
            }
            fn set_process_callback(&self, _cb: Box<dyn FnMut(&ClockTick)>) {}
            fn run(&self, _: Arc<AtomicBool>) -> IoResult<()> {
                Ok(())
            }
            fn stop(&self) -> IoResult<()> {
                Ok(())
            }
        }
        let b = NoControl;
        assert!(b.as_control().is_none());
    }
}
