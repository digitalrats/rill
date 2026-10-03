//! Foreign-function registry: whole-buffer built-ins callable from rill-lang.
//!
//! `BlockBuiltin` (`rill_core::Algorithm`, opaque whole-buffer) is the only
//! built-in form. Concrete bindings live outside this crate (e.g. `rill-adrift`);
//! the language owns the registry contract. Built-in SIGNATURES come from the FFI
//! catalog (`foreign fn` declarations in `TypeEnv::foreign_sigs`); this registry
//! holds only the Rust FACTORIES that build runtime instances.

use std::collections::HashMap;

use rill_core::buffer::ResourceRegistry;
use rill_core::math::Transcendental;
use rill_core::traits::ParamValue;

/// A whole-buffer built-in with settable params.
pub trait BlockBuiltin<T: Transcendental>: rill_core::traits::Algorithm<T> {
    /// Set a parameter by index.
    fn set_param(&mut self, _index: usize, _value: &ParamValue) {}
}

/// A whole-buffer multi-channel built-in with settable params.
pub trait MultichannelBlockBuiltin<T: Transcendental>:
    rill_core::traits::MultichannelAlgorithm<T> + Send + Sync
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

/// How a registered built-in factory builds its runtime instance.
///
/// Mirrors the [`Factory`] variants. Runtime variant selection follows the
/// factory kind rather than the signal arity: a `Block` factory can serve a
/// multi-channel arity through the interpreter's interleaved path, so a name
/// must not be forced into the multichannel variant merely because it has
/// more than one input or output channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinFactoryKind {
    /// A whole-buffer single-channel factory.
    Block,
    /// A whole-buffer multi-channel factory.
    MultichannelBlock,
    /// A resource-backed whole-buffer single-channel factory.
    ResourceBlock,
    /// A resource-backed whole-buffer multi-channel factory.
    ResourceMultichannelBlock,
}

enum Factory<T: Transcendental> {
    Block(BlockFactory<T>),
    MultichannelBlock(MultichannelBlockFactory<T>),
    ResourceBlock(ResourceBlockFactory<T>),
    ResourceMultichannelBlock(ResourceMultichannelBlockFactory<T>),
}

/// A registry entry.
pub struct Entry<T: Transcendental> {
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

    /// Register a whole-buffer (`Algorithm`) built-in factory by name.
    pub fn register_block(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(&[f64], f32) -> Box<dyn BlockBuiltin<T>> + Send + Sync + 'static,
    ) {
        self.entries.insert(
            name.into(),
            Entry {
                factory: Factory::Block(Box::new(factory)),
            },
        );
    }

    /// Register a whole-buffer multi-channel built-in factory by name.
    pub fn register_multichannel_block(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(usize, &[f64], f32) -> Box<dyn MultichannelBlockBuiltin<T>>
            + Send
            + Sync
            + 'static,
    ) {
        self.entries.insert(
            name.into(),
            Entry {
                factory: Factory::MultichannelBlock(Box::new(factory)),
            },
        );
    }

    /// Register a resource-backed whole-buffer built-in factory by name. The
    /// factory receives the resource registry to resolve named resources
    /// (e.g. tape loops).
    pub fn register_resource_block(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(&[f64], f32, &mut ResourceRegistry<T>, &str) -> Box<dyn BlockBuiltin<T>>
            + Send
            + Sync
            + 'static,
    ) {
        self.entries.insert(
            name.into(),
            Entry {
                factory: Factory::ResourceBlock(Box::new(factory)),
            },
        );
    }

    /// Register a resource-backed multi-channel whole-buffer built-in factory
    /// by name.
    pub fn register_resource_multichannel_block(
        &mut self,
        name: impl Into<String>,
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
        self.entries.insert(
            name.into(),
            Entry {
                factory: Factory::ResourceMultichannelBlock(Box::new(factory)),
            },
        );
    }

    /// Look up an entry by name.
    pub fn get(&self, name: &str) -> Option<&Entry<T>> {
        self.entries.get(name)
    }

    /// The factory kind registered for `name`, if any.
    pub fn kind(&self, name: &str) -> Option<BuiltinFactoryKind> {
        self.entries.get(name).map(|e| match &e.factory {
            Factory::Block(_) => BuiltinFactoryKind::Block,
            Factory::MultichannelBlock(_) => BuiltinFactoryKind::MultichannelBlock,
            Factory::ResourceBlock(_) => BuiltinFactoryKind::ResourceBlock,
            Factory::ResourceMultichannelBlock(_) => BuiltinFactoryKind::ResourceMultichannelBlock,
        })
    }
}
