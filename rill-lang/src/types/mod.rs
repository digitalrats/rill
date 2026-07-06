//! HM type system: scalar unification + arity synthesis.

pub mod infer;
pub mod ty;
pub mod unify;

pub use ty::{Scalar, Scheme, Subst, Type, TypeVarId};
