/// rill-lang builtins for rill-router.
use std::marker::PhantomData;

use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, MultichannelAlgorithm, ParamValue, ProcessResult};
use rill_lang::builtin::{
    BlockBuiltin, BuiltinKind, BuiltinSig, MultichannelBlockBuiltin, ParamType, RecordField,
    RecordSchema, Registry,
};

use crate::builtins::dry_wet::DryWetBuiltin;
use crate::builtins::eq::{BandType, EqBandConfig, EqBuiltin, EqConfig, EqState};
use crate::builtins::mixer::{MixerAlgorithmWrapper, MixerConfig};
use crate::eq::{FilterFactory, GraphicEq};
use crate::pan::{MonoToStereo, PanLaw};
use rill_core_dsp::filters::{Biquad, FilterParams, FilterType};

/// Default factory that creates `Biquad<f32>` filters.
#[derive(Debug, Clone, Default)]
struct BiquadFactory;

impl FilterFactory<Biquad<f32>> for BiquadFactory {
    fn create_filter(
        &self,
        filter_type: FilterType,
        frequency: f32,
        q: f32,
        gain_db: f32,
    ) -> Biquad<f32> {
        let params = FilterParams {
            filter_type,
            cutoff: frequency,
            q,
            gain_db,
        };
        Biquad::new(params)
    }
}

struct GraphicEqBuiltin<T: Transcendental> {
    eq: GraphicEq<Biquad<f32>>,
    scratch_in: Vec<f32>,
    scratch_out: Vec<f32>,
    _phantom: PhantomData<T>,
}

impl<T: Transcendental> Algorithm<T> for GraphicEqBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let n = output.len();
        if self.scratch_in.len() < n {
            self.scratch_in.resize(n, 0.0);
            self.scratch_out.resize(n, 0.0);
        }
        let inp_buf = &mut self.scratch_in[..n];
        let out_buf = &mut self.scratch_out[..n];
        if let Some(inp) = input {
            for (b, &s) in inp_buf.iter_mut().zip(inp.iter()) {
                *b = s.to_f32();
            }
        } else {
            inp_buf.fill(0.0);
        }
        self.eq.process_block(inp_buf, out_buf);
        for (o, &s) in output.iter_mut().zip(out_buf.iter()) {
            *o = T::from_f32(s);
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.eq.reset();
    }
}

impl<T: Transcendental> BlockBuiltin<T> for GraphicEqBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        if index == 0 {
            if let Some(v) = value.as_f32() {
                self.eq.set_output_gain(v.clamp(0.0, 4.0));
            }
        }
    }
}

struct MonoToStereoBuiltin<T: Transcendental> {
    inner: MonoToStereo<T>,
}

impl<T: Transcendental> MultichannelAlgorithm<T> for MonoToStereoBuiltin<T> {
    fn num_inputs(&self) -> usize {
        self.inner.num_inputs()
    }
    fn num_outputs(&self) -> usize {
        self.inner.num_outputs()
    }
    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        self.inner.process(inputs, outputs)
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl<T: Transcendental> MultichannelBlockBuiltin<T> for MonoToStereoBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        match index {
            0 => {
                if let Some(v) = value.as_f32() {
                    self.inner.set_pan(v);
                }
            }
            1 => {
                if let Some(v) = value.as_f32() {
                    self.inner.set_smoothing(v);
                }
            }
            _ => {}
        }
    }
}

