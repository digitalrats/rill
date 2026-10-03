use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::ProcessResult;
use rill_lang::builtin::BlockBuiltin;

use crate::filters::MoogLadder;
use crate::lang::pv_f32;

pub struct MoogBuiltin<T: Transcendental> {
    pub inner: MoogLadder<T>,
}

impl<T: Transcendental> Algorithm<T> for MoogBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        Algorithm::process(&mut self.inner, input, output)
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.inner, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for MoogBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => self.inner.set_cutoff(v),
            1 => self.inner.set_resonance(v),
            _ => {}
        }
    }
}
