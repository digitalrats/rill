//! rill-lang built-in bindings for rill-core-dsp / rill-core-model blocks.

use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::{ParamValue, ProcessResult};
use rill_lang::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry, SampleBuiltin};

// --- sample built-ins ---

fn pv_f32(v: &ParamValue) -> f32 {
    match v {
        ParamValue::Float(f) => *f,
        ParamValue::Int(i) => *i as f32,
        _ => 0.0,
    }
}

fn pv_bool(v: &ParamValue) -> bool {
    match v {
        ParamValue::Bool(b) => *b,
        ParamValue::Float(f) => *f > 0.5,
        ParamValue::Int(i) => *i != 0,
        _ => false,
    }
}

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
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = pv_f32(value);
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
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = pv_f32(value);
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
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = pv_f32(value);
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
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = T::from_f32(pv_f32(value));
        match index {
            0 => self.inner.set_cutoff(v),
            1 => self.inner.set_resonance(v),
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
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = T::from_f32(pv_f32(value));
        match index {
            0 => self.inner.set_threshold(v),
            1 => self.inner.set_ratio(pv_f32(value)),
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
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => self.inner.set_mix(v),
            1 => self.inner.set_feedback(v),
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
// ============================================================================
// Oscillator built-ins (generators)
// ============================================================================

struct OscBuiltin<T: Transcendental> {
    osc: rill_core_dsp::BasicOscillator<T>,
}

impl<T: Transcendental> Algorithm<T> for OscBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.osc.process(input, output)
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.osc, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.osc);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for OscBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        use rill_core_dsp::Generator;
        match index {
            0 => self.osc.set_frequency(pv_f32(value)),
            1 => self.osc.set_amplitude(T::from_f32(pv_f32(value))),
            2 => self.osc.set_phase(T::from_f32(pv_f32(value))),
            _ => {}
        }
    }
}

struct NoiseGenBuiltin<T: Transcendental> {
    gen: rill_core_dsp::NoiseGenerator<T>,
}

impl<T: Transcendental> Algorithm<T> for NoiseGenBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.gen.process(input, output)
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.gen, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.gen);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for NoiseGenBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        use rill_core_dsp::Generator;
        if index == 1 {
            self.gen.set_amplitude(T::from_f32(pv_f32(value)));
        }
    }
}

/// Register oscillator built-ins: `sine`, `saw`, `square`, `triangle`, `noise`.
pub fn register_oscillator_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    use rill_core_dsp::{BasicOscillator, Generator, NoiseGenerator, NoiseType, Waveform};

    reg.register_block(
        BuiltinSig {
            name: "sine",
            signal_ins: 0,
            signal_outs: 1,
            num_params: 3,
            kind: BuiltinKind::Block,
        },
        |p, sr| {
            let freq = p[0] as f32;
            let amp = T::from_f64(p[1]);
            let mut osc = BasicOscillator::<T>::new(Waveform::Sine, freq, amp);
            osc.set_phase(T::from_f64(p[2]));
            Algorithm::init(&mut osc, sr);
            Box::new(OscBuiltin { osc })
        },
    );
    reg.register_block(
        BuiltinSig {
            name: "saw",
            signal_ins: 0,
            signal_outs: 1,
            num_params: 3,
            kind: BuiltinKind::Block,
        },
        |p, sr| {
            let freq = p[0] as f32;
            let amp = T::from_f64(p[1]);
            let mut osc = BasicOscillator::<T>::new(Waveform::Saw, freq, amp);
            osc.set_phase(T::from_f64(p[2]));
            Algorithm::init(&mut osc, sr);
            Box::new(OscBuiltin { osc })
        },
    );
    reg.register_block(
        BuiltinSig {
            name: "square",
            signal_ins: 0,
            signal_outs: 1,
            num_params: 3,
            kind: BuiltinKind::Block,
        },
        |p, sr| {
            let freq = p[0] as f32;
            let amp = T::from_f64(p[1]);
            let mut osc = BasicOscillator::<T>::new(Waveform::Square, freq, amp);
            osc.set_phase(T::from_f64(p[2]));
            Algorithm::init(&mut osc, sr);
            Box::new(OscBuiltin { osc })
        },
    );
    reg.register_block(
        BuiltinSig {
            name: "triangle",
            signal_ins: 0,
            signal_outs: 1,
            num_params: 3,
            kind: BuiltinKind::Block,
        },
        |p, sr| {
            let freq = p[0] as f32;
            let amp = T::from_f64(p[1]);
            let mut osc = BasicOscillator::<T>::new(Waveform::Triangle, freq, amp);
            osc.set_phase(T::from_f64(p[2]));
            Algorithm::init(&mut osc, sr);
            Box::new(OscBuiltin { osc })
        },
    );
    reg.register_block(
        BuiltinSig {
            name: "noise",
            signal_ins: 0,
            signal_outs: 1,
            num_params: 2,
            kind: BuiltinKind::Block,
        },
        |p, _sr| {
            let amp = T::from_f64(p[1]);
            let mut gen = NoiseGenerator::<T>::new(
                match p[0].round() as i32 {
                    1 => NoiseType::Pink,
                    2 => NoiseType::Brown,
                    _ => NoiseType::White,
                },
                amp,
            );
            Box::new(NoiseGenBuiltin { gen })
        },
    );
}

// ============================================================================
// Lo-fi built-ins (feature-gated)
// ============================================================================

#[cfg(feature = "lofi")]
struct LofiBuiltin {
    inner: rill_lofi::LofiProcessor<64>,
}

