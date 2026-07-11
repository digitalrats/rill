/// rill-lang builtins for rill-sampler.
use rill_core::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};
use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, ParamValue, ProcessResult};
use rill_core_dsp::generators::SamplePlayer;

struct SamplerBuiltin<T: Transcendental> {
    inner: SamplePlayer<T>,
    amplitude: T,
}

impl<T: Transcendental> Algorithm<T> for SamplerBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.inner.process(input, output)?;
        if self.amplitude != T::ONE {
            for s in output.iter_mut() {
                *s *= self.amplitude;
            }
        }
        Ok(())
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for SamplerBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = value.as_f32().unwrap_or(0.0);
        match index {
            0 => {
                self.inner.set_gate(v > 0.0);
            }
            1 => {
                self.inner.set_playback_rate(v as f64);
            }
            2 => {
                self.amplitude = T::from_f32(v).clamp(T::ZERO, T::ONE);
            }
            3 => {
                self.inner.set_cubic(v > 0.0);
            }
            _ => {}
        }
    }
}

pub fn register_sampler_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("sampler", 0, 1, 4, BuiltinKind::Block),
        |p, _sr| {
            let mut player = SamplePlayer::new(Vec::new());
            player.set_gate(p[0] > 0.0);
            player.set_playback_rate(p[1].clamp(0.0, 4.0));
            player.set_cubic(p[3] > 0.0);
            Box::new(SamplerBuiltin {
                inner: player,
                amplitude: T::from_f64(p[2].clamp(0.0, 1.0)),
            })
        },
    );
}
