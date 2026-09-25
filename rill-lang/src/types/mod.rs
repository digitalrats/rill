//! HM type system: scalar unification + arity synthesis.

pub mod arrow;
pub mod infer;
pub mod ty;
pub mod unify;

pub use ty::{ArrowTy, Block, Channel, Rate, Scalar, Scheme, Subst, TypeVarId, ValueTy};
