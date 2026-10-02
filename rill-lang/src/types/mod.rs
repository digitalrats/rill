//! HM type system: scalar unification + arity synthesis.

pub mod arrow;
pub mod ffi;
pub mod infer;
pub mod ty;
pub mod unify;

pub use ffi::{ffi_sig_from_typeexpr, FfiParam, FfiSig};
pub use ty::{ArrowTy, Block, Channel, Rate, Scalar, Scheme, Subst, TypeVarId, ValueTy};
