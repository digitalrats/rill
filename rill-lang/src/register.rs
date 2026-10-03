//! Self-registration for rill-lang's own builtins (mixer, eq, dry/wet, complex).

use crate::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};
#[cfg(feature = "router")]
use crate::builtin::{MultichannelBlockBuiltin, ParamType, RecordField, RecordSchema};
use rill_core::math::Transcendental;
#[cfg(feature = "router")]
use rill_core::traits::MultichannelAlgorithm;
use rill_core::traits::{Algorithm, ProcessResult};

// ============================================================================
// Complex number built-in structs
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

// ============================================================================
// Mixer built-in struct
// ============================================================================

#[cfg(feature = "router")]
struct MixerAlgorithmWrapper<T: Transcendental> {
    state: crate::builtins::mixer::MixerState<T, 512>,
    cfg: crate::builtins::mixer::MixerConfig,
}

#[cfg(feature = "router")]
impl<T: Transcendental> MixerAlgorithmWrapper<T> {
    fn new(config: crate::builtins::mixer::MixerConfig) -> Self {
        Self {
            state: crate::builtins::mixer::MixerState::<T, 512>::new(config.clone()),
            cfg: config,
        }
    }
}

#[cfg(feature = "router")]
impl<T: Transcendental> MultichannelAlgorithm<T> for MixerAlgorithmWrapper<T> {
    fn num_inputs(&self) -> usize {
        self.state.num_inputs()
    }
    fn num_outputs(&self) -> usize {
        self.state.num_outputs()
    }
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.state.process(inputs, outputs)
    }
    fn reset(&mut self) {
        self.state = crate::builtins::mixer::MixerState::<T, 512>::new(self.cfg.clone());
    }
}

#[cfg(feature = "router")]
impl<T: Transcendental> MultichannelBlockBuiltin<T> for MixerAlgorithmWrapper<T> {}

// ============================================================================
// EQ built-in struct
// ============================================================================

#[cfg(feature = "router")]
struct EqBuiltin<T: Transcendental> {
    inner: crate::builtins::eq::EqState<T>,
}

#[cfg(feature = "router")]
impl<T: Transcendental> Algorithm<T> for EqBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => self.inner.process_slice(inp, output),
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {}
}

#[cfg(feature = "router")]
impl<T: Transcendental> BlockBuiltin<T> for EqBuiltin<T> {}

// ============================================================================
// Dry/Wet built-in struct
// ============================================================================

#[cfg(feature = "router")]
struct DryWetBuiltin<T: Transcendental> {
    state: crate::builtins::dry_wet::DryWetState,
    _phantom: std::marker::PhantomData<T>,
}

#[cfg(feature = "router")]
impl<T: Transcendental> DryWetBuiltin<T> {
    fn new(mix: f64) -> Self {
        Self {
            state: crate::builtins::dry_wet::DryWetState::new(
                crate::builtins::dry_wet::DryWetConfig { mix },
            ),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[cfg(feature = "router")]
impl<T: Transcendental> MultichannelAlgorithm<T> for DryWetBuiltin<T> {
    fn num_inputs(&self) -> usize {
        self.state.num_inputs()
    }
    fn num_outputs(&self) -> usize {
        self.state.num_outputs()
    }
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.state.process::<T>(inputs, outputs)
    }
    fn reset(&mut self) {}
}

#[cfg(feature = "router")]
impl<T: Transcendental> MultichannelBlockBuiltin<T> for DryWetBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        if index == 0 {
            let mix = value.as_f32().unwrap_or(0.5) as f64;
            self.state = crate::builtins::dry_wet::DryWetState::new(
                crate::builtins::dry_wet::DryWetConfig { mix },
            );
        }
    }
}

// ============================================================================
// Registration functions
// ============================================================================

