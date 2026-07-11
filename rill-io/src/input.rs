use rill_core::{io::IoCapture, math::Transcendental};
use std::sync::Arc;

pub struct Input<T: Transcendental, const BUF_SIZE: usize> {
    capture: Arc<dyn IoCapture>,
    num_channels: usize,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Input<T, BUF_SIZE> {
    pub fn new(capture: Arc<dyn IoCapture>) -> Self {
        Self::with_channels(capture, 2)
    }
    pub fn with_channels(capture: Arc<dyn IoCapture>, num: usize) -> Self {
        Self {
            capture,
            num_channels: num,
            _phantom: std::marker::PhantomData,
        }
    }
    pub fn num_channels(&self) -> usize {
        self.num_channels
    }
    pub fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize {
        self.capture.read_input(channel, dst)
    }
    pub fn set_capture(&mut self, capture: Arc<dyn IoCapture>) {
        self.capture = capture;
    }
}
pub type AudioInput<T, const B: usize> = Input<T, B>;
