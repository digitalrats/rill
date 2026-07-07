//! Foreign-function registry: DSP/model built-ins callable from rill-lang.
//!
//! Two kinds: [`SampleBuiltin`] (per-sample, feedback-legal) and block built-ins
//! (`rill_core::Algorithm`, opaque whole-buffer). Concrete bindings live outside
//! this crate (e.g. `rill-adrift`); core stays `rill-core`-only.

use std::collections::HashMap;

use rill_core::math::Transcendental;
use rill_core::traits::ParamValue;

/// A stateful per-sample built-in: `signal_ins` inputs → 1 output.
pub trait SampleBuiltin<T: Transcendental>: Send + Sync {
    /// Process one sample. `inputs.len() == signal_ins`.
    fn process_sample(&mut self, inputs: &[T]) -> T;
    /// Re-initialise for a sample rate (default no-op).
    fn init(&mut self, _sample_rate: f32) {}
    /// Clear internal state.
    fn reset(&mut self);
    /// Set a parameter by index.
    fn set_param(&mut self, _index: usize, _value: &ParamValue) {}
}

/// A whole-buffer built-in with settable params.
pub trait BlockBuiltin<T: Transcendental>: rill_core::traits::Algorithm<T> {
    /// Set a parameter by index.
    fn set_param(&mut self, _index: usize, _value: &ParamValue) {}
}

/// Whether a built-in is per-sample or whole-buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinKind {
    /// Per-sample [`SampleBuiltin`].
    Sample,
    /// Whole-buffer `Algorithm` (1→1).
    Block,
}

/// Type-checker-facing signature of a built-in (independent of `T`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinSig {
    /// Registered name.
    pub name: &'static str,
    /// Number of signal inputs.
    pub signal_ins: usize,
    /// Number of signal outputs (1 in this increment).
    pub signal_outs: usize,
    /// Number of constant params.
    pub num_params: usize,
    /// Sample vs block.
    pub kind: BuiltinKind,
}

/// A boxed factory building an instance from folded params + a sample rate.
type SampleFactory<T> = Box<dyn Fn(&[f64], f32) -> Box<dyn SampleBuiltin<T>> + Send + Sync>;
type BlockFactory<T> = Box<dyn Fn(&[f64], f32) -> Box<dyn BlockBuiltin<T>> + Send + Sync>;

enum Factory<T: Transcendental> {
    Sample(SampleFactory<T>),
    Block(BlockFactory<T>),
}

/// A registry entry.
pub struct Entry<T: Transcendental> {
    /// The signature.
    pub sig: BuiltinSig,
    factory: Factory<T>,
}

impl<T: Transcendental> Entry<T> {
    /// Build a sample instance (panics if this entry is a block built-in — callers
    /// gate on `sig.kind`).
    pub fn build_sample(
        &self,
        params: &[f64],
        sample_rate: f32,
    ) -> Option<Box<dyn SampleBuiltin<T>>> {
        match &self.factory {
            Factory::Sample(f) => Some(f(params, sample_rate)),
            Factory::Block(_) => None,
        }
    }
    /// Build a block instance.
    pub fn build_block(
        &self,
        params: &[f64],
        sample_rate: f32,
    ) -> Option<Box<dyn BlockBuiltin<T>>> {
        match &self.factory {
            Factory::Block(f) => Some(f(params, sample_rate)),
            Factory::Sample(_) => None,
        }
    }
}

/// A collection of built-in definitions.
pub struct Registry<T: Transcendental> {
    entries: HashMap<String, Entry<T>>,
}

impl<T: Transcendental> Default for Registry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental> Registry<T> {
    /// An empty registry.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Register a per-sample built-in.
    pub fn register_sample(
        &mut self,
        sig: BuiltinSig,
        factory: impl Fn(&[f64], f32) -> Box<dyn SampleBuiltin<T>> + Send + Sync + 'static,
    ) {
        debug_assert_eq!(sig.kind, BuiltinKind::Sample);
        self.entries.insert(
            sig.name.to_string(),
            Entry {
                sig,
                factory: Factory::Sample(Box::new(factory)),
            },
        );
    }

    /// Register a whole-buffer (`Algorithm`) built-in.
    pub fn register_block(
        &mut self,
        sig: BuiltinSig,
        factory: impl Fn(&[f64], f32) -> Box<dyn BlockBuiltin<T>> + Send + Sync + 'static,
    ) {
        debug_assert_eq!(sig.kind, BuiltinKind::Block);
        self.entries.insert(
            sig.name.to_string(),
            Entry {
                sig,
                factory: Factory::Block(Box::new(factory)),
            },
        );
    }

    /// Look up an entry by name.
    pub fn get(&self, name: &str) -> Option<&Entry<T>> {
        self.entries.get(name)
    }
}

/// A `T`-independent signature lookup used by the type checker and lowering.
pub trait SignatureSource {
    /// The signature for `name`, if registered.
    fn builtin_sig(&self, name: &str) -> Option<&BuiltinSig>;
}

impl<T: Transcendental> SignatureSource for Registry<T> {
    fn builtin_sig(&self, name: &str) -> Option<&BuiltinSig> {
        self.entries.get(name).map(|e| &e.sig)
    }
}

/// A signature source with no built-ins (used by `compile()` / existing tests).
pub struct NoSigs;
impl SignatureSource for NoSigs {
    fn builtin_sig(&self, _name: &str) -> Option<&BuiltinSig> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Gain {
        k: f32,
    }
    impl SampleBuiltin<f32> for Gain {
        fn process_sample(&mut self, inputs: &[f32]) -> f32 {
            inputs[0] * self.k
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn register_and_lookup_sample() {
        let mut reg = Registry::<f32>::new();
        reg.register_sample(
            BuiltinSig {
                name: "gain",
                signal_ins: 1,
                signal_outs: 1,
                num_params: 1,
                kind: BuiltinKind::Sample,
            },
            |params, _sr| {
                Box::new(Gain {
                    k: params[0] as f32,
                })
            },
        );
        let sig = reg.builtin_sig("gain").unwrap();
        assert_eq!((sig.signal_ins, sig.num_params), (1, 1));
        let mut inst = reg
            .get("gain")
            .unwrap()
            .build_sample(&[0.5], 44100.0)
            .unwrap();
        assert_eq!(inst.process_sample(&[2.0]), 1.0);
        assert!(reg.builtin_sig("missing").is_none());
    }
}
