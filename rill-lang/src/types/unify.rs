//! Value unification with occurs check.

use std::collections::HashSet;

use super::ty::{Subst, TypeVarId, ValueTy};
use crate::error::{CompileError, Span};

/// Whether the (resolved) value type `ty` transitively contains the variable
/// `v` (a `Var` or `TyConVar` id) — the occurs-check. A variable chain is
/// followed (with a visited set, so a pre-existing cyclic binding cannot loop),
/// and compound types (`Thunk`, `Con`, `Newtype`) are walked
/// structurally. `Int`/`Double`/`String` are leaves.
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
        ValueTy::Var(w) | ValueTy::TyConVar(w) => {
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
        ValueTy::Thunk(args, rets) => {
            args.iter()
                .any(|a| value_contains_var_impl(subst, a, v, seen))
                || rets
                    .iter()
                    .any(|r| value_contains_var_impl(subst, r, v, seen))
        }
        ValueTy::Con(_, args) | ValueTy::Newtype(_, args) => args
            .iter()
            .any(|a| value_contains_var_impl(subst, a, v, seen)),
        // `m b` (a type-constructor variable applied to args): check the head
        // id against `v` (same id space as `Var`/`TyConVar`), then recurse into
        // the arguments.
        ValueTy::TyConApp(f, args) => {
            if *f == v {
                return true;
            }
            args.iter()
                .any(|a| value_contains_var_impl(subst, a, v, seen))
        }
        ValueTy::String => false,
        // The element is part of the buffer type: an occurs-check must look
        // inside it, or `a ~ FixedBuffer a` would be accepted.
        ValueTy::Buffer(elem) => value_contains_var_impl(subst, elem, v, seen),
        _ => false,
    }
}

