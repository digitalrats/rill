/// Registration functions for rill-graph and rill-lang built-ins.
#[cfg(feature = "graph")]
use rill_core::traits::{Node, NodeId, NodeVariant, Params};
#[cfg(feature = "graph")]
use rill_graph::{node_ctor, NodeFactory};

#[cfg(feature = "graph")]
pub fn register_graph_nodes<const BUF_SIZE: usize>(factory: &mut NodeFactory<f32, BUF_SIZE>) {
    node_ctor!(factory, "rill/convolver", |id: NodeId, params: &Params| {
        let ir_len = params.get_f32("ir_len", 4096.0) as usize;
        let mut n = crate::nodes::convolver_node::ConvolverNode::<f32, BUF_SIZE>::new(
            ir_len,
            params.sample_rate,
        );
        Node::set_id(&mut n, id);
        Node::init(&mut n, params.sample_rate);
        NodeVariant::Processor(Box::new(n))
    });
}

#[cfg(feature = "lang")]
mod lang_helpers {
    use rill_core::math::Transcendental;
    use rill_core::traits::algorithm::Algorithm;
    use rill_core::traits::{ParamValue, ProcessResult};
    use rill_lang::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};

    fn pv_f32(v: &ParamValue) -> f32 {
        match v {
            ParamValue::Float(f) => *f,
            ParamValue::Int(i) => *i as f32,
            _ => 0.0,
        }
    }

    struct SpectralGateBuiltin<T: Transcendental> {
        inner: crate::effects::spectral_gate::SpectralGate<T, 64>,
    }

    impl<T: Transcendental> Algorithm<T> for SpectralGateBuiltin<T> {
        fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
            Algorithm::process(&mut self.inner, input, output)
        }
        fn reset(&mut self) {
            Algorithm::reset(&mut self.inner);
        }
    }

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

    struct SpectralDelayBuiltin<T: Transcendental> {
        inner: crate::effects::spectral_delay::SpectralDelay<T, 64, 16>,
    }

    impl<T: Transcendental> Algorithm<T> for SpectralDelayBuiltin<T> {
        fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
            Algorithm::process(&mut self.inner, input, output)
        }
        fn reset(&mut self) {
            Algorithm::reset(&mut self.inner);
        }
    }

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

    pub fn register_fft_builtins<T: Transcendental>(reg: &mut Registry<T>) {
        reg.register_block(
            BuiltinSig::simple("spectralgate", 1, 1, 2, BuiltinKind::Block),
            |p, _sr| {
                let mut gate = crate::effects::spectral_gate::SpectralGate::<T, 64>::new();
                gate.set_threshold(T::from_f64(p[0]));
                gate.set_ratio(p[1] as f32);
                Box::new(SpectralGateBuiltin { inner: gate })
            },
        );
        reg.register_block(
            BuiltinSig::simple("spectraldelay", 1, 1, 2, BuiltinKind::Block),
            |p, _sr| {
                let mut delay = crate::effects::spectral_delay::SpectralDelay::<T, 64, 16>::new();
                delay.set_mix(p[0] as f32);
                delay.set_feedback(p[1] as f32);
                Box::new(SpectralDelayBuiltin { inner: delay })
            },
        );
    }
}

#[cfg(feature = "lang")]
pub fn register_lang_builtins<T: rill_core::math::Transcendental>(
    reg: &mut rill_lang::builtin::Registry<T>,
) {
    lang_helpers::register_fft_builtins(reg);
}
