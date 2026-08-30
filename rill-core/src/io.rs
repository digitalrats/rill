//! # Signal I/O — generic multi-channel real-time I/O abstraction

use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::queues::{SpscQueue, TelemetryBlock};
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
    fn set_callback(&self, cb: Box<dyn FnMut(&ClockTick)>);

    /// Enter the I/O lifecycle.
    ///
    /// Blocks until the driver is stopped (via [`stop`](IoDriver::stop) or
    /// the `running` flag becomes `false`). The process callback set via
    /// [`set_callback`](IoDriver::set_callback) fires inside this call.
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

/// A passive I/O backend that writes to nowhere and reads zeros.
///
/// Implements both [`IoCapture`] and [`IoPlayback`] as no-ops.
/// Useful as a placeholder for the unused direction in input-only
/// or output-only scenarios.
pub struct NullBackend {
    channels: usize,
}

impl NullBackend {
    /// Create a null backend with the given number of channels.
    pub fn new(channels: usize) -> Self {
        Self { channels }
    }
}

impl IoCapture for NullBackend {
    fn read_input(&self, _channel: usize, dst: &mut [f32]) -> usize {
        dst.fill(0.0);
        dst.len()
    }

    fn num_input_channels(&self) -> usize {
        self.channels
    }
}

impl IoPlayback for NullBackend {
    fn write_output(&self, _channel: usize, _src: &[f32]) -> usize {
        _src.len()
    }

    fn num_output_channels(&self) -> usize {
        self.channels
    }
}

/// An `IoPlayback` that pushes signal blocks into a lock-free SPSC queue.
///
/// Each `write_output` call wraps the signal data into a [`TelemetryBlock`]
/// and pushes it into a [`SpscQueue`]. No allocations, no locks — safe to
/// call from the RT signal path. A non-RT collector drains the queue.
pub struct SpmcPlayback<T: crate::math::Transcendental, const BUF: usize, const CAP: usize> {
    queue: Arc<SpscQueue<TelemetryBlock<T, BUF>, CAP>>,
    channels: usize,
    sample_rate: f32,
    sample_pos: AtomicU64,
}

impl<T: crate::math::Transcendental, const BUF: usize, const CAP: usize> SpmcPlayback<T, BUF, CAP> {
    /// Create an SPSC-based playback that writes to `queue`.
    pub fn new(
        queue: Arc<SpscQueue<TelemetryBlock<T, BUF>, CAP>>,
        channels: usize,
        sample_rate: f32,
    ) -> Self {
        Self {
            queue,
            channels,
            sample_rate,
            sample_pos: AtomicU64::new(0),
        }
    }

    /// Return the shared queue for draining on the non-RT side.
    pub fn queue(&self) -> &Arc<SpscQueue<TelemetryBlock<T, BUF>, CAP>> {
        &self.queue
    }
}

impl IoPlayback for SpmcPlayback<f32, 256, 64> {
    fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        let n = src.len();
        if n == 0 {
            return 0;
        }
        let pos = self.sample_pos.fetch_add(n as u64, Ordering::Relaxed);
        let mut block = TelemetryBlock::default();
        let limit = n.min(256);
        block.data[..limit].copy_from_slice(&src[..limit]);
        block.channel = channel as u32;
        block.sample_rate = self.sample_rate;
        block.block_index = pos;
        block.timestamp = pos;
        block.compute_metrics();
        let _ = self.queue.push(block);
        limit
    }

    fn num_output_channels(&self) -> usize {
        self.channels
    }
}

// ============================================================================
// BackendMeta — static backend metadata
// ============================================================================

/// Static metadata for a backend definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendMeta {
    /// Active backends create a callback that drives the signal (rill-io
    /// input/output). Passive backends (sampler, tape) provide or consume data
    /// without a callback.
    pub active: bool,
}

impl BackendMeta {
    /// An active backend — always creates a callback (e.g. rill-io input/output).
    pub const fn active() -> Self {
        Self { active: true }
    }

    /// A passive backend — no callback (e.g. sampler, tape).
    pub const fn passive() -> Self {
        Self { active: false }
    }
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
        fn set_callback(&self, _cb: Box<dyn FnMut(&ClockTick)>) {}

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
            fn set_callback(&self, _cb: Box<dyn FnMut(&ClockTick)>) {}
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
