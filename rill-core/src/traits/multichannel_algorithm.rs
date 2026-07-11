use crate::math::Transcendental;
use crate::traits::ProcessResult;

/// A signal processing algorithm with multiple inputs and outputs.
///
/// Unlike `Algorithm<T>` which is strictly single-input/single-output (SISO),
/// this trait supports N-to-M channel processing in a single call.
pub trait MultichannelAlgorithm<T: Transcendental>: Send {
    /// Number of signal input channels.
    fn num_inputs(&self) -> usize;

    /// Number of signal output channels.
    fn num_outputs(&self) -> usize;

    /// Process one buffer of samples.
    ///
    /// - `inputs.len() == num_inputs()`
    /// - `outputs.len() == num_outputs()`
    /// - Each inner slice has exactly BUF_SIZE samples (determined by the caller).
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()>;

    /// Reset internal state.
    fn reset(&mut self);
}

/// Adapter: wrap a SISO Algorithm as a MultichannelAlgorithm.
///
/// Useful for mixed graphs where most nodes are SISO but some are multi-IO.
pub struct SisoAdapter<A, T: Transcendental> {
    /// The wrapped SISO algorithm.
    pub inner: A,
    _phantom: std::marker::PhantomData<T>,
}

impl<A, T: Transcendental> SisoAdapter<A, T>
where
    A: crate::traits::Algorithm<T>,
{
    /// Create a new adapter wrapping a SISO algorithm.
    pub fn new(inner: A) -> Self {
        Self {
            inner,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<A, T: Transcendental> MultichannelAlgorithm<T> for SisoAdapter<A, T>
where
    A: crate::traits::Algorithm<T>,
{
    fn num_inputs(&self) -> usize {
        1
    }

    fn num_outputs(&self) -> usize {
        1
    }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        let input = if inputs.is_empty() {
            None
        } else {
            Some(inputs[0])
        };
        crate::traits::Algorithm::process(&mut self.inner, input, outputs[0])
    }

    fn reset(&mut self) {
        crate::traits::Algorithm::reset(&mut self.inner);
    }
}