/// Unify two value types.
///
/// Value-type variables resolve structurally and RECORD their bindings in
/// `subst.value_map`: unifying a `Var` with a concrete type binds it (a lambda
/// parameter used as a higher-order-function callee becomes a `Func`; a record
/// parameter becomes its `Con` type), so lowering sees concrete parameter
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
        (ValueTy::TyConVar(v), ValueTy::TyConVar(w)) => {
            if v == w {
                return Ok(());
            }
            // Union-find by lower id, exactly like the Var↔Var arm: kind
            // variables chain in `value_map`, so `TyConVar(7) ~ TyConVar(3)`
            // followed by `TyConVar(3) ~ Con(..)` makes `TyConVar(7)` resolve
            // to the concrete constructor. Kind-aware unification (which would
            // reject a kind variable chained to a plain `*` variable) is
            // refined in Phase 8.
            if *v < *w {
                subst.value_map.insert(*w, ValueTy::TyConVar(*v));
            } else {
                subst.value_map.insert(*v, ValueTy::TyConVar(*w));
            }
            Ok(())
        }
        // Cross-kind chaining: `Var` and `TyConVar` draw ids from the same
        // counter, so a value variable may share a union-find chain with a
        // kind variable. Chain by lower id as above; kind-aware unification
        // (distinguishing `*` from `* -> *` variables) is refined in Phase 8.
        (ValueTy::Var(v), ValueTy::TyConVar(w)) | (ValueTy::TyConVar(w), ValueTy::Var(v)) => {
            if *v < *w {
                subst.value_map.insert(*w, ValueTy::Var(*v));
            } else {
                subst.value_map.insert(*v, ValueTy::TyConVar(*w));
            }
            Ok(())
        }
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
        (ValueTy::Int, ValueTy::Int)
        | (ValueTy::Double, ValueTy::Double)
        | (ValueTy::Integer, ValueTy::Integer) => Ok(()),
        (ValueTy::String, ValueTy::String) => Ok(()),
        // `FixedBuffer a` unifies with `FixedBuffer b` exactly when `a` unifies
        // with `b`: the element is part of the type, so `FixedBuffer Int` and
        // `FixedBuffer Double` are different types. A buffer NEVER unifies with a
        // non-buffer — this arm is the whole rule, and there is no
        // `Con("FixedBuffer", [elem])` second shape.
        (ValueTy::Buffer(a), ValueTy::Buffer(b)) => unify_value(a, b, subst, span),
        (ValueTy::Void, ValueTy::Void) => Ok(()),
        // Dictionaries unify by CLASS IDENTITY: a `Num` witness is exactly a
        // `Num` witness and never another class's. A dictionary is not a data
        // type, so every mixed pair (`Dict` vs `Int`/`Con`/…) falls
        // through to the catch-all and is rejected.
        (ValueTy::Dict(x), ValueTy::Dict(y)) if x == y => Ok(()),
        // THE constructor-application rule: `Con(x, ax)` ~ `Con(y, ay)` when the
        // heads name the same constructor and the arguments unify positionally.
        // There is exactly one such arm because there is exactly one
        // representation of a constructor application (GHC's `TyConApp`).
        (ValueTy::Con(x, ax), ValueTy::Con(y, ay)) if x == y && ax.len() == ay.len() => {
            for (m, n) in ax.iter().zip(ay.iter()) {
                unify_value(m, n, subst, span)?;
            }
            Ok(())
        }
        (ValueTy::Newtype(x, ax), ValueTy::Newtype(y, ay)) if x == y && ax.len() == ay.len() => {
            for (m, n) in ax.iter().zip(ay.iter()) {
                unify_value(m, n, subst, span)?;
            }
            Ok(())
        }
        // A class-var (kind variable) unifies with a concrete constructor
        // application head, checking nothing here beyond the binding — arity is
        // checked at instance-resolution time (Phase 8).
        (ValueTy::TyConVar(f), other @ ValueTy::Con(..))
        | (other @ ValueTy::Con(..), ValueTy::TyConVar(f)) => {
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
        (ValueTy::Thunk(ax, rx), ValueTy::Thunk(by, sy)) => {
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
        // `m b` (a type-constructor variable applied to args) unifies with a
        // concrete application `Maybe Double`: bind the head var to the bare
        // constructor, unify the args.
        (ValueTy::TyConApp(f, ax), other @ ValueTy::Con(..))
        | (other @ ValueTy::Con(..), ValueTy::TyConApp(f, ax)) => {
            let ValueTy::Con(c, ay) = other else {
                unreachable!()
            };
            if ax.len() != ay.len() {
                return Err(CompileError::Type {
                    msg: format!("cannot unify type-constructor application {a:?} with {b:?}"),
                    span,
                });
            }
            // Bind the head variable to the bare constructor
            // (`TyConVar ~ Con`-head).
            subst.value_map.insert(*f, ValueTy::Con(c.clone(), vec![]));
            for (x, y) in ax.iter().zip(ay.iter()) {
                unify_value(x, y, subst, span)?;
            }
            Ok(())
        }
        // `m b ~ n b'` (two type-constructor-variable applications).
        (ValueTy::TyConApp(f, ax), ValueTy::TyConApp(g, ay)) => {
            if ax.len() != ay.len() {
                return Err(CompileError::Type {
                    msg: format!("cannot unify type-constructor applications {a:?} with {b:?}"),
                    span,
                });
            }
            unify_value(&ValueTy::TyConVar(*f), &ValueTy::TyConVar(*g), subst, span)?;
            for (x, y) in ax.iter().zip(ay.iter()) {
                unify_value(x, y, subst, span)?;
            }
            Ok(())
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
    fn value_int_float_mismatch_errors() {
        let mut s = Subst::default();
        assert!(unify_value(&ValueTy::Int, &ValueTy::Double, &mut s, sp()).is_err());
    }

    #[test]
    fn value_matching_con_unifies() {
        let mut s = Subst::default();
        let p = ValueTy::Con("Point".into(), vec![]);
        unify_value(&p, &p, &mut s, sp()).unwrap();
    }

    #[test]
    fn value_var_records_binding_against_concrete() {
        // Value-type vars RECORD their bindings (Task 6): unifying against a
        // concrete type binds it in `subst.value_map`, so a later resolve sees
        // the concrete type (a lambda parameter used as an HOF callee).
        let mut s = Subst::default();
        unify_value(&ValueTy::Var(0), &ValueTy::Double, &mut s, sp()).unwrap();
        assert_eq!(s.resolve_value(&ValueTy::Var(0)), ValueTy::Double);
        unify_value(&ValueTy::Var(1), &ValueTy::Var(0), &mut s, sp()).unwrap();
        assert_eq!(s.resolve_value(&ValueTy::Var(1)), ValueTy::Double);
        unify_value(&ValueTy::Int, &ValueTy::Int, &mut s, sp()).unwrap();
        // A Func binds structurally: var 2 -> Func([Double],[Double]).
        unify_value(
            &ValueTy::Var(2),
            &ValueTy::Thunk(vec![ValueTy::Double], vec![ValueTy::Double]),
            &mut s,
            sp(),
        )
        .unwrap();
        assert_eq!(
            s.resolve_value(&ValueTy::Var(2)),
            ValueTy::Thunk(vec![ValueTy::Double], vec![ValueTy::Double])
        );
        // Arity mismatch through a bound var is still rejected.
        assert!(unify_value(
            &ValueTy::Var(2),
            &ValueTy::Thunk(
                vec![ValueTy::Double, ValueTy::Double],
                vec![ValueTy::Double]
            ),
            &mut s,
            sp(),
        )
        .is_err());
    }

    #[test]
    fn unifies_con_with_matching_ctor() {
        let mut s = Subst::default();
        let a = ValueTy::Con("List".into(), vec![ValueTy::Double]);
        let b = ValueTy::Con("List".into(), vec![ValueTy::Double]);
        unify_value(&a, &b, &mut s, sp()).unwrap();
    }

    #[test]
    fn unifies_con_with_type_var_binding() {
        let mut s = Subst::default();
        let a = ValueTy::Con("List".into(), vec![ValueTy::Var(1)]);
        let b = ValueTy::Con("List".into(), vec![ValueTy::Double]);
        unify_value(&a, &b, &mut s, sp()).unwrap();
        assert_eq!(s.resolve_value(&ValueTy::Var(1)), ValueTy::Double);
    }

    #[test]
    fn unifies_tyconvar_with_ctor_head() {
        let mut s = Subst::default();
        let pat = ValueTy::TyConVar(10);
        let ctor = ValueTy::Con("List".into(), vec![ValueTy::Double]);
        unify_value(&pat, &ctor, &mut s, sp()).unwrap();
        assert_eq!(
            s.resolve_value(&ValueTy::TyConVar(10)),
            ValueTy::Con("List".into(), vec![ValueTy::Double])
        );
    }

    #[test]
    fn recursive_kind_is_rejected_by_occurs_check() {
        // TyConVar(5) unified with Con("List", [TyConVar(5)]) is a recursive
        // kind: `f = List f`. The occurs-check must catch the TyConVar nested
        // inside the Con argument list.
        let mut s = Subst::default();
        assert!(unify_value(
            &ValueTy::TyConVar(5),
            &ValueTy::Con("List".into(), vec![ValueTy::TyConVar(5)]),
            &mut s,
            sp(),
        )
        .is_err());
        // ... and the recursive binding must not have been recorded.
        assert_eq!(s.resolve_value(&ValueTy::TyConVar(5)), ValueTy::TyConVar(5));
    }

    #[test]
    fn tyconvar_union_find_chains_to_concrete() {
        let mut s = Subst::default();
        unify_value(&ValueTy::TyConVar(7), &ValueTy::TyConVar(3), &mut s, sp()).unwrap();
        unify_value(
            &ValueTy::TyConVar(3),
            &ValueTy::Con("Maybe".into(), vec![ValueTy::Double]),
            &mut s,
            sp(),
        )
        .unwrap();
        assert_eq!(
            s.resolve_value(&ValueTy::TyConVar(7)),
            ValueTy::Con("Maybe".into(), vec![ValueTy::Double])
        );
    }

    #[test]
    fn var_tyconvar_cross_kind_chains() {
        // `Var` and `TyConVar` share one id counter, so a value variable and a
        // kind variable can chain in the same union-find.
        let mut s = Subst::default();
        unify_value(&ValueTy::Var(4), &ValueTy::TyConVar(6), &mut s, sp()).unwrap();
        unify_value(
            &ValueTy::TyConVar(6),
            &ValueTy::Con("List".into(), vec![ValueTy::Int]),
            &mut s,
            sp(),
        )
        .unwrap();
        assert_eq!(
            s.resolve_value(&ValueTy::Var(4)),
            ValueTy::Con("List".into(), vec![ValueTy::Int])
        );
    }

    #[test]
    fn bool_is_a_con_application_and_string_is_a_leaf() {
        let mut s = Subst::default();
        let b = ValueTy::bool_ty();
        unify_value(&b, &b, &mut s, sp()).unwrap();
        unify_value(&ValueTy::String, &ValueTy::String, &mut s, sp()).unwrap();
        assert!(unify_value(&ValueTy::bool_ty(), &ValueTy::Double, &mut s, sp()).is_err());
    }

    #[test]
    fn con_ctor_head_mismatch_errors() {
        let mut s = Subst::default();
        let a = ValueTy::Con("List".into(), vec![ValueTy::Double]);
        let b = ValueTy::Con("Map".into(), vec![ValueTy::Double]);
        assert!(unify_value(&a, &b, &mut s, sp()).is_err());
    }
}