#[cfg(feature = "lofi")]
impl Algorithm<f32> for LofiBuiltin {
    fn process(&mut self, input: Option<&[f32]>, output: &mut [f32]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                for (i, out) in output.iter_mut().enumerate() {
                    *out = self.inner.process_sample(inp[i.min(inp.len() - 1)]);
                }
            }
            None => output.fill(0.0),
        }
        Ok(())
    }
    fn reset(&mut self) {}
}

#[cfg(feature = "lofi")]
impl BlockBuiltin<f32> for LofiBuiltin {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        use rill_core::traits::Node;
        use rill_core::ParameterId;
        let (name, pv) = match index {
            0 => ("bit_depth", ParamValue::Int(pv_f32(value).round() as i32)),
            1 => (
                "sample_rate",
                ParamValue::Float(pv_f32(value).clamp(8000.0, 192000.0)),
            ),
            2 => ("dry_wet", ParamValue::Float(pv_f32(value).clamp(0.0, 1.0))),
            3 => ("output_gain", ParamValue::Float(pv_f32(value).max(0.0))),
            4 => ("enable_bitcrush", ParamValue::Bool(pv_bool(value))),
            5 => ("enable_sr_reduction", ParamValue::Bool(pv_bool(value))),
            6 => ("enable_noise", ParamValue::Bool(pv_bool(value))),
            _ => return,
        };
        let _ = self
            .inner
            .set_parameter(&ParameterId::new(name).unwrap(), pv);
    }
}

/// Register lo-fi built-in: `lofi(bit_depth, sr, dry_wet, gain, bitcrush, sr_reduce, noise)`.
#[cfg(feature = "lofi")]
pub fn register_lofi_builtins(reg: &mut Registry<f32>) {
    use rill_core::traits::Node;
    use rill_lofi::ClassicSystem;
    reg.register_block(
        BuiltinSig {
            name: "lofi",
            signal_ins: 1,
            signal_outs: 1,
            num_params: 7,
            kind: BuiltinKind::Block,
        },
        |p, sr| {
            let config = rill_lofi::LofiConfig {
                system: ClassicSystem::Custom {
                    bit_depth: p[0].round() as u8,
                    sample_rate: p[1].clamp(8000.0, 192000.0) as f32,
                    nonlinear: false,
                    noise_floor: -48.0,
                },
                hardware: rill_lofi::HardwareEmulation::default(),
                enable_bitcrush: p[4] > 0.5,
                enable_sr_reduction: p[5] > 0.5,
                enable_noise: p[6] > 0.5,
                output_gain: p[3].max(0.0) as f32,
                dc_offset: 0.0,
                output_ceiling: 1.0,
                dry_wet: p[2].clamp(0.0, 1.0) as f32,
            };
            let mut inner = rill_lofi::LofiProcessor::<64>::new(config);
            Node::init(&mut inner, sr);
            Box::new(LofiBuiltin { inner })
        },
    );
}

// ============================================================================
// AY-3-8910 chip emulator built-in
// ============================================================================

#[cfg(feature = "lofi")]
struct Ay38910Builtin {
    chip: rill_lofi::Ay38910Chip,
    last_regs: Option<Vec<u8>>,
}

#[cfg(feature = "lofi")]
impl Algorithm<f32> for Ay38910Builtin {
    fn process(&mut self, _input: Option<&[f32]>, output: &mut [f32]) -> ProcessResult<()> {
        self.chip.process(None, output)
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.chip, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.chip);
    }
}

#[cfg(feature = "lofi")]
impl BlockBuiltin<f32> for Ay38910Builtin {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        if index == 1 {
            if let ParamValue::Bytes(regs) = value {
                if self.last_regs.as_deref() != Some(regs.as_slice()) {
                    use rill_lofi::ChipEmulator;
                    self.chip.write_registers(regs);
                    self.last_regs = Some(regs.clone());
                }
            }
        }
    }
}

/// Register AY-3-8910 chip built-in: `ay38910(chip_clock_hz)`.
#[cfg(feature = "lofi")]
pub fn register_chip_builtins(reg: &mut Registry<f32>) {
    use rill_core::traits::Algorithm;
    reg.register_block(
        BuiltinSig {
            name: "ay38910",
            signal_ins: 0,
            signal_outs: 1,
            num_params: 2,
            kind: BuiltinKind::Block,
        },
        |p, sr| {
            let clock = p[0] as f32;
            let mut chip = rill_lofi::Ay38910Chip::new(clock);
            Algorithm::init(&mut chip, sr);
            Box::new(Ay38910Builtin {
                chip,
                last_regs: None,
            })
        },
    );
}

/// Build a complete builtin registry: DSP primitives, oscillators, complex
/// arithmetic, and optionally analog models, FFT nodes, lo-fi, and chip emulators.
pub fn full_registry<T: Transcendental>() -> Registry<T> {
    let mut reg = Registry::new();
    register_dsp_builtins(&mut reg);
    register_oscillator_builtins(&mut reg);
    register_complex_builtins(&mut reg);
    #[cfg(feature = "analog")]
    register_model_builtins(&mut reg);
    #[cfg(feature = "fft")]
    register_fft_builtins(&mut reg);
    reg
}

/// Build a lofi-capable registry (concrete `f32`).
#[cfg(feature = "lofi")]
pub fn full_registry_f32() -> Registry<f32> {
    let mut reg = full_registry::<f32>();
    register_lofi_builtins(&mut reg);
    register_chip_builtins(&mut reg);
    reg
}
