//! ParamSmoother — one-pole smoother that implements `Algorithm<T>`.
//!
//! This is useful for smoothing parameter changes to avoid zipper noise.

use rill_core::math::Transcendental;
use rill_core::traits::ProcessResult;
use rill_core::traits::{Algorithm, AlgorithmCategory, AlgorithmMetadata};

/// One-pole exponential smoother that implements `Algorithm<T>`.
///
/// Receives target values via `apply_command(value)`. Each `process()` call
/// steps the current value toward the target using the smoothing coefficient.
///
/// # Example
/// ```rust
/// use rill_core_dsp::smoothing::ParamSmoother;
/// use rill_core::traits::Algorithm;
/// use rill_core::time::ClockTick;
/// use rill_core::traits::ActionContext;
///
/// let mut smoother = ParamSmoother::new(0.1);
/// let tick = ClockTick::default();
/// let ctx = ActionContext::new(&tick);
///
/// smoother.apply_command(1.0);
/// let mut output = [0.0f32; 4];
/// smoother.process(None, &mut output).unwrap();
/// // output approaches 1.0 via exponential smoothing
/// ```
#[derive(Debug, Clone)]
pub struct ParamSmoother<T: Transcendental> {
    /// Current (smoothed) value
    current: T,
    /// Target value
    target: T,
    /// Smoothing coefficient (0.0 = no smoothing, 1.0 = instant)
    coeff: T,
}

impl<T: Transcendental> ParamSmoother<T> {
    /// Create a new smoother with the given coefficient.
    ///
    /// `coeff` should be in (0, 1]. Lower values = slower smoothing.
    pub fn new(coeff: T) -> Self {
        Self {
            current: T::ZERO,
            target: T::ZERO,
            coeff,
        }
    }

    /// Set the smoothing coefficient.
    pub fn set_coeff(&mut self, coeff: T) {
        self.coeff = coeff;
    }

    /// Get the current smoothed value (without processing).
    pub fn current(&self) -> T {
        self.current
    }

    /// Get the current target value.
    pub fn target(&self) -> T {
        self.target
    }

    /// Immediately snap to a value (skip smoothing).
    pub fn snap_to(&mut self, value: T) {
        self.current = value;
        self.target = value;
    }

    /// Process a single sample value (useful outside the Algorithm interface).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> T {
        let diff = self.target - self.current;
        let step = diff * self.coeff;
        self.current += step;
        self.current
    }
}

impl<T: Transcendental> Algorithm<T> for ParamSmoother<T> {
    fn process(&mut self, _input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        for sample in output.iter_mut() {
            *sample = self.next();
        }
        Ok(())
    }

    fn apply_command(&mut self, value: T) {
        self.target = value;
    }

    fn init(&mut self, _sample_rate: f32) {}

    fn reset(&mut self) {
        self.current = T::ZERO;
        self.target = T::ZERO;
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "ParamSmoother",
            category: AlgorithmCategory::Utility,
            description: "One-pole smoother for zipper-free parameter transitions",
            author: "Rill",
            version: "0.1.0",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smoother_basic() {
        let mut s = ParamSmoother::new(0.5f32);

        s.apply_command(1.0);
        let mut buf = [0.0f32; 4];
        s.process(None, &mut buf).unwrap();
        // 0 + (1-0)*0.5 = 0.5
        assert!((buf[0] - 0.5).abs() < 1e-6);
        // 0.5 + (1-0.5)*0.5 = 0.75
        assert!((buf[1] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn test_smoother_snap() {
        let mut s = ParamSmoother::new(0.1f32);
        s.snap_to(42.0);
        assert!((s.current() - 42.0).abs() < 1e-6);
        assert!((s.target() - 42.0).abs() < 1e-6);
    }

    #[test]
    fn test_smoother_empty_block() {
        let mut s = ParamSmoother::new(0.1f32);
        let buf: &mut [f32] = &mut [];
        assert!(s.process(None, buf).is_ok());
    }
}
