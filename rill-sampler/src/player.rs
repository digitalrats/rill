use crate::buffer::SampleBuffer;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ParamValue;
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;
use rill_core_dsp::generators::{LoopMode, SamplePlayer};
use std::marker::PhantomData;

pub struct SamplePlayerNode<T: Transcendental, const BUF_SIZE: usize> {
    left: SamplePlayer<T>,
    right: Option<SamplePlayer<T>>,
    gate: bool,
    amplitude: T,
    rate: f64,
    loop_mode: LoopMode,
    loop_start: f64,
    loop_end: f64,
    cubic: bool,
    _phantom: PhantomData<[T; BUF_SIZE]>,
}

impl<T: Transcendental, const BUF_SIZE: usize> SamplePlayerNode<T, BUF_SIZE> {
    pub fn new() -> Self {
        Self {
            left: SamplePlayer::new(Vec::new()),
            right: None,
            gate: false,
            amplitude: T::from_f32(1.0),
            rate: 1.0,
            loop_mode: LoopMode::OneShot,
            loop_start: 0.0,
            loop_end: 0.0,
            cubic: false,
            _phantom: PhantomData,
        }
    }
    pub fn load(&mut self, sample: SampleBuffer<T>) {
        let len = sample.len() as f64;
        self.loop_end = len;
        self.loop_start = 0.0;
        self.left.set_buffer(sample.data);
        self.left.set_loop_start(self.loop_start);
        self.left.set_loop_end(self.loop_end);
        self.left.set_loop_mode(self.loop_mode);
        self.left.set_playback_rate(self.rate);
        self.left.set_cubic(self.cubic);
        if let Some(right_data) = sample.right {
            let mut rp = SamplePlayer::new(right_data);
            rp.set_loop_start(self.loop_start);
            rp.set_loop_end(self.loop_end);
            rp.set_loop_mode(self.loop_mode);
            rp.set_playback_rate(self.rate);
            rp.set_cubic(self.cubic);
            self.right = Some(rp);
        } else {
            self.right = None;
        }
    }
    pub fn play(&mut self) {
        self.gate = true;
        self.left.set_gate(true);
        if let Some(ref mut r) = self.right {
            r.set_gate(true);
        }
    }
    pub fn stop(&mut self) {
        self.gate = false;
        self.left.set_gate(false);
        if let Some(ref mut r) = self.right {
            r.set_gate(false);
        }
    }
    pub fn set_amplitude(&mut self, amp: T) {
        self.amplitude = amp.clamp(T::ZERO, T::from_f32(1.0));
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T> for SamplePlayerNode<T, BUF_SIZE> {
    fn process(&mut self, _input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let mut buf = vec![T::ZERO; output.len()];
        self.left.process(None, &mut buf)?;
        for (out, s) in output.iter_mut().zip(&buf) {
            *out = s.mul(self.amplitude);
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.left.reset();
        if let Some(ref mut r) = self.right {
            r.reset();
        }
    }
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "SamplePlayer",
            category: AlgorithmCategory::Generator,
            description: "Sample playback with stereo support and looping",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for SamplePlayerNode<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_sample_player_creation() {
        let player = SamplePlayerNode::<f32, 64>::new();
        assert!(!player.gate);
    }
    #[test]
    fn test_sample_player_play_stop() {
        let mut player = SamplePlayerNode::<f32, 64>::new();
        player.play();
        assert!(player.gate);
        player.stop();
        assert!(!player.gate);
    }
}
