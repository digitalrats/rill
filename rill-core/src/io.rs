//! # Signal I/O — generic multi-channel real-time I/O abstraction

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::time::ClockTick;

/// Result alias for signal I/O operations.
pub type IoResult<T> = Result<T, String>;

/// Control interface for backends that accept operational data
/// separate from the signal stream (e.g. chip register writes).
pub trait IoControl {
    /// Write control data. Interpretation is device-specific.
    fn write_data(&self, data: &[u8]) -> usize;
}

// ============================================================================
// IoDriver — drives the graph
// ============================================================================

/// A backend that can be the **clock driver** for the signal graph.
///
/// The driver owns the timing loop: it registers a process callback and
/// fires it on every I/O tick. Only one driver is active per rack.
///
/// A single backend struct may implement `IoDriver` together with
/// [`IoCapture`] and/or [`IoPlayback`] — capturing and playing are
/// orthogonal capabilities on top of the driver role.
pub trait IoDriver: Send + Sync {
    /// Register the process callback that the driver calls each tick.
    ///
    /// The callback receives a [`ClockTick`] with timing metadata
    /// (sample position, rate, speed_ratio, etc.).
    fn set_process_callback(&self, cb: Box<dyn FnMut(&ClockTick)>);

    /// Enter the I/O lifecycle. Called on the pre-created signal thread.
    ///
    /// For poll-driven backends (ALSA, PipeWire) this blocks inside the
    /// I/O loop and returns only after `running` becomes false.
    /// For callback-driven backends (JACK, PortAudio) it sets up the
    /// stream and returns immediately — the process callback fires on
    /// the I/O API's own thread.
    fn run(&self, running: Arc<AtomicBool>) -> IoResult<()>;

    /// Signal the driver to shut down. Called from the control thread.
    /// After this returns the driver must be safe to drop.
    fn stop(&self) -> IoResult<()>;

    /// Returns a control interface if this driver supports runtime
    /// register/data writes. Returns `None` by default.
    fn as_control(&self) -> Option<&dyn IoControl> {
        None
    }
}

// ============================================================================
// IoCapture — reads input samples
// ============================================================================

/// A backend that **captures** (reads) signal data from hardware.
///
/// Nodes of type `rill/input` hold an `Arc<dyn IoCapture>` and call
/// [`read_input`](IoCapture::read_input) directly from `generate()`.
///
/// A capture backend may or may not also be the driver. When it is not
/// the driver, the driver's callback ensures that fresh capture data is
/// available before the graph runs (e.g. PipeWire processes all streams
/// in the same cycle).
pub trait IoCapture: Send + Sync {
    /// Read captured samples for one channel into `dst`.
    ///
    /// Returns the number of samples actually read (may be less than
    /// `dst.len()` if insufficient data is available).
    fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize;

    /// Number of capture channels.
    fn num_input_channels(&self) -> usize;
}

// ============================================================================
// IoPlayback — writes output samples
// ============================================================================

/// A backend that **plays** (writes) signal data to hardware.
///
/// Nodes of type `rill/output` hold an `Arc<dyn IoPlayback>` and call
/// [`write_output`](IoPlayback::write_output) directly from `consume()`.
pub trait IoPlayback: Send + Sync {
    /// Write output samples for one channel from `src`.
    ///
    /// Returns the number of samples actually written (may be less than
    /// `src.len()` if insufficient space is available).
    fn write_output(&self, channel: usize, src: &[f32]) -> usize;

    /// Number of playback channels.
    fn num_output_channels(&self) -> usize;
}

// ============================================================================
// Backward-compatible alias
// ============================================================================

/// Backward-compatible alias for code that only needs a driver.
pub trait IoBackend: IoDriver {}

impl<T: IoDriver> IoBackend for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

    struct TestBackend {
        reg: AtomicU8,
    }

    impl IoDriver for TestBackend {
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
        impl IoDriver for NoControl {
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
