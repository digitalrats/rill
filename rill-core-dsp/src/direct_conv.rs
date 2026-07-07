// rill-core-dsp/src/direct_conv.rs
//! Direct (time-domain) convolution.
//!
//! Efficient for short impulse responses where FFT-based convolution
//! overhead would dominate. For longer IRs, use `OverlapAddConvolver`
//! or `PartitionedConvolver` from `rill-fft`.

use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;

/// Direct time-domain convolver.
///
/// Uses a ring buffer for input history. All memory is stack-allocated
/// via const generics — zero heap allocations in `process()`.
///
/// # Type parameters
///
/// - `T` — sample type (`f32` or `f64`)
/// - `IR_LEN` — length of the impulse response
/// - `BUF_SIZE` — processing block size
pub struct DirectConvolver<T: Transcendental, const IR_LEN: usize, const BUF_SIZE: usize> {
    ir: [T; IR_LEN],
    delay_line: [T; IR_LEN],
    write_head: usize,
}

impl<T: Transcendental, const IR_LEN: usize, const BUF_SIZE: usize>
    DirectConvolver<T, IR_LEN, BUF_SIZE>
{
    /// Create a new direct convolver with a zero impulse response.
    pub fn new() -> Self {
        Self {
            ir: [T::ZERO; IR_LEN],
            delay_line: [T::ZERO; IR_LEN],
            write_head: 0,
        }
    }

    /// Set the impulse response.
    ///
    /// `ir` must have exactly `IR_LEN` elements. Extra elements are ignored;
    /// if shorter, remaining taps are zeroed.
    pub fn set_ir(&mut self, ir: &[T]) {
        let len = ir.len().min(IR_LEN);
        self.ir[..len].copy_from_slice(&ir[..len]);
        for i in len..IR_LEN {
            self.ir[i] = T::ZERO;
        }
    }

    /// Returns the impulse response length.
    pub fn ir_len(&self) -> usize {
        IR_LEN
    }
}

impl<T: Transcendental, const IR_LEN: usize, const BUF_SIZE: usize> Default
    for DirectConvolver<T, IR_LEN, BUF_SIZE>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const IR_LEN: usize, const BUF_SIZE: usize> Algorithm<T>
    for DirectConvolver<T, IR_LEN, BUF_SIZE>
{
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(samples) => {
                for (out, &inp) in output.iter_mut().zip(samples.iter()) {
                    self.delay_line[self.write_head] = inp;
                    let mut acc = T::ZERO;
                    for k in 0..IR_LEN {
                        let idx = if self.write_head >= k {
                            self.write_head - k
                        } else {
                            IR_LEN + self.write_head - k
                        };
                        acc += self.delay_line[idx] * self.ir[k];
                    }
                    *out = acc;
                    self.write_head = (self.write_head + 1) % IR_LEN;
                }
                Ok(())
            }
            None => {
                output.fill(T::ZERO);
                Ok(())
            }
        }
    }

    fn reset(&mut self) {
        self.delay_line.fill(T::ZERO);
        self.write_head = 0;
        self.ir.fill(T::ZERO);
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "DirectConvolver",
            category: AlgorithmCategory::Effect,
            description: "Direct time-domain convolution",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unit_impulse_is_passthrough() {
        let mut conv = DirectConvolver::<f32, 4, 8>::new();
        conv.set_ir(&[1.0, 0.0, 0.0, 0.0]);

        let input = [0.5f32, 0.3, -0.2, 0.8, 0.1, -0.5, 0.4, 0.0];
        let mut output = [0.0f32; 8];
        conv.process(Some(&input), &mut output).unwrap();

        for (i, o) in input.iter().zip(output.iter()) {
            assert!((i - o).abs() < 1e-6, "expected {i}, got {o}");
        }
    }

    #[test]
    fn test_delayed_impulse_is_delay() {
        let mut conv = DirectConvolver::<f32, 4, 8>::new();
        conv.set_ir(&[0.0, 0.0, 1.0, 0.0]);

        let input = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut output = [0.0f32; 8];
        conv.process(Some(&input), &mut output).unwrap();

        assert!((output[0] - 0.0).abs() < 1e-6);
        assert!((output[1] - 0.0).abs() < 1e-6);
        assert!((output[2] - 1.0).abs() < 1e-6);
        assert!((output[3] - 2.0).abs() < 1e-6);
        assert!((output[4] - 3.0).abs() < 1e-6);
        assert!((output[5] - 4.0).abs() < 1e-6);
        assert!((output[6] - 5.0).abs() < 1e-6);
        assert!((output[7] - 6.0).abs() < 1e-6);
    }

    #[test]
    fn test_identity_convolution() {
        let mut conv = DirectConvolver::<f32, 3, 4>::new();
        conv.set_ir(&[1.0, 0.0, 0.0]);

        let input = [2.0f32, 3.0, 4.0, 5.0];
        let mut output = [0.0f32; 4];
        conv.process(Some(&input), &mut output).unwrap();

        for (i, o) in input.iter().zip(output.iter()) {
            assert!((i - o).abs() < 1e-6);
        }
    }

    #[test]
    fn test_averaging_ir() {
        let mut conv = DirectConvolver::<f32, 2, 4>::new();
        conv.set_ir(&[0.5, 0.5]);

        let input = [1.0f32, 0.0, 0.0, 0.0];
        let mut output = [0.0f32; 4];
        conv.process(Some(&input), &mut output).unwrap();

        assert!((output[0] - 0.5).abs() < 1e-6);
        assert!((output[1] - 0.5).abs() < 1e-6);
        assert!((output[2] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_no_input_zeroes_output() {
        let mut conv = DirectConvolver::<f32, 4, 4>::new();
        conv.set_ir(&[1.0, 0.5, 0.25, 0.125]);
        let mut output = [1.0f32; 4];
        conv.process(None, &mut output).unwrap();

        for o in output.iter() {
            assert!((o - 0.0).abs() < 1e-6);
        }
    }

    #[test]
    fn test_reset_clears_state() {
        let mut conv = DirectConvolver::<f32, 4, 4>::new();
        conv.set_ir(&[1.0, 1.0, 1.0, 1.0]);

        let input = [1.0f32; 4];
        let mut output = [0.0f32; 4];
        conv.process(Some(&input), &mut output).unwrap();

        conv.reset();
        let input2 = [0.0f32; 4];
        conv.process(Some(&input2), &mut output).unwrap();

        for o in output.iter() {
            assert!((o - 0.0).abs() < 1e-6);
        }
    }

    #[test]
    fn test_ir_shorter_than_ir_len_zeros_remainder() {
        let mut conv = DirectConvolver::<f32, 4, 4>::new();
        conv.set_ir(&[1.0, 2.0]);

        let input = [1.0f32, 2.0, 3.0, 4.0];
        let mut output = [0.0f32; 4];
        conv.process(Some(&input), &mut output).unwrap();

        let mut conv2 = DirectConvolver::<f32, 4, 4>::new();
        conv2.set_ir(&[1.0, 2.0, 0.0, 0.0]);

        let mut output2 = [0.0f32; 4];
        conv2.process(Some(&input), &mut output2).unwrap();

        for (o1, o2) in output.iter().zip(output2.iter()) {
            assert!((o1 - o2).abs() < 1e-6);
        }
    }
}
