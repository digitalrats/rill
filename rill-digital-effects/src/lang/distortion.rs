use rill_core::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::{Node, ProcessResult};

use super::pv_f32;
use crate::DistortionType;

struct DistortionBuiltin<T: Transcendental, const BUF_SIZE: usize> {
    inner: crate::Distortion<T, BUF_SIZE>,
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T> for DistortionBuiltin<T, BUF_SIZE> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                for (i, out) in output.iter_mut().enumerate() {
                    *out = self.inner.process_sample(inp[i.min(inp.len() - 1)]);
                }
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

impl<T: Transcendental, const BUF_SIZE: usize> BlockBuiltin<T> for DistortionBuiltin<T, BUF_SIZE> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => self.inner.set_drive(v),
            1 => self.inner.set_output_gain(v),
            _ => {}
        }
    }
}

pub fn register_distortion_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("distortion", 1, 1, 2, BuiltinKind::Block),
        |p, sr| {
            let mut d = crate::Distortion::<T, 64>::with_params(
                sr,
                DistortionType::SoftClip,
                p[0] as f32,
                1.0,
            );
            d.set_output_gain(p[1] as f32);
            Node::init(&mut d, sr);
            Box::new(DistortionBuiltin::<T, 64> { inner: d })
        },
    );
}
