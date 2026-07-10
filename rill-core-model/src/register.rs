#![allow(deprecated)]
/// Registration functions for rill-graph and rill-lang built-ins.
#[cfg(feature = "lang")]
mod lang_helpers {
    use rill_core::math::Transcendental;
    use rill_core::traits::algorithm::Algorithm;
    use rill_core::traits::{ParamValue, ProcessResult};
    use rill_lang::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};

    struct AnalogMoogBuiltin<T: Transcendental> {
        inner: crate::wdf::MoogLadder<T>,
    }

    impl<T: Transcendental> Algorithm<T> for AnalogMoogBuiltin<T> {
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

    impl<T: Transcendental> BlockBuiltin<T> for AnalogMoogBuiltin<T> {
        fn set_param(&mut self, index: usize, value: &ParamValue) {
            let v = T::from_f32(match value {
                ParamValue::Float(f) => *f,
                ParamValue::Int(i) => *i as f32,
                _ => 0.0,
            });
            match index {
                0 => self.inner.set_cutoff(v),
                1 => self.inner.set_resonance(v),
                _ => {}
            }
        }
    }

    pub fn register_model_builtins<T: Transcendental>(reg: &mut Registry<T>) {
        reg.register_block(
            BuiltinSig::simple("analog_moog", 1, 1, 2, BuiltinKind::Block),
            |p, _sr| {
                let pole = crate::wdf::RcPole::new(T::ZERO);
                let mut f = crate::wdf::MoogLadder::<T>::new(
                    pole,
                    T::from_f32(p[0] as f32),
                    T::from_f32(p[1] as f32),
                    T::from_f32(44100.0),
                );
                f.update_coeffs();
                Box::new(AnalogMoogBuiltin { inner: f })
            },
        );
    }
}

#[cfg(feature = "lang")]
pub fn register_lang_builtins<T: rill_core::math::Transcendental>(
    reg: &mut rill_lang::builtin::Registry<T>,
) {
    lang_helpers::register_model_builtins(reg);
}
