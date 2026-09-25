//! Scalar unification with occurs check.

use super::ty::{Scalar, Subst, TypeVarId, ValueTy};
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

/// Unify two value types.
///
/// Value-type variables resolve structurally (v1: concrete types only; Task 6
/// typeclass constraints extend this to bind through `subst`).
pub fn unify_value(
    a: &ValueTy,
    b: &ValueTy,
    _subst: &mut Subst,
    span: Span,
) -> Result<(), CompileError> {
    match (a, b) {
        (ValueTy::Var(v), other) | (other, ValueTy::Var(v)) => {
            if let ValueTy::Var(w) = other {
                if v == w {
                    return Ok(());
                }
            }
            Ok(())
        }
        (ValueTy::Int, ValueTy::Int) | (ValueTy::Float, ValueTy::Float) => Ok(()),
        (ValueTy::Data(x), ValueTy::Data(y)) if x == y => Ok(()),
        (ValueTy::Newtype(x), ValueTy::Newtype(y)) if x == y => Ok(()),
        (ValueTy::Func(ax, rx), ValueTy::Func(by, sy)) => {
            if ax.len() == by.len() && rx.len() == sy.len() {
                for i in 0..ax.len() {
                    unify_value(&ax[i], &by[i], _subst, span)?;
                }
                for i in 0..rx.len() {
                    unify_value(&rx[i], &sy[i], _subst, span)?;
                }
                Ok(())
            } else {
                Err(CompileError::Type {
                    msg: format!("cannot unify value type {a:?} with {b:?}"),
                    span,
                })
            }
        }
        _ => Err(CompileError::Type {
            msg: format!("cannot unify value type {a:?} with {b:?}"),
            span,
        }),
    }
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

    #[test]
    fn value_int_float_mismatch_errors() {
        let mut s = Subst::default();
        assert!(unify_value(&ValueTy::Int, &ValueTy::Float, &mut s, sp()).is_err());
    }

    #[test]
    fn value_matching_data_unifies() {
        let mut s = Subst::default();
        let p = ValueTy::Data("Point".into());
        unify_value(&p, &p, &mut s, sp()).unwrap();
    }

    #[test]
    fn value_var_against_concrete_ok() {
        // Value-type vars are structural in v1: unifying against a concrete
        // type records nothing yet (resolution lands with the `Subst` value
        // map in Task 6), so this only checks that no error is produced.
        let mut s = Subst::default();
        unify_value(&ValueTy::Var(0), &ValueTy::Float, &mut s, sp()).unwrap();
        unify_value(&ValueTy::Var(1), &ValueTy::Var(0), &mut s, sp()).unwrap();
        unify_value(&ValueTy::Int, &ValueTy::Int, &mut s, sp()).unwrap();
    }
}
