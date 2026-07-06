//! rill-lang built-in bindings for rill-core-dsp / rill-core-model blocks.

use rill_core::math::Transcendental;
use rill_core::traits::algorithm::{Algorithm, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_lang::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry, SampleBuiltin};

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
    fn set_param(&mut self, index: usize, value: T) {
        let v = value.to_f32();
        match index {
            0 => rill_core_dsp::filters::Filter::set_cutoff(&mut self.inner, v),
            1 => rill_core_dsp::filters::Filter::set_q(&mut self.inner, v),
            _ => {}
        }
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
    fn set_param(&mut self, index: usize, value: T) {
        let v = value.to_f32();
        match index {
            0 => self.inner.set_cutoff(v),
            1 => self.inner.set_resonance(v),
            _ => {}
        }
    }
}

// --- block built-in wrappers ---

struct BiquadBuiltin<T: Transcendental> {
    inner: rill_core_dsp::filters::Biquad<T>,
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
    fn metadata(&self) -> AlgorithmMetadata {
        Algorithm::metadata(&self.inner)
    }
}

impl<T: Transcendental> BlockBuiltin<T> for BiquadBuiltin<T> {
    fn set_param(&mut self, index: usize, value: T) {
        let v = value.to_f32();
        match index {
            0 => rill_core_dsp::filters::Filter::set_cutoff(&mut self.inner, v),
            1 => rill_core_dsp::filters::Filter::set_q(&mut self.inner, v),
            _ => {}
        }
    }
}

#[cfg(feature = "analog")]
struct AnalogMoogBuiltin<T: Transcendental> {
    inner: rill_core_model::wdf::MoogLadder<T>,
}

#[cfg(feature = "analog")]
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
    fn metadata(&self) -> AlgorithmMetadata {
        Algorithm::metadata(&self.inner)
    }
}

#[cfg(feature = "analog")]
impl<T: Transcendental> BlockBuiltin<T> for AnalogMoogBuiltin<T> {
    fn set_param(&mut self, index: usize, value: T) {
        match index {
            0 => self.inner.set_cutoff(value),
            1 => self.inner.set_resonance(value),
            _ => {}
        }
    }
}

/// Register the always-available rill-core-dsp built-ins.
pub fn register_dsp_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    use rill_core_dsp::filters::{FilterParams, FilterType, OnePole};

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
            let mut b = rill_core_dsp::filters::Biquad::<T>::new(FilterParams {
                filter_type: FilterType::LowPass,
                cutoff: p[0] as f32,
                q: p[1] as f32,
                gain_db: 0.0,
            });
            Algorithm::init(&mut b, sr);
            Box::new(BiquadBuiltin { inner: b })
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
            let mut b = rill_core_dsp::filters::Biquad::<T>::new(FilterParams {
                filter_type: FilterType::HighPass,
                cutoff: p[0] as f32,
                q: p[1] as f32,
                gain_db: 0.0,
            });
            Algorithm::init(&mut b, sr);
            Box::new(BiquadBuiltin { inner: b })
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
            Box::new(AnalogMoogBuiltin { inner: f })
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
