//! Foreign-function registry: DSP/model built-ins callable from rill-lang.
//!
//! Two kinds: [`SampleBuiltin`] (per-sample, feedback-legal) and block built-ins
//! (`rill_core::Algorithm`, opaque whole-buffer). Concrete bindings live outside
//! this crate (e.g. `rill-adrift`); core stays `rill-core`-only.

use std::collections::HashMap;

use crate::math::Transcendental;
use crate::traits::ParamValue;

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
pub trait BlockBuiltin<T: Transcendental>: crate::traits::Algorithm<T> {
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

/// The type of a parameter in a built-in function signature.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamType {
    /// A signal wire argument — contributes to the built-in's input arity.
    Signal,
    /// A compile-time f64 constant.
    Float,
    /// A compile-time i64 constant.
    Int,
    /// A compile-time string literal.
    String,
    /// A compile-time boolean.
    Bool,
    /// A compile-time record literal with a known schema.
    Record(RecordSchema),
    /// A compile-time enum value with allowed variants.
    Enum(&'static [&'static str]),
    /// Zero or more arguments of the inner type.
    Variadic(Box<ParamType>),
}

/// Schema for a record literal.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordSchema {
    /// Fields in declaration order.
    pub fields: Vec<RecordField>,
}

/// A single field in a record schema.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordField {
    /// Field name.
    pub name: &'static str,
    /// Field type.
    pub ty: ParamType,
    /// Default value, if any.
    pub default: Option<f64>,
}

impl RecordSchema {
    /// Create a schema from a field list.
    pub fn new(fields: Vec<RecordField>) -> Self {
        Self { fields }
    }
}

/// Type-checker-facing signature of a built-in (independent of `T`).
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinSig {
    /// Registered name.
    pub name: &'static str,
    /// Parameter list: first N entries are signal inputs, remainder are compile-time params.
    pub params: Vec<ParamType>,
    /// Number of signal outputs (1 in this increment).
    pub signal_outs: usize,
    /// Sample vs block.
    pub kind: BuiltinKind,
}

impl BuiltinSig {
    /// Convenience constructor for SISO built-ins with only Float params.
    /// Maintains backward compatibility during migration.
    pub fn simple(
        name: &'static str,
        signal_ins: usize,
        signal_outs: usize,
        num_params: usize,
        kind: BuiltinKind,
    ) -> Self {
        let mut params = Vec::with_capacity(signal_ins + num_params);
        for _ in 0..signal_ins {
            params.push(ParamType::Signal);
        }
        for _ in 0..num_params {
            params.push(ParamType::Float);
        }
        Self {
            name,
            params,
            signal_outs,
            kind,
        }
    }

    /// Number of signal inputs = count of Signal params (non-variadic).
    pub fn signal_ins(&self) -> usize {
        self.params
            .iter()
            .filter(|p| matches!(p, ParamType::Signal))
            .count()
    }

    /// Minimum number of Apply arguments (excludes Signal params).
    pub fn min_args(&self) -> usize {
        let mut count = 0;
        for p in &self.params {
            match p {
                ParamType::Signal | ParamType::Variadic(_) => {}
                _ => count += 1,
            }
        }
        count
    }

    /// Maximum number of Apply arguments (None if variadic; excludes Signal params).
    pub fn max_args(&self) -> Option<usize> {
        if self
            .params
            .iter()
            .any(|p| matches!(p, ParamType::Variadic(_)))
        {
            None
        } else {
            Some(
                self.params
                    .iter()
                    .filter(|p| !matches!(p, ParamType::Signal))
                    .count(),
            )
        }
    }
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
            BuiltinSig::simple("gain", 1, 1, 1, BuiltinKind::Sample),
            |params, _sr| {
                Box::new(Gain {
                    k: params[0] as f32,
                })
            },
        );
        let sig = reg.builtin_sig("gain").unwrap();
        assert_eq!((sig.signal_ins(), sig.params.len()), (1, 2));
        let mut inst = reg
            .get("gain")
            .unwrap()
            .build_sample(&[0.5], 44100.0)
            .unwrap();
        assert_eq!(inst.process_sample(&[2.0]), 1.0);
        assert!(reg.builtin_sig("missing").is_none());
    }
}
