//! Self-registration for the FFI factories of the DSP crates
//! (rill-digital-effects, rill-core-dsp).

#[cfg(feature = "dsp")]
use crate::builtin::{BlockBuiltin, Registry};
#[cfg(any(feature = "dsp", feature = "model"))]
use rill_core::math::Transcendental;
#[cfg(any(feature = "dsp", feature = "model"))]
use rill_core::traits::{Algorithm, ProcessResult};

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

    ffi.register_block("delay", |p: &[f64], sr: f32, _alloc| {
        let mut d = Delay::<T, 64>::with_params(sr, p[0] as f32, p[1] as f32, p[2] as f32);
        Algorithm::init(&mut d, sr);
        Box::new(DelayBuiltin(d))
    });
    ffi.register_block("distortion", |p: &[f64], sr: f32, _alloc| {
        let mut d = Distortion::<T, 64>::with_params(DistortionType::SoftClip, p[0] as f32, 1.0);
        d.set_output_gain(p[1] as f32);
        Algorithm::init(&mut d, sr);
        Box::new(DistortionBuiltin(d))
    });
    ffi.register_block("limiter", |p: &[f64], sr: f32, _alloc| {
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

    ffi.register_block("sine", |p: &[f64], sr: f32, _alloc| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Sine, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    ffi.register_block("saw", |p: &[f64], sr: f32, _alloc| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Saw, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    ffi.register_block("square", |p: &[f64], sr: f32, _alloc| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Square, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    ffi.register_block("triangle", |p: &[f64], sr: f32, _alloc| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Triangle, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    ffi.register_block("noise", |p: &[f64], _sr: f32, _alloc| {
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
    ffi.register_block("integrator", |_p: &[f64], _sr: f32, _alloc| {
        Box::new(IntegratorBuiltin::<T>::new())
    });
    ffi.register_block("leaky_integrator", |p: &[f64], _sr: f32, _alloc| {
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

    ffi.register_block("onepole", |p: &[f64], sr: f32, _alloc| {
        let mut inner = OnePole::<T>::new(FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: p[0] as f32,
            q: p[1] as f32,
            gain_db: 0.0,
        });
        Algorithm::init(&mut inner, sr);
        Box::new(OnePoleBuiltin { inner })
    });
    ffi.register_block("moog", |p: &[f64], sr: f32, _alloc| {
        let mut inner = MoogLadder::<T>::new(p[0] as f32, p[1] as f32);
        Algorithm::init(&mut inner, sr);
        Box::new(MoogBuiltin { inner })
    });
    ffi.register_block("lowpass", |p: &[f64], sr: f32, _alloc| {
        let mut b = Biquad::<T>::new(FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: p[0] as f32,
            q: p[1] as f32,
            gain_db: 0.0,
        });
        Algorithm::init(&mut b, sr);
        Box::new(BiquadBuiltin { inner: b })
    });
    ffi.register_block("highpass", |p: &[f64], sr: f32, _alloc| {
        let mut b = Biquad::<T>::new(FilterParams {
            filter_type: FilterType::HighPass,
            cutoff: p[0] as f32,
            q: p[1] as f32,
            gain_db: 0.0,
        });
        Algorithm::init(&mut b, sr);
        Box::new(BiquadBuiltin { inner: b })
    });
    ffi.register_block("biquad", |p: &[f64], sr: f32, _alloc| {
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
/// [`Registry`]. Factory-only bridge: downstream crates (rill-adrift) consume
/// the legacy `Registry<T>` (signatures come from the catalog); the equivalent
/// `register_foreign_*` functions above target `ForeignRegistry` for
/// `compile_with_ffi`. Mirrors the deleted
/// `rill-core-dsp::lang::register::register_lang_builtins` factories exactly.
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

    reg.register_block("onepole", |p, sr, _alloc| {
        let mut inner = OnePole::<T>::new(FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: p[0] as f32,
            q: p[1] as f32,
            gain_db: 0.0,
        });
        Algorithm::init(&mut inner, sr);
        Box::new(OnePoleBuiltin { inner })
    });
    reg.register_block("moog", |p, sr, _alloc| {
        let mut inner = MoogLadder::<T>::new(p[0] as f32, p[1] as f32);
        Algorithm::init(&mut inner, sr);
        Box::new(MoogBuiltin { inner })
    });
    reg.register_block("lowpass", |p, sr, _alloc| {
        let mut b = Biquad::<T>::new(FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: p[0] as f32,
            q: p[1] as f32,
            gain_db: 0.0,
        });
        Algorithm::init(&mut b, sr);
        Box::new(BiquadBuiltin { inner: b })
    });
    reg.register_block("highpass", |p, sr, _alloc| {
        let mut b = Biquad::<T>::new(FilterParams {
            filter_type: FilterType::HighPass,
            cutoff: p[0] as f32,
            q: p[1] as f32,
            gain_db: 0.0,
        });
        Algorithm::init(&mut b, sr);
        Box::new(BiquadBuiltin { inner: b })
    });
    reg.register_block("biquad", |p, sr, _alloc| {
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

    reg.register_block("sine", |p, sr, _alloc| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Sine, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    reg.register_block("saw", |p, sr, _alloc| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Saw, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    reg.register_block("square", |p, sr, _alloc| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Square, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    reg.register_block("triangle", |p, sr, _alloc| {
        let freq = p[0] as f32;
        let amp = T::from_f64(p[1]);
        let mut osc = BasicOscillator::<T>::new(Waveform::Triangle, freq, amp);
        osc.set_phase(T::from_f64(p[2]));
        Algorithm::init(&mut osc, sr);
        Box::new(OscBuiltin { osc })
    });
    reg.register_block("noise", |p, _sr, _alloc| {
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

    reg.register_block("integrator", |_p, _sr, _alloc| {
        Box::new(IntegratorBuiltin::<T>::new())
    });
    reg.register_block("leaky_integrator", |p, _sr, _alloc| {
        Box::new(LeakyIntegratorBuiltin::<T>::new(T::from_f64(p[0])))
    });
}

// ============================================================================
// rill-core-model FFI factories (feature `model`)
// ============================================================================

/// Register the rill-core-model analog filter builtin (`analog_moog`) as an
/// FFI factory. The algorithm lives in `rill-core-model` (a pure library, no
/// rill-lang dependency); the wrapper struct implementing `BlockBuiltin` lives
/// in [`crate::builtins::model`]. Call this on the `ForeignRegistry` you pass
/// to `compile_with_ffi` (or any other FFI assembly point). Mirrors the deleted
/// `rill-core-model::register::register_lang_builtins` factory exactly.
#[cfg(feature = "model")]
pub fn register_foreign_model<T: Transcendental + 'static>(
    ffi: &mut crate::ffi::ForeignRegistry<T>,
) {
    use crate::builtins::model::AnalogMoogBuiltin;
    use rill_core_model::wdf::RcPole;

    ffi.register_block("analog_moog", |p: &[f64], sr: f32, _alloc| {
        let pole = RcPole::new(T::ZERO);
        let mut inner = rill_core_model::wdf::MoogLadder::<T>::new(
            pole,
            T::from_f32(p[0] as f32),
            T::from_f32(p[1] as f32),
            T::from_f32(sr),
        );
        Algorithm::init(&mut inner, sr);
        Box::new(AnalogMoogBuiltin { inner })
    });
}

// ============================================================================
// EQ FFI factories (graphic_eq / eq_parametric)
// ============================================================================

#[cfg(feature = "dsp")]
/// Default factory creating `Biquad<f32>` filters for the graphic EQ.
#[derive(Debug, Clone, Default)]
struct BiquadFactory;

#[cfg(feature = "dsp")]
impl rill_core_dsp::eq::FilterFactory<rill_core_dsp::filters::Biquad<f32>> for BiquadFactory {
    fn create_filter(
        &self,
        filter_type: rill_core_dsp::filters::FilterType,
        frequency: f32,
        q: f32,
        gain_db: f32,
    ) -> rill_core_dsp::filters::Biquad<f32> {
        let params = rill_core_dsp::filters::FilterParams {
            filter_type,
            cutoff: frequency,
            q,
            gain_db,
        };
        rill_core_dsp::filters::Biquad::new(params)
    }
}

#[cfg(feature = "dsp")]
/// A `graphic_eq` block builtin: wraps a [`rill_core_dsp::eq::GraphicEq`] over
/// third-octave `Biquad` filters. Scalar `f32` scratch in/out because the EQ
/// is a fixed-`f32` filter bank (the language's `T` may be `f64`).
struct GraphicEqBuiltin<T: Transcendental> {
    eq: rill_core_dsp::eq::GraphicEq<rill_core_dsp::filters::Biquad<f32>>,
    scratch_in: Vec<f32>,
    scratch_out: Vec<f32>,
    _phantom: std::marker::PhantomData<T>,
}

#[cfg(feature = "dsp")]
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

#[cfg(feature = "dsp")]
impl<T: Transcendental> BlockBuiltin<T> for GraphicEqBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        if index == 0 {
            if let Some(v) = value.as_f32() {
                self.eq.set_output_gain(v.clamp(0.0, 4.0));
            }
        }
    }
}

/// Register the EQ FFI factories (`graphic_eq` / `eq_parametric`). The
/// algorithms live in `rill-core-dsp` (moved from the deleted `rill-router`);
/// the wrappers implementing `BlockBuiltin` live in [`crate::builtins::eq`].
/// Call this on the `ForeignRegistry` you pass to `compile_with_ffi`.
#[cfg(feature = "dsp")]
pub fn register_foreign_eq<T: Transcendental + 'static>(ffi: &mut crate::ffi::ForeignRegistry<T>) {
    use crate::builtins::eq::{BandType, EqBandConfig, EqBuiltin, EqConfig, EqState};

    ffi.register_block("graphic_eq", |p: &[f64], sr: f32, _alloc| {
        let factory = BiquadFactory;
        let mut eq = rill_core_dsp::eq::GraphicEq::new_third_octave(factory, sr);
        eq.set_output_gain(p[0] as f32);
        eq.init(sr);
        Box::new(GraphicEqBuiltin::<T> {
            eq,
            scratch_in: vec![0.0f32; 64],
            scratch_out: vec![0.0f32; 64],
            _phantom: std::marker::PhantomData,
        })
    });

    ffi.register_block("eq_parametric", |p: &[f64], sr: f32, _alloc| {
        let mut bands = Vec::new();
        let mut i = 0usize;
        while i + 3 < p.len() {
            bands.push(EqBandConfig {
                freq: p[i],
                q: p[i + 1],
                gain_db: p[i + 2],
                band_type: match p[i + 3].round() as i32 {
                    1 => BandType::LowShelf,
                    2 => BandType::HighShelf,
                    3 => BandType::LowPass,
                    4 => BandType::HighPass,
                    5 => BandType::BandPass,
                    6 => BandType::Notch,
                    _ => BandType::Peak,
                },
            });
            i += 4;
        }
        let inner = EqState::new(EqConfig { bands }, sr);
        Box::new(EqBuiltin::new(inner))
    });
}
