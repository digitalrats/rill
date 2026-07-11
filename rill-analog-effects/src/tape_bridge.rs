use rill_core::math::Transcendental;
use rill_core::traits::bridge::BridgeAlgorithm;
use rill_core::traits::{Algorithm, ProcessResult};

/// Configuration for a single tape head.
pub struct HeadConfig {
    /// Position in samples from the write head (delay).
    pub position: usize,
    /// Output gain (0.0–1.0).
    pub gain: f64,
    /// Optional decorator chain applied to the signal passing through this head.
    pub decorators: Vec<Box<dyn Algorithm<f32>>>,
}

/// Unified tape bridge: write_head + tape loop + multiple read_heads.
///
/// Head 0 is the write head; all remaining heads are read heads.
/// Each read head outputs a stereo pair (L/R = same signal).
pub struct TapeBridgeAlgorithm<T: Transcendental> {
    /// Circular tape buffer.
    tape: Vec<T>,
    /// Current write position.
    write_pos: usize,
    /// Tape capacity in samples.
    capacity: usize,
    /// Head configurations (index 0 = write head, the rest are read heads).
    heads: Vec<HeadConfig>,
    /// Sample rate for decorator `init`.
    sample_rate: f32,
}

impl<T: Transcendental> TapeBridgeAlgorithm<T> {
    /// Create a new tape bridge with the given tape capacity and head configurations.
    pub fn new(capacity: usize, heads: Vec<HeadConfig>) -> Self {
        Self {
            tape: vec![T::ZERO; capacity],
            write_pos: 0,
            capacity,
            heads,
            sample_rate: 44100.0,
        }
    }
}

impl<T: Transcendental> BridgeAlgorithm<T> for TapeBridgeAlgorithm<T> {
    fn num_inputs(&self) -> usize {
        1
    }

    fn num_outputs(&self) -> usize {
        let n_read_heads = self.heads.len().saturating_sub(1);
        n_read_heads * 2
    }

    fn process_left(&mut self, inputs: &[&[T]]) -> ProcessResult<()> {
        let n_samples = inputs[0].len();
        let write_gain = self.heads[0].gain;
        let write_capacity = self.capacity;

        for sample in 0..n_samples {
            let mut signal = inputs[0][sample];

            let n_decos = self.heads[0].decorators.len();
            for i in 0..n_decos {
                let mut single = [0.0f32];
                self.heads[0].decorators[i].process(Some(&[signal.to_f32()]), &mut single)?;
                signal = T::from_f32(single[0]);
            }

            signal = signal.mul(T::from_f64(write_gain));
            self.tape[self.write_pos] = signal;
            self.write_pos = (self.write_pos + 1) % write_capacity;
        }
        Ok(())
    }

    fn process_right(&mut self, outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        let n_samples = outputs[0].len();
        let n_read_heads = self.heads.len().saturating_sub(1);
        let capacity = self.capacity;
        let write_pos = self.write_pos;

        for sample in 0..n_samples {
            for head_idx in 0..n_read_heads {
                let read_head_idx = head_idx + 1;
                let pos = self.heads[read_head_idx]
                    .position
                    .min(capacity.saturating_sub(1));
                let gain = self.heads[read_head_idx].gain;
                let tape_idx = (write_pos + capacity - pos) % capacity;
                let mut signal = self.tape[tape_idx];

                let n_decos = self.heads[read_head_idx].decorators.len();
                for i in 0..n_decos {
                    let mut single = [0.0f32];
                    self.heads[read_head_idx].decorators[i]
                        .process(Some(&[signal.to_f32()]), &mut single)?;
                    signal = T::from_f32(single[0]);
                }

                signal = signal.mul(T::from_f64(gain));

                let l_idx = head_idx * 2;
                let r_idx = head_idx * 2 + 1;
                outputs[l_idx][sample] = signal;
                outputs[r_idx][sample] = signal;
            }
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.tape.fill(T::ZERO);
        self.write_pos = 0;
    }
}

impl<T: Transcendental> Algorithm<T> for TapeBridgeAlgorithm<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        if let Some(inp) = input {
            let len = inp.len().min(output.len());
            output[..len].copy_from_slice(&inp[..len]);
            output[len..].fill(T::ZERO);
        } else {
            output.fill(T::ZERO);
        }
        Ok(())
    }

    fn init(&mut self, sr: f32) {
        self.sample_rate = sr;
    }

    fn reset(&mut self) {
        BridgeAlgorithm::reset(self);
    }
}
