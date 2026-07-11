use rill_core::builtin::SampleBuiltin;
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;

use crate::filters::{Filter, OnePole};
use crate::lang::pv_f32;

pub struct OnePoleBuiltin<T: Transcendental> {
    pub inner: OnePole<T>,
}

impl<T: Transcendental> SampleBuiltin<T> for OnePoleBuiltin<T> {
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
            0 => Filter::set_cutoff(&mut self.inner, v),
            1 => Filter::set_q(&mut self.inner, v),
            _ => {}
        }
    }
}
