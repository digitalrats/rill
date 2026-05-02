use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use crate::generators::{Generator, InterpolatedReader};
use rill_core::traits::{ActionContext, ProcessResult};
use rill_core::Transcendental;

/// Loop behaviour for [`SamplePlayer`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoopMode {
    /// Play once from start to end, then output silence.
    OneShot,
    /// Loop forward when reaching the end position.
    Forward,
    /// Reverse direction at loop boundaries (bouncing loop).
    PingPong,
}

/// Playback state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayState {
    Stopped,
    Playing,
}

/// Sample-playback algorithm built on [`InterpolatedReader`].
///
/// Plays back a fixed buffer with variable rate, interpolation,
/// and configurable looping. Implements [`Algorithm`] for use in
/// audio graphs, and [`Generator`] for compatibility with the
/// oscillator trait hierarchy.
///
/// # Generator parameter mapping
///
/// | `Generator` | Meaning for `SamplePlayer` |
/// |---|---|
/// | `frequency` | Pitch: `rate = freq * len / sample_rate` |
/// | `phase` | Normalised position (0 = start, 1 = end of buffer) |
/// | `amplitude` | Output gain |
pub struct SamplePlayer<T: Transcendental> {
    reader: InterpolatedReader<T>,
    loop_mode: LoopMode,
    loop_start: f64,
    loop_end: f64,
    state: PlayState,
    gate: bool,
    amplitude: T,
    sample_rate: f32,
}

impl<T: Transcendental> SamplePlayer<T> {
    pub fn new(buffer: Vec<T>) -> Self {
        let len = buffer.len() as f64;
        Self {
            reader: InterpolatedReader::new(buffer),
            loop_mode: LoopMode::OneShot,
            loop_start: 0.0,
            loop_end: len,
            state: PlayState::Stopped,
            gate: false,
            amplitude: T::from_f32(1.0),
            sample_rate: 44100.0,
        }
    }

    pub fn from_boxed(buffer: Box<[T]>) -> Self {
        let len = buffer.len() as f64;
        Self {
            reader: InterpolatedReader::from_boxed(buffer),
            loop_mode: LoopMode::OneShot,
            loop_start: 0.0,
            loop_end: len,
            state: PlayState::Stopped,
            gate: false,
            amplitude: T::from_f32(1.0),
            sample_rate: 44100.0,
        }
    }

    pub fn len(&self) -> usize {
        self.reader.len()
    }

    pub fn is_empty(&self) -> bool {
        self.reader.is_empty()
    }

    pub fn loop_mode(&self) -> LoopMode {
        self.loop_mode
    }

    pub fn set_loop_mode(&mut self, mode: LoopMode) {
        self.loop_mode = mode;
    }

    pub fn loop_start(&self) -> f64 {
        self.loop_start
    }

    pub fn set_loop_start(&mut self, start: f64) {
        self.loop_start = start.clamp(0.0, self.reader.len() as f64);
    }

    pub fn loop_end(&self) -> f64 {
        self.loop_end
    }

    pub fn set_loop_end(&mut self, end: f64) {
        let max = self.reader.len() as f64;
        self.loop_end = end.clamp(0.0, max);
    }

    pub fn gate(&self) -> bool {
        self.gate
    }

    /// Start / stop playback.
    ///
    /// Setting `gate = true` restarts from the beginning.
    pub fn set_gate(&mut self, gate: bool) {
        if gate && !self.gate {
            self.reader.set_position(self.loop_start);
            self.state = PlayState::Playing;
        } else if !gate {
            self.state = PlayState::Stopped;
        }
        self.gate = gate;
    }

    pub fn play_state(&self) -> PlayState {
        self.state
    }

    /// Replace the sample buffer and reset to loop-start.
    pub fn set_buffer(&mut self, buffer: Vec<T>) {
        self.reader.set_buffer(buffer);
        self.loop_end = self.reader.len() as f64;
        self.loop_start = 0.0;
    }

    pub fn set_cubic(&mut self, cubic: bool) {
        self.reader.set_cubic(cubic);
    }

    pub fn is_cubic(&self) -> bool {
        self.reader.is_cubic()
    }

    pub fn set_playback_rate(&mut self, rate: f64) {
        self.reader.set_rate(rate);
    }

    pub fn playback_rate(&self) -> f64 {
        self.reader.rate()
    }

    fn playable_len(&self) -> f64 {
        (self.loop_end - self.loop_start).max(1.0)
    }
}