/// The legacy `Registry` registration (graph-compile path + downstream crates).
///
/// `register_lang_builtins` in [`crate::register`] forwards here; SP-3b Task 12
/// drops this legacy bridge once the graph path compiles against the FFI
/// catalog + `ForeignRegistry`.
pub fn register_router_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("graphic_eq", 1, 1, 1, BuiltinKind::Block).with_names(vec!["gain"]),
        |p, sr| {
            let factory = BiquadFactory;
            let mut eq = GraphicEq::new_third_octave(factory, sr);
            eq.set_output_gain(p[0] as f32);
            eq.init(sr);
            Box::new(GraphicEqBuiltin::<T> {
                eq,
                scratch_in: vec![0.0f32; 64],
                scratch_out: vec![0.0f32; 64],
                _phantom: PhantomData,
            })
        },
    );

    reg.register_multichannel_block(
        BuiltinSig {
            name: "mono_to_stereo",
            params: vec![ParamType::Signal, ParamType::Float, ParamType::Float],
            signal_outs: 2,
            kind: BuiltinKind::Block,
            param_names: vec!["pan", "smoothing"],
        },
        |_signal_ins, params, _sr| {
            Box::new(MonoToStereoBuiltin::<T> {
                inner: MonoToStereo::new(PanLaw::ConstantPower, params[0] as f32, params[1] as f32),
            })
        },
    );

    // --- Mixer / eq_parametric / dry_wet (moved from rill-lang, SP-3b Task 7) ---

    reg.register_multichannel_block(
        BuiltinSig {
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
        },
        |signal_ins, params, _sr| -> Box<dyn MultichannelBlockBuiltin<T>> {
            let num_channels = signal_ins.max(1);
            let mut config = MixerConfig::new(num_channels, 0);
            if params.len() > 1 {
                config.master_vol = params[1];
            }
            Box::new(MixerAlgorithmWrapper::<T>::new(config))
        },
    );

    reg.register_block(
        BuiltinSig {
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
        },
        |_params: &[f64], sample_rate: f32| -> Box<dyn BlockBuiltin<T>> {
            let inner = EqState::new(EqConfig { bands: vec![] }, sample_rate);
            Box::new(EqBuiltin::new(inner))
        },
    );

    reg.register_multichannel_block(
        BuiltinSig {
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
        },
        |_signal_ins, params, _sr| -> Box<dyn MultichannelBlockBuiltin<T>> {
            let mix = params.first().copied().unwrap_or(0.5);
            Box::new(DryWetBuiltin::<T>::new(mix))
        },
    );
}

/// The `band_type` field value → [`BandType`] mapping, mirroring the `BandType`
/// enum ordering (0 = Peak).
fn band_type_from_f64(v: f64) -> BandType {
    match v.round() as i32 {
        1 => BandType::LowShelf,
        2 => BandType::HighShelf,
        3 => BandType::LowPass,
        4 => BandType::HighPass,
        5 => BandType::BandPass,
        6 => BandType::Notch,
        _ => BandType::Peak,
    }
}

/// Register the rill-router builtins (graphic_eq/mono_to_stereo/mixer/
/// eq_parametric/dry_wet) into a [`rill_lang::ffi::ForeignRegistry`]. Call this
/// on the registry you pass to `compile_with_ffi` (or any other FFI assembly
/// point). `eq_parametric`'s flattened band params arrive as groups of four
/// (freq, q, gain_db, band_type) per band — the `BandList` field's flattening.
pub fn register_foreign_router<T: Transcendental + 'static>(
    ffi: &mut rill_lang::ffi::ForeignRegistry<T>,
) {
    ffi.register_block("graphic_eq", |p: &[f64], sr: f32| {
        let factory = BiquadFactory;
        let mut eq = GraphicEq::new_third_octave(factory, sr);
        eq.set_output_gain(p[0] as f32);
        eq.init(sr);
        Box::new(GraphicEqBuiltin::<T> {
            eq,
            scratch_in: vec![0.0f32; 64],
            scratch_out: vec![0.0f32; 64],
            _phantom: PhantomData,
        })
    });

    ffi.register_multichannel_block("mono_to_stereo", |_signal_ins, params, _sr| {
        Box::new(MonoToStereoBuiltin::<T> {
            inner: MonoToStereo::new(PanLaw::ConstantPower, params[0] as f32, params[1] as f32),
        })
    });

    ffi.register_multichannel_block("mixer", |signal_ins, params, _sr| {
        let num_channels = signal_ins.max(1);
        let mut config = MixerConfig::new(num_channels, 0);
        if params.len() > 1 {
            config.master_vol = params[1];
        }
        Box::new(MixerAlgorithmWrapper::<T>::new(config))
    });

    ffi.register_block("eq_parametric", |p: &[f64], sr: f32| {
        let mut bands = Vec::new();
        let mut i = 0usize;
        while i + 3 < p.len() {
            bands.push(EqBandConfig {
                freq: p[i],
                q: p[i + 1],
                gain_db: p[i + 2],
                band_type: band_type_from_f64(p[i + 3]),
            });
            i += 4;
        }
        let inner = EqState::new(EqConfig { bands }, sr);
        Box::new(EqBuiltin::new(inner))
    });

    ffi.register_multichannel_block("dry_wet", |_signal_ins, params, _sr| {
        let mix = params.first().copied().unwrap_or(0.5);
        Box::new(DryWetBuiltin::<T>::new(mix))
    });
}