/// Register rill-lang core builtins. Call after rill_core_dsp::register_lang_builtins().
pub fn register_core_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    register_complex(reg);

    #[cfg(feature = "router")]
    {
        register_mixer(reg);
        register_eq(reg);
        register_dry_wet(reg);
    }
}

/// Register complex number built-ins (dsl: complex, conj, re, im, norm, arg, cmul, cadd).
fn register_complex<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("complex", 0, 2, 2, BuiltinKind::Block).with_names(vec!["re", "im"]),
        |p, _sr| {
            let re = T::from_f64(p[0]);
            let im = T::from_f64(p[1]);
            Box::new(ComplexGenBuiltin { re, im })
        },
    );
    reg.register_block(
        BuiltinSig::simple("conj", 2, 2, 0, BuiltinKind::Block),
        |_p, _sr| Box::new(ComplexConjBuiltin),
    );
    reg.register_block(
        BuiltinSig::simple("re", 2, 1, 0, BuiltinKind::Block),
        |_p, _sr| Box::new(ComplexReBuiltin),
    );
    reg.register_block(
        BuiltinSig::simple("im", 2, 1, 0, BuiltinKind::Block),
        |_p, _sr| Box::new(ComplexImBuiltin),
    );
    reg.register_block(
        BuiltinSig::simple("norm", 2, 1, 0, BuiltinKind::Block),
        |_p, _sr| Box::new(ComplexNormBuiltin),
    );
    reg.register_block(
        BuiltinSig::simple("arg", 2, 1, 0, BuiltinKind::Block),
        |_p, _sr| Box::new(ComplexArgBuiltin),
    );
    reg.register_block(
        BuiltinSig::simple("cmul", 4, 2, 0, BuiltinKind::Block),
        |_p, _sr| Box::new(ComplexMulBuiltin),
    );
    reg.register_block(
        BuiltinSig::simple("cadd", 4, 2, 0, BuiltinKind::Block),
        |_p, _sr| Box::new(ComplexAddBuiltin),
    );
}

/// Register the mixer built-in: `mixer(signal..., { buses, master_vol })`.
#[cfg(feature = "router")]
fn register_mixer<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    use crate::builtins::mixer::MixerConfig;

    let mixer_sig = BuiltinSig {
        name: "mixer",
        params: vec![
            ParamType::Variadic(Box::new(ParamType::Signal)),
            ParamType::Record(RecordSchema::new(vec![
                RecordField {
                    name: "buses",
                    ty: ParamType::Int,
                    default: Some(0.0),
                },
                RecordField {
                    name: "master_vol",
                    ty: ParamType::Float,
                    default: Some(1.0),
                },
            ])),
        ],
        signal_outs: 2,
        kind: BuiltinKind::Block,
        param_names: Vec::new(),
    };

    reg.register_multichannel_block(
        mixer_sig,
        |signal_ins, params, _sr| -> Box<dyn MultichannelBlockBuiltin<T>> {
            let num_channels = signal_ins.max(1);
            let mut config = MixerConfig::new(num_channels, 0);
            if params.len() > 1 {
                config.master_vol = params[1];
            }
            Box::new(MixerAlgorithmWrapper::<T>::new(config))
        },
    );
}

/// Register the EQ parametric built-in: `eq_parametric(signal, { bands })`.
#[cfg(feature = "router")]
fn register_eq<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    use crate::builtins::eq::{EqConfig, EqState};

    let sig = BuiltinSig {
        name: "eq_parametric",
        params: vec![
            ParamType::Signal,
            ParamType::Record(RecordSchema::new(vec![RecordField {
                name: "bands",
                ty: ParamType::Variadic(Box::new(ParamType::Record(RecordSchema::new(vec![
                    RecordField {
                        name: "freq",
                        ty: ParamType::Float,
                        default: Some(1000.0),
                    },
                    RecordField {
                        name: "q",
                        ty: ParamType::Float,
                        default: Some(1.0),
                    },
                    RecordField {
                        name: "gain_db",
                        ty: ParamType::Float,
                        default: Some(0.0),
                    },
                    RecordField {
                        name: "band_type",
                        ty: ParamType::Int,
                        default: Some(0.0),
                    },
                ])))),
                default: None,
            }])),
        ],
        signal_outs: 1,
        kind: BuiltinKind::Block,
        param_names: Vec::new(),
    };

    reg.register_block(
        sig,
        |_params: &[f64], sample_rate: f32| -> Box<dyn BlockBuiltin<T>> {
            let inner = EqState::new(EqConfig { bands: vec![] }, sample_rate);
            Box::new(EqBuiltin { inner })
        },
    );
}

