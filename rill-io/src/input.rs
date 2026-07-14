use rill_core::{io::IoCapture, math::Transcendental};
use std::sync::Arc;

/// Signal input node that reads from an I/O capture backend.
pub struct Input<T: Transcendental, const BUF_SIZE: usize> {
    capture: Arc<dyn IoCapture>,
    num_channels: usize,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Input<T, BUF_SIZE> {
    /// Creates a new input with the given capture backend (defaults to 2 channels).
    pub fn new(capture: Arc<dyn IoCapture>) -> Self {
        Self::with_channels(capture, 2)
    }
    /// Creates a new input with the given capture backend and channel count.
    pub fn with_channels(capture: Arc<dyn IoCapture>, num: usize) -> Self {
        Self {
            capture,
            num_channels: num,
            _phantom: std::marker::PhantomData,
        }
    }
    /// Returns the number of input channels.
    pub fn num_channels(&self) -> usize {
        self.num_channels
    }
    /// Reads samples from the given input channel.
    pub fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize {
        self.capture.read_input(channel, dst)
    }
    /// Replaces the capture backend.
    pub fn set_capture(&mut self, capture: Arc<dyn IoCapture>) {
        self.capture = capture;
    }
}
/// Type alias for audio input (backward compatibility).
pub type AudioInput<T, const B: usize> = Input<T, B>;
