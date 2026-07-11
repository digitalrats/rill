use rill_core::builtin::BlockBuiltin;
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::ProcessResult;

use crate::algorithm::ParameterizedAlgorithm;
use crate::filters::{Biquad, Filter, FilterType};
use crate::lang::pv_f32;

pub struct BiquadBuiltin<T: Transcendental> {
    pub inner: Biquad<T>,
}

impl<T: Transcendental> Algorithm<T> for BiquadBuiltin<T> {
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

impl<T: Transcendental> BlockBuiltin<T> for BiquadBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => Filter::set_cutoff(&mut self.inner, v),
            1 => Filter::set_q(&mut self.inner, v),
            _ => {}
        }
    }
}

pub struct GeneralBiquadBuiltin<T: Transcendental> {
    pub inner: Biquad<T>,
}

impl<T: Transcendental> Algorithm<T> for GeneralBiquadBuiltin<T> {
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

impl<T: Transcendental> BlockBuiltin<T> for GeneralBiquadBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => {
                let ft = match v as u8 {
                    0 => FilterType::LowPass,
                    1 => FilterType::HighPass,
                    2 => FilterType::BandPass,
                    3 => FilterType::Notch,
                    4 => FilterType::Peak,
                    5 => FilterType::LowShelf,
                    6 => FilterType::HighShelf,
                    _ => FilterType::LowPass,
                };
                let mut params = self.inner.params().clone();
                params.filter_type = ft;
                self.inner.set_params(params);
            }
            1 => Filter::set_cutoff(&mut self.inner, v),
            2 => Filter::set_q(&mut self.inner, v),
            3 => Filter::set_gain_db(&mut self.inner, v),
            _ => {}
        }
    }
}
