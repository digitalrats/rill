//! # Signal I/O — generic multi‑channel real‑time I/O abstraction

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::math::Scalar;

/// Result alias for signal I/O operations.
pub type IoResult<T> = Result<T, String>;

/// Control interface for backends that accept operational data
/// separate from the audio stream (e.g. chip register writes).
pub trait IoControl: Send + Sync {
    /// Write control data. Interpretation is device-specific.
    fn write_data(&self, data: &[u8]) -> usize;
}

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

    /// Returns a control interface if this backend supports runtime
    /// register/data writes. Returns `None` by default.
    fn as_control(&self) -> Option<&dyn IoControl> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::sync::Arc;

    struct TestBackend {
        reg: AtomicU8,
    }

    impl IoBackend<f32> for TestBackend {
        fn set_process_callback(&self, _cb: Box<dyn Fn()>) {}
        fn read(&self, _: &mut [&mut [f32]]) -> usize {
            0
        }
        fn write(&self, _: &[&[f32]]) -> usize {
            0
        }
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
        impl IoBackend<f32> for NoControl {
            fn set_process_callback(&self, _cb: Box<dyn Fn()>) {}
            fn read(&self, _: &mut [&mut [f32]]) -> usize {
                0
            }
            fn write(&self, _: &[&[f32]]) -> usize {
                0
            }
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