impl<T: Transcendental> Algorithm<T> for SamplePlayer<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.reader.set_position(self.loop_start);
        self.state = PlayState::Stopped;
    }

    fn reset(&mut self) {
        self.gate = false;
        self.state = PlayState::Stopped;
        self.reader.set_position(self.loop_start);
    }

    fn process(
        &mut self,
        _input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        if !self.gate || self.state == PlayState::Stopped || self.is_empty() {
            for s in output.iter_mut() {
                *s = T::ZERO;
            }
            return Ok(());
        }

        let amp = self.amplitude;
        let start = self.loop_start;
        let end = self.loop_end;

        for s in output.iter_mut() {
            if !self.gate || self.state == PlayState::Stopped {
                *s = T::ZERO;
                continue;
            }

            *s = self.reader.read_one() * amp;
            self.reader.advance();

            let pos = self.reader.position();
            let going_forward = self.reader.rate() >= 0.0;

            if self.loop_mode == LoopMode::OneShot {
                if pos >= end || pos < 0.0 {
                    self.state = PlayState::Stopped;
                    self.gate = false;
                }
            } else if self.loop_mode == LoopMode::Forward {
                if going_forward && pos >= end {
                    self.reader.set_position(start + (pos - end));
                } else if !going_forward && pos < start {
                    self.reader.set_position(end - (start - pos));
                }
            } else if going_forward && pos >= end {
                // PingPong: hit forward boundary → reverse
                self.reader.set_rate(-self.reader.rate());
                self.reader.set_position(end - 1.0);
            } else if !going_forward && pos <= start {
                // PingPong: hit backward boundary → forward
                self.reader.set_rate(-self.reader.rate());
                self.reader.set_position(start);
            }
        }

        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "SamplePlayer",
            category: AlgorithmCategory::Generator,
            description: "Sample playback with loop modes".into(),
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental + Copy> Generator<T> for SamplePlayer<T> {
    fn phase(&self) -> T {
        let len = self.reader.len() as f64;
        if len == 0.0 {
            return T::ZERO;
        }
        T::from_f64((self.reader.position() / len).clamp(0.0, 1.0))
    }

    fn set_phase(&mut self, phase: T) {
        let p = phase.to_f64().clamp(0.0, 1.0);
        let len = self.reader.len() as f64;
        self.reader.set_position(p * len);
    }

    fn reset_phase(&mut self) {
        self.reader.set_position(0.0);
    }

    fn frequency(&self) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        let rate = self.reader.rate();
        let len = self.reader.len() as f64;
        (rate * self.sample_rate as f64 / len) as f32
    }

    fn set_frequency(&mut self, freq: f32) {
        if self.is_empty() {
            return;
        }
        let len = self.reader.len() as f64;
        let rate = freq as f64 * len / self.sample_rate as f64;
        self.reader.set_rate(rate);
    }

    fn amplitude(&self) -> T {
        self.amplitude
    }

    fn set_amplitude(&mut self, amp: T) {
        self.amplitude = amp.clamp(T::ZERO, T::from_f32(1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::time::ClockTick;
    use rill_core::traits::ActionContext;

    fn process(player: &mut SamplePlayer<f64>, out: &mut [f64]) {
        let tick = ClockTick::new(0, 0, 44100.0);
        player.process(None, out, &ActionContext::new(&tick)).unwrap();
    }

    #[test]
    fn test_one_shot() {
        let buf = vec![1.0, 2.0, 3.0, 4.0];
        let mut player = SamplePlayer::new(buf);
        player.set_gate(true);
        let mut out = [0.0f64; 6];
        process(&mut player, &mut out[..3]);
        assert!(player.play_state() == PlayState::Playing, "still playing after 3/4");
        process(&mut player, &mut out[3..]);
        assert_eq!(out[0..4], [1.0, 2.0, 3.0, 4.0], "all samples read");
        assert_eq!(out[4..6], [0.0, 0.0], "silence after end");
        assert_eq!(player.play_state(), PlayState::Stopped, "stopped after end");
    }

    #[test]
    fn test_loop_forward() {
        let buf = vec![1.0, 2.0, 3.0];
        let mut player = SamplePlayer::new(buf);
        player.set_loop_mode(LoopMode::Forward);
        player.set_gate(true);
        let mut out = [0.0f64; 6];
        process(&mut player, &mut out);
        assert_eq!(out, [1.0, 2.0, 3.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_ping_pong() {
        let buf = vec![1.0, 2.0, 3.0, 4.0];
        let mut player = SamplePlayer::new(buf);
        player.set_loop_mode(LoopMode::PingPong);
        player.set_gate(true);
        let mut out = [0.0f64; 12];
        process(&mut player, &mut out);
        // Forward [1,2,3,4], reverse [4,3,2,1], forward...
        assert_eq!(out[0..4], [1.0, 2.0, 3.0, 4.0], "forward pass");
        assert_eq!(out[5..8], [3.0, 2.0, 1.0], "reverse pass (minus endpoint)");
        assert_eq!(out[8..11], [2.0, 3.0, 4.0], "second forward pass");
    }

    #[test]
    fn test_gate_restart() {
        let buf = vec![10.0, 20.0, 30.0];
        let mut player = SamplePlayer::new(buf);
        player.set_gate(true);
        let mut out = [0.0f64; 2];
        process(&mut player, &mut out);
        assert_eq!(out, [10.0, 20.0]);
        player.set_gate(false);
        process(&mut player, &mut out);
        assert_eq!(out, [0.0, 0.0]);
        player.set_gate(true);
        process(&mut player, &mut out);
        assert_eq!(out, [10.0, 20.0]);
    }

    #[test]
    fn test_frequency_mapping() {
        let buf = vec![1.0, 2.0, 3.0, 4.0];
        let mut player = SamplePlayer::new(buf);
        player.init(44100.0);
        let freq_at_unit_rate = player.frequency();
        assert!((freq_at_unit_rate - 44100.0 / 4.0).abs() < 1.0,
            "expected ~11025 Hz at rate=1, got {}", freq_at_unit_rate);
        player.set_frequency(freq_at_unit_rate * 2.0);
        assert!((player.playback_rate() - 2.0).abs() < 1e-6,
            "expected rate=2.0, got {}", player.playback_rate());
    }

    #[test]
    fn test_empty_buffer() {
        let buf: Vec<f64> = vec![];
        let mut player = SamplePlayer::new(buf);
        player.set_gate(true);
        let mut out = [1.0f64; 4];
        process(&mut player, &mut out);
        assert_eq!(out, [0.0; 4]);
    }
}
