#![cfg(feature = "lang")]
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::{ParamValue, ProcessResult};
use rill_lang::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};

fn pv_f32(v: &ParamValue) -> f32 {
    match v {
        ParamValue::Float(f) => *f,
        ParamValue::Int(i) => *i as f32,
        _ => 0.0,
    }
}

struct SpectralGateBuiltin<T: Transcendental> {
    inner: crate::effects::spectral_gate::SpectralGate<T, 64>,
}

impl<T: Transcendental> Algorithm<T> for SpectralGateBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        Algorithm::process(&mut self.inner, input, output)
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for SpectralGateBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = T::from_f32(pv_f32(value));
        match index {
            0 => self.inner.set_threshold(v),
            1 => self.inner.set_ratio(pv_f32(value)),
            _ => {}
        }
    }
}

struct SpectralDelayBuiltin<T: Transcendental> {
    inner: crate::effects::spectral_delay::SpectralDelay<T, 64, 16>,
}

impl<T: Transcendental> Algorithm<T> for SpectralDelayBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        Algorithm::process(&mut self.inner, input, output)
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for SpectralDelayBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => self.inner.set_mix(v),
            1 => self.inner.set_feedback(v),
            _ => {}
        }
    }
}

struct ConvolverBuiltin<T: Transcendental> {
    inner: crate::partitioned_conv::PartitionedConvolver<T, 64>,
    ir_gain: f32,
    mix: f32,
}

impl<T: Transcendental> Algorithm<T> for ConvolverBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                self.inner.process(inp, output);
                let gain = T::from_f32(self.ir_gain);
                let mix_gain = T::from_f32(self.mix);
                let dry_gain = T::ONE - mix_gain;
                for i in 0..output.len() {
                    output[i] = inp[i] * dry_gain + output[i] * gain * mix_gain;
                }
                Ok(())
            }
            None => {
                output.fill(T::ZERO);
                Ok(())
            }
        }
    }
    fn reset(&mut self) {}
}

impl<T: Transcendental> BlockBuiltin<T> for ConvolverBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = value.as_f32().unwrap_or(0.0);
        match index {
            0 => {
                self.ir_gain = v.clamp(0.0, 4.0);
            }
            1 => {
                self.mix = v.clamp(0.0, 1.0);
            }
            _ => {}
        }
    }
}

pub fn register_fft_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("spectralgate", 1, 1, 2, BuiltinKind::Block),
        |p, _sr| {
            let mut gate = crate::effects::spectral_gate::SpectralGate::<T, 64>::new();
            gate.set_threshold(T::from_f64(p[0]));
            gate.set_ratio(p[1] as f32);
            Box::new(SpectralGateBuiltin { inner: gate })
        },
    );
    reg.register_block(
        BuiltinSig::simple("spectraldelay", 1, 1, 2, BuiltinKind::Block),
        |p, _sr| {
            let mut delay = crate::effects::spectral_delay::SpectralDelay::<T, 64, 16>::new();
            delay.set_mix(p[0] as f32);
            delay.set_feedback(p[1] as f32);
            Box::new(SpectralDelayBuiltin { inner: delay })
        },
    );
    reg.register_block(
        BuiltinSig::simple("convolver", 1, 1, 2, BuiltinKind::Block),
        |p, _sr| {
            let ir_gain = p[0] as f32;
            let mix = p[1] as f32;
            let conv = crate::partitioned_conv::PartitionedConvolver::<T, 64>::new(4096);
            Box::new(ConvolverBuiltin {
                inner: conv,
                ir_gain,
                mix,
            })
        },
    );
}
