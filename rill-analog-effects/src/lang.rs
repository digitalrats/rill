/// rill-lang builtins for rill-analog-effects.
use std::marker::PhantomData;

use rill_core::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};
use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, ParamValue, ProcessResult};

use crate::CassetteDeck;
use crate::{HeadConfig, TapeBridgeAlgorithm};

struct CassetteDeckBuiltin<T: Transcendental> {
    inner: CassetteDeck,
    sample_rate: f32,
    _phantom: PhantomData<T>,
}

impl<T: Transcendental> Algorithm<T> for CassetteDeckBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                for (o, &s) in output.iter_mut().zip(inp.iter()) {
                    *o = T::from_f64(self.inner.process(s.to_f64()));
                }
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.inner = CassetteDeck::new(self.sample_rate as f64);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for CassetteDeckBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        let v = value.as_f32().unwrap_or(0.0) as f64;
        match index {
            0 => self.inner.set_tape_speed(v),
            1 => self.inner.set_bias_level(v),
            2 => self.inner.playback_head_mut().noise_floor = v.max(0.0),
            3 => self.inner.playback_head_mut().wow_flutter = v.max(0.0),
            _ => {}
        }
    }
}

struct TapeBridgeBuiltin<T: Transcendental> {
    inner: TapeBridgeAlgorithm<T>,
    _phantom: PhantomData<T>,
}

impl<T: Transcendental> Algorithm<T> for TapeBridgeBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.inner.process(input, output)
    }

    fn init(&mut self, sr: f32) {
        self.inner.init(sr);
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}

impl<T: Transcendental> BlockBuiltin<T> for TapeBridgeBuiltin<T> {}

pub fn register_analog_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("cassette_deck", 1, 1, 4, BuiltinKind::Block),
        |p, sr| {
            let mut deck = CassetteDeck::new(sr as f64);
            deck.set_tape_speed(p[0].clamp(1.19, 19.05));
            deck.set_bias_level(p[1].clamp(0.0, 1.0));
            deck.playback_head_mut().noise_floor = p[2].max(0.0);
            deck.playback_head_mut().wow_flutter = p[3].max(0.0);
            Box::new(CassetteDeckBuiltin::<T> {
                inner: deck,
                sample_rate: sr,
                _phantom: PhantomData,
            })
        },
    );

    reg.register_block(
        BuiltinSig::simple("tape_bridge", 1, 2, 2, BuiltinKind::Block),
        |p, sr| {
            let capacity = p[0] as usize;
            let n_read_heads = (p[1] as usize).max(1);
            let mut heads = vec![HeadConfig {
                position: 0,
                gain: 0.8,
                decorators: vec![],
            }];
            for _ in 0..n_read_heads {
                heads.push(HeadConfig {
                    position: 48000,
                    gain: 0.5,
                    decorators: vec![],
                });
            }
            let mut algo = TapeBridgeAlgorithm::<T>::new(capacity, heads);
            algo.init(sr);
            Box::new(TapeBridgeBuiltin {
                inner: algo,
                _phantom: PhantomData,
            })
        },
    );
}
