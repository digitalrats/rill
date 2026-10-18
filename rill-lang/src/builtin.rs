//! Built-in factory contracts: whole-buffer built-ins callable from rill-lang.
//!
//! `BlockBuiltin` (`rill_core::Algorithm`, opaque whole-buffer) is the only
//! built-in form. Concrete bindings live outside this crate (e.g. `rill-adrift`);
//! the language owns the registry contract. Built-in SIGNATURES come from the FFI
//! catalog (`foreign` declarations in `TypeEnv::foreign_sigs`); the
//! [`ForeignRegistry`](crate::ffi::ForeignRegistry) holds only the Rust
//! FACTORIES that build runtime instances.

use rill_core::math::Transcendental;
use rill_core::traits::ParamValue;

/// A whole-buffer built-in with settable params.
pub trait BlockBuiltin<T: Transcendental>: rill_core::traits::Algorithm<T> {
    /// Set a parameter by index.
    fn set_param(&mut self, _index: usize, _value: &ParamValue) {}
}

/// A whole-buffer multi-channel built-in with settable params.
pub trait MultichannelBlockBuiltin<T: Transcendental>:
    rill_core::traits::MultichannelAlgorithm<T>
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

/// How a registered built-in factory builds its runtime instance.
///
/// Mirrors the factory variants of
/// [`ForeignRegistry`](crate::ffi::ForeignRegistry). Runtime variant selection
/// follows the factory kind rather than the signal arity: a `Block` factory can
/// serve a multi-channel arity through the interpreter's interleaved path, so a
/// name must not be forced into the multichannel variant merely because it has
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
