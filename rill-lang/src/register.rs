//! Self-registration for rill-lang's own builtins (mixer, eq, dry/wet, complex).

use rill_core::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};
#[cfg(feature = "router")]
use rill_core::builtin::{ParamType, RecordField, RecordSchema};
use rill_core::math::Transcendental;
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
    state: crate::builtins::mixer::MixerState<T>,
    cfg: crate::builtins::mixer::MixerConfig,
}

#[cfg(feature = "router")]
impl<T: Transcendental> MixerAlgorithmWrapper<T> {
    fn new(config: crate::builtins::mixer::MixerConfig) -> Self {
        Self {
            state: crate::builtins::mixer::MixerState::<T>::new(config.clone(), 512),
            cfg: config,
        }
    }
}

#[cfg(feature = "router")]
impl<T: Transcendental> Algorithm<T> for MixerAlgorithmWrapper<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        if let Some(inp) = input {
            output.copy_from_slice(inp);
        } else {
            output.fill(T::ZERO);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.state = crate::builtins::mixer::MixerState::<T>::new(self.cfg.clone(), 512);
    }
}

#[cfg(feature = "router")]
impl<T: Transcendental> BlockBuiltin<T> for MixerAlgorithmWrapper<T> {}

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
    mix: T,
}

#[cfg(feature = "router")]
impl<T: Transcendental> Algorithm<T> for DryWetBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                let n = (inp.len() / 2).min(output.len() / 2);
                let dry_gain = T::ONE - self.mix;
                for i in 0..n {
                    let dry = inp[2 * i];
                    let wet = inp[2 * i + 1];
                    let out = dry * dry_gain + wet * self.mix;
                    output[2 * i] = out;
                    output[2 * i + 1] = out;
                }
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {}
}

#[cfg(feature = "router")]
impl<T: Transcendental> BlockBuiltin<T> for DryWetBuiltin<T> {}

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
        BuiltinSig::simple("complex", 0, 2, 2, BuiltinKind::Block),
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
    };

    reg.register_block(
        mixer_sig,
        |params: &[f64], _sample_rate: f32| -> Box<dyn BlockBuiltin<T>> {
            let num_channels = if params.len() > 1 {
                params.len() - 1
            } else {
                1
            };
            let num_buses = params.last().copied().unwrap_or(0.0) as usize;

            let config = MixerConfig::new(num_channels.max(1), num_buses);
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
    };

    reg.register_block(
        sig,
        |_params: &[f64], _sr: f32| -> Box<dyn BlockBuiltin<T>> {
            Box::new(DryWetBuiltin {
                mix: T::from_f64(0.5),
            })
        },
    );
}
