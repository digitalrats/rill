//! Builtin wrapper for the rill-core-model WDF analog filter (feature `model`).

use crate::builtin::BlockBuiltin;
use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, ParamValue, ProcessResult};

/// Block-builtin wrapper around the WDF `MoogLadder`. Moved from
/// `rill-core-model/src/register.rs` (SP-3b Task 9): the algorithm lives in
/// `rill-core-model` (algorithms-only), rill-lang supplies the `BlockBuiltin`
/// adapter so the DSL can expose `analog_moog`.
pub(crate) struct AnalogMoogBuiltin<T: Transcendental> {
    pub(crate) inner: rill_core_model::wdf::MoogLadder<T>,
}

impl<T: Transcendental> Algorithm<T> for AnalogMoogBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.inner.process(input, output)
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
    fn init(&mut self, sample_rate: f32) {
        Algorithm::init(&mut self.inner, sample_rate);
    }
    fn apply_command(&mut self, value: T) {
        Algorithm::apply_command(&mut self.inner, value);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for AnalogMoogBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = T::from_f32(crate::builtins::pv_f32(value));
        match index {
            0 => self.inner.set_cutoff(v),
            1 => self.inner.set_resonance(v),
            _ => {}
        }
    }
}
