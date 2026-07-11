use rill_core::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::ProcessResult;

use super::pv_f32;

struct DelayBuiltin<T: Transcendental, const BUF_SIZE: usize> {
    inner: crate::Delay<T, BUF_SIZE>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T> for DelayBuiltin<T, BUF_SIZE> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        Algorithm::process(&mut self.inner, input, output)
    }

    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }

    fn init(&mut self, sample_rate: f32) {
        Algorithm::init(&mut self.inner, sample_rate);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> BlockBuiltin<T> for DelayBuiltin<T, BUF_SIZE> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => self.inner.set_delay_time(v),
            1 => self.inner.set_feedback(v),
            2 => self.inner.set_mix(v),
            _ => {}
        }
    }
}

pub fn register_delay_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("delay", 1, 1, 3, BuiltinKind::Block),
        |p, sr| {
            let mut d =
                crate::Delay::<T, 64>::with_params(sr, p[0] as f32, p[1] as f32, p[2] as f32);
            Algorithm::init(&mut d, sr);
            Box::new(DelayBuiltin::<T, 64> { inner: d })
        },
    );
}
