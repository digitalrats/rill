/// rill-lang builtins for rill-analog-effects.
use std::marker::PhantomData;

use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, ParamValue, ProcessResult};
use rill_lang::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};

use crate::CassetteDeck;

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

/// Register analog effects builtins (cassette deck).
pub fn register_analog_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("cassettedeck", 1, 1, 4, BuiltinKind::Block).with_names(vec![
            "tape_speed",
            "bias_level",
            "noise_floor",
            "wow_flutter",
        ]),
        |p, sr| {
            let mut deck = CassetteDeck::new(sr as f64);
            deck.set_tape_speed(p[0]);
            deck.set_bias_level(p[1]);
            deck.playback_head_mut().noise_floor = p[2].max(0.0);
            deck.playback_head_mut().wow_flutter = p[3].max(0.0);
            Box::new(CassetteDeckBuiltin {
                inner: deck,
                sample_rate: sr,
                _phantom: PhantomData,
            })
        },
    );
}
