use std::sync::Arc;

use rill_core::math::Transcendental;
use rill_core::queues::spsc::SpscQueue;
use rill_core::queues::TelemetryBlock;

/// Non-real-time consumer of telemetry frames from a shared `SpscQueue`.
///
/// Runs on the control thread (or any non-RT context). Drains available
/// frames from the ring buffer and delivers them to a user-supplied callback.
///
/// # Example
/// ```
/// use std::sync::Arc;
/// use rill_core::queues::spsc::SpscQueue;
/// use rill_core::queues::TelemetryBlock;
/// use rill_telemetry::collector::TelemetryCollector;
///
/// let queue = Arc::new(SpscQueue::<TelemetryBlock<f32, 64>, 8>::new());
/// let mut collector = TelemetryCollector::new(queue, |frame| {
///     println!("peak={} rms={} dc={}", frame.peak, frame.rms, frame.dc_offset);
/// });
/// collector.poll();  // drain available frames
/// ```
pub struct TelemetryCollector<T: Transcendental, const BUF_SIZE: usize, const QUEUE_CAP: usize> {
    queue: Arc<SpscQueue<TelemetryBlock<T, BUF_SIZE>, QUEUE_CAP>>,
    callback: Box<dyn FnMut(TelemetryBlock<T, BUF_SIZE>)>,
}

impl<T: Transcendental, const BUF_SIZE: usize, const QUEUE_CAP: usize>
    TelemetryCollector<T, BUF_SIZE, QUEUE_CAP>
{
    /// Create a new collector backed by the shared ring buffer.
    ///
    /// The `callback` is invoked for every frame drained from the queue.
    pub fn new(
        queue: Arc<SpscQueue<TelemetryBlock<T, BUF_SIZE>, QUEUE_CAP>>,
        callback: impl FnMut(TelemetryBlock<T, BUF_SIZE>) + 'static,
    ) -> Self {
        Self {
            queue,
            callback: Box::new(callback),
        }
    }

    /// Drain all available telemetry frames from the ring buffer.
    ///
    /// Returns the number of frames processed.
    pub fn poll(&mut self) -> usize {
        let mut count = 0;
        while let Some(frame) = self.queue.pop() {
            (self.callback)(frame);
            count += 1;
        }
        count
    }

    /// Drain at most `max` frames from the ring buffer.
    ///
    /// Useful when you want to bound processing time per tick.
    pub fn poll_n(&mut self, max: usize) -> usize {
        let mut count = 0;
        while count < max {
            match self.queue.pop() {
                Some(frame) => {
                    (self.callback)(frame);
                    count += 1;
                }
                None => break,
            }
        }
        count
    }

    /// Number of frames currently available in the ring buffer (approximate).
    pub fn available(&self) -> usize {
        self.queue.len()
    }

    /// Consume the collector and return the underlying queue Arc.
    pub fn into_queue(self) -> Arc<SpscQueue<TelemetryBlock<T, BUF_SIZE>, QUEUE_CAP>> {
        self.queue
    }
}
