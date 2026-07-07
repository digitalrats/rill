//! rill-lang built-in bindings for rill-core-dsp / rill-core-model blocks.

use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
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

// ============================================================================
// FFT built-ins (behind `fft` feature)
// ============================================================================

#[cfg(feature = "fft")]
struct SpectralGateBuiltin<T: Transcendental> {
    inner: rill_fft::effects::spectral_gate::SpectralGate<T, 64>,
}

#[cfg(feature = "fft")]
impl<T: Transcendental> Algorithm<T> for SpectralGateBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        Algorithm::process(&mut self.inner, input, output)
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
}

#[cfg(feature = "fft")]
impl<T: Transcendental> BlockBuiltin<T> for SpectralGateBuiltin<T> {
    fn set_param(&mut self, index: usize, value: T) {
        match index {
            0 => self.inner.set_threshold(value),
            1 => self.inner.set_ratio(value.to_f32()),
            _ => {}
        }
    }
}

#[cfg(feature = "fft")]
struct SpectralDelayBuiltin<T: Transcendental> {
    inner: rill_fft::effects::spectral_delay::SpectralDelay<T, 64, 16>,
}

#[cfg(feature = "fft")]
impl<T: Transcendental> Algorithm<T> for SpectralDelayBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        Algorithm::process(&mut self.inner, input, output)
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
}

#[cfg(feature = "fft")]
impl<T: Transcendental> BlockBuiltin<T> for SpectralDelayBuiltin<T> {
    fn set_param(&mut self, index: usize, value: T) {
        match index {
            0 => self.inner.set_mix(value.to_f32()),
            1 => self.inner.set_feedback(value.to_f32()),
            _ => {}
        }
    }
}

/// Register rill-fft built-ins.
#[cfg(feature = "fft")]
pub fn register_fft_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig {
            name: "spectralgate",
            signal_ins: 1,
            signal_outs: 1,
            num_params: 2,
            kind: BuiltinKind::Block,
        },
        |p, _sr| {
            let mut gate = rill_fft::effects::spectral_gate::SpectralGate::<T, 64>::new();
            gate.set_threshold(T::from_f64(p[0]));
            gate.set_ratio(p[1] as f32);
            Box::new(SpectralGateBuiltin { inner: gate })
        },
    );
    reg.register_block(
        BuiltinSig {
            name: "spectraldelay",
            signal_ins: 1,
            signal_outs: 1,
            num_params: 2,
            kind: BuiltinKind::Block,
        },
        |p, _sr| {
            let mut delay = rill_fft::effects::spectral_delay::SpectralDelay::<T, 64, 16>::new();
            delay.set_mix(p[0] as f32);
            delay.set_feedback(p[1] as f32);
            Box::new(SpectralDelayBuiltin { inner: delay })
        },
    );
}

// ============================================================================
// Complex number built-ins
// ============================================================================

