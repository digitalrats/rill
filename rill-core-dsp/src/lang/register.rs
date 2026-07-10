use rill_core::builtin::{BuiltinKind, BuiltinSig, Registry};
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;

use crate::filters::{Biquad, FilterParams, FilterType, MoogLadder, OnePole};
use crate::generators::{BasicOscillator, Generator, NoiseGenerator, NoiseType, Waveform};

use super::biquad::{BiquadBuiltin, GeneralBiquadBuiltin};
use super::moog::MoogBuiltin;
use super::noise::NoiseGenBuiltin;
use super::onepole::OnePoleBuiltin;
use super::osc::OscBuiltin;

pub fn register_lang_builtins<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    register_filters(reg);
    register_oscillators(reg);
}

fn register_filters<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_sample(
        BuiltinSig::simple("onepole", 1, 1, 2, BuiltinKind::Sample),
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
        BuiltinSig::simple("moog", 1, 1, 2, BuiltinKind::Sample),
        |p, sr| {
            let mut inner = MoogLadder::<T>::new(p[0] as f32, p[1] as f32);
            Algorithm::init(&mut inner, sr);
            Box::new(MoogBuiltin { inner })
        },
    );
    reg.register_block(
        BuiltinSig::simple("lowpass", 1, 1, 2, BuiltinKind::Block),
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
        BuiltinSig::simple("highpass", 1, 1, 2, BuiltinKind::Block),
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
        BuiltinSig::simple("biquad", 1, 1, 4, BuiltinKind::Block),
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
}

fn register_oscillators<T: Transcendental + 'static>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("sine", 0, 1, 3, BuiltinKind::Block),
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
        BuiltinSig::simple("saw", 0, 1, 3, BuiltinKind::Block),
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
        BuiltinSig::simple("square", 0, 1, 3, BuiltinKind::Block),
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
        BuiltinSig::simple("triangle", 0, 1, 3, BuiltinKind::Block),
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
        BuiltinSig::simple("noise", 0, 1, 2, BuiltinKind::Block),
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
}
