//! ControlMapper — maps normalized [0,1] control values to parameter ranges.
//!
//! Provides `MappingStrategy` to select the mapping curve and `ControlMapper<T>`
//! which implements `Algorithm<T>`.

use rill_core::math::AudioNum;
use rill_core::traits::ProcessResult;
use rill_core::traits::{ActionContext, Algorithm, AlgorithmCategory, AlgorithmMetadata};

/// Mapping strategy for translating normalized [0,1] values to a parameter range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MappingStrategy {
    /// Linear mapping: `min + value * (max - min)`
    Linear,
    /// Exponential mapping: `min + value^exp * (max - min)`
    Exponential { exponent: f32 },
    /// Logarithmic mapping: `min + log(1 + value * (e - 1)) / log(e) * (max - min)`
    Logarithmic,
    /// Inverted linear mapping: `max - value * (max - min)`
    Inverted,
}

impl MappingStrategy {
    /// Map a normalized value `x` in [0,1] to [min, max] using this strategy.
    pub fn map<T: AudioNum>(&self, x: T, min: T, max: T) -> T {
        let xf: f32 = x.to_f32();
        let minf: f32 = min.to_f32();
        let maxf: f32 = max.to_f32();
        let range = maxf - minf;
        let result = match self {
            MappingStrategy::Linear => minf + xf * range,
            MappingStrategy::Exponential { exponent } => minf + xf.powf(*exponent) * range,
            MappingStrategy::Logarithmic => {
                let one = 1.0f32;
                let mapped =
                    (one + xf * (core::f32::consts::E - one)).ln() / core::f32::consts::E.ln();
                minf + mapped * range
            }
            MappingStrategy::Inverted => maxf - xf * range,
        };
        T::from_f32(result)
    }
}

/// Maps an incoming normalized control value [0,1] to a parameter range
/// using a `MappingStrategy`.
///
/// Implements `Algorithm<T>`. The input (if present) is treated as the
/// normalized value; when `input` is `None` (source mode), the value
/// received via `apply_command()` is used instead.
///
/// # Example
/// ```rust
/// use rill_core_dsp::mapping::{ControlMapper, MappingStrategy};
/// use rill_core::traits::Algorithm;
/// use rill_core::time::ClockTick;
/// use rill_core::traits::ActionContext;
///
/// let mut mapper = ControlMapper::new(20.0, 20000.0, MappingStrategy::Exponential { exponent: 2.0 });
/// let tick = ClockTick::default();
/// let ctx = ActionContext::new(&tick);
///
/// // Use apply_command to set the incoming value
/// mapper.apply_command(0.5);    // halfway in normalized range
/// let mut output = [0.0f32; 1];
/// mapper.process(None, &mut output, &ctx).unwrap();
/// // output maps 0.5 exponentially between 20..20000
/// ```
#[derive(Debug, Clone)]
pub struct ControlMapper<T: AudioNum> {
    /// Minimum of the output range
    min: T,
    /// Maximum of the output range
    max: T,
    /// Mapping strategy
    strategy: MappingStrategy,
    /// Current incoming normalized value
    value: T,
}

impl<T: AudioNum> ControlMapper<T> {
    /// Create a new `ControlMapper`.
    pub fn new(min: T, max: T, strategy: MappingStrategy) -> Self {
        Self {
            min,
            max,
            strategy,
            value: T::ZERO,
        }
    }

    /// Update the mapping range.
    pub fn set_range(&mut self, min: T, max: T) {
        self.min = min;
        self.max = max;
    }

    /// Update the mapping strategy.
    pub fn set_strategy(&mut self, strategy: MappingStrategy) {
        self.strategy = strategy;
    }

    /// Get the current mapped value (without processing).
    pub fn current_mapped(&self) -> T {
        self.strategy.map(self.value, self.min, self.max)
    }
}

impl<T: AudioNum> Algorithm<T> for ControlMapper<T> {
    fn process(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        for (i, sample) in output.iter_mut().enumerate() {
            // If input is available, use it as the normalized value.
            // Otherwise, use the value set by apply_command.
            let normalized = match input {
                Some(buf) => {
                    if i < buf.len() {
                        buf[i]
                    } else {
                        self.value
                    }
                }
                None => self.value,
            };
            *sample = self.strategy.map(normalized, self.min, self.max);
        }
        Ok(())
    }

    fn apply_command(&mut self, value: T) {
        self.value = value;
    }

    fn init(&mut self, _sample_rate: f32) {}

    fn reset(&mut self) {
        self.value = T::ZERO;
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "ControlMapper",
            category: AlgorithmCategory::Utility,
            description: "Maps normalized [0,1] control values to a parameter range",
            author: "Rill",
            version: "0.1.0",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::time::ClockTick;

    #[test]
    fn test_linear_mapping() {
        let mapper = ControlMapper::new(0.0f32, 100.0, MappingStrategy::Linear);
        assert!((mapper.current_mapped() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_mapping_strategies() {
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);

        let mut mapper = ControlMapper::new(0.0f32, 100.0, MappingStrategy::Linear);
        mapper.apply_command(0.5);
        let mut out = [0.0f32];
        mapper.process(None, &mut out, &ctx).unwrap();
        assert!((out[0] - 50.0).abs() < 1e-6);

        mapper.set_strategy(MappingStrategy::Inverted);
        mapper.apply_command(0.5);
        mapper.process(None, &mut out, &ctx).unwrap();
        assert!((out[0] - 50.0).abs() < 1e-6);

        mapper.set_strategy(MappingStrategy::Exponential { exponent: 2.0 });
        mapper.apply_command(0.5); // 0.5^2 = 0.25, 0..100 => 25
        mapper.process(None, &mut out, &ctx).unwrap();
        assert!((out[0] - 25.0).abs() < 1e-6);
    }

    #[test]
    fn test_mapping_with_input() {
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);

        let mut mapper = ControlMapper::new(0.0f32, 100.0, MappingStrategy::Linear);
        let input = [0.25f32, 0.75f32];
        let mut output = [0.0f32; 2];
        mapper.process(Some(&input), &mut output, &ctx).unwrap();
        assert!((output[0] - 25.0).abs() < 1e-6);
        assert!((output[1] - 75.0).abs() < 1e-6);
    }

    #[test]
    fn test_log_mapping_bounds() {
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);

        let mut mapper = ControlMapper::new(20.0f32, 20000.0, MappingStrategy::Logarithmic);
        mapper.apply_command(0.0);
        let mut out = [0.0f32];
        mapper.process(None, &mut out, &ctx).unwrap();
        assert!((out[0] - 20.0).abs() < 1.0);

        mapper.apply_command(1.0);
        mapper.process(None, &mut out, &ctx).unwrap();
        assert!((out[0] - 20000.0).abs() < 1.0);
    }
}
