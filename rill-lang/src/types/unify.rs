//! Scalar unification with occurs check.

use std::collections::HashSet;

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

/// Whether the (resolved) value type `ty` transitively contains `Var(v)` — the
/// occurs-check. A `Var` chain is followed (with a visited set, so a
/// pre-existing cyclic binding cannot loop), and `Func` signatures are walked
/// structurally. `Data`/`Newtype`/`Int`/`Float` never carry vars in v1 but are
/// handled as leaves.
fn value_contains_var(subst: &Subst, ty: &ValueTy, v: TypeVarId) -> bool {
    let mut seen = HashSet::new();
    value_contains_var_impl(subst, ty, v, &mut seen)
}

fn value_contains_var_impl(
    subst: &Subst,
    ty: &ValueTy,
    v: TypeVarId,
    seen: &mut HashSet<TypeVarId>,
) -> bool {
    let resolved = subst.resolve_value(ty);
    match &resolved {
        ValueTy::Var(w) => {
            if *w == v {
                return true;
            }
            // The resolved representative is normally unbound; follow its
            // direct binding anyway (guarded against revisits) so a chain cut
            // short by the resolve depth guard still terminates.
            if !seen.insert(*w) {
                return false;
            }
            match subst.value_map.get(w) {
                Some(inner) => value_contains_var_impl(subst, inner, v, seen),
                None => false,
            }
        }
        ValueTy::Func(args, rets) => {
            args.iter()
                .any(|a| value_contains_var_impl(subst, a, v, seen))
                || rets
                    .iter()
                    .any(|r| value_contains_var_impl(subst, r, v, seen))
        }
        ValueTy::Data(_, args) | ValueTy::Newtype(_, args) | ValueTy::App(_, args) => args
            .iter()
            .any(|a| value_contains_var_impl(subst, a, v, seen)),
        ValueTy::Bool | ValueTy::String | ValueTy::Cap(_) => false,
        _ => false,
    }
}

