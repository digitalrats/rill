use rill_core::builtin::BlockBuiltin;
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::ProcessResult;

use crate::generators::{Generator, NoiseGenerator};
use crate::lang::pv_f32;

pub struct NoiseGenBuiltin<T: Transcendental> {
    pub gen: NoiseGenerator<T>,
}

impl<T: Transcendental> Algorithm<T> for NoiseGenBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.gen.process(input, output)
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.gen, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.gen);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for NoiseGenBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        if index == 1 {
            self.gen.set_amplitude(T::from_f32(pv_f32(value)));
        }
    }
}
