//! HM type system: scalar unification + arity synthesis.

pub mod infer;
pub mod ty;
pub mod unify;

pub use ty::{ArrowTy, Block, Scalar, Scheme, Subst, TypeVarId};