/// Register the dry/wet built-in: `dry_wet(dry, wet, { mix })`.
#[cfg(feature = "router")]
fn register_dry_wet<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    let sig = BuiltinSig {
        name: "dry_wet",
        params: vec![
            ParamType::Signal,
            ParamType::Signal,
            ParamType::Record(RecordSchema::new(vec![RecordField {
                name: "mix",
                ty: ParamType::Float,
                default: Some(0.5),
            }])),
        ],
        signal_outs: 2,
        kind: BuiltinKind::Block,
        param_names: Vec::new(),
    };

    reg.register_multichannel_block(
        sig,
        |_signal_ins, params, _sr| -> Box<dyn MultichannelBlockBuiltin<T>> {
            let mix = params.first().copied().unwrap_or(0.5);
            Box::new(DryWetBuiltin::<T>::new(mix))
        },
    );
}

// ============================================================================
// rill-digital-effects FFI factories (feature `dsp`)
// ============================================================================

/// Register the rill-digital-effects builtins (delay/distortion/limiter) as FFI
/// factories. The algorithms live in `rill-digital-effects` (a pure library,
/// no rill-lang dependency); rill-lang supplies the wrapper structs
/// implementing `BlockBuiltin`. Call this on the `ForeignRegistry` you pass to
/// `compile_with_ffi` (or any other FFI assembly point).
#[cfg(feature = "dsp")]
pub fn register_foreign_digital_effects<T: Transcendental + 'static>(
    ffi: &mut crate::ffi::ForeignRegistry<T>,
) {
    use rill_digital_effects::{Delay, Distortion, DistortionType, Limiter};

    struct DelayBuiltin<T: Transcendental>(Delay<T, 64>);
    impl<T: Transcendental> Algorithm<T> for DelayBuiltin<T> {
        fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
            Algorithm::process(&mut self.0, input, output)
        }
        fn reset(&mut self) {
            Algorithm::reset(&mut self.0);
        }
        fn init(&mut self, sample_rate: f32) {
            Algorithm::init(&mut self.0, sample_rate);
        }
    }
    impl<T: Transcendental> BlockBuiltin<T> for DelayBuiltin<T> {
        fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
            let v = match value {
                rill_core::traits::ParamValue::Float(f) => *f,
                rill_core::traits::ParamValue::Int(i) => *i as f32,
                _ => 0.0,
            };
            match index {
                0 => self.0.set_delay_time(v),
                1 => self.0.set_feedback(v),
                2 => self.0.set_mix(v),
                _ => {}
            }
        }
    }

    struct DistortionBuiltin<T: Transcendental>(Distortion<T, 64>);
    impl<T: Transcendental> Algorithm<T> for DistortionBuiltin<T> {
        fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
            Algorithm::process(&mut self.0, input, output)
        }
        fn reset(&mut self) {
            Algorithm::reset(&mut self.0);
        }
        fn init(&mut self, sample_rate: f32) {
            Algorithm::init(&mut self.0, sample_rate);
        }
    }
    impl<T: Transcendental> BlockBuiltin<T> for DistortionBuiltin<T> {
        fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
            let v = match value {
                rill_core::traits::ParamValue::Float(f) => *f,
                rill_core::traits::ParamValue::Int(i) => *i as f32,
                _ => 0.0,
            };
            match index {
                0 => self.0.set_drive(v),
                1 => self.0.set_output_gain(v),
                _ => {}
            }
        }
    }

    struct LimiterBuiltin<T: Transcendental>(Limiter<T, 64>);
    impl<T: Transcendental> Algorithm<T> for LimiterBuiltin<T> {
        fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
            match input {
                Some(inp) => self.0.process_block(inp, output),
                None => output.fill(T::ZERO),
            }
            Ok(())
        }
        fn reset(&mut self) {
            Algorithm::reset(&mut self.0);
        }
        fn init(&mut self, sample_rate: f32) {
            Algorithm::init(&mut self.0, sample_rate);
        }
    }
    impl<T: Transcendental> BlockBuiltin<T> for LimiterBuiltin<T> {
        fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
            let v = match value {
                rill_core::traits::ParamValue::Float(f) => *f,
                rill_core::traits::ParamValue::Int(i) => *i as f32,
                _ => 0.0,
            };
            match index {
                0 => self.0.set_threshold(v),
                1 => self.0.set_release(v),
                _ => {}
            }
        }
    }

    ffi.register_block("delay", |p: &[f64], sr: f32| {
        let mut d = Delay::<T, 64>::with_params(sr, p[0] as f32, p[1] as f32, p[2] as f32);
        Algorithm::init(&mut d, sr);
        Box::new(DelayBuiltin(d))
    });
    ffi.register_block("distortion", |p: &[f64], sr: f32| {
        let mut d = Distortion::<T, 64>::with_params(DistortionType::SoftClip, p[0] as f32, 1.0);
        d.set_output_gain(p[1] as f32);
        Algorithm::init(&mut d, sr);
        Box::new(DistortionBuiltin(d))
    });
    ffi.register_block("limiter", |p: &[f64], sr: f32| {
        let mut l = Limiter::<T, 64>::new(sr, p[0] as f32, 1.0, p[1] as f32, 0.0);
        Algorithm::init(&mut l, sr);
        Box::new(LimiterBuiltin(l))
    });
}

