use rill_core::{io::IoPlayback, math::Transcendental};
use std::sync::Arc;

/// Signal output node that writes to an I/O playback backend.
pub struct Output<T: Transcendental, const BUF_SIZE: usize> {
    playback: Arc<dyn IoPlayback>,
    num_channels: usize,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Output<T, BUF_SIZE> {
    /// Creates a new output with the given playback backend (defaults to 2 channels).
    pub fn new(playback: Arc<dyn IoPlayback>) -> Self {
        Self::with_channels(playback, 2)
    }
    /// Creates a new output with the given playback backend and channel count.
    pub fn with_channels(playback: Arc<dyn IoPlayback>, num: usize) -> Self {
        Self {
            playback,
            num_channels: num,
            _phantom: std::marker::PhantomData,
        }
    }
    /// Returns the number of output channels.
    pub fn num_channels(&self) -> usize {
        self.num_channels
    }
    /// Writes samples to the given output channel.
    pub fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        self.playback.write_output(channel, src)
    }
    /// Replaces the playback backend.
    pub fn set_playback(&mut self, playback: Arc<dyn IoPlayback>) {
        self.playback = playback;
    }
}
/// Type alias for audio output (backward compatibility).
pub type AudioOutput<T, const B: usize> = Output<T, B>;
