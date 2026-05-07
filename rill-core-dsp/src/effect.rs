//! Traits for effects

use crate::algorithm::ParameterizedAlgorithm;
use rill_core::time::ClockTick;
use rill_core::traits::ActionContext;
use rill_core::Transcendental;

/// Base trait for effects
pub trait Effect<T: Transcendental>: ParameterizedAlgorithm<T> {
    /// Get number of input channels
    fn num_inputs(&self) -> usize {
        1
    }

    /// Get number of output channels
    fn num_outputs(&self) -> usize {
        1
    }

    /// Process stereo pair (if supported)
    fn process_stereo(&mut self, left: T, right: T) -> (T, T) {
        let input = [left, right];
        let mut output = [T::ZERO, T::ZERO];
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);
        let _ = self.process(Some(&input), &mut output, &ctx);
        (output[0], output[1])
    }

    /// Process block using vector eDSL (optional)
    fn process_block_vector(&mut self, input: &[T], output: &mut [T]) {
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);
        let _ = self.process(Some(input), output, &ctx);
    }
}

/// Effect with bypass support
pub trait Bypassable<T: Transcendental>: Effect<T> {
    /// Enable/disable bypass
    fn set_bypass(&mut self, bypass: bool);

    /// Current bypass state
    fn bypass(&self) -> bool;

    /// Process with bypass consideration
    fn process_with_bypass(&mut self, input: T) -> T {
        if self.bypass() {
            input
        } else {
            let mut output = [T::ZERO];
            let tick = ClockTick::default();
            let ctx = ActionContext::new(&tick);
            let _ = self.process(Some(&[input]), &mut output, &ctx);
            output[0]
        }
    }
}

/// Effect with dry/wet support
pub trait DryWet<T: Transcendental>: Effect<T> {
    /// Set dry/wet ratio (0.0 = fully dry, 1.0 = fully wet)
    fn set_dry_wet(&mut self, mix: f32);

    /// Current dry/wet value
    fn dry_wet(&self) -> f32;

    /// Process with dry/wet consideration
    fn process_with_dry_wet(&mut self, input: T, dry: T) -> T {
        let mut wet = [T::ZERO];
        let tick = ClockTick::default();
        let ctx = ActionContext::new(&tick);
        let _ = self.process(Some(&[input]), &mut wet, &ctx);
        let mix = T::from_f32(self.dry_wet());
        let one_minus_mix = T::from_f32(1.0 - self.dry_wet());

        dry.mul(one_minus_mix).add(wet[0].mul(mix))
    }
}

/// Effect with modulation
pub trait Modulatable<T: Transcendental>: Effect<T> {
    /// Number of modulation inputs
    fn num_mod_inputs(&self) -> usize;

    /// Apply modulation
    fn apply_modulation(&mut self, index: usize, value: T);

    /// Modulation depth
    fn modulation_depth(&self, index: usize) -> f32;

    /// Set modulation depth
    fn set_modulation_depth(&mut self, index: usize, depth: f32);
}