// ============================================================================
// rill-core-dsp FFI factories (feature `dsp`)
// ============================================================================

/// Register the rill-core-dsp generators (sine/saw/square/triangle/noise) and
/// integrators (integrator/leaky_integrator) as FFI factories. The algorithms
/// live in `rill-core-dsp` (a pure library, no rill-lang dependency); the
/// wrapper structs implementing `BlockBuiltin` live in
/// [`crate::builtins::generators`]. Call this on the `ForeignRegistry` you pass
/// to `compile_with_ffi` (or any other FFI assembly point).
#[cfg(feature = "dsp")]
pub fn register_foreign_generators<T: Transcendental + 'static>(
    ffi: &mut crate::ffi::ForeignRegistry<T>,
) {
    use crate::builtins::generators::{
        IntegratorBuiltin, LeakyIntegratorBuiltin, NoiseGenBuiltin, OscBuiltin,
    };
    use rill_core_dsp::generators::{
        BasicOscillator, Generator, NoiseGenerator, NoiseType, Waveform,
    };

    ffi.register_block("sine", |p: &[f64], sr: f32| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Sine, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    ffi.register_block("saw", |p: &[f64], sr: f32| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Saw, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    ffi.register_block("square", |p: &[f64], sr: f32| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Square, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    ffi.register_block("triangle", |p: &[f64], sr: f32| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Triangle, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    ffi.register_block("noise", |p: &[f64], _sr: f32| {
        let amp = T::from_f64(p[1]);
        let gen = NoiseGenerator::<T>::new(
            match p[0].round() as i32 {
                1 => NoiseType::Pink,
                2 => NoiseType::Brown,
                _ => NoiseType::White,
            },
            amp,
        );
        Box::new(NoiseGenBuiltin { gen })
    });
    ffi.register_block("integrator", |_p: &[f64], _sr: f32| {
        Box::new(IntegratorBuiltin::<T>::new())
    });
    ffi.register_block("leaky_integrator", |p: &[f64], _sr: f32| {
        Box::new(LeakyIntegratorBuiltin::<T>::new(T::from_f64(p[0])))
    });
}

/// Register the rill-core-dsp filters (onepole/moog/lowpass/highpass/biquad)
/// as FFI factories. The algorithms live in `rill-core-dsp` (pure library);
/// the wrapper structs implementing `BlockBuiltin` live in
/// [`crate::builtins::filters`]. `lowpass`/`highpass` are `biquad` presets.
#[cfg(feature = "dsp")]
pub fn register_foreign_filters<T: Transcendental + 'static>(
    ffi: &mut crate::ffi::ForeignRegistry<T>,
) {
    use crate::builtins::filters::{
        BiquadBuiltin, GeneralBiquadBuiltin, MoogBuiltin, OnePoleBuiltin,
    };
    use rill_core_dsp::filters::{Biquad, FilterParams, FilterType, MoogLadder, OnePole};

    ffi.register_block("onepole", |p: &[f64], sr: f32| {
        let mut inner = OnePole::<T>::new(FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: p[0] as f32,
            q: p[1] as f32,
            gain_db: 0.0,
        });
        Algorithm::init(&mut inner, sr);
        Box::new(OnePoleBuiltin { inner })
    });
    ffi.register_block("moog", |p: &[f64], sr: f32| {
        let mut inner = MoogLadder::<T>::new(p[0] as f32, p[1] as f32);
        Algorithm::init(&mut inner, sr);
        Box::new(MoogBuiltin { inner })
    });
    ffi.register_block("lowpass", |p: &[f64], sr: f32| {
        let mut b = Biquad::<T>::new(FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: p[0] as f32,
            q: p[1] as f32,
            gain_db: 0.0,
        });
        Algorithm::init(&mut b, sr);
        Box::new(BiquadBuiltin { inner: b })
    });
    ffi.register_block("highpass", |p: &[f64], sr: f32| {
        let mut b = Biquad::<T>::new(FilterParams {
            filter_type: FilterType::HighPass,
            cutoff: p[0] as f32,
            q: p[1] as f32,
            gain_db: 0.0,
        });
        Algorithm::init(&mut b, sr);
        Box::new(BiquadBuiltin { inner: b })
    });
    ffi.register_block("biquad", |p: &[f64], sr: f32| {
        let ft = match p[0] as u8 {
            0 => FilterType::LowPass,
            1 => FilterType::HighPass,
            2 => FilterType::BandPass,
            3 => FilterType::Notch,
            4 => FilterType::Peak,
            5 => FilterType::LowShelf,
            6 => FilterType::HighShelf,
            _ => FilterType::LowPass,
        };
        let mut b = Biquad::<T>::new(FilterParams {
            filter_type: ft,
            cutoff: p[1] as f32,
            q: p[2] as f32,
            gain_db: p[3] as f32,
        });
        Algorithm::init(&mut b, sr);
        Box::new(GeneralBiquadBuiltin { inner: b })
    });
}

/// Register the rill-core-dsp generators/integrators/filters into the legacy
/// [`Registry`]. Transitional bridge: the graph-compile path
/// (`compile_spec`/`compile_graph`) and downstream crates (rill-adrift) still
/// consume the legacy `Registry<T>`; SP-3b Task 13 wires them onto
/// `compile_with_ffi` + the foreign registrations above. Mirrors the deleted
/// `rill-core-dsp::lang::register::register_lang_builtins` exactly.
#[cfg(feature = "dsp")]
pub fn register_core_dsp_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    use crate::builtins::filters::{
        BiquadBuiltin, GeneralBiquadBuiltin, MoogBuiltin, OnePoleBuiltin,
    };
    use crate::builtins::generators::{
        IntegratorBuiltin, LeakyIntegratorBuiltin, NoiseGenBuiltin, OscBuiltin,
    };
    use rill_core_dsp::filters::{Biquad, FilterParams, FilterType, MoogLadder, OnePole};
    use rill_core_dsp::generators::{
        BasicOscillator, Generator, NoiseGenerator, NoiseType, Waveform,
    };

    reg.register_block(
        BuiltinSig::simple("onepole", 1, 1, 2, BuiltinKind::Block).with_names(vec!["cutoff", "q"]),
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
    reg.register_block(
        BuiltinSig::simple("moog", 1, 1, 2, BuiltinKind::Block)
            .with_names(vec!["cutoff", "resonance"]),
        |p, sr| {
            let mut inner = MoogLadder::<T>::new(p[0] as f32, p[1] as f32);
            Algorithm::init(&mut inner, sr);
            Box::new(MoogBuiltin { inner })
        },
    );
    reg.register_block(
        BuiltinSig::simple("lowpass", 1, 1, 2, BuiltinKind::Block).with_names(vec!["cutoff", "q"]),
        |p, sr| {
            let mut b = Biquad::<T>::new(FilterParams {
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
        BuiltinSig::simple("highpass", 1, 1, 2, BuiltinKind::Block).with_names(vec!["cutoff", "q"]),
        |p, sr| {
            let mut b = Biquad::<T>::new(FilterParams {
                filter_type: FilterType::HighPass,
                cutoff: p[0] as f32,
                q: p[1] as f32,
                gain_db: 0.0,
            });
            Algorithm::init(&mut b, sr);
            Box::new(BiquadBuiltin { inner: b })
        },
    );
    reg.register_block(
        BuiltinSig::simple("biquad", 1, 1, 4, BuiltinKind::Block)
            .with_names(vec!["type", "cutoff", "q", "gain_db"]),
        |p, sr| {
            let ft = match p[0] as u8 {
                0 => FilterType::LowPass,
                1 => FilterType::HighPass,
                2 => FilterType::BandPass,
                3 => FilterType::Notch,
                4 => FilterType::Peak,
                5 => FilterType::LowShelf,
                6 => FilterType::HighShelf,
                _ => FilterType::LowPass,
            };
            let mut b = Biquad::<T>::new(FilterParams {
                filter_type: ft,
                cutoff: p[1] as f32,
                q: p[2] as f32,
                gain_db: p[3] as f32,
            });
            Algorithm::init(&mut b, sr);
            Box::new(GeneralBiquadBuiltin { inner: b })
        },
    );

    reg.register_block(
        BuiltinSig::simple("sine", 0, 1, 3, BuiltinKind::Block)
            .with_names(vec!["freq", "amp", "phase"]),
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
        BuiltinSig::simple("saw", 0, 1, 3, BuiltinKind::Block)
            .with_names(vec!["freq", "amp", "phase"]),
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
        BuiltinSig::simple("square", 0, 1, 3, BuiltinKind::Block)
            .with_names(vec!["freq", "amp", "phase"]),
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
        BuiltinSig::simple("triangle", 0, 1, 3, BuiltinKind::Block)
            .with_names(vec!["freq", "amp", "phase"]),
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
        BuiltinSig::simple("noise", 0, 1, 2, BuiltinKind::Block).with_names(vec!["type", "amp"]),
        |p, _sr| {
            let amp = T::from_f64(p[1]);
            let gen = NoiseGenerator::<T>::new(
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

    reg.register_block(
        BuiltinSig::simple("integrator", 1, 1, 0, BuiltinKind::Block),
        |_p, _sr| Box::new(IntegratorBuiltin::<T>::new()),
    );
    reg.register_block(
        BuiltinSig::simple("leaky_integrator", 1, 1, 1, BuiltinKind::Block)
            .with_names(vec!["coeff"]),
        |p, _sr| Box::new(LeakyIntegratorBuiltin::<T>::new(T::from_f64(p[0]))),
    );
}
