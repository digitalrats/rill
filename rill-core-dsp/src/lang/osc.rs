use rill_core::builtin::BlockBuiltin;
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::ProcessResult;

use crate::generators::{BasicOscillator, Generator};
use crate::lang::pv_f32;

pub struct OscBuiltin<T: Transcendental> {
    pub osc: BasicOscillator<T>,
}

impl<T: Transcendental> Algorithm<T> for OscBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.osc.process(input, output)
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.osc, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.osc);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for OscBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        match index {
            0 => self.osc.set_frequency(pv_f32(value)),
            1 => self.osc.set_amplitude(T::from_f32(pv_f32(value))),
            2 => self.osc.set_phase(T::from_f32(pv_f32(value))),
            _ => {}
        }
    }
}
