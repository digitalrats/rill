/// rill-lang builtins for rill-router.
use std::marker::PhantomData;

use rill_core::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};
use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, ParamValue, ProcessResult};

use crate::eq::{FilterFactory, GraphicEq};
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

pub fn register_router_builtins<T: Transcendental>(reg: &mut Registry<T>) {
    reg.register_block(
        BuiltinSig::simple("graphic_eq", 1, 1, 1, BuiltinKind::Block),
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
}