struct ComplexConjBuiltin;
impl<T: Transcendental> Algorithm<T> for ComplexConjBuiltin {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                let n = inp.len().min(output.len()) / 2;
                for i in 0..n {
                    output[2 * i] = inp[2 * i];
                    output[2 * i + 1] = -inp[2 * i + 1];
                }
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
impl<T: Transcendental> BlockBuiltin<T> for ComplexConjBuiltin {}

struct ComplexReBuiltin;
impl<T: Transcendental> Algorithm<T> for ComplexReBuiltin {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                let n = inp.len().min(output.len() * 2) / 2;
                for i in 0..n {
                    output[i] = inp[2 * i];
                }
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
impl<T: Transcendental> BlockBuiltin<T> for ComplexReBuiltin {}

struct ComplexImBuiltin;
impl<T: Transcendental> Algorithm<T> for ComplexImBuiltin {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                let n = inp.len().min(output.len() * 2) / 2;
                for i in 0..n {
                    output[i] = inp[2 * i + 1];
                }
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
impl<T: Transcendental> BlockBuiltin<T> for ComplexImBuiltin {}

struct ComplexNormBuiltin;
impl<T: Transcendental> Algorithm<T> for ComplexNormBuiltin {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                let n = inp.len().min(output.len() * 2) / 2;
                for i in 0..n {
                    let re = inp[2 * i];
                    let im = inp[2 * i + 1];
                    output[i] = (re * re + im * im).sqrt();
                }
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
impl<T: Transcendental> BlockBuiltin<T> for ComplexNormBuiltin {}

struct ComplexArgBuiltin;
impl<T: Transcendental> Algorithm<T> for ComplexArgBuiltin {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                let n = inp.len().min(output.len() * 2) / 2;
                for i in 0..n {
                    let im = inp[2 * i + 1];
                    let re = inp[2 * i];
                    let arg = im.to_f64().atan2(re.to_f64()) as f32;
                    output[i] = T::from_f32(arg);
                }
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
impl<T: Transcendental> BlockBuiltin<T> for ComplexArgBuiltin {}

struct ComplexMulBuiltin;
impl<T: Transcendental> Algorithm<T> for ComplexMulBuiltin {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                let n = inp.len().min(output.len() * 2) / 4;
                for i in 0..n {
                    let a_re = inp[4 * i];
                    let a_im = inp[4 * i + 1];
                    let b_re = inp[4 * i + 2];
                    let b_im = inp[4 * i + 3];
                    output[2 * i] = a_re * b_re - a_im * b_im;
                    output[2 * i + 1] = a_re * b_im + a_im * b_re;
                }
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
impl<T: Transcendental> BlockBuiltin<T> for ComplexMulBuiltin {}

struct ComplexAddBuiltin;
impl<T: Transcendental> Algorithm<T> for ComplexAddBuiltin {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                let n = inp.len().min(output.len() * 2) / 4;
                for i in 0..n {
                    output[2 * i] = inp[4 * i] + inp[4 * i + 2];
                    output[2 * i + 1] = inp[4 * i + 1] + inp[4 * i + 3];
                }
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
impl<T: Transcendental> BlockBuiltin<T> for ComplexAddBuiltin {}

struct ComplexGenBuiltin<T: Transcendental> {
    re: T,
    im: T,
}
impl<T: Transcendental> Algorithm<T> for ComplexGenBuiltin<T> {
    fn process(&mut self, _input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let n = output.len() / 2;
        for i in 0..n {
            output[2 * i] = self.re;
            output[2 * i + 1] = self.im;
        }
        Ok(())
    }
    fn reset(&mut self) {}
}
impl<T: Transcendental> BlockBuiltin<T> for ComplexGenBuiltin<T> {}

/// Register complex number built-ins (dsl: complex, conj, re, im, norm, arg, cmul, cadd).
pub fn register_complex_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig {
            name: "complex",
            signal_ins: 0,
            signal_outs: 2,
            num_params: 2,
            kind: BuiltinKind::Block,
        },
        |p, _sr| {
            let re = T::from_f64(p[0]);
            let im = T::from_f64(p[1]);
            Box::new(ComplexGenBuiltin { re, im })
        },
    );
    reg.register_block(
        BuiltinSig {
            name: "conj",
            signal_ins: 2,
            signal_outs: 2,
            num_params: 0,
            kind: BuiltinKind::Block,
        },
        |_p, _sr| Box::new(ComplexConjBuiltin),
    );
    reg.register_block(
        BuiltinSig {
            name: "re",
            signal_ins: 2,
            signal_outs: 1,
            num_params: 0,
            kind: BuiltinKind::Block,
        },
        |_p, _sr| Box::new(ComplexReBuiltin),
    );
    reg.register_block(
        BuiltinSig {
            name: "im",
            signal_ins: 2,
            signal_outs: 1,
            num_params: 0,
            kind: BuiltinKind::Block,
        },
        |_p, _sr| Box::new(ComplexImBuiltin),
    );
    reg.register_block(
        BuiltinSig {
            name: "norm",
            signal_ins: 2,
            signal_outs: 1,
            num_params: 0,
            kind: BuiltinKind::Block,
        },
        |_p, _sr| Box::new(ComplexNormBuiltin),
    );
    reg.register_block(
        BuiltinSig {
            name: "arg",
            signal_ins: 2,
            signal_outs: 1,
            num_params: 0,
            kind: BuiltinKind::Block,
        },
        |_p, _sr| Box::new(ComplexArgBuiltin),
    );
    reg.register_block(
        BuiltinSig {
            name: "cmul",
            signal_ins: 4,
            signal_outs: 2,
            num_params: 0,
            kind: BuiltinKind::Block,
        },
        |_p, _sr| Box::new(ComplexMulBuiltin),
    );
    reg.register_block(
        BuiltinSig {
            name: "cadd",
            signal_ins: 4,
            signal_outs: 2,
            num_params: 0,
            kind: BuiltinKind::Block,
        },
        |_p, _sr| Box::new(ComplexAddBuiltin),
    );
}
pub fn full_registry<T: Transcendental>() -> Registry<T> {
    let mut reg = Registry::new();
    register_dsp_builtins(&mut reg);
    register_complex_builtins(&mut reg);
    #[cfg(feature = "analog")]
    register_model_builtins(&mut reg);
    #[cfg(feature = "fft")]
    register_fft_builtins(&mut reg);
    reg
}
