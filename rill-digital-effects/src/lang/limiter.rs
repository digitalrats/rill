use rill_core::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::ProcessResult;

use super::pv_f32;

struct LimiterBuiltin<T: Transcendental, const BUF_SIZE: usize> {
    inner: crate::Limiter<T, BUF_SIZE>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T> for LimiterBuiltin<T, BUF_SIZE> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                self.inner.process_block(inp, output);
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {
        Node::reset(&mut self.inner);
    }
    fn init(&mut self, sample_rate: f32) {
        Node::init(&mut self.inner, sample_rate);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> BlockBuiltin<T> for LimiterBuiltin<T, BUF_SIZE> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => self.inner.set_threshold(v),
            1 => self.inner.set_release(v),
            _ => {}
        }
    }
}

pub fn register_limiter_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("limiter", 1, 1, 2, BuiltinKind::Block),
        |p, sr| {
            let mut l = crate::Limiter::<T, 64>::new(sr, p[0] as f32, 1.0, p[1] as f32, 0.0);
            Node::init(&mut l, sr);
            Box::new(LimiterBuiltin::<T, 64> { inner: l })
        },
    );
}
