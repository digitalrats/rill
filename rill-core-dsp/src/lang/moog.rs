use rill_core::builtin::SampleBuiltin;
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;

use crate::filters::MoogLadder;
use crate::lang::pv_f32;

pub struct MoogBuiltin<T: Transcendental> {
    pub inner: MoogLadder<T>,
}

impl<T: Transcendental> SampleBuiltin<T> for MoogBuiltin<T> {
    fn process_sample(&mut self, inputs: &[T]) -> T {
        self.inner.process_sample(inputs[0])
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.inner, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => self.inner.set_cutoff(v),
            1 => self.inner.set_resonance(v),
            _ => {}
        }
    }
}
