use rill_core::{io::IoPlayback, math::Transcendental};
use std::sync::Arc;

pub struct Output<T: Transcendental, const BUF_SIZE: usize> {
    playback: Arc<dyn IoPlayback>,
    num_channels: usize,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Output<T, BUF_SIZE> {
    pub fn new(playback: Arc<dyn IoPlayback>) -> Self {
        Self::with_channels(playback, 2)
    }
    pub fn with_channels(playback: Arc<dyn IoPlayback>, num: usize) -> Self {
        Self {
            playback,
            num_channels: num,
            _phantom: std::marker::PhantomData,
        }
    }
    pub fn num_channels(&self) -> usize {
        self.num_channels
    }
    pub fn write_output(&self, channel: usize, src: &[f32]) -> usize {
        self.playback.write_output(channel, src)
    }
    pub fn set_playback(&mut self, playback: Arc<dyn IoPlayback>) {
        self.playback = playback;
    }
}
pub type AudioOutput<T, const B: usize> = Output<T, B>;