/// Unify two value types.
///
/// Value-type variables resolve structurally and RECORD their bindings in
/// `subst.value_map`: unifying a `Var` with a concrete type binds it (a lambda
/// parameter used as a higher-order-function callee becomes a `Func`; a record
/// parameter becomes its `Data` type), so lowering sees concrete parameter
/// types. A `Var` unified with another `Var` chains the bindings (union-find by
/// variable id keeps the chains acyclic). Binding a `Var` to a COMPOUND type
/// that (transitively) contains it is an **occurs-check** violation — it would
/// construct an infinite type (`f f` makes `f = Func([f], [r])`) — and is
/// rejected with a clean compile error instead of a cyclic binding.
pub fn unify_value(
    a: &ValueTy,
    b: &ValueTy,
    subst: &mut Subst,
    span: Span,
) -> Result<(), CompileError> {
    let a = subst.resolve_value(a);
    let b = subst.resolve_value(b);
    match (&a, &b) {
        (ValueTy::Var(v), other) | (other, ValueTy::Var(v)) => {
            match other {
                ValueTy::Var(w) => {
                    if v == w {
                        return Ok(());
                    }
                    // Union-find: point the HIGHER variable id at the lower one
                    // so binding chains stay acyclic (a later reverse unification
                    // cannot create a cycle).
                    if *v < *w {
                        subst.value_map.insert(*w, ValueTy::Var(*v));
                    } else {
                        subst.value_map.insert(*v, ValueTy::Var(*w));
                    }
                    Ok(())
                }
                _ => {
                    // Occurs-check: a compound type containing this var would be
                    // self-referential (an infinite type).
                    if value_contains_var(subst, other, *v) {
                        return Err(CompileError::Type {
                            msg: format!(
                                "recursive function type: type variable {} cannot unify with \
                                 the self-referential type {other:?}",
                                *v
                            ),
                            span,
                        });
                    }
                    subst.value_map.insert(*v, other.clone());
                    Ok(())
                }
            }
        }
        (ValueTy::Int, ValueTy::Int) | (ValueTy::Float, ValueTy::Float) => Ok(()),
        (ValueTy::Bool, ValueTy::Bool) => Ok(()),
        (ValueTy::String, ValueTy::String) => Ok(()),
        (ValueTy::Cap(x), ValueTy::Cap(y)) if x == y => Ok(()),
        (ValueTy::Data(x, ax), ValueTy::Data(y, ay))
        | (ValueTy::Newtype(x, ax), ValueTy::Newtype(y, ay))
            if x == y && ax.len() == ay.len() =>
        {
            for (m, n) in ax.iter().zip(ay.iter()) {
                unify_value(m, n, subst, span)?;
            }
            Ok(())
        }
        (ValueTy::App(cx, ax), ValueTy::App(cy, ay)) if cx == cy && ax.len() == ay.len() => {
            for (x, y) in ax.iter().zip(ay.iter()) {
                unify_value(x, y, subst, span)?;
            }
            Ok(())
        }
        // A class-var (kind variable) unifies with a concrete constructor
        // application head, checking nothing here beyond the binding — arity is
        // checked at instance-resolution time (Phase 8).
        (ValueTy::TyConVar(f), other @ ValueTy::App(..))
        | (other @ ValueTy::App(..), ValueTy::TyConVar(f)) => {
            if value_contains_var(subst, other, *f) {
                return Err(CompileError::Type {
                    msg: format!(
                        "recursive kind: type-constructor variable {f} cannot unify with {other:?}"
                    ),
                    span,
                });
            }
            subst.value_map.insert(*f, other.clone());
            Ok(())
        }
        (ValueTy::Func(ax, rx), ValueTy::Func(by, sy)) => {
            if ax.len() == by.len() && rx.len() == sy.len() {
                for i in 0..ax.len() {
                    unify_value(&ax[i], &by[i], subst, span)?;
                }
                for i in 0..rx.len() {
                    unify_value(&rx[i], &sy[i], subst, span)?;
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
        let p = ValueTy::Data("Point".into(), vec![]);
        unify_value(&p, &p, &mut s, sp()).unwrap();
    }

    #[test]
    fn value_var_records_binding_against_concrete() {
        // Value-type vars RECORD their bindings (Task 6): unifying against a
        // concrete type binds it in `subst.value_map`, so a later resolve sees
        // the concrete type (a lambda parameter used as an HOF callee).
        let mut s = Subst::default();
        unify_value(&ValueTy::Var(0), &ValueTy::Float, &mut s, sp()).unwrap();
        assert_eq!(s.resolve_value(&ValueTy::Var(0)), ValueTy::Float);
        unify_value(&ValueTy::Var(1), &ValueTy::Var(0), &mut s, sp()).unwrap();
        assert_eq!(s.resolve_value(&ValueTy::Var(1)), ValueTy::Float);
        unify_value(&ValueTy::Int, &ValueTy::Int, &mut s, sp()).unwrap();
        // A Func binds structurally: var 2 -> Func([Float],[Float]).
        unify_value(
            &ValueTy::Var(2),
            &ValueTy::Func(vec![ValueTy::Float], vec![ValueTy::Float]),
            &mut s,
            sp(),
        )
        .unwrap();
        assert_eq!(
            s.resolve_value(&ValueTy::Var(2)),
            ValueTy::Func(vec![ValueTy::Float], vec![ValueTy::Float])
        );
        // Arity mismatch through a bound var is still rejected.
        assert!(unify_value(
            &ValueTy::Var(2),
            &ValueTy::Func(vec![ValueTy::Float, ValueTy::Float], vec![ValueTy::Float]),
            &mut s,
            sp(),
        )
        .is_err());
    }

    #[test]
    fn unifies_app_with_matching_ctor() {
        let mut s = Subst::default();
        let a = ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
        let b = ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
        unify_value(&a, &b, &mut s, sp()).unwrap();
    }

    #[test]
    fn unifies_app_with_cap_var_binding() {
        let mut s = Subst::default();
        let a = ValueTy::App("List".into(), vec![ValueTy::Var(1), ValueTy::Var(2)]);
        let b = ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
        unify_value(&a, &b, &mut s, sp()).unwrap();
        assert_eq!(s.resolve_value(&ValueTy::Var(1)), ValueTy::Float);
        assert_eq!(s.resolve_value(&ValueTy::Var(2)), ValueTy::Cap(4));
    }

    #[test]
    fn unifies_tyconvar_with_ctor_head() {
        let mut s = Subst::default();
        let pat = ValueTy::TyConVar(10);
        let ctor = ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
        unify_value(&pat, &ctor, &mut s, sp()).unwrap();
        assert_eq!(
            s.resolve_value(&ValueTy::TyConVar(10)),
            ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)])
        );
    }

    #[test]
    fn bool_string_unify_are_leaves() {
        let mut s = Subst::default();
        unify_value(&ValueTy::Bool, &ValueTy::Bool, &mut s, sp()).unwrap();
        unify_value(&ValueTy::String, &ValueTy::String, &mut s, sp()).unwrap();
        assert!(unify_value(&ValueTy::Bool, &ValueTy::Float, &mut s, sp()).is_err());
    }

    #[test]
    fn app_ctor_head_mismatch_errors() {
        let mut s = Subst::default();
        let a = ValueTy::App("List".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
        let b = ValueTy::App("Map".into(), vec![ValueTy::Float, ValueTy::Cap(4)]);
        assert!(unify_value(&a, &b, &mut s, sp()).is_err());
    }
}
