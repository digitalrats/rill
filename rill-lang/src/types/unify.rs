//! Scalar unification with occurs check.

use super::ty::{Scalar, Subst, TypeVarId};
use crate::error::{CompileError, Span};

/// Unify two scalars, extending `subst`. On mismatch, produce a type error at `span`.
pub fn unify_scalar(
    a: &Scalar,
    b: &Scalar,
    subst: &mut Subst,
    span: Span,
) -> Result<(), CompileError> {
    let a = subst.resolve_scalar(a);
    let b = subst.resolve_scalar(b);
    match (&a, &b) {
        (Scalar::Int, Scalar::Int) | (Scalar::Float, Scalar::Float) => Ok(()),
        (Scalar::Var(v), other) | (other, Scalar::Var(v)) => {
            if let Scalar::Var(w) = other {
                if v == w {
                    return Ok(());
                }
            }
            subst.map.insert(*v, other.clone());
            Ok(())
        }
        _ => Err(CompileError::Type {
            msg: format!("cannot unify scalar {a:?} with {b:?}"),
            span,
        }),
    }
}

/// Default any still-unresolved variable to `Float` (the runtime `T`).
pub fn default_var(v: TypeVarId, subst: &mut Subst) {
    subst.map.entry(v).or_insert(Scalar::Float);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp() -> Span {
        Span::new(0, 1)
    }

    #[test]
    fn var_unifies_with_float() {
        let mut s = Subst::default();
        unify_scalar(&Scalar::Var(0), &Scalar::Float, &mut s, sp()).unwrap();
        assert_eq!(s.resolve_scalar(&Scalar::Var(0)), Scalar::Float);
    }

    #[test]
    fn int_float_mismatch_errors() {
        let mut s = Subst::default();
        assert!(unify_scalar(&Scalar::Int, &Scalar::Float, &mut s, sp()).is_err());
    }

    #[test]
    fn transitive_var_chain_resolves() {
        let mut s = Subst::default();
        unify_scalar(&Scalar::Var(0), &Scalar::Var(1), &mut s, sp()).unwrap();
        unify_scalar(&Scalar::Var(1), &Scalar::Int, &mut s, sp()).unwrap();
        assert_eq!(s.resolve_scalar(&Scalar::Var(0)), Scalar::Int);
    }
}
