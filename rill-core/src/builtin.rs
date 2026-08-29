//! Foreign-function registry: whole-buffer built-ins callable from rill-lang.
//!
//! `BlockBuiltin` (`rill_core::Algorithm`, opaque whole-buffer) is the only
//! built-in form. Concrete bindings live outside this crate (e.g. `rill-adrift`);
//! core stays `rill-core`-only.

use std::collections::HashMap;

use crate::buffer::ResourceRegistry;
use crate::math::Transcendental;
use crate::traits::ParamValue;

/// A whole-buffer built-in with settable params.
pub trait BlockBuiltin<T: Transcendental>: crate::traits::Algorithm<T> {
    /// Set a parameter by index.
    fn set_param(&mut self, _index: usize, _value: &ParamValue) {}
}

/// A whole-buffer multi-channel built-in with settable params.
pub trait MultichannelBlockBuiltin<T: Transcendental>:
    crate::traits::MultichannelAlgorithm<T> + Send + Sync
{
    /// Set a parameter by index.
    fn set_param(&mut self, _index: usize, _value: &ParamValue) {}
}

/// Whether a built-in is per-sample or whole-buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinKind {
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
    /// A compile-time symbolic reference to a named resource (e.g. a tape loop).
    Resource,
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
    /// Names of compile-time parameters in `params` order (after signal inputs).
    /// When non-empty, graph-level `build_ir()` uses these names to match recipe
    /// params to builtin arg positions — eliminating ordering fragility from
    /// `HashMap`-based param bags. Left empty for backward-compatible registrations.
    pub param_names: Vec<&'static str>,
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
            param_names: Vec::new(),
        }
    }

    /// Attach human-readable names to compile-time parameters.
    ///
    /// `names.len()` must equal the number of non-signal params in `self.params`.
    /// When set, graph-level `build_ir()` in `rill-graph` uses these names to
    /// match recipe param keys to builtin arg positions, fixing the ordering
    /// fragility of `HashMap`-based param bags.
    pub fn with_names(mut self, names: Vec<&'static str>) -> Self {
        self.param_names = names;
        self
    }

    /// Number of signal inputs = count of Signal params (non-variadic).
    pub fn signal_ins(&self) -> usize {
        self.params
            .iter()
            .filter(|p| matches!(p, ParamType::Signal))
            .count()
    }

    /// Whether the built-in takes variadic signal inputs (e.g. a mixer).
    pub fn has_variadic_signal(&self) -> bool {
        self.params.iter().any(
            |p| matches!(p, ParamType::Variadic(inner) if matches!(**inner, ParamType::Signal)),
        )
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
type BlockFactory<T> = Box<dyn Fn(&[f64], f32) -> Box<dyn BlockBuiltin<T>> + Send + Sync>;
type MultichannelBlockFactory<T> =
    Box<dyn Fn(usize, &[f64], f32) -> Box<dyn MultichannelBlockBuiltin<T>> + Send + Sync>;
/// A block factory that also receives the resource registry and the resource
/// name to resolve named resources (e.g. tape loops) for resource-backed built-ins.
type ResourceBlockFactory<T> = Box<
    dyn Fn(&[f64], f32, &mut ResourceRegistry<T>, &str) -> Box<dyn BlockBuiltin<T>> + Send + Sync,
>;
/// A multi-channel block factory that also receives the signal input count, the
/// resource registry, and the resource name.
type ResourceMultichannelBlockFactory<T> = Box<
    dyn Fn(
            usize,
            &[f64],
            f32,
            &mut ResourceRegistry<T>,
            &str,
        ) -> Box<dyn MultichannelBlockBuiltin<T>>
        + Send
        + Sync,
>;

enum Factory<T: Transcendental> {
    Block(BlockFactory<T>),
    MultichannelBlock(MultichannelBlockFactory<T>),
    ResourceBlock(ResourceBlockFactory<T>),
    ResourceMultichannelBlock(ResourceMultichannelBlockFactory<T>),
}

/// A registry entry.
pub struct Entry<T: Transcendental> {
    /// The signature.
    pub sig: BuiltinSig,
    factory: Factory<T>,
}

impl<T: Transcendental> Entry<T> {
    /// Build a block instance.
    pub fn build_block(
        &self,
        params: &[f64],
        sample_rate: f32,
    ) -> Option<Box<dyn BlockBuiltin<T>>> {
        match &self.factory {
            Factory::Block(f) => Some(f(params, sample_rate)),
            Factory::MultichannelBlock(_)
            | Factory::ResourceBlock(_)
            | Factory::ResourceMultichannelBlock(_) => None,
        }
    }
    /// Build a multichannel block instance.
    pub fn build_multichannel_block(
        &self,
        signal_ins: usize,
        params: &[f64],
        sample_rate: f32,
    ) -> Option<Box<dyn MultichannelBlockBuiltin<T>>> {
        match &self.factory {
            Factory::MultichannelBlock(f) => Some(f(signal_ins, params, sample_rate)),
            _ => None,
        }
    }
    /// Build a resource-backed block instance, resolving named resources (e.g.
    /// tape loops) via the provided registry.
    pub fn build_resource_block(
        &self,
        params: &[f64],
        sample_rate: f32,
        registry: &mut ResourceRegistry<T>,
        resource_name: &str,
    ) -> Option<Box<dyn BlockBuiltin<T>>> {
        match &self.factory {
            Factory::ResourceBlock(f) => Some(f(params, sample_rate, registry, resource_name)),
            _ => None,
        }
    }

    /// Build a resource-backed multi-channel block instance.
    pub fn build_resource_multichannel_block(
        &self,
        signal_ins: usize,
        params: &[f64],
        sample_rate: f32,
        registry: &mut ResourceRegistry<T>,
        resource_name: &str,
    ) -> Option<Box<dyn MultichannelBlockBuiltin<T>>> {
        match &self.factory {
            Factory::ResourceMultichannelBlock(f) => {
                Some(f(signal_ins, params, sample_rate, registry, resource_name))
            }
            _ => None,
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

    /// Register a whole-buffer multi-channel built-in.
    pub fn register_multichannel_block(
        &mut self,
        sig: BuiltinSig,
        factory: impl Fn(usize, &[f64], f32) -> Box<dyn MultichannelBlockBuiltin<T>>
            + Send
            + Sync
            + 'static,
    ) {
        debug_assert_eq!(sig.kind, BuiltinKind::Block);
        self.entries.insert(
            sig.name.to_string(),
            Entry {
                sig,
                factory: Factory::MultichannelBlock(Box::new(factory)),
            },
        );
    }

    /// Register a resource-backed whole-buffer built-in. The factory receives
    /// the resource registry to resolve named resources (e.g. tape loops).
    pub fn register_resource_block(
        &mut self,
        sig: BuiltinSig,
        factory: impl Fn(&[f64], f32, &mut ResourceRegistry<T>, &str) -> Box<dyn BlockBuiltin<T>>
            + Send
            + Sync
            + 'static,
    ) {
        debug_assert_eq!(sig.kind, BuiltinKind::Block);
        self.entries.insert(
            sig.name.to_string(),
            Entry {
                sig,
                factory: Factory::ResourceBlock(Box::new(factory)),
            },
        );
    }

    /// Register a resource-backed multi-channel whole-buffer built-in.
    pub fn register_resource_multichannel_block(
        &mut self,
        sig: BuiltinSig,
        factory: impl Fn(
                usize,
                &[f64],
                f32,
                &mut ResourceRegistry<T>,
                &str,
            ) -> Box<dyn MultichannelBlockBuiltin<T>>
            + Send
            + Sync
            + 'static,
    ) {
        debug_assert_eq!(sig.kind, BuiltinKind::Block);
        self.entries.insert(
            sig.name.to_string(),
            Entry {
                sig,
                factory: Factory::ResourceMultichannelBlock(Box::new(factory)),
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
