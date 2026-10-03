use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, ParamValue, ProcessResult};
use rill_core_dsp::generators::SamplePlayer;
/// rill-lang builtins for rill-sampler.
use rill_lang::builtin::{BlockBuiltin, Registry};

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
            4 => {
                if let ParamValue::SignalSlab(ref slab) = value {
                    if let Some(first_ch) = slab.channels.first() {
                        let buffer: Vec<T> = first_ch.iter().map(|&s| T::from_f32(s)).collect();
                        self.inner.set_buffer(buffer);
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn register_sampler_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    reg.register_block("sampler", |p, _sr| {
        let mut player = SamplePlayer::new(Vec::new());
        player.set_gate(p[0] > 0.0);
        player.set_playback_rate(p[1].clamp(0.0, 4.0));
        player.set_cubic(p[3] > 0.0);
        Box::new(SamplerBuiltin {
            inner: player,
            amplitude: T::from_f64(p[2].clamp(0.0, 1.0)),
        })
    });
}

/// Register the sampler builtin as an FFI factory (for `compile_with_ffi`).
pub fn register_foreign_sampler_builtins<T: Transcendental + 'static>(
    ffi: &mut rill_lang::ffi::ForeignRegistry<T>,
) {
    ffi.register_block("sampler", |p: &[f64], _sr: f32| {
        let mut player = SamplePlayer::new(Vec::new());
        player.set_gate(p[0] > 0.0);
        player.set_playback_rate(p[1].clamp(0.0, 4.0));
        player.set_cubic(p[3] > 0.0);
        Box::new(SamplerBuiltin {
            inner: player,
            amplitude: T::from_f64(p[2].clamp(0.0, 1.0)),
        })
    });
}
