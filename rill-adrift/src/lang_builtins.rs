//! rill-lang built-in bindings for rill-core-dsp / rill-core-model blocks.

use rill_core::math::Transcendental;
use rill_core::traits::Algorithm;
use rill_lang::builtin::{BuiltinKind, BuiltinSig, Registry, SampleBuiltin};

// --- sample built-ins ---

struct OnePoleBuiltin<T: Transcendental> {
    inner: rill_core_dsp::filters::OnePole<T>,
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
}

struct MoogBuiltin<T: Transcendental> {
    inner: rill_core_dsp::filters::MoogLadder<T>,
}
impl<T: Transcendental> SampleBuiltin<T> for MoogBuiltin<T> {
    fn process_sample(&mut self, inputs: &[T]) -> T {
        self.inner.process_sample(inputs[0])
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.inner, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
}

/// Register the always-available rill-core-dsp built-ins.
pub fn register_dsp_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    use rill_core_dsp::filters::{Biquad, FilterParams, FilterType, OnePole};

    reg.register_sample(
        BuiltinSig {
            name: "onepole",
            signal_ins: 1,
            signal_outs: 1,
            num_params: 2,
            kind: BuiltinKind::Sample,
        },
        |p, sr| {
            let mut inner = OnePole::<T>::new(FilterParams {
                filter_type: FilterType::LowPass,
                cutoff: p[0] as f32,
                q: p[1] as f32,
                gain_db: 0.0,
            });
            Algorithm::init(&mut inner, sr);
            Box::new(OnePoleBuiltin { inner })
        },
    );
    reg.register_sample(
        BuiltinSig {
            name: "moog",
            signal_ins: 1,
            signal_outs: 1,
            num_params: 2,
            kind: BuiltinKind::Sample,
        },
        |p, sr| {
            let mut inner = rill_core_dsp::filters::MoogLadder::<T>::new(p[0] as f32, p[1] as f32);
            Algorithm::init(&mut inner, sr);
            Box::new(MoogBuiltin { inner })
        },
    );
    reg.register_block(
        BuiltinSig {
            name: "lowpass",
            signal_ins: 1,
            signal_outs: 1,
            num_params: 2,
            kind: BuiltinKind::Block,
        },
        |p, sr| {
            let mut b = Biquad::<T>::new(FilterParams {
                filter_type: FilterType::LowPass,
                cutoff: p[0] as f32,
                q: p[1] as f32,
                gain_db: 0.0,
            });
            Algorithm::init(&mut b, sr);
            Box::new(b)
        },
    );
    reg.register_block(
        BuiltinSig {
            name: "highpass",
            signal_ins: 1,
            signal_outs: 1,
            num_params: 2,
            kind: BuiltinKind::Block,
        },
        |p, sr| {
            let mut b = Biquad::<T>::new(FilterParams {
                filter_type: FilterType::HighPass,
                cutoff: p[0] as f32,
                q: p[1] as f32,
                gain_db: 0.0,
            });
            Algorithm::init(&mut b, sr);
            Box::new(b)
        },
    );
}

/// Register rill-core-model / analog built-ins.
#[cfg(feature = "analog")]
pub fn register_model_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig {
            name: "analog_moog",
            signal_ins: 1,
            signal_outs: 1,
            num_params: 2,
            kind: BuiltinKind::Block,
        },
        |p, sr| {
            let pole = rill_core_model::wdf::RcPole::new(T::ZERO);
            let mut f = rill_core_model::wdf::MoogLadder::<T>::new(
                pole,
                T::from_f32(p[0] as f32),
                T::from_f32(p[1] as f32),
                T::from_f32(sr),
            );
            f.update_coeffs();
            Box::new(f)
        },
    );
}

/// A registry populated with all available built-ins.
pub fn full_registry<T: Transcendental>() -> Registry<T> {
    let mut reg = Registry::new();
    register_dsp_builtins(&mut reg);
    #[cfg(feature = "analog")]
    register_model_builtins(&mut reg);
    reg
}
