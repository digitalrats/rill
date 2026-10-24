//! FFI factory registry: name → Rust implementation factory for foreign
//! builtins declared in the language (`foreign name : TypeExpr;`).
//!
//! The language owns the signature; this registry owns the implementation. A
//! foreign call's `Instr::CallBlock` is resolved against
//! [`WorldState::ffi`](crate::program::WorldState) when the world is seated for
//! an IR (see [`WorldState::seat`](crate::program::WorldState::seat)).
//!
//! The runtime dispatches by [`crate::program::BuiltinInst`] variant: a `Block`
//! builtin runs through `Algorithm::process`, a multichannel builtin through
//! the multi-channel algorithm trait's `process`. Variant selection is by
//! factory kind, not signal arity — a `Block` factory is a whole-buffer block
//! builtin that may serve multi-channel signal arity via the interpreter's
//! interleaved path, so it stays a [`BlockBuiltin`] regardless of arity and is
//! never wrapped into a multichannel adapter.

use std::collections::HashMap;

use crate::builtin::{BlockBuiltin, BuiltinFactoryKind, MultichannelBlockBuiltin};
use rill_core::buffer::{SharedReader, SharedWriter};
use rill_core::math::Transcendental;
use rill_core::memory::BufferAllocator;

type BlockFactory<T> = Box<
    dyn Fn(&[f64], f32, Option<&mut BufferAllocator<T>>) -> Box<dyn BlockBuiltin<T>> + Send + Sync,
>;
type MultichannelBlockFactory<T> = Box<
    dyn Fn(
            usize,
            &[f64],
            f32,
            Option<&mut BufferAllocator<T>>,
        ) -> Box<dyn MultichannelBlockBuiltin<T>>
        + Send
        + Sync,
>;
/// A resource-backed factory: resolves its shared tape cell by INDEX (the
/// `tape_loop` path) instead of a name against an external registry. Receives
/// both the write and read handles of the cell; a head uses the one it needs.
type ResourceBlockFactory<T> = Box<
    dyn Fn(
            &[f64],
            f32,
            SharedWriter<T>,
            SharedReader<T>,
            Option<&mut BufferAllocator<T>>,
        ) -> Box<dyn BlockBuiltin<T>>
        + Send
        + Sync,
>;
type ResourceMultichannelBlockFactory<T> = Box<
    dyn Fn(
            usize,
            &[f64],
            f32,
            SharedWriter<T>,
            SharedReader<T>,
            Option<&mut BufferAllocator<T>>,
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
        factory: impl Fn(&[f64], f32, Option<&mut BufferAllocator<T>>) -> Box<dyn BlockBuiltin<T>>
            + Send
            + Sync
            + 'static,
    ) {
        self.entries
            .insert(name.into(), Factory::Block(Box::new(factory)));
    }

    /// Register a multi-channel block builtin.
    pub fn register_multichannel_block(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(
                usize,
                &[f64],
                f32,
                Option<&mut BufferAllocator<T>>,
            ) -> Box<dyn MultichannelBlockBuiltin<T>>
            + Send
            + Sync
            + 'static,
    ) {
        self.entries
            .insert(name.into(), Factory::MultichannelBlock(Box::new(factory)));
    }

    /// Register a resource-backed whole-buffer `Block` factory. The factory
    /// receives the shared tape cell's writer/reader handles (resolved by tape
    /// index at build) instead of a resource name.
    pub fn register_resource_block(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(
                &[f64],
                f32,
                SharedWriter<T>,
                SharedReader<T>,
                Option<&mut BufferAllocator<T>>,
            ) -> Box<dyn BlockBuiltin<T>>
            + Send
            + Sync
            + 'static,
    ) {
        self.entries
            .insert(name.into(), Factory::ResourceBlock(Box::new(factory)));
    }

    /// Register a resource-backed multi-channel block builtin. The factory
    /// receives the shared tape cell's writer/reader handles.
    pub fn register_resource_multichannel_block(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(
                usize,
                &[f64],
                f32,
                SharedWriter<T>,
                SharedReader<T>,
                Option<&mut BufferAllocator<T>>,
            ) -> Box<dyn MultichannelBlockBuiltin<T>>
            + Send
            + Sync
            + 'static,
    ) {
        self.entries.insert(
            name.into(),
            Factory::ResourceMultichannelBlock(Box::new(factory)),
        );
    }

    /// Build a whole-buffer `Block` factory instance for `name`, if registered.
    pub(crate) fn build_block(
        &self,
        name: &str,
        params: &[f64],
        sample_rate: f32,
        allocator: Option<&mut BufferAllocator<T>>,
    ) -> Option<Box<dyn BlockBuiltin<T>>> {
        match self.entries.get(name)? {
            Factory::Block(f) => Some(f(params, sample_rate, allocator)),
            Factory::MultichannelBlock(_)
            | Factory::ResourceBlock(_)
            | Factory::ResourceMultichannelBlock(_) => None,
        }
    }

    /// Build a multi-channel instance for `name`, if registered.
    pub(crate) fn build_multichannel_block(
        &self,
        name: &str,
        signal_ins: usize,
        params: &[f64],
        sample_rate: f32,
        allocator: Option<&mut BufferAllocator<T>>,
    ) -> Option<Box<dyn MultichannelBlockBuiltin<T>>> {
        match self.entries.get(name)? {
            Factory::MultichannelBlock(f) => Some(f(signal_ins, params, sample_rate, allocator)),
            Factory::Block(_)
            | Factory::ResourceBlock(_)
            | Factory::ResourceMultichannelBlock(_) => None,
        }
    }

    /// Build a resource-backed `Block` instance for `name` against a shared
    /// tape cell's writer/reader handles.
    pub(crate) fn build_resource_block(
        &self,
        name: &str,
        params: &[f64],
        sample_rate: f32,
        writer: SharedWriter<T>,
        reader: SharedReader<T>,
        allocator: Option<&mut BufferAllocator<T>>,
    ) -> Option<Box<dyn BlockBuiltin<T>>> {
        match self.entries.get(name)? {
            Factory::ResourceBlock(f) => Some(f(params, sample_rate, writer, reader, allocator)),
            _ => None,
        }
    }

    /// Build a resource-backed multi-channel instance for `name` against a
    /// shared tape cell's writer/reader handles.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_resource_multichannel_block(
        &self,
        name: &str,
        signal_ins: usize,
        params: &[f64],
        sample_rate: f32,
        writer: SharedWriter<T>,
        reader: SharedReader<T>,
        allocator: Option<&mut BufferAllocator<T>>,
    ) -> Option<Box<dyn MultichannelBlockBuiltin<T>>> {
        match self.entries.get(name)? {
            Factory::ResourceMultichannelBlock(f) => Some(f(
                signal_ins,
                params,
                sample_rate,
                writer,
                reader,
                allocator,
            )),
            _ => None,
        }
    }

    /// The factory kind registered for `name`, if any.
    pub(crate) fn kind(&self, name: &str) -> Option<BuiltinFactoryKind> {
        self.entries.get(name).map(|f| match f {
            Factory::Block(_) => BuiltinFactoryKind::Block,
            Factory::MultichannelBlock(_) => BuiltinFactoryKind::MultichannelBlock,
            Factory::ResourceBlock(_) => BuiltinFactoryKind::ResourceBlock,
            Factory::ResourceMultichannelBlock(_) => BuiltinFactoryKind::ResourceMultichannelBlock,
        })
    }
}
