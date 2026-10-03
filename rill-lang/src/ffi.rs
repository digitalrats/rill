//! FFI factory registry: name → Rust implementation factory for foreign
//! builtins declared in the language (`foreign fn name : TypeExpr;`).
//!
//! The language owns the signature; this registry owns the implementation. A
//! foreign call's `Instr::CallBlock` is resolved here at `RillProgram::build`
//! when the name is not in the legacy rill-core `Registry<T>`.
//!
//! The runtime dispatches by [`crate::program::BuiltinInst`] variant: a `Block`
//! builtin runs through `Algorithm::process`, a multichannel builtin through
//! `MultichannelAlgorithm::process`. Variant selection is by factory kind, not
//! signal arity — a `Block` factory is a whole-buffer block builtin that may
//! serve multi-channel signal arity via the interpreter's interleaved path, so
//! it stays a [`BlockBuiltin`] regardless of arity and is never wrapped into a
//! multichannel adapter.

use std::collections::HashMap;

use crate::builtin::{BlockBuiltin, BuiltinFactoryKind, MultichannelBlockBuiltin};
use rill_core::math::Transcendental;

type BlockFactory<T> = Box<dyn Fn(&[f64], f32) -> Box<dyn BlockBuiltin<T>> + Send + Sync>;
type MultichannelBlockFactory<T> =
    Box<dyn Fn(usize, &[f64], f32) -> Box<dyn MultichannelBlockBuiltin<T>> + Send + Sync>;

enum Factory<T: Transcendental> {
    Block(BlockFactory<T>),
    MultichannelBlock(MultichannelBlockFactory<T>),
}

/// A registry of Rust implementations for foreign-declared builtins.
pub struct ForeignRegistry<T: Transcendental> {
    entries: HashMap<String, Factory<T>>,
}

impl<T: Transcendental> Default for ForeignRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental> ForeignRegistry<T> {
    /// An empty registry.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Register a whole-buffer `Block` factory.
    pub fn register_block(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(&[f64], f32) -> Box<dyn BlockBuiltin<T>> + Send + Sync + 'static,
    ) {
        self.entries
            .insert(name.into(), Factory::Block(Box::new(factory)));
    }

    /// Register a multi-channel block builtin.
    pub fn register_multichannel_block(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(usize, &[f64], f32) -> Box<dyn MultichannelBlockBuiltin<T>>
            + Send
            + Sync
            + 'static,
    ) {
        self.entries
            .insert(name.into(), Factory::MultichannelBlock(Box::new(factory)));
    }

    /// Build a whole-buffer `Block` factory instance for `name`, if registered.
    pub(crate) fn build_block(
        &self,
        name: &str,
        params: &[f64],
        sample_rate: f32,
    ) -> Option<Box<dyn BlockBuiltin<T>>> {
        match self.entries.get(name)? {
            Factory::Block(f) => Some(f(params, sample_rate)),
            Factory::MultichannelBlock(_) => None,
        }
    }

    /// Build a multi-channel instance for `name`, if registered.
    pub(crate) fn build_multichannel_block(
        &self,
        name: &str,
        signal_ins: usize,
        params: &[f64],
        sample_rate: f32,
    ) -> Option<Box<dyn MultichannelBlockBuiltin<T>>> {
        match self.entries.get(name)? {
            Factory::MultichannelBlock(f) => Some(f(signal_ins, params, sample_rate)),
            Factory::Block(_) => None,
        }
    }

    /// The factory kind registered for `name`, if any.
    pub(crate) fn kind(&self, name: &str) -> Option<BuiltinFactoryKind> {
        self.entries.get(name).map(|f| match f {
            Factory::Block(_) => BuiltinFactoryKind::Block,
            Factory::MultichannelBlock(_) => BuiltinFactoryKind::MultichannelBlock,
        })
    }
}
