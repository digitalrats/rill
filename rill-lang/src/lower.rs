//! Lower a type-checked program to linear IR.

use std::collections::{HashMap, HashSet};

use crate::arena::KernelId;
use crate::ast::{Def, Expr, MatchArm, Param, Pattern, Program};
use crate::error::{CompileError, Span};
use crate::ir::{
    BinArith, BuiltinInstance, CmpOp, FragmentIr, FuncSig, Ir, ParamDef, StateLayout, UnOp,
    ValueBlock, ValueBuiltinOp, ValueEffectOp, ValueInstr, ValueLayout, ValueTerm,
};
use crate::mode::Mode;
use crate::types::infer::TypedProgram;
use crate::types::ty::{concat_map_result_elem, DataInfo, Subst, TypeEnv, ValueTy};
use crate::types::unify::unify_value;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::ToPrimitive;

/// Match a concrete value type against a structural pattern that may contain
/// fresh `Var` positions (a record's freshened field type), recording each
/// fresh var → concrete-type binding positionally. A `TyConApp` pattern head
/// binds to the concrete constructor's head (`m := Maybe`). Returns `false` on
/// any structural mismatch, leaving `bindings` partial (callers only use it on
/// success).
///
/// Used by the field-projection expected threading: `(compose …).unKleisli`
/// expected `Double -> Maybe Double` pins the record's type args (`m := Maybe`,
/// `a := Double`) before the record — a method chain — is lowered, so
/// `return`/`bind` inside the inlined instance bodies see the concrete monad.
fn match_field_expected(
    pattern: &ValueTy,
    concrete: &ValueTy,
    bindings: &mut Vec<(u32, ValueTy)>,
) -> bool {
    match pattern {
        ValueTy::Var(v) => {
            if let Some((_, existing)) = bindings.iter().find(|(b, _)| b == v) {
                return existing == concrete;
            }
            bindings.push((*v, concrete.clone()));
            true
        }
        ValueTy::TyConApp(f, pat_args) => match concrete {
            ValueTy::Con(c, c_args) | ValueTy::Newtype(c, c_args) => {
                if pat_args.len() != c_args.len() {
                    return false;
                }
                if let Some((_, existing)) = bindings.iter().find(|(b, _)| b == f) {
                    if !matches!(existing, ValueTy::Con(h, _) | ValueTy::Newtype(h, _) if h == c) {
                        return false;
                    }
                } else {
                    bindings.push((*f, ValueTy::Con(c.clone(), vec![])));
                }
                pat_args
                    .iter()
                    .zip(c_args.iter())
                    .all(|(p, c)| match_field_expected(p, c, bindings))
            }
            _ => false,
        },
        ValueTy::Thunk(pat_args, pat_rets) => match concrete {
            ValueTy::Thunk(c_args, c_rets) => {
                pat_args.len() == c_args.len()
                    && pat_rets.len() == c_rets.len()
                    && pat_args
                        .iter()
                        .zip(c_args.iter())
                        .all(|(p, c)| match_field_expected(p, c, bindings))
                    && pat_rets
                        .iter()
                        .zip(c_rets.iter())
                        .all(|(p, c)| match_field_expected(p, c, bindings))
            }
            _ => false,
        },
        ValueTy::Con(n, pat_args) | ValueTy::Newtype(n, pat_args) => match concrete {
            ValueTy::Con(c, c_args) | ValueTy::Newtype(c, c_args) => {
                n == c
                    && pat_args.len() == c_args.len()
                    && pat_args
                        .iter()
                        .zip(c_args.iter())
                        .all(|(p, c)| match_field_expected(p, c, bindings))
            }
            _ => false,
        },
        _ => pattern == concrete,
    }
}

/// The highest type-parameter placeholder id appearing in a declared field type:
/// `Pair`'s `first: Var(1)` → 1, `Kleisli m a b`'s `unKleisli` → 3. Used to size
/// a record's type-argument vector ([`record_args_from_fields`]).
fn param_placeholder_max(ty: &ValueTy) -> u32 {
    match ty {
        ValueTy::Var(k) | ValueTy::TyConVar(k) => *k,
        ValueTy::TyConApp(f, args) => args.iter().fold(*f, |m, a| m.max(param_placeholder_max(a))),
        ValueTy::Con(_, args) | ValueTy::Newtype(_, args) => {
            args.iter().fold(0, |m, a| m.max(param_placeholder_max(a)))
        }
        ValueTy::Thunk(args, rets) => args
            .iter()
            .chain(rets.iter())
            .fold(0, |m, a| m.max(param_placeholder_max(a))),
        // A buffer's element is a type argument position like any other.
        ValueTy::Buffer(elem) => param_placeholder_max(elem),
        _ => 0,
    }
}

/// The numeric LEAF a type annotation names (`Int`, `Integer` or `Double`), or
/// `None` when it names anything else.
///
/// An integer literal's representation is decided by its expected type alone
/// (the invariant this module enforces). An `:: T` annotation IS an expected
/// type, so `(2 :: Integer)` and `(123456789012345678901234567890 :: Double)`
/// must both take the annotated leaf. Resolving aliases through
/// [`TypeEnv::vty_of_name`] means `type D = Double; (2 :: D)` behaves like
/// `Double`, and a type VARIABLE (`(2 :: a)`) resolves to `None` — it names no
/// leaf, so the annotation stays inert and the literal keeps the caller's
/// expected type.
fn numeric_leaf_of(ty: &crate::ast::TypeExpr, env: &crate::types::ty::TypeEnv) -> Option<ValueTy> {
    let crate::ast::TypeExpr::TName(n) = ty else {
        return None;
    };
    match env.vty_of_name(n) {
        Some(leaf @ (ValueTy::Int | ValueTy::Integer | ValueTy::Double)) => Some(leaf),
        _ => None,
    }
}

/// Whether a value type is fully concrete — no unresolved type variable, no
/// type-constructor variable, no function type, no dictionary. Exactly the
/// types [`crate::types::ty::value_type_key`] can spell as an
/// instance-registry key.
fn is_concrete_ty(v: &ValueTy) -> bool {
    match v {
        ValueTy::Var(_) | ValueTy::TyConVar(_) | ValueTy::TyConApp(..) | ValueTy::Thunk(..) => {
            false
        }
        // A dictionary is concrete in the type-variable sense, but it is not a
        // DATA type: it can never be a record's type argument, and it spells no
        // registry key. Excluding it keeps this predicate exactly in step with
        // `value_type_key`.
        ValueTy::Dict(_) => false,
        ValueTy::Con(_, args) | ValueTy::Newtype(_, args) => args.iter().all(is_concrete_ty),
        // A buffer is concrete only when its element is: `FixedBuffer a` has no
        // registry key, exactly like `App("FixedBuffer", [Var])` does not.
        ValueTy::Buffer(elem) => is_concrete_ty(elem),
        _ => true,
    }
}

/// Derive a record's concrete type ARGUMENTS from its lowered field types when
/// no expected type supplied them: a record literal `(0,9)` (`Pair { first: 0,
/// second: 9 }`) in an ordinary argument position has no expected type, so
/// without this it would lower to a bare `Data("Pair", [])` and a head-arg-aware
/// class dispatch could not tell `C Int` from `C (Pair Int Int)`.
///
/// Each type-parameter placeholder in a declared field type (`Pair`'s
/// `first: Var(1)`) is bound to the matching part of the field's actual type.
/// Returns an EMPTY vector unless the WHOLE record matches: a field that does
/// not match contributes no binding at all (a partial binding from it could
/// pin the wrong parameter — e.g. `first: m a` matching `List Int` binds `m`
/// before discovering `a` disagrees), and an unresolved placeholder keeps the
/// parameterized type opaque (`Data("Kleisli", [])`), exactly as before.
fn record_args_from_fields(
    fields: &[(String, ValueTy, bool)],
    field_tys: &[ValueTy],
) -> Vec<ValueTy> {
    let mut bindings: Vec<(u32, ValueTy)> = Vec::new();
    let mut arity = 0;
    for ((_, fty, _), aty) in fields.iter().zip(field_tys.iter()) {
        arity = arity.max(param_placeholder_max(fty));
        if !match_field_expected(fty, aty, &mut bindings) {
            return vec![];
        }
    }
    let args: Vec<ValueTy> = (1..=arity)
        .map(|k| {
            bindings
                .iter()
                .find(|(v, _)| *v == k)
                .map(|(_, t)| t.clone())
                .unwrap_or(ValueTy::Var(k))
        })
        .collect();
    if args.iter().all(is_concrete_ty) {
        args
    } else {
        vec![]
    }
}

/// Whether a value instruction allocates a fresh arena slot when executed.
///
/// Drives the v1 `ValueLayout::capacity` heuristic: value registers are per-tick
/// scratch (cleared at tick end by `clear_value_regs`), so the maximum number of
/// simultaneously-live slots is bounded by the total number of allocations one
/// tick can issue. Every instruction below allocates at most one slot per tick;
/// `ValueUpdateField`'s COW copy is net-zero (a fresh slot replaces the
/// original), so counting it is a conservative over-approximation that also
/// covers the transient extra slot mid-COW. `ValueCallFunc` counts too: its
/// `dst` result is a fresh `copy` of the fragment's output.
fn is_alloc_producing(i: &ValueInstr) -> bool {
    matches!(
        i,
        ValueInstr::ValueConstInt { .. }
            | ValueInstr::ValueConstInteger { .. }
            | ValueInstr::ValueConstDouble { .. }
            | ValueInstr::ValueAdd { .. }
            | ValueInstr::ValueSub { .. }
            | ValueInstr::ValueMul { .. }
            | ValueInstr::ValueDiv { .. }
            | ValueInstr::ValueConstructRecord { .. }
            | ValueInstr::ValueConstructSum { .. }
            | ValueInstr::ValueNewtype { .. }
            | ValueInstr::ValueBindCell { .. }
            | ValueInstr::ValueReadCell { .. }
            | ValueInstr::ValueStateRead { .. }
            | ValueInstr::ValueUpdateField { .. }
            | ValueInstr::ValueMakeClosure { .. }
            | ValueInstr::ValueCallFunc { .. }
            | ValueInstr::ValueConstVoid { .. }
            | ValueInstr::ValueConstOpaque { .. }
            | ValueInstr::ValueReadParam { .. }
            | ValueInstr::ValueReadSampleRate { .. }
            | ValueInstr::ValueConstString { .. }
            | ValueInstr::ValueListLit { .. }
            | ValueInstr::ValueMapLit { .. }
            | ValueInstr::ValueCompare { .. }
            | ValueInstr::ValueCompare3 { .. }
            | ValueInstr::ValueNegate { .. }
            | ValueInstr::ValueAbs { .. }
            | ValueInstr::ValueSignum { .. }
            | ValueInstr::ValueRecip { .. }
            | ValueInstr::ValueFromInteger { .. }
            | ValueInstr::ValueNot { .. }
            | ValueInstr::ValueCallBuiltin { .. }
            | ValueInstr::ValueLazy { .. }
            | ValueInstr::ValueForce { .. }
            // Primitive effects allocate the effect closure and (for `RunST`)
            // the region's token, pair, and result.
            | ValueInstr::ValueEffectOp { .. }
            // Whole-buffer value ops allocate the `Value::Buffer` marker slot.
            | ValueInstr::ValueBufferConst { .. }
            | ValueInstr::ValueBufferLoadInput { .. }
            | ValueInstr::ValueBufferStateRead { .. }
            | ValueInstr::ValueBufferDelayRead { .. }
            | ValueInstr::ValueBufferUn { .. }
            | ValueInstr::ValueBufferBin { .. }
            | ValueInstr::ValueBufferMove { .. }
            | ValueInstr::ValueBufferFill { .. }
            | ValueInstr::ValueCallForeign { .. }
            | ValueInstr::ValueBufferParamRead { .. }
    )
}

/// The host boolean a condition expression statically IS, when it is written as
/// a bare constructor reference (`True` / `False`, or the lowercase aliases the
/// parser resolves into the same references).
///
/// This is a compile-time constant-folding convenience, not a boolean case in
/// the IR: a literal `if True then a else b` selects `a` without leaving a dead
/// branch in the program.
fn static_bool_ctor(e: &Expr) -> Option<bool> {
    match e {
        Expr::Ref(name, _) => match name.as_str() {
            "True" => Some(true),
            "False" => Some(false),
            _ => None,
        },
        // An annotation is erased at lowering: `True :: Bool` is `True`.
        Expr::Annot { expr, .. } => static_bool_ctor(expr),
        _ => None,
    }
}

/// Resolve a leftover inference variable to the runtime's default numeric type.
///
/// Lowering has no unification substitution: an effect payload whose static type
/// was still an inference variable (`Var`/`TyConVar`/`TyConApp`) types as
/// `Double`, the runtime representation of every scalar the effects manipulate.
fn concrete_or_double(t: ValueTy) -> ValueTy {
    match t {
        ValueTy::Var(_) | ValueTy::TyConVar(_) | ValueTy::TyConApp(..) => ValueTy::Double,
        other => other,
    }
}

/// Whether `main`'s lowered type is `IO ()` — the
/// [`ProgramRunner`](crate::program_runner::ProgramRunner) contract.
///
/// The runtime payload of `()` is erased to `Double(0.0)` when it travels
/// through a closure cell, so the static type recorded here is the only way
/// the runner can accept the erased unit without also accepting an `IO Double`
/// that returns `0.0`.
/// Whether the entry has the language's SHAPE for a program entry: an `IO t`
/// action, for ANY `t`.
///
/// Haskell 2010 §5.1 declares `main :: IO t` — the payload is a VARIABLE, and
/// GHC 9.10.3 measures it as usable for every `t`:
///
/// | program | GHC | runs |
/// |---|---|---|
/// | `main = return ()` | `IO ()` | yes |
/// | `main = return 42` | `IO Integer` | yes (result discarded) |
/// | `main = do { print 1; return 42 }` | `IO Integer` | yes, prints `1` |
///
/// So the entry's RESULT is discarded, not required to be `()`. Requiring
/// `IO ()` rejects programs Haskell accepts; that was this crate's own
/// restriction, not the Report's.
fn main_is_io_action(t: &ValueTy) -> bool {
    match t {
        ValueTy::Con(name, args) => name == "IO" && args.len() == 1,
        _ => false,
    }
}

/// The type arguments of a `List` value type (`[elem]`), if `t` is one.
fn list_type_args(t: &ValueTy) -> Option<&Vec<ValueTy>> {
    match t {
        ValueTy::Con(name, inner) if name == "List" => Some(inner),
        _ => None,
    }
}

/// The type arguments of an `Array i e` value type (`[index, elem]`), if `t`
/// is one.
fn array_type_args(t: &ValueTy) -> Option<&Vec<ValueTy>> {
    match t {
        ValueTy::Con(name, inner) if name == "Array" => Some(inner),
        _ => None,
    }
}

/// The index type `i` of an `Array i e` value type, or the builtin constant
/// `Int` when the argument is not an array.
///
/// Deliberately NOT routed through `concrete_or_double`: an unresolved index
/// type is not a `Double`. Arrays store their bounds verbatim as a `Pair i i`
/// whose fields the buffer bridge always builds as `Int`, so a `Double`
/// fallback would mistype the stored value the caller is about to read.
fn array_index_ty(t: Option<&ValueTy>) -> ValueTy {
    t.and_then(array_type_args)
        .and_then(|inner| inner.first().cloned())
        .unwrap_or(ValueTy::Int)
}

/// The element type `e` of an `Array i e` value type; `Double` when unknown.
fn array_elem_ty(t: Option<&ValueTy>) -> ValueTy {
    concrete_or_double(
        t.and_then(array_type_args)
            .and_then(|inner| inner.get(1).cloned())
            .unwrap_or(ValueTy::Double),
    )
}

/// The index type `i` of a `Pair i i` value type (`arrayFromList`'s bounds), or
/// the builtin constant `Int` when the argument is not a pair.
///
/// As with [`array_index_ty`], the fallback is the builtin `Int`, not a
/// `concrete_or_double` numeric fallback: the stored bounds are `Int` pairs on
/// every path that the interpreter builds (`arrayFromBuffer`), so typing an
/// unresolved parameter as `Double` would drift from the runtime value.
fn pair_index_ty(t: Option<&ValueTy>) -> ValueTy {
    // `Pair` reaches this helper in BOTH flavours: a record literal lowers to
    // `Data("Pair", …)` while an inferred/parameter type is `App("Pair", …)`.
    // Matching only `App` silently degraded a 2-D bound (`Pair (Pair Int Int)
    // (Pair Int Int)`) to a flat `Int` index.
    match t {
        Some(ValueTy::Con(name, inner)) if name == "Pair" => {
            inner.first().cloned().unwrap_or(ValueTy::Int)
        }
        _ => ValueTy::Int,
    }
}

/// The `Pair i e` association-list element's SECOND type argument (the
/// element type `e` of an index-typed `List (Pair i e)`), or the builtin `Int`
/// when the argument is not a `Pair` (mirrors [`pair_index_ty`] but reads the
/// value side of the pair).
fn pair_second_ty(t: Option<&ValueTy>) -> ValueTy {
    match t {
        Some(ValueTy::Con(name, inner)) if name == "Pair" => {
            inner.get(1).cloned().unwrap_or(ValueTy::Int)
        }
        _ => ValueTy::Int,
    }
}

/// Structurally match a signature parameter type against the actual argument
/// type, recording the signature's fresh variables (`Var(id)` from
/// `signature_param_tys_conv`) in `out`. First binding wins; a mismatch binds
/// nothing (lowering never fails on a type it cannot pattern-match).
///
/// This is the one-directional, non-failing cousin of the unifier: it exists so
/// an inlined method body sees the concrete types its earlier arguments
/// revealed (`bind m f`: `f`'s parameter is `m`'s payload).
fn bind_pattern_vars(
    pat: &ValueTy,
    actual: &ValueTy,
    out: &mut HashMap<crate::types::TypeVarId, ValueTy>,
) {
    match (pat, actual) {
        (ValueTy::Var(v), actual) if !matches!(actual, ValueTy::Var(w) if w == v) => {
            out.entry(*v).or_insert_with(|| actual.clone());
        }
        (ValueTy::Con(pn, pa), ValueTy::Con(an, aa)) if pn == an && pa.len() == aa.len() => {
            for (p, a) in pa.iter().zip(aa.iter()) {
                bind_pattern_vars(p, a, out);
            }
        }
        (ValueTy::Thunk(pa, pr), ValueTy::Thunk(aa, ar))
            if pa.len() == aa.len() && pr.len() == ar.len() =>
        {
            for (p, a) in pa.iter().zip(aa.iter()) {
                bind_pattern_vars(p, a, out);
            }
            for (p, a) in pr.iter().zip(ar.iter()) {
                bind_pattern_vars(p, a, out);
            }
        }
        (ValueTy::Newtype(pn, pa), ValueTy::Newtype(an, aa))
            if pn == an && pa.len() == aa.len() =>
        {
            for (p, a) in pa.iter().zip(aa.iter()) {
                bind_pattern_vars(p, a, out);
            }
        }
        _ => {}
    }
}

/// Apply [`bind_pattern_vars`]' recorded bindings to a type (structural,
/// transitive through bound variables).
fn subst_value_vars(t: &ValueTy, vars: &HashMap<crate::types::TypeVarId, ValueTy>) -> ValueTy {
    match t {
        ValueTy::Var(v) => match vars.get(v) {
            Some(bound @ ValueTy::Var(w)) if *w != *v => subst_value_vars(bound, vars),
            Some(bound) => bound.clone(),
            None => t.clone(),
        },
        ValueTy::Con(n, args) => ValueTy::Con(
            n.clone(),
            args.iter().map(|a| subst_value_vars(a, vars)).collect(),
        ),
        ValueTy::Newtype(n, args) => ValueTy::Newtype(
            n.clone(),
            args.iter().map(|a| subst_value_vars(a, vars)).collect(),
        ),
        ValueTy::TyConApp(f, args) => {
            ValueTy::TyConApp(*f, args.iter().map(|a| subst_value_vars(a, vars)).collect())
        }
        ValueTy::Thunk(args, rets) => ValueTy::Thunk(
            args.iter().map(|a| subst_value_vars(a, vars)).collect(),
            rets.iter().map(|a| subst_value_vars(a, vars)).collect(),
        ),
        ValueTy::Buffer(elem) => ValueTy::Buffer(Box::new(subst_value_vars(elem, vars))),
        other => other.clone(),
    }
}

/// The `(i, e)` halves of an `STArray s i e` value type.
///
/// The index falls back to the builtin `Int` (the stored bounds are `Int` pairs
/// on every path the language builds) and the element to the runtime's numeric
/// value type — the same fallbacks [`array_index_ty`]/[`array_elem_ty`] use for
/// a frozen `Array i e`.
fn starray_handle_types(t: Option<&ValueTy>) -> (ValueTy, ValueTy) {
    let args = match t {
        Some(ValueTy::Con(name, args)) if name == "STArray" && args.len() == 3 => Some(args),
        _ => None,
    };
    let index = args
        .and_then(|inner| inner.get(1).cloned())
        .unwrap_or(ValueTy::Int);
    let elem = concrete_or_double(
        args.and_then(|inner| inner.get(2).cloned())
            .unwrap_or(ValueTy::Double),
    );
    (index, elem)
}

/// The operand count of an STArray effect op.
///
/// `newArrayFlat` takes the bounds, the flat size, and the initial element;
/// `readArrayFlat` the handle and the flat offset; `writeArrayFlat` /
/// `modifyArrayFlat` those plus the stored value / modifier. The whole-cell
/// readers `getBounds` / `getElems` take the handle alone and land in the `_`
/// fallback (which also absorbs the non-STArray variants — those never reach
/// here).
fn starray_op_arity(op: ValueEffectOp) -> usize {
    match op {
        ValueEffectOp::NewArrayFlat | ValueEffectOp::NewArrayFromListFlat => 3,
        ValueEffectOp::ReadArrayFlat => 2,
        ValueEffectOp::WriteArrayFlat | ValueEffectOp::ModifyArrayFlat => 3,
        _ => 1,
    }
}

/// Re-associate a right-nested `ApplyExpr` chain whose innermost callee is a
/// name into the equivalent `Expr::Apply` argument list.
///
/// The parser's currying pass turns a multi-argument application (`cons n xs`)
/// into single-argument `ApplyExpr` nodes, but the name-dispatched lowering
/// arms (collection ops, constructors, FFI, user definitions) expect the full
/// argument list. Returns `None` when the callee is not a name chain (a field
/// projection, a lambda, an application of a function value), which must stay
/// on the opaque call path.
fn uncurry_apply_chain(e: &Expr) -> Option<(String, Vec<Expr>)> {
    match e {
        Expr::Ref(name, _) => Some((name.clone(), Vec::new())),
        Expr::ApplyExpr { callee, args, .. } => {
            let (name, mut acc) = uncurry_apply_chain(callee)?;
            acc.extend(args.iter().cloned());
            Some((name, acc))
        }
        _ => None,
    }
}

/// Flatten a curried `Thunk` chain into its total argument list and final
/// result, mirroring `types::infer::flatten_thunk`:
/// `Thunk([a], [Thunk([b], [r])])` → `(vec![a, b], vec![r])`. A flat signature
/// (`Thunk([a, b], [r])`) or a non-`Thunk` is returned as its own single level.
fn flatten_value_thunk(vty: &ValueTy) -> (Vec<ValueTy>, Vec<ValueTy>) {
    let mut args: Vec<ValueTy> = Vec::new();
    let mut cur = vty.clone();
    loop {
        match cur {
            ValueTy::Thunk(caps, res) if res.len() == 1 && matches!(res[0], ValueTy::Thunk(..)) => {
                args.extend(caps);
                cur = res[0].clone();
            }
            ValueTy::Thunk(caps, res) => {
                args.extend(caps);
                return (args, res);
            }
            other => return (args, vec![other]),
        }
    }
}

/// Whether a value type is a curried nest (`Thunk([a], [Thunk(…, …)])`) rather
/// than a flat single-level signature. A curried nest is applied one argument
/// at a time; a flat signature consumes all its arguments in one dispatch.
fn is_curried_thunk(vty: &ValueTy) -> bool {
    matches!(vty, ValueTy::Thunk(caps, res)
        if caps.len() == 1 && res.len() == 1 && matches!(res[0], ValueTy::Thunk(..)))
}

/// The DECLARED parameter types of a builtin, from its `foreign fn` signature
/// in [`TypeEnv::builtin_sigs`]. An empty list means the name has no declared
/// signature (or its signature is not a function type), in which case the
/// caller lowers arguments without an expected type.
///
/// A `TForall`-bound or free type-variable parameter (`a`, `e`, `s`) resolves
/// to a fresh unification variable, which the caller must NOT thread as an
/// expected type — it would pin an unrelated type at the argument. Concrete
/// entries are what make a literal take the declared parameter's track.
fn builtin_param_tys(env: &TypeEnv, name: &str) -> Vec<ValueTy> {
    let Some(sig) = env.builtin_sigs.get(name) else {
        return Vec::new();
    };
    let mut vars: HashMap<String, ValueTy> = HashMap::new();
    let mut next = 0u32;
    let mut fresh = || {
        next += 1;
        ValueTy::Var(u32::MAX - next)
    };
    sig.arg_types()
        .iter()
        .map(|te| {
            crate::types::infer::sig_ty_to_vty(env, te, &HashMap::new(), &mut vars, &mut fresh)
                .unwrap_or(ValueTy::Var(u32::MAX))
        })
        .collect()
}

/// Wrap a flat argument list back into the curried nest a leftover closure
/// value has at runtime: `([b, c], [r])` → `Thunk([b], [Thunk([c], [r])])`.
/// Keeping the nest (rather than a flat `Thunk([b, c], [r])`) means a further
/// partial application is applied one argument at a time as well.
fn nest_value_thunk(arg_tys: &[ValueTy], ret_tys: Vec<ValueTy>) -> ValueTy {
    match arg_tys.split_last() {
        Some((last, rest)) => {
            let mut ty = ValueTy::Thunk(vec![last.clone()], ret_tys);
            for a in rest.iter().rev() {
                ty = ValueTy::Thunk(vec![a.clone()], vec![ty]);
            }
            ty
        }
        None => ret_tys.first().cloned().unwrap_or(ValueTy::Double),
    }
}

/// Mirror inference's `pin_result_to_expected` at a user-def call site: when the
/// callee's result is a type-constructor variable applied to args (`m b`) and the
/// call site expects a concrete container (`Maybe`, from a match arm or an
/// enclosing method parameter), bind that head var to the expected's constructor
/// throughout the callee's argument and result types. Lowering has no
/// unification substitution, so the bind is a structural replacement:
/// `Kleisli m a b` becomes `Kleisli Maybe a b`, so a result-directed method
/// chain in an argument (`arr f` inside `apply (compose …) 3.0`) threads the
/// concrete monad into the instance body's `return`/`bind`.
///
/// This is a FALLBACK for a callee with no scheme to unify (the caller tries
/// [`Lowerer::scheme_pinned_params`] first). It deliberately does NOT try to bind
/// a bare `Var` result: the type lists it receives come from a LOWERED closure
/// signature, where the parameter and the result are independent residual
/// variables even for `f x = x`. Pinning the result variable there pins nothing
/// about the parameter — a guess in disguise: it converts no literal but does
/// suppress the literal's own exact conversion.
fn pin_ret_head_types(
    arg_tys: &[ValueTy],
    ret_tys: &[ValueTy],
    expected: Option<&ValueTy>,
) -> (Vec<ValueTy>, Vec<ValueTy>) {
    let Some(exp) = expected else {
        return (arg_tys.to_vec(), ret_tys.to_vec());
    };
    let repl = match exp {
        ValueTy::Con(c, _) => ValueTy::Con(c.clone(), vec![]),
        _ => return (arg_tys.to_vec(), ret_tys.to_vec()),
    };
    let Some(ValueTy::TyConApp(hv, _)) = ret_tys.first() else {
        return (arg_tys.to_vec(), ret_tys.to_vec());
    };
    let hv = *hv;
    (
        arg_tys
            .iter()
            .map(|t| subst_tycon_head(t, hv, &repl))
            .collect(),
        ret_tys
            .iter()
            .map(|t| subst_tycon_head(t, hv, &repl))
            .collect(),
    )
}

/// Replace a type-constructor variable (`hv`) with `repl` throughout `t`: bare
/// occurrences (`Kleisli m a b` → `Kleisli Maybe a b`) and applied heads
/// (`m b` → `Maybe b`).
fn subst_tycon_head(t: &ValueTy, hv: u32, repl: &ValueTy) -> ValueTy {
    match t {
        ValueTy::Var(v) | ValueTy::TyConVar(v) if *v == hv => repl.clone(),
        ValueTy::TyConApp(f, args) => {
            let args: Vec<ValueTy> = args.iter().map(|a| subst_tycon_head(a, hv, repl)).collect();
            if *f == hv {
                match repl {
                    ValueTy::Con(c, _) => ValueTy::Con(c.clone(), args),
                    _ => ValueTy::TyConApp(*f, args),
                }
            } else {
                ValueTy::TyConApp(*f, args)
            }
        }
        ValueTy::Thunk(a, r) => ValueTy::Thunk(
            a.iter().map(|x| subst_tycon_head(x, hv, repl)).collect(),
            r.iter().map(|x| subst_tycon_head(x, hv, repl)).collect(),
        ),
        ValueTy::Con(n, a) => ValueTy::Con(
            n.clone(),
            a.iter().map(|x| subst_tycon_head(x, hv, repl)).collect(),
        ),
        ValueTy::Newtype(n, a) => ValueTy::Newtype(
            n.clone(),
            a.iter().map(|x| subst_tycon_head(x, hv, repl)).collect(),
        ),
        ValueTy::Buffer(elem) => ValueTy::Buffer(Box::new(subst_tycon_head(elem, hv, repl))),
        other => other.clone(),
    }
}

struct Lowerer<'a> {
    defs: HashMap<String, Def>,
    cafs: &'a HashSet<String>,
    /// Lowered registers of each lifted CAF, keyed by `(name, call-site args)`.
    /// Task 5 threads the caller's signal args into a value-CAF's body (a
    /// signal-lambda macro), so the lowered body is SITE-DEPENDENT: two sites
    /// applying the same CAF to different wires must each lower with their own
    /// wire. Genuine signal CAFs (empty signal-ins) never consume the threaded
    /// args, so identical-args sites share the entry; distinct-arg sites get
    /// separate (still correct) copies.
    caf_cache: HashMap<(String, Vec<usize>), Vec<usize>>,
    caf_lifting: HashSet<String>,
    block_state_slots: usize,
    delay_lens: Vec<usize>,
    locals: Vec<HashMap<String, Vec<usize>>>,
    builtins: Vec<BuiltinInstance>,
    params: Vec<ParamDef>,
    param_names: HashMap<String, usize>,
    /// Value-track blocks, executed once per run by
    /// [`exec_entry`](crate::backend::interp::exec_entry) (see
    /// [`Ir::value_blocks`](crate::ir::Ir)).
    value_blocks: Vec<ValueBlock>,
    /// The block currently being appended to.
    cur_value_block: usize,
    /// Entry block of the program's value track (always 0).
    value_entry: usize,
    /// Next value register index (SSA value registers are per-run scratch).
    next_value_reg: usize,
    /// Fresh type-variable counter (for `signature_param_tys_conv` in the
    /// typeclass method-call lowering, which threads expected types).
    next_tyvar: u32,
    /// The dictionary the type checker SELECTED at each method call, keyed by
    /// the call's span AND the class whose method is called (from
    /// `TypedProgram::dict_selections`). Lowering has no
    /// unification substitution, so its own static types are often coarser than
    /// inference's (`x` bound by `Just x` from a `Maybe a` scrutinee is a `Var`
    /// here, `Double` there). The recorded selection is the AUTHORITATIVE
    /// answer: a method call that finds one uses it instead of re-deriving the
    /// instance from the coarser type.
    dict_selections: HashMap<(Span, String), ValueTy>,
    /// The value type inference gave each LITERAL, keyed by its source span —
    /// stage 1 unified it, stage 2 discharged it, or stage 3 defaulted it.
    ///
    /// This is the literal's ANSWER, and reading it is what makes lowering free
    /// of per-position heuristics: a literal's representation is decided once, by
    /// inference, and never re-guessed from where it happens to sit.
    literal_tys: HashMap<(usize, usize), ValueTy>,
    /// The type each literal was DECIDED to have at its own span, from
    /// inference's `record_literal_instance`. Takes precedence over `literal_tys`
    /// because it is written at the moment unification settled the variable.
    literal_instances: HashMap<(usize, usize), ValueTy>,
    /// Fresh-name counter for capture-avoiding method instantiation. Every
    /// instantiation renames the method's own parameters to `$`-prefixed names
    /// carrying this value, so two instantiations never share a binding name
    /// and no `$` name can collide with a source identifier.
    next_method_param: usize,
    /// Value registers holding the program's value outputs (value-channel
    /// entry). `[0]` becomes [`Ir::result_reg`](crate::ir::Ir) at the end of
    /// lowering.
    value_regs_out: Vec<usize>,
    /// Static value type of each value output, parallel to [`Self::value_regs_out`].
    /// The arena-capacity heuristic needs it: a compound output (record/sum/
    /// newtype) pins its whole subtree across runs, so the bound must account
    /// for the output's subtree size, not just one slot per channel.
    value_out_tys: Vec<ValueTy>,
    /// Static types of every container-typed value subexpression (`List`,
    /// `Map`, `Set`) produced during lowering. A container pins its element
    /// slots while it lives, so the arena bound must sum each container's
    /// subtree size (the `is_alloc_producing` count alone credits one slot per
    /// op and undercounts `map`/`filter`/`cons`/literal containers by their
    /// element slots). With `value_builtin_ty` propagating source caps, these
    /// types carry the exact capacities the runtime allocates.
    container_tys: Vec<ValueTy>,
    /// First-class function values referenced by [`ValueInstr::ValueMakeClosure`].
    /// A bare reference to a user definition in value position allocates an
    /// eager [`Value::Callable`] referencing the entry's index.
    value_funcs: Vec<crate::ir::ValueFunc>,
    /// Scope stack of value locals: name → (value reg, value type). Match-arm
    /// bindings (and, in a later task, `main` λ-params) are aliased by name to
    /// per-run value registers — a `Ref` to one returns the register directly.
    value_locals: Vec<HashMap<String, (usize, ValueTy)>>,
    /// Statically-known constructor of each value local, parallel to the scope
    /// stack [`Self::value_locals`]: a method param bound to `Just 1.0` records
    /// `Some("Just")`, so a `match` over the param's `Ref` selects its arm at
    /// compile time (v1 static dispatch). `None` when the local's ctor is not
    /// statically known.
    value_local_ctors: Vec<HashMap<String, Option<String>>>,
    /// The shared compile-time type environment (aliases, newtypes, data-type
    /// shapes) carried from inference — the single source of truth for name
    /// resolution in both infer and lower.
    env: &'a TypeEnv,
    /// Evaluation mode: strict rejects recursion (the v1 acyclic-call
    /// contract); complete allows it (lazy value track, later tasks).
    mode: Mode,
    /// Recursion guard while inlining value definitions.
    value_inline: HashSet<String>,
    /// Recursion guard for typeclass method inlining: resolved (class, type,
    /// method) calls currently on the expansion path.
    method_lifting: HashSet<(String, String, String)>,
    /// Recursion guard for class-constrained definition specialization: the
    /// `=>`-definitions currently being inlined at a call site. Their bodies
    /// mention the constrained class, so a self- or mutually-recursive call
    /// cannot be specialized eagerly (there is no dictionary to thread).
    constrained_lifting: HashSet<String>,
    /// Head-argument bindings of a constraint instance (`instance (Monad m) =>
    /// Arrow (Kleisli m)`): head arg name → concrete type taken from the
    /// call-site concrete container's LEADING args. The instance body's own
    /// record construction resolves against the same concrete type via the
    /// expected-type threading.
    head_arg_bindings: HashMap<String, ValueTy>,
    /// Compiled function bodies (lambda literals), indexed by
    /// [`ValueInstr::ValueMakeClosure`]'s `fragment` field.
    fragments: Vec<std::sync::Arc<FragmentIr>>,
    /// Free-variable names of the lambda currently being lowered into a
    /// fragment (empty at the program level). A `Ref` to one of these inside
    /// the fragment emits a `ValueReadCell { cell: capture_index }` reading the
    /// call's env frame.
    fragment_captures: Vec<(String, ValueTy)>,
    /// Resolved λ-parameter value types of every named lambda-literal
    /// definition (from inference). Lowering types fragment-local parameter
    /// registers with these: a higher-order parameter is a `Func` (so the
    /// callee dispatches), a record parameter its `Data` type (so field
    /// projection resolves indices).
    fn_param_tys: HashMap<String, Vec<ValueTy>>,
    /// Flat RESULT value types of every def's inferred scheme (from inference),
    /// parallel in spirit to [`Self::fn_param_tys`]. The signature-less
    /// call-site specialization needs them to mirror the ordinary application
    /// path's `pin_ret_head_types`: a polymorphic result (`apply k x =
    /// k.unKleisli x` → `m b`) carries a type-constructor variable that the
    /// caller's expected type pins so a result-directed/constructor-class
    /// method inside an argument (`arr f`) threads the concrete monad into its
    /// instance body.
    def_result_tys: HashMap<String, Vec<ValueTy>>,
    /// Static RESULT value type of every shared-cell def (named λ-parameter
    /// function or self-recursive local), from inference. Types the def's
    /// shared cell so a recursive reference inside the body (`tail fibs`)
    /// resolves its static type (List) instead of the Double fallback.
    shared_cell_result_tys: HashMap<String, ValueTy>,
    /// Parameter types for the lambda literal currently being lowered, when it
    /// is a NAMED lambda-literal definition. Consumed by the `Expr::Lambda`
    /// arm; anonymous lambdas leave it unset and default to `Double`.
    pending_param_tys: Option<Vec<ValueTy>>,
    /// Complete-mode shared cells for top-level Anchor defs (name → cell reg).
    /// A named λ-param def referenced as a value compiles its closure ONCE and
    /// stores it in this cell; every other reference — including a mutually
    /// recursive partner (`even`/`odd`) — captures the CELL, not a recompiled
    /// closure. This is the STG model: a top-level binding is a thunk in a
    /// cell that holds either a pure value or a closure, and recompiling the
    /// binding on reference is what made mutual recursion overflow the
    /// compile-time stack.
    anchor_cells: HashMap<String, usize>,
    /// Memoized closures of top-level Anchor defs (name → closure value reg),
    /// so a def's fragment is compiled exactly once per program.
    anchor_closures: HashMap<String, usize>,
    /// Static `Func` type of each memoized Anchor-def closure, parallel to
    /// [`Self::anchor_closures`].
    anchor_types: HashMap<String, ValueTy>,
    /// Top-level Anchor defs whose closures are currently being compiled (the
    /// mutual-recursion guard): when `even`'s compile reaches `odd` and `odd`'s
    /// compile reaches `even` again, the cell (not a fresh compile) is the
    /// forward reference.
    anchor_compiling: HashSet<String>,
    /// Complete mode: LOCAL defs that participate in a reference cycle (self-
    /// OR mutual-recursion: `a = a`, `a = b; b = a`). Like Anchor closures,
    /// they get a SHARED cell so the recursion resolves through the cell and
    /// forcing a divergent cycle yields `Bottom` (not an unbound register).
    recursive_locals: HashSet<String>,
    /// Named tape declarations (`name = tape_loop <capacity>`) extracted from
    /// the AST. A resource param's `Ref(name)` resolves to the capacity here to
    /// assign a tape index; a `Ref` NOT present stays a name-based resource
    /// binding, which seating rejects as an undeclared resource.
    tape_decls: HashMap<String, usize>,
    /// Span (`start`, `end`) of every real-`ST` `runST` application → the
    /// region's payload type `a`, from inference. Lowering has no substitution,
    /// so this is the only source of the payload type (the lowered `ST` record
    /// type carries no argument).
    runst_payload_tys: HashMap<(usize, usize), ValueTy>,
    /// Program input → value-register mapping: input `k` materializes into
    /// `buffers[input_buffers[k]]` (the value register holding the input's
    /// `Value::Buffer`). Filled by the `LoadInput` emission at program entry.
    input_buffers: Vec<usize>,
    /// Tape cell capacities, one entry per tape; the index IS the tape index
    /// referenced by [`BuiltinInstance::tape_index`]. A NAMED declaration
    /// (`name = tape_loop <capacity>`) gets one cell per name — repeated
    /// `Ref(name)` uses share it (write head + read heads on one tape). An
    /// INLINE `tape_loop <capacity>` call allocates a FRESH cell per occurrence
    /// (each constructor call is a distinct tape).
    tapes: Vec<usize>,
    /// Tape index per named declaration (`name -> index into [`Self::tapes`]`),
    /// so all `Ref(name)` uses of the same declared tape resolve to one cell.
    named_tapes: HashMap<String, usize>,
    /// Whether lowering lowered an `arrayFromAssocs` call (the M8 polymorphic
    /// array construction). Set during `value_builtin` name resolution; the
    /// assembly step appends one synthetic hole fragment when set and points
    /// [`Ir::array_hole_frag`](crate::ir::Ir) at it.
    uses_array_hole: bool,
    /// Index of the appended synthetic hole fragment; set together with
    /// `uses_array_hole` during assembly and copied into the `Ir`. `None`
    /// (the default) for a program that never lowers `arrayFromAssocs`.
    array_hole_frag: Option<u32>,
}

/// Reject a specialized constrained-definition body whose static type
/// contradicts the signature's declared result.
///
/// Inference never types the deferred body (the class variable is abstract
/// until a call site solves it), so this is the only place the body/result
/// contract is checked. Lowering has no unification substitution, so the check
/// is deliberately conservative: it fires only when BOTH types are concrete —
/// [`value_type_key`] canonicalizes `Data`/`App` and returns `None` for
/// anything involving a type variable or a function type.
///
/// [`value_type_key`]: crate::types::ty::value_type_key
fn check_specialized_result(
    env: &TypeEnv,
    name: &str,
    actual: &ValueTy,
    declared: &ValueTy,
    span: Span,
) -> Result<(), CompileError> {
    let (Some(actual_key), Some(declared_key)) = (
        crate::types::ty::value_type_key(env, actual),
        crate::types::ty::value_type_key(env, declared),
    ) else {
        return Ok(());
    };
    if actual_key == declared_key {
        return Ok(());
    }
    // The comparison is on registry KEYS (nominal constructor names), but the
    // diagnostic quotes the SOURCE spelling, so a list or tuple reads back as
    // `[Int]` / `(a, b)` rather than `List Int` / `Pair a b`.
    Err(CompileError::Type {
        msg: format!(
            "body of `{name}` has type `{}`, but its signature declares `{}`",
            env.type_spelling(actual),
            env.type_spelling(declared)
        ),
        span,
    })
}

/// Whether a `let`/`where` binding denotes a FUNCTION: a body whose leading
/// λ-nest binds at least one parameter (`g y = e` IS `g = \y -> e`, Haskell
/// 2010 §4.4.3.1, so arity is a property of the BODY).
fn is_local_function(d: &Def) -> bool {
    d.arity() > 0
}

impl<'a> Lowerer<'a> {
    fn fresh_value_reg(&mut self) -> usize {
        let r = self.next_value_reg;
        self.next_value_reg += 1;
        r
    }

    /// Lower a numeric literal's exact payload and the conversion that makes it
    /// the value it denotes: `fromInteger (n :: Integer)` for an integer literal
    /// and `fromRational (r :: Rational)` for a fractional one (Haskell 2010
    /// §6.4.1).
    ///
    /// `expected` is the type INFERENCE decided for this literal — stage 1
    /// unified it, stage 2 discharged it, or stage 3 defaulted it. Lowering
    /// therefore converts to that type and guesses nothing: the per-position
    /// heuristics this replaces (`Int` here, `Double` there, `Integer` under an
    /// annotation) are gone.
    ///
    /// The payload is always the EXACT value from the source digits. An `Int`
    /// target converts through `ValueFromInteger` too — that is where the ONE
    /// truncation (`literal_to_i64`) lives, exactly as GHC's `fromInteger ::
    /// Integer -> Int` truncates — so a literal past 2^63 keeps GHC's low-64-bit
    /// answer while the `Integer`/`Double` tracks stay exact.
    ///
    /// Kept out of line: `lower_value`'s stack frame is at its ceiling (five
    /// prior tasks needed the same treatment), and two literal arms inline would
    /// push it over.
    #[inline(never)]
    fn lower_integer_literal(
        &mut self,
        v: &BigInt,
        expected: Option<ValueTy>,
    ) -> Result<(usize, ValueTy), CompileError> {
        // An exact `Integer` payload register: the literal's own value.
        let exact = self.emitter_integer(v);
        // `fromInteger`'s target is the literal's type. With no expectation at
        // all — which only a bare, unconstrained literal reaches — the Haskell
        // 2010 default for an ambiguous numeric variable applies, and stage 3
        // has already made that decision; `Integer` is the list's first entry.
        let target = expected.unwrap_or(ValueTy::Integer);
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueFromInteger {
            dst,
            src: exact,
            target: self.conversion_tag(&target),
        });
        Ok((dst, target))
    }

    /// Emit the exact `Integer` register for a literal payload. A separate
    /// helper so both literal kinds and the negated-constant path share one
    /// emission site.
    fn emitter_integer(&mut self, v: &BigInt) -> usize {
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueConstInteger {
            dst,
            value: Box::new(v.clone()),
        });
        dst
    }

    /// The instance inference gave a LITERAL at `span`, if it recorded one.
    ///
    /// A literal's type is decided ONCE, by the three inference stages
    /// (`Num a`'s `fromInteger` / `Fractional a`'s `fromRational`), so lowering
    /// asks for that answer instead of guessing from the position. A literal no
    /// stage could decide (a polymorphic `Num a => a` position) has no entry:
    /// lowering then keeps the position's own expected type, which IS the
    /// caller's instantiation.
    fn literal_instance(&self, span: Span) -> Option<ValueTy> {
        self.literal_instances
            .get(&(span.start, span.end))
            .or_else(|| self.literal_tys.get(&(span.start, span.end)))
            .cloned()
    }

    /// The `ValueTyTag` a `fromInteger`/`fromRational` conversion builds for the
    /// target type the literal's inference settled on.
    ///
    /// `Int` is the truncation target (`fromInteger :: Integer -> Int` in
    /// Haskell 2010 truncates), `Integer`/`Double` are exact, and a buffer
    /// target fills the block. A target the conversion cannot serve (a `Ratio`,
    /// whose `fromInteger` is a SOURCE equation) is reported rather than
    /// guessed — lowering must never invent an instance.
    fn conversion_tag(&self, target: &ValueTy) -> crate::ir::ValueTyTag {
        match target {
            ValueTy::Int => crate::ir::ValueTyTag::Int,
            ValueTy::Integer => crate::ir::ValueTyTag::Integer,
            ValueTy::Buffer(_) => crate::ir::ValueTyTag::Buffer,
            ValueTy::Double => crate::ir::ValueTyTag::Double,
            // An unresolved target is a position inference never pinned (a
            // polymorphic signature-less definition's λ-parameter). Stage 3's
            // `default (Integer, Double)` takes its FIRST entry that satisfies
            // the literal's `Num` constraint — `Integer`.
            _ => crate::ir::ValueTyTag::Integer,
        }
    }

    /// Lower `fromRational (r :: Rational)`.
    #[inline(never)]
    fn lower_fractional_literal(
        &mut self,
        v: &BigRational,
        expected: Option<ValueTy>,
    ) -> Result<(usize, ValueTy), CompileError> {
        // The exact payload the source digits denote (Task 6 keeps it exact).
        let exact = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueConstRational {
            dst: exact,
            value: Box::new(v.clone()),
        });
        // `fromRational`'s target: `Fractional`'s only scalar instance is
        // `Double`, plus the buffer constant. Stage 3 never lands a fractional
        // literal on `Integer` (it has no `Fractional` instance), so `Double` is
        // the fallback only when the position was not resolved to a buffer.
        let target = match expected {
            Some(t @ ValueTy::Buffer(_)) => t,
            _ => ValueTy::Double,
        };
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueFromRational {
            dst,
            src: exact,
            target: self.conversion_tag(&target),
        });
        Ok((dst, target))
    }

    /// Emit a function application: eager `ValueCallFunc` in strict mode,
    /// lazy `ValueCallLazy` (a thunk) in complete mode. The lazy form is what
    /// makes recursion and infinite structures productive — the call suspends
    /// instead of running.
    fn emit_call(&mut self, dst: usize, callee_reg: usize, args: Vec<usize>) {
        if self.mode == Mode::Complete {
            self.emit_value(ValueInstr::ValueCallLazy {
                dst,
                closure_slot: callee_reg,
                args,
            });
        } else {
            self.emit_value(ValueInstr::ValueCallFunc {
                dst,
                closure_slot: callee_reg,
                args,
            });
        }
    }

    fn emit_value(&mut self, i: ValueInstr) {
        self.value_blocks[self.cur_value_block].instrs.push(i);
    }

    /// Emit the runtime one-pole smoother over the Buffer register `x` and
    /// return the output Buffer register.
    ///
    /// `smooth x ms` is `y = prev + a·(x − prev)` where `prev` is the site's
    /// block-feedback slot and `a` is computed per run (see
    /// [`Self::emit_smooth_coef`]) — the compiler never needs a sample rate.
    fn emit_smooth(&mut self, x: usize, ms: f64) -> usize {
        let acoef = self.emit_smooth_coef(ms);
        let slot = self.block_state_slots;
        self.block_state_slots += 1;
        let prev = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferStateRead { dst: prev, slot });
        let diff = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferBin {
            dst: diff,
            op: BinArith::Sub,
            a: x,
            b: prev,
        });
        let scaled = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferBin {
            dst: scaled,
            op: BinArith::Mul,
            a: acoef,
            b: diff,
        });
        let y = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferBin {
            dst: y,
            op: BinArith::Add,
            a: prev,
            b: scaled,
        });
        self.emit_value(ValueInstr::ValueBufferStateWrite { slot, src: y });
        y
    }

    /// Emit the smoother coefficient `a` as a whole-block buffer constant,
    /// computed **at run time** from the world's sample rate:
    /// `a = 1 − exp(−1 / max((ms/1000)·sr, 0))` — the formula the compiler used
    /// to bake in f64 at build time.
    ///
    /// Guards: `ms <= 0` bakes `a = 1` (a static property of the call). A
    /// non-positive rate must yield the same identity, and the rate is only
    /// known at run time, so the *denominator* is clamped at zero in the
    /// emitted sequence: at `sr == 0` the denominator is `0`, `-1/0 = −∞`,
    /// `exp(−∞) = 0`, so `a = 1` exactly; at `sr < 0` the clamp turns the
    /// negative denominator into the same `0`, instead of
    /// `-1/den > 0 → exp(+240) → −inf → a = −inf`. No NaN or infinity escapes
    /// into the output. The rate itself is read from the world's
    /// `Params::sample_rate` field through [`ValueInstr::ValueReadSampleRate`],
    /// the same field `?sample_rate` resolves to.
    fn emit_smooth_coef(&mut self, ms: f64) -> usize {
        let dst = self.fresh_value_reg();
        if ms <= 0.0 {
            self.emit_value(ValueInstr::ValueBufferConst { dst, value: 1.0 });
            return dst;
        }
        let sr = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueReadSampleRate { dst: sr });
        let sr_buf = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferFill {
            dst: sr_buf,
            src: sr,
        });
        let tau = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferConst {
            dst: tau,
            value: ms / 1000.0,
        });
        let tau_sr = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferBin {
            dst: tau_sr,
            op: BinArith::Mul,
            a: tau,
            b: sr_buf,
        });
        // `max((ms/1000)·sr, 0)`: a non-positive rate degenerates to the
        // `sr == 0` identity instead of overflowing `exp`.
        let zero = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferConst {
            dst: zero,
            value: 0.0,
        });
        let den = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferBin {
            dst: den,
            op: BinArith::Max,
            a: tau_sr,
            b: zero,
        });
        let neg_one = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferConst {
            dst: neg_one,
            value: -1.0,
        });
        let inv = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferBin {
            dst: inv,
            op: BinArith::Div,
            a: neg_one,
            b: den,
        });
        let decay = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferUn {
            dst: decay,
            op: UnOp::Exp,
            src: inv,
        });
        let one = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferConst {
            dst: one,
            value: 1.0,
        });
        self.emit_value(ValueInstr::ValueBufferBin {
            dst,
            op: BinArith::Sub,
            a: one,
            b: decay,
        });
        dst
    }

    /// Wrap an already-emitted primitive effect closure in the `ST s a` record
    /// (its `unST` field) and return the record's register with its `ST` type.
    ///
    /// The region phantom `s` has no runtime representation, so it is typed
    /// `Void` here; the payload `a` is what `runST` extracts after running the
    /// region.
    fn finish_st_effect(&mut self, effect: usize, payload: ValueTy) -> (usize, ValueTy) {
        let rec = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueConstructRecord {
            dst: rec,
            fields: vec![effect],
        });
        (rec, ValueTy::Con("ST".into(), vec![ValueTy::Void, payload]))
    }

    /// Lower `runSTArray (fn s -> body)` — the freezing sibling of `runST`.
    ///
    /// Like `runST`, the phantom-`s` lambda is dropped at lowering: the body is
    /// the region computation and the emitted `RunSTArray` effect freezes its
    /// `STArray` result into an immutable `Array i e`. Inference already
    /// rejected a body that is not an `ST s (STArray s i e)` computation and
    /// recorded the frozen result type in `runst_payload_tys`.
    fn lower_runst_array(
        &mut self,
        call_args: &[Expr],
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        if call_args.len() != 1 {
            return Err(CompileError::Type {
                msg: "runSTArray expects 1 argument".into(),
                span,
            });
        }
        let Expr::Lambda { body: lam, .. } = &call_args[0] else {
            return Err(CompileError::Type {
                msg: "runSTArray expects a parenthesized lambda `fn s -> body`".into(),
                span,
            });
        };
        let Some(payload) = self.runst_payload_tys.get(&(span.start, span.end)).cloned() else {
            return Err(CompileError::Type {
                msg: "runSTArray body must be an `ST s (STArray s i e)` computation".into(),
                span,
            });
        };
        let (body_reg, _) = self.lower_value_expected(lam, None)?;
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueEffectOp {
            dst,
            op: ValueEffectOp::RunSTArray,
            args: vec![body_reg],
        });
        // The frozen `Array i e` is returned with its STRUCTURE intact.
        // `concrete_or_double` collapsed the whole type to `Double` whenever a
        // COMPONENT was still a variable, so an index the region never pinned
        // (the common case: `newListArray (Pair 0 2) xs` leaves `i` free at the
        // `runSTArray` site) made the result a bare `Double`. Indexing that with
        // `!` then had no `Array i e` to read and reported "no instance of `Ix`
        // for type `Pair`". The element half still needs the numeric fallback
        // (a `Double` element is the value track's scalar), but the ARRAY — and
        // so its index — must survive.
        let frozen = match payload {
            ValueTy::Con(name, args) if name == "Array" && args.len() == 2 => {
                let index = args[0].clone();
                let elem = concrete_or_double(args[1].clone());
                ValueTy::Con("Array".into(), vec![index, elem])
            }
            other => concrete_or_double(other),
        };
        Ok((dst, frozen))
    }

    /// Lower one STArray primitive (`newArrayFlat` … `getElems`): the operand
    /// registers go to the effect op and the result is wrapped in the `ST s a`
    /// record, exactly like the `STRef` family.
    ///
    /// The payload type comes from the operands: `newArrayFlat` derives the
    /// handle's `i` from its bounds operand and `e` from the initial element,
    /// every other operation reads both off the handle operand.
    fn lower_starray_effect(
        &mut self,
        name: &str,
        op: ValueEffectOp,
        call_args: &[Expr],
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        // `newListArray` pads a short list with the shared hole fragment, so
        // lowering must emit it (the same flag as `arrayFromAssocs` and
        // `arrayFromListChecked`).
        if op == ValueEffectOp::NewArrayFromListFlat {
            self.uses_array_hole = true;
        }
        let arity = starray_op_arity(op);
        if call_args.len() != arity {
            return Err(CompileError::Type {
                msg: format!(
                    "`{name}` expects {arity} argument(s), got {}",
                    call_args.len()
                ),
                span,
            });
        }
        let mut regs = Vec::with_capacity(arity);
        let mut arg_tys = Vec::with_capacity(arity);
        for a in call_args {
            let (r, ty) = self.lower_value(a)?;
            regs.push(r);
            arg_tys.push(ty);
        }
        let allocates = matches!(
            op,
            ValueEffectOp::NewArrayFlat | ValueEffectOp::NewArrayFromListFlat
        );
        let (index, elem) = if allocates {
            (
                pair_index_ty(arg_tys.first()),
                concrete_or_double(arg_tys.get(2).cloned().unwrap_or(ValueTy::Double)),
            )
        } else {
            starray_handle_types(arg_tys.first())
        };
        let payload = if allocates {
            ValueTy::Con("STArray".into(), vec![ValueTy::Void, index.clone(), elem])
        } else if matches!(op, ValueEffectOp::ReadArrayFlat) {
            elem
        } else if matches!(op, ValueEffectOp::GetArrayBounds) {
            ValueTy::Con("Pair".into(), vec![index.clone(), index])
        } else if matches!(op, ValueEffectOp::GetArrayElems) {
            ValueTy::Con("List".into(), vec![elem])
        } else {
            // `writeArrayFlat` / `modifyArrayFlat`.
            ValueTy::Void
        };
        let effect = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueEffectOp {
            dst: effect,
            op,
            args: regs,
        });
        Ok(self.finish_st_effect(effect, payload))
    }

    /// Lower `error msg` — Haskell 2010 Prelude §9.1's `error :: String -> a`.
    ///
    /// The result is BOTTOM of whatever type the context wants, so the static
    /// result is the call site's expectation when there is a concrete one
    /// (a `Ratio a` field, a `Double`) and the runtime's default numeric value
    /// type otherwise; `error` never RETURNS, so no consumer can observe a
    /// guess here. It lowers to a fault instruction: the message operand is read
    /// and latched as the run's clean fault, and no value is produced.
    ///
    /// Also lowers [`VALUE_FAULT_PRIMITIVE`](crate::ir::VALUE_FAULT_PRIMITIVE),
    /// the prelude-internal spelling of the same instruction whose NAME cannot
    /// be shadowed; `name` selects the diagnostic text only.
    ///
    /// Kept out of the `Apply` arm so `lower_value`'s stack frame does not grow
    /// (a large inline branch has pushed a nested-array program over the stack
    /// limit before).
    fn lower_error_apply(
        &mut self,
        name: &str,
        call_args: &[Expr],
        span: &Span,
        expected: Option<&ValueTy>,
    ) -> Result<(usize, ValueTy), CompileError> {
        if call_args.len() != 1 {
            return Err(CompileError::Type {
                msg: format!("`{name}` expects 1 argument, got {}", call_args.len()),
                span: *span,
            });
        }
        let (src, ty) = self.lower_value(&call_args[0])?;
        if ty != ValueTy::String {
            return Err(CompileError::Type {
                msg: format!("`{name}` expects a String message, got {ty:?}"),
                span: call_args[0].span(),
            });
        }
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueFault { dst, src });
        let ret = match expected {
            Some(t) if is_concrete_ty(t) => t.clone(),
            _ => ValueTy::Double,
        };
        Ok((dst, ret))
    }

    /// Lower one of the PRELUDE's internal INTEGER kernels: `__ratio_percent
    /// n d` (`RatioPercent`) and `__gcd a b` (`Gcd`).
    ///
    /// Both are reached by [`crate::ast::Kernel`], the node the PARSER binds —
    /// not by a name resolved in this pass. That is what makes the language's
    /// own behaviour un-interceptable: `Ratio`'s instance bodies are inlined
    /// BY NAME, so a program's own `%` must not be able to answer `a / b`, and a
    /// program's own `__gcd` must not be able to answer the Prelude's `gcd`
    /// definition. MEASURED before the ordering was fixed: a user
    /// `__ratio_percent` answered `a / b` with `7/1` (the same defect class
    /// Task 3 closed for `error`).
    ///
    /// A reserved SPELLING compared ahead of the definition-site arms (the
    /// previous technique) was NOT sufficient: `reduce` runs between `infer`
    /// and this pass, and a user `__gcd` carrying a SIGNATURE was β-reduced
    /// into the Prelude's `gcd` body before either comparison could see it —
    /// MEASURED: `__gcd :: Integer -> Integer -> Integer; __gcd a b = 99;` then
    /// `gcd 12 18` answered `99` instead of `6`. Binding the operation to a
    /// node removes the carrier, so no pass has a name to substitute.
    ///
    /// Both kernels are `Integer`-only: `gcd` because `Integral` does not exist
    /// (design A6 / OD-24), `__ratio_percent` because it is built on `gcd`. A
    /// non-`Integer` operand is a LOUD rejection naming that, never a silent
    /// computation on the `f64` track.
    ///
    /// Kept out of the `Apply` arm so `lower_value`'s stack frame does not grow
    /// — that frame is at its ceiling (Task 3 recorded the same cost for
    /// `error`, whose arm this mirrors).
    fn lower_integer_kernel(
        &mut self,
        kernel: crate::ast::Kernel,
        call_args: &[Expr],
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        let name = kernel.spelling();
        let is_ratio = kernel == crate::ast::Kernel::RatioPercent;
        if call_args.len() != 2 {
            return Err(CompileError::Type {
                msg: format!("`{name}` expects 2 argument(s), got {}", call_args.len()),
                span,
            });
        }
        // Each operand is lowered with `Integer` EXPECTED, so an integer
        // LITERAL becomes an `Integer` constant rather than the `Int` a
        // context-free literal defaults to (Haskell 2010 §6.4.1's
        // `fromInteger`-polymorphism at this position). Inference already
        // pinned these to `Integer`; without the expectation lowering would
        // emit an `Int` payload for a typed-`Integer` slot — MEASURED: `3 % 6`
        // failed with "expects Integer argument 1, got Int".
        let int_exp = ValueTy::Integer;
        let (a, a_ty) = self.lower_value_expected(&call_args[0], Some(&int_exp))?;
        let (b, b_ty) = self.lower_value_expected(&call_args[1], Some(&int_exp))?;
        for (i, (t, e)) in [(a_ty, &call_args[0]), (b_ty, &call_args[1])]
            .into_iter()
            .enumerate()
        {
            if !matches!(t, ValueTy::Integer | ValueTy::Var(_)) {
                return Err(CompileError::Type {
                    msg: format!(
                        "`{name}` expects Integer argument {}, got {t:?} (no `Integral` class \
                         exists, so this kernel is Integer-only — OD-24)",
                        i + 1
                    ),
                    span: e.span(),
                });
            }
        }
        let dst = self.fresh_value_reg();
        let (op, ret) = if is_ratio {
            (
                ValueBuiltinOp::RatioPercent,
                ValueTy::Con("Ratio".into(), vec![ValueTy::Integer]),
            )
        } else {
            (ValueBuiltinOp::Gcd, ValueTy::Integer)
        };
        self.emit_value(ValueInstr::ValueCallBuiltin {
            dst,
            op,
            args: vec![a, b],
        });
        Ok((dst, ret))
    }

    /// Lower `e` as a LAZY argument: in complete mode, wrap it in a
    /// [`ValueInstr::ValueLazy`] thunk (a fragment computing `e` capturing its
    /// free variables), so the value is NOT forced at the call site — it is
    /// forced on demand by the callee's strict ops. This is the plan's
    /// "application arguments are wrapped in thunks" rule; without it,
    /// `zipWith … (tail fibs)` would force `tail fibs` eagerly while `fibs` is
    /// still Forcing, yielding bottom. In strict mode there are no thunks, so
    /// `e` lowers eagerly as usual.
    fn lower_lazy_arg(&mut self, e: &Expr) -> Result<usize, CompileError> {
        self.lower_lazy_arg_expected(e, None)
    }

    /// [`Self::lower_lazy_arg`] with an optional expected value type, threaded
    /// into the argument's own lowering so a result-directed/constructor-class
    /// method inside it can select its instance by the callee's parameter type
    /// (`apply (arr f) 3.0` with `apply k x = k.unKleisli x`: `arr f` is
    /// lowered with `k`'s parameter type `Kleisli m a b` expected).
    fn lower_lazy_arg_expected(
        &mut self,
        e: &Expr,
        expected: Option<&ValueTy>,
    ) -> Result<usize, CompileError> {
        if self.mode == Mode::Strict {
            let (r, _) = self.lower_value_expected(e, expected)?;
            return Ok(r);
        }
        // A lazy argument that is ALREADY a value (a lambda literal → a
        // closure, a constant, or a reference to a shared-cell def → its thunk
        // in the cell) needs no wrapping: wrapping it would spawn a fragment
        // whose lowered body re-enters the shared-cell def's compile, recursing
        // forever. Only a COMPOUND expression (`tail fibs`) becomes a thunk.
        match e {
            // A bare reference is already lazy: to a shared-cell def it is its
            // thunk/closure in the cell; to anything else it is a capture or an
            // inlined constant — never a computation that must be suspended.
            Expr::Ref(..) => {
                let (r, _) = self.lower_value_expected(e, expected)?;
                return Ok(r);
            }
            Expr::Lambda { .. }
            | Expr::FractionalLit(_, _, _)
            | Expr::IntegerLit(_, _)
            | Expr::Str(_, _) => {
                let (r, _) = self.lower_value_expected(e, expected)?;
                return Ok(r);
            }
            // An annotation around an already-lazy form is equally lazy: the
            // type is erased, so `\x -> x :: T` and `x :: T` must take the same
            // no-thunk path as their inner expression, not spawn a fragment
            // that suspends a value.
            Expr::Annot { expr, .. } => match expr.as_ref() {
                Expr::Ref(..)
                | Expr::Lambda { .. }
                | Expr::FractionalLit(_, _, _)
                | Expr::IntegerLit(_, _)
                | Expr::Str(_, _) => {
                    let (r, _) = self.lower_value_expected(e, expected)?;
                    return Ok(r);
                }
                _ => {}
            },
            _ => {}
        }
        let free = self.free_vars(e, &HashSet::new());
        let (fragment_id, _) = self.lower_fragment(
            &[],
            e,
            &free,
            &self.capture_tys(&free),
            &[],
            expected,
            e.span(),
        )?;
        let (env_reg, _) = self.emit_env_snapshot(&free, e.span(), None)?;
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueLazy {
            dst,
            env: env_reg,
            fragment: fragment_id,
        });
        Ok(dst)
    }

    /// Resolve an INLINE `tape_loop <capacity>` call to a tape index, always
    /// appending a fresh cell — each constructor call is a distinct tape. To
    /// share a tape between heads, bind it to a name (`tape = tape_loop
    /// <capacity>`) and reference the name; named tapes dedupe by name (see
    /// [`Self::resolve_named_tape`]).
    fn resolve_tape(&mut self, capacity: usize) -> usize {
        self.tapes.push(capacity);
        self.tapes.len() - 1
    }

    /// Resolve a NAMED tape declaration to a tape index, creating one cell per
    /// name on first use. All `Ref(name)` uses share that cell, so a write head
    /// and its read heads reference the same buffer.
    fn resolve_named_tape(&mut self, name: &str, capacity: usize) -> usize {
        if let Some(&i) = self.named_tapes.get(name) {
            return i;
        }
        let i = self.tapes.len();
        self.tapes.push(capacity);
        self.named_tapes.insert(name.to_string(), i);
        i
    }

    fn new_value_block(&mut self) -> usize {
        self.value_blocks.push(ValueBlock::default());
        self.value_blocks.len() - 1
    }

    fn set_value_term(&mut self, block: usize, term: ValueTerm) {
        self.value_blocks[block].term = term;
    }

    /// Record a container-typed subexpression (`List`/`Map`/`Set`) for the
    /// arena-capacity heuristic: the container pins its element slots while it
    /// lives, so the bound must include its full subtree size.
    fn note_container(&mut self, vty: &ValueTy) {
        if matches!(
            vty,
            ValueTy::Con(name, _) if matches!(name.as_str(), "List" | "Map" | "Set" | "Array")
        ) {
            self.container_tys.push(vty.clone());
        }
    }

    /// Worst-case element estimate for open collections. Exact counts are
    /// data-dependent (a `concat_map` result length is unknown statically), so
    /// this is a conservative per-op estimate; the runtime safety net is pool
    /// exhaustion (default mode) or pool growth (`growable-arena`).
    const ELEM_EST: usize = 16;
    /// Element estimate for an immutable `Array`: larger than [`Self::ELEM_EST`]
    /// because `arrayFromBuffer` copies a whole signal block (the default `BUF`
    /// of the public compile entry points) into per-element arena slots.
    const ARRAY_ELEM_EST: usize = 256;
    /// Multiplier on the estimated slot/buffer budgets for the default RT mode
    /// so legitimate open-collection programs do not trip the exhaustion error.
    /// Tuned against the collection stress tests (Task 8).
    const POOL_SAFETY_MULTIPLIER: usize = 4;

    /// Lower a value expression to a value register, returning its static value
    /// type. Signal expressions lower to block registers through [`Self::lower`];
    /// value expressions (record/field/match/ctor/literal-in-value-position)
    /// lower here and the two tracks never mix.
    ///
    /// The returned type is the static `ValueTy` of the expression — inference
    /// keeps value channels monomorphic, so this is exact. It is threaded
    /// through because field indices (records) and constructor indices (sums)
    /// depend on the concrete data type of the value.
    fn lower_value(&mut self, e: &Expr) -> Result<(usize, ValueTy), CompileError> {
        self.lower_value_expected(e, None)
    }

    /// [`Self::lower_value`] with an optional expected value type, used for
    /// result-directed typeclass dispatch (`mempty`, `pure`, `return`).
    fn lower_value_expected(
        &mut self,
        e: &Expr,
        expected: Option<&ValueTy>,
    ) -> Result<(usize, ValueTy), CompileError> {
        match e {
            // `fromInteger (n :: Integer)` (Haskell 2010 §6.4.1). The literal's
            // EXACT `Integer` payload is emitted first (never a truncated
            // `i64`), then `ValueFromInteger` converts it to the instance's
            // target. `expected` is the type inference gave the literal — the
            // answer stages 1–3 settled — so no representation is guessed here.
            Expr::IntegerLit(v, sp) => self
                .lower_integer_literal(v, self.literal_instance(*sp).or_else(|| expected.cloned())),
            // `fromRational (r :: Rational)`, the fractional companion.
            Expr::FractionalLit(v, _, sp) => self.lower_fractional_literal(
                v,
                self.literal_instance(*sp).or_else(|| expected.cloned()),
            ),
            Expr::Ref(name, span) => self.lower_value_ref_expected(name, *span, expected),
            // `e :: T` erases: lowering the inner expression with the same
            // expected type AND returning its own type means the annotation
            // contributes no instruction and no register — it is not a runtime
            // no-op node either, just absent from the IR. Returning the INNER
            // type (not the annotation's) keeps every downstream static check
            // reading the value the way it actually lowers: an `Int` literal
            // stays `ValueTy::Int` in an `Ix` offset position even when it is
            // annotated `:: Int`.
            //
            // The ONE exception is a numeric literal under a NUMERIC-LEAF
            // annotation (`Int`, `Integer`, `Double`, or an alias of one):
            // there the annotation IS the literal's expected type, so
            // `(2 :: Integer)` lowers as an `Integer` constant and
            // `(123456789012345678901234567890 :: Double)` as the correctly
            // rounded `Double`. The literal may be under unary minus (`-3 ::
            // Integer`), which is the same constant shape — the shared
            // [`Expr::is_int_literal`] decides, so `Expr::Neg` counts here
            // exactly as it does in inference. The annotation is still erased
            // (no instruction, no register); every non-numeric annotation keeps
            // passing the caller's `expected` through untouched.
            //
            // Honouring only `Integer` made a `:: Double` annotation INERT:
            // `x = (9223372036854775808 :: Double)` lowered the literal on the
            // `Int` track and printed `-9.223372036854776e18`.
            Expr::Annot { expr, ty, .. } => {
                match numeric_leaf_of(ty, self.env).filter(|_| expr.is_int_literal()) {
                    Some(leaf) => self.lower_value_expected(expr, Some(&leaf)),
                    None => self.lower_value_expected(expr, expected),
                }
            }
            Expr::FieldProject {
                record,
                field,
                span,
            } => {
                // A result-directed typeclass call in record position
                // (`(arr f).unKleisli`) selects its instance by the record
                // type, which only the FIELD name identifies (`unKleisli` →
                // `Kleisli m a b`). When the field names exactly one record
                // type, thread it as the expected type so the record's own
                // lowering resolves (a projection on a concrete expression is
                // unaffected — the expected is only a hint).
                let rec_candidates: Vec<String> = self
                    .env
                    .data_types
                    .iter()
                    .filter_map(|(n, info)| match info {
                        DataInfo::Record(fields) if fields.iter().any(|(f, _, _)| f == field) => {
                            Some(n.clone())
                        }
                        _ => None,
                    })
                    .collect();
                let rec_expected: Option<ValueTy> = if rec_candidates.len() == 1 {
                    // `type_ctor_arity` is the ONE arity answer: it covers a
                    // builtin record (`Pair a b` in `ctor_kinds`) and a declared
                    // one alike, so the record's type args are fresh vars either
                    // way.
                    let arity = self.env.type_ctor_arity(&rec_candidates[0]).unwrap_or(0);
                    let args = (0..arity)
                        .map(|_| {
                            let v = self.next_tyvar;
                            self.next_tyvar += 1;
                            ValueTy::Var(v)
                        })
                        .collect::<Vec<_>>();
                    Some(ValueTy::Con(rec_candidates[0].clone(), args))
                } else {
                    None
                };
                // Bind the record's type args from the projection's OWN expected
                // result type when possible: `(compose (arr f) (arr g)).unKleisli`
                // applied in a `Just`-matched scrutinee is expected
                // `Double -> Maybe Double`, so substituting that expected against
                // the record's field type `a -> m b` pins `m := Maybe`,
                // `a := Double` BEFORE the record (a method chain) is lowered —
                // the concrete monad then resolves `return`/`bind` inside the
                // inlined instance bodies. Mismatch/unknown → keep the fresh
                // args (the record's own lowering stays as before).
                let rec_expected = match (&rec_expected, expected) {
                    (Some(ValueTy::Con(rname, rargs)), Some(exp)) => {
                        let fty = match self.env.data_types.get(rname) {
                            Some(DataInfo::Record(fields)) => fields
                                .iter()
                                .find(|(f, _, _)| f == field)
                                .map(|(_, t, _)| t.clone()),
                            _ => None,
                        };
                        let bound = fty.and_then(|ft| {
                            let mut bindings: Vec<(u32, ValueTy)> = Vec::new();
                            let pat = crate::types::infer::substitute_field_params(&ft, rargs);
                            if match_field_expected(&pat, exp, &mut bindings) {
                                Some(
                                    rargs
                                        .iter()
                                        .map(|a| match a {
                                            ValueTy::Var(v) => bindings
                                                .iter()
                                                .find(|(b, _)| b == v)
                                                .map(|(_, t)| t.clone())
                                                .unwrap_or_else(|| a.clone()),
                                            _ => a.clone(),
                                        })
                                        .collect::<Vec<_>>(),
                                )
                            } else {
                                None
                            }
                        });
                        match bound {
                            Some(args) => Some(ValueTy::Con(rname.clone(), args)),
                            None => rec_expected,
                        }
                    }
                    _ => rec_expected,
                };
                let (rec_reg, rec_vty) = match &rec_expected {
                    Some(e) => self.lower_value_expected(record, Some(e))?,
                    None => self.lower_value(record)?,
                };
                let rec_name = match &rec_vty {
                    // Every constructor application is the one `Con` shape: a
                    // builtin record (`Pair a b`) names its type the same way a
                    // declared one does.
                    ValueTy::Con(n, _) => n.clone(),
                    // An unresolved record (a lambda parameter typed
                    // structurally, e.g. `p` of the swap lambda `fn p -> Pair {
                    // first: p.second, … }`): the field name uniquely identifies
                    // the record type (`first`/`second` → `Pair`) — use the
                    // threaded candidate.
                    ValueTy::Var(_) => match &rec_expected {
                        Some(ValueTy::Con(n, _)) => n.clone(),
                        _ => {
                            return Err(CompileError::Type {
                                msg: "field projection requires a record value".into(),
                                span: *span,
                            });
                        }
                    },
                    _ => {
                        return Err(CompileError::Type {
                            msg: "field projection requires a record value".into(),
                            span: *span,
                        });
                    }
                };
                let fields = match self.env.data_types.get(&rec_name) {
                    Some(DataInfo::Record(fields)) => fields.clone(),
                    _ => {
                        return Err(CompileError::Type {
                            msg: format!("`{rec_name}` is not a record type"),
                            span: *span,
                        });
                    }
                };
                let field_index =
                    fields
                        .iter()
                        .position(|(f, _, _)| f == field)
                        .ok_or_else(|| CompileError::Type {
                            msg: format!("no field `{field}` in `{rec_name}`"),
                            span: *span,
                        })?;
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueProject {
                    dst,
                    slot: rec_reg,
                    field: field_index,
                });
                // The projected field's static type: the env field type with the
                // record's type args substituted (a parameterized record), so a
                // downstream method call (`bind (k1.unKleisli x)`) sees the
                // concrete monad, not placeholder ids.
                let field_ty = &fields[field_index].1;
                let rec_args: Option<Vec<ValueTy>> = match &rec_vty {
                    ValueTy::Con(_, args) if !args.is_empty() => Some(args.clone()),
                    ValueTy::Var(_) => match &rec_expected {
                        Some(ValueTy::Con(_, args)) => Some(args.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                let ret_ty = match rec_args {
                    Some(args) => crate::types::infer::substitute_field_params(field_ty, &args),
                    None => field_ty.clone(),
                };
                Ok((dst, ret_ty))
            }
            Expr::FieldUpdate {
                record,
                field,
                value,
                span,
            } => {
                let (rec_reg, rec_vty) = self.lower_value(record)?;
                let rec_name = match &rec_vty {
                    // Every constructor application is the one `Con` shape: a
                    // builtin record (`Pair a b`) names its type the same way a
                    // declared one does.
                    ValueTy::Con(n, _) => n.clone(),
                    _ => {
                        return Err(CompileError::Type {
                            msg: "field update requires a data record".into(),
                            span: *span,
                        });
                    }
                };
                let fields = match self.env.data_types.get(&rec_name) {
                    Some(DataInfo::Record(fields)) => fields.clone(),
                    _ => {
                        return Err(CompileError::Type {
                            msg: format!("`{rec_name}` is not a record type"),
                            span: *span,
                        });
                    }
                };
                let field_index =
                    fields
                        .iter()
                        .position(|(f, _, _)| f == field)
                        .ok_or_else(|| CompileError::Type {
                            msg: format!("no field `{field}` in `{rec_name}`"),
                            span: *span,
                        })?;
                let (src, _) = self.lower_value(value)?;
                self.emit_value(ValueInstr::ValueUpdateField {
                    slot: rec_reg,
                    field: field_index,
                    src,
                });
                Ok((rec_reg, rec_vty))
            }
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => {
                // Lower the scrutinee with the arm-derived sum type as expected
                // when the arms pin it by constructor (`Just v` / `Nothing` →
                // `Maybe`). A result-directed method chain inside the scrutinee
                // (`(compose (arr f) (arr g)).unKleisli 3.0`) then resolves its
                // monad from that concrete pin type before the method bodies are
                // lowered. A scrutinee whose own static type already pins the sum
                // is unaffected — the expected is only a hint.
                let pre_sum: Option<String> = if arms
                    .iter()
                    .any(|a| matches!(a.pattern.strip_bang(), Pattern::Ctor(_, _)))
                {
                    self.resolve_match_sum(arms, *span).ok()
                } else {
                    None
                };
                let scrutinee_expected: Option<ValueTy> = match &pre_sum {
                    Some(n) if self.env.type_ctor_arity(n).is_some() => {
                        let v = self.next_tyvar;
                        self.next_tyvar += 1;
                        Some(ValueTy::Con(n.clone(), vec![ValueTy::Var(v)]))
                    }
                    Some(n) => Some(ValueTy::Con(n.clone(), vec![])),
                    None => None,
                };
                let (scrutinee_reg, scrutinee_vty) = match &scrutinee_expected {
                    Some(se) => self.lower_value_expected(scrutinee, Some(se))?,
                    None => self.lower_value(scrutinee)?,
                };
                // The sum type: a concrete scrutinee type pins it; otherwise the
                // arm constructors determine it (mirroring inference's
                // intersection of candidate sum types). A scalar match (only
                // literal/wildcard/variable arms) has no sum.
                let sum_name = match &scrutinee_vty {
                    ValueTy::Con(n, _) => Some(n.clone()),
                    _ if arms
                        .iter()
                        .any(|a| matches!(a.pattern.strip_bang(), Pattern::Ctor(_, _))) =>
                    {
                        Some(self.resolve_match_sum(arms, *span)?)
                    }
                    _ => None,
                };
                let ctors: Vec<(String, Vec<ValueTy>)> = match &sum_name {
                    Some(n) => match self.env.data_types.get(n) {
                        Some(DataInfo::Sum(ctors)) => ctors
                            .iter()
                            .map(|(c, p)| (c.clone(), p.iter().map(|(t, _)| t.clone()).collect()))
                            .collect(),
                        _ => {
                            return Err(CompileError::Type {
                                msg: format!("`{n}` is not a sum type"),
                                span: *span,
                            });
                        }
                    },
                    None => Vec::new(),
                };
                if arms.is_empty() {
                    return Err(CompileError::Type {
                        msg: "match requires at least one arm".into(),
                        span: *span,
                    });
                }
                // Builtin sums are parameterized (`Maybe a`, `Either a b`): the
                // arm payload shapes carry placeholder `Var(k)` type params that
                // resolve against the scrutinee's concrete type args by position
                // (mirroring inference). Lowering gives a direct builtin-sum
                // construction (`Just x`) the type `Data(name, [])`, so the args
                // are recovered from the constructor application when the static
                // type carries none.
                let is_builtin_sum = sum_name
                    .as_ref()
                    .map(|n| self.env.type_ctor_arity(n).is_some())
                    .unwrap_or(false);
                let scrutinee_args: Vec<ValueTy> = match &scrutinee_vty {
                    ValueTy::Con(_, args) if !args.is_empty() => args.clone(),
                    _ if is_builtin_sum => self.builtin_sum_scrutinee_args(scrutinee),
                    _ => vec![],
                };
                // A guarded arm can fail its guard at runtime, so the static
                // fast path (compile-time ctor selection) is only sound for
                // guard-free matches.
                let has_guards = arms
                    .iter()
                    .any(|a| a.guards.len() > 1 || !matches!(a.guards.first(), Some((None, _))));
                // Static fast path (guard-free, statically-known ctor): lower
                // ONLY the matching arm — preserves exact-capacity behavior.
                if !has_guards {
                    if let Some(cname) = self.static_scrutinee_ctor(scrutinee.as_ref()) {
                        // The static path binds flat `Ctor(_, [Var/Wild ...])`
                        // patterns (plus Var/Wild arms) in one pass; a
                        // nested-pattern arm needs recursive runtime payload
                        // extraction, so it is routed to runtime dispatch.
                        let statically_lowerable =
                            arms.iter().all(|arm| match arm.pattern.strip_bang() {
                                Pattern::Ctor(_, args) => args.iter().all(|a| {
                                    matches!(a.strip_bang(), Pattern::Var(_) | Pattern::Wild)
                                }),
                                Pattern::Var(_) | Pattern::Wild => true,
                                _ => true,
                            });
                        if statically_lowerable {
                            let static_sum = sum_name.clone().unwrap_or_default();
                            return self.lower_static_match_arm(
                                scrutinee_reg,
                                &scrutinee_vty,
                                &static_sum,
                                &ctors,
                                &scrutinee_args,
                                is_builtin_sum,
                                arms,
                                &cname,
                                *span,
                            );
                        }
                    }
                }
                // Runtime dispatch: a sequential test chain per arm ending in a
                // fail block that latches a ProcessError when no arm matched.
                //
                // Matching forces the scrutinee: in complete mode a `do`-bound
                // value (`x <- mx`) is a lazy projection thunk, while the
                // runtime constructor test inspects the register's value
                // directly. Without the force, a `case` on a bound `Maybe` saw
                // no constructor and fell through to the non-exhaustive latch.
                // The static fast path above matched a syntactically known
                // constructor and needs no force.
                let scrutinee_reg = if self.mode == Mode::Complete {
                    let forced = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueForce {
                        dst: forced,
                        src: scrutinee_reg,
                    });
                    forced
                } else {
                    scrutinee_reg
                };
                let scrutinee_block = self.cur_value_block;
                let out = self.fresh_value_reg();
                let join = self.new_value_block();
                let fail = self.new_value_block();
                // fail block: latch the error, then halt the value track.
                self.cur_value_block = fail;
                self.emit_value(ValueInstr::ValueSetError);
                self.set_value_term(fail, ValueTerm::Halt);
                let n = arms.len();
                // All arm-entry blocks are created up front so the `else`
                // target of every arm is known before any body is lowered.
                let entries: Vec<usize> = (0..n).map(|_| self.new_value_block()).collect();
                // The scrutinee's block falls through into the first arm's
                // entry; the last arm's else is the fail block.
                self.set_value_term(scrutinee_block, ValueTerm::Fallthrough(entries[0]));
                let mut out_ty: Option<ValueTy> = None;
                for i in 0..n {
                    let else_t = if i + 1 < n { entries[i + 1] } else { fail };
                    let arm = &arms[i];
                    self.cur_value_block = entries[i];
                    // One scope per arm: pushed before the pattern test so the
                    // arm's `Var` bindings are visible to its guards + body, and
                    // popped after the body. Only one arm runs at runtime, but
                    // all arms are lowered at compile time.
                    self.value_locals.push(HashMap::new());
                    self.value_local_ctors.push(HashMap::new());
                    let pass = self.lower_pattern_test(
                        &arm.pattern,
                        scrutinee_reg,
                        &scrutinee_vty,
                        &scrutinee_args,
                        else_t,
                        arm.span,
                    )?;
                    self.cur_value_block = pass;
                    let arm_ty =
                        self.lower_match_guards_and_body(arm, &out, join, else_t, expected)?;
                    self.value_locals.pop();
                    self.value_local_ctors.pop();
                    if out_ty.is_none() {
                        out_ty = Some(arm_ty);
                    }
                }
                self.cur_value_block = join;
                Ok((out, out_ty.unwrap_or(ValueTy::Double)))
            }
            Expr::If {
                cond,
                then,
                els,
                span,
            } => {
                // Static fast path: a condition written as a constructor
                // REFERENCE (`if True then …`, `if False then …`) selects one
                // branch at compile time (single block, no branch term).
                if let Some(b) = static_bool_ctor(cond.as_ref()) {
                    let (reg, ty) = if b {
                        self.lower_value_expected(then, expected)?
                    } else {
                        self.lower_value_expected(els, expected)?
                    };
                    return Ok((reg, ty));
                }
                let (cond_reg, cond_ty) = self.lower_value(cond)?;
                if cond_ty != ValueTy::bool_ty() {
                    return Err(CompileError::Type {
                        msg: "if condition must be a Bool value".into(),
                        span: *span,
                    });
                }
                // The condition must be forced before the branch reads it: in
                // complete mode a `do`-bound Bool (`b <- atEnd s`) is a lazy
                // thunk, and the block term inspects the register directly.
                let cond_reg = if self.mode == Mode::Complete {
                    let forced = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueForce {
                        dst: forced,
                        src: cond_reg,
                    });
                    forced
                } else {
                    cond_reg
                };
                let then_b = self.new_value_block();
                let els_b = self.new_value_block();
                let join = self.new_value_block();
                let out = self.fresh_value_reg();
                // A boolean condition is an ordinary `Bool` sum value, so the
                // branch is the SAME constructor-tag terminator a `case` uses:
                // `True` is tag 1 (`data Bool = False | True`), so the `then`
                // block runs when the tag matches it.
                self.set_value_term(
                    self.cur_value_block,
                    ValueTerm::BranchCtor {
                        slot: cond_reg,
                        ctor: crate::arena::BOOL_TRUE_TAG,
                        then: then_b,
                        els: els_b,
                    },
                );
                self.cur_value_block = then_b;
                // Both branches are lowered with the `if`'s own expected type:
                // inside a `make*Stream` handler the surrounding `IO ()` pins a
                // result-directed method in an arm (`if c then write … stdout
                // else return ()`), whose instance only the expected type
                // selects.
                let (t_reg, t_ty) = self.lower_value_expected(then, expected)?;
                self.emit_value(ValueInstr::ValueMove {
                    dst: out,
                    src: t_reg,
                });
                self.set_value_term(self.cur_value_block, ValueTerm::Fallthrough(join));
                self.cur_value_block = els_b;
                let (e_reg, e_ty) = self.lower_value_expected(els, expected)?;
                self.emit_value(ValueInstr::ValueMove {
                    dst: out,
                    src: e_reg,
                });
                self.set_value_term(self.cur_value_block, ValueTerm::Fallthrough(join));
                // A branch whose static type lowering never resolved is the
                // RECURSIVE-call shape (`countdown n = … else countdown (n -
                // 1.0)`): the placeholder scheme of a def being lowered carries
                // a bare variable for its own result, so the recursive branch
                // has no type of its own. Inference already unified the two
                // branches, so the branch's type IS the other one's — take it
                // rather than rejecting a program that type-checked.
                let (t_ty, e_ty) = match (&t_ty, &e_ty) {
                    (ValueTy::Var(_), other) => (other.clone(), other.clone()),
                    (other, ValueTy::Var(_)) => (other.clone(), other.clone()),
                    _ => (t_ty, e_ty),
                };
                if t_ty != e_ty {
                    return Err(CompileError::Type {
                        msg: "if branches must have the same type".into(),
                        span: *span,
                    });
                }
                self.cur_value_block = join;
                Ok((out, t_ty))
            }
            // A Prelude-internal kernel, bound at parse time. There is no NAME
            // here, so no definition-site arm can run for it and no pass can
            // substitute a user definition into its place — see
            // [`Expr::Kernel`] for why a reserved SPELLING (the previous
            // technique, compared in `infer` and here) was not enough: `reduce`
            // runs between those two passes and β-reduced a SIGNED user
            // definition of the spelling into the Prelude's own body.
            //
            // Lowered out of line: `lower_value_expected`'s frame is at its
            // ceiling (Task 3 recorded the same cost for `error`).
            Expr::Kernel {
                kernel,
                args: call_args,
                span,
            } => match kernel {
                crate::ast::Kernel::Fault => {
                    self.lower_error_apply(kernel.spelling(), call_args, span, expected)
                }
                crate::ast::Kernel::RatioPercent | crate::ast::Kernel::Gcd => {
                    self.lower_integer_kernel(*kernel, call_args, *span)
                }
            },
            Expr::Apply {
                name,
                args: call_args,
                span,
            } => {
                // A reserved kernel spelling that reached THIS path — only a
                // hand-built AST can (the parser binds the node). Typing and
                // lowering agree by construction because both route through the
                // same `lower_kernel`/`lower_integer_kernel` functions the node
                // arm uses.
                if let Some(kernel) = crate::ast::Kernel::from_reserved_spelling(name) {
                    return match kernel {
                        crate::ast::Kernel::Fault => {
                            self.lower_error_apply(name, call_args, span, expected)
                        }
                        crate::ast::Kernel::RatioPercent | crate::ast::Kernel::Gcd => {
                            self.lower_integer_kernel(kernel, call_args, *span)
                        }
                    };
                }
                // A binding whose whole body is an UNDER-APPLIED method call
                // (`g = fmap (+ 1.0)`) applied to its remaining arguments. The
                // value has no dictionary to carry: a constructor-class method's
                // instance is selected by the class-var-applied CONTAINER, which
                // is the argument the binding has not received yet. Lowering a
                // value binding inlines its body at every use site, so the
                // use-site application IS the specialization point — reduce
                // `g x` to `fmap (+ 1.0) x` and the container is present, the
                // instance selects, and two uses at different containers
                // (`Maybe`, `List`) each get their own instance. That is the
                // same per-call-site specialization constrained definitions use.
                if let Some(out) =
                    self.lower_applied_method_binding_site(name, call_args, expected, *span)
                {
                    return out;
                }
                // Class-constrained definition (`mk :: Ix i => Pair i i -> …`):
                // the call site is the specialization point. See
                // [`Self::lower_constrained_apply_site`]. Kept out of line so
                // this arm's stack frame stays unchanged for the common
                // (non-constrained) case — the typeclass-default expansion
                // below is deep enough to be stack-sensitive.
                if let Some(out) =
                    self.lower_constrained_apply_site(name, call_args, expected, *span)
                {
                    return out;
                }
                // A signature-less curried def (`at a i = a ! i` — the parser
                // drains its parameter list into a λ-nest) is specialized HERE
                // just like a class-constrained def. Its recovered λ-parameter
                // types (`fn_param_tys`) carry type variables lowering cannot
                // substitute, so a body that dispatches on a parameter's shape
                // (`a ! i` → the constrained accessor `(!)`) died in the typeclass
                // dispatch with an unsolved class variable. Binding the
                // parameters to the argument registers' CONCRETE static types
                // lets those method calls select their instances. See
                // [`Self::lower_signature_less_apply_site`].
                if let Some(out) =
                    self.lower_signature_less_apply_site(name, call_args, expected, *span)
                {
                    return out;
                }
                // PARTIAL application of a builtin-signature function. A
                // `builtin_sigs` entry is an ordinary curried function type
                // (Haskell 2010 §4.3.3), so `w = write (Just "x")` is a
                // function value — the lowering of each such builtin below is
                // written for its FULL argument list. Rewriting the under-applied
                // call into `\ $pK -> … f supplied… $pK` reuses the closure
                // machinery for arity, partial application and re-application;
                // `$`-prefixed names cannot collide with a source identifier.
                if let Some(sig) = self.env.builtin_sigs.get(name) {
                    let declared = sig.arg_types().len();
                    if !call_args.is_empty() && call_args.len() < declared {
                        return self.lower_under_applied(name, call_args, declared, *span);
                    }
                }
                // `runST (fn s -> body)`: `runST` is a built-in function whose
                // signature `(forall s. ST s a) -> a` carries a type-level-only
                // phantom — inference (the rank-2 application in `infer_apply`)
                // rejected any body whose result mentions `s`, so the phantom
                // cannot escape the region. Lower the lambda BODY directly (the
                // phantom has no runtime representation): a body that is an
                // `ST s a` computation becomes a `RunST` effect instruction; a
                // legacy body that is not `ST`-headed is the erased region
                // result itself.
                if name == "runST" {
                    if call_args.len() != 1 {
                        return Err(CompileError::Type {
                            msg: "runST expects 1 argument".into(),
                            span: *span,
                        });
                    }
                    let Expr::Lambda { body: lam, .. } = &call_args[0] else {
                        return Err(CompileError::Type {
                            msg: "runST expects a parenthesized lambda `fn s -> body`".into(),
                            span: *span,
                        });
                    };
                    let payload = self.runst_payload_tys.get(&(span.start, span.end)).cloned();
                    // Legacy shape (`runST (fn s -> 1.0)`): no ST computation
                    // was inferred, so the lambda body IS the region result —
                    // the phantom has no runtime representation. Downstream
                    // inference already rejected an escaping phantom.
                    let (body_reg, body_ty) = self.lower_value_expected(lam, expected)?;
                    let Some(payload) = payload else {
                        return Ok((body_reg, body_ty));
                    };
                    // Real ST: the region body is an `ST s a` value — a record
                    // whose `unST` field is the region computation. Running it
                    // allocates a FRESH region, applies `unST` to the new state
                    // token, discards the final state, and returns the payload.
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueEffectOp {
                        dst,
                        op: ValueEffectOp::RunST,
                        args: vec![body_reg],
                    });
                    return Ok((dst, concrete_or_double(payload)));
                }
                // `runSTArray (fn s -> body)`: the freezing sibling of `runST`.
                // Its signature `(forall s. ST s (STArray s i e)) -> Array i e`
                // carries the same type-level-only phantom, so lowering drops
                // the lambda and emits the region computation with the
                // `RunSTArray` effect: running it freezes the region's STArray
                // into an immutable `Array i e`.
                if name == "runSTArray" {
                    return self.lower_runst_array(call_args, *span);
                }
                // ST cell primitives (`STD_PRELUDE`): `newSTRef`/`readSTRef`/
                // `writeSTRef`/`modifySTRef` lower to a primitive effect
                // closure wrapped in the `ST s a` record (the `unST` field).
                // `runST` supplies the region's state token; `Monad (ST s)`'s
                // `bind` threads the successor token between effects.
                if let Some(op) = match name.as_str() {
                    "newSTRef" => Some(ValueEffectOp::NewStRef),
                    "readSTRef" => Some(ValueEffectOp::ReadStRef),
                    "writeSTRef" => Some(ValueEffectOp::WriteStRef),
                    "modifySTRef" => Some(ValueEffectOp::ModifyStRef),
                    _ => None,
                } {
                    let arity = if matches!(op, ValueEffectOp::NewStRef | ValueEffectOp::ReadStRef)
                    {
                        1
                    } else {
                        2
                    };
                    if call_args.len() != arity {
                        return Err(CompileError::Type {
                            msg: format!(
                                "`{name}` expects {arity} argument(s), got {}",
                                call_args.len()
                            ),
                            span: *span,
                        });
                    }
                    // The payload `a` of the resulting `ST s a`: the new cell
                    // for `newSTRef`, the cell's element for `readSTRef`, and
                    // unit for the writes. Unresolved element types fall back
                    // to the runtime's numeric value type.
                    let payload = match op {
                        ValueEffectOp::NewStRef => {
                            let (r, ty) = self.lower_value(&call_args[0])?;
                            let init = self.fresh_value_reg();
                            self.emit_value(ValueInstr::ValueEffectOp {
                                dst: init,
                                op,
                                args: vec![r],
                            });
                            return Ok(self.finish_st_effect(
                                init,
                                ValueTy::Con(
                                    "STRef".into(),
                                    vec![ValueTy::Void, concrete_or_double(ty)],
                                ),
                            ));
                        }
                        ValueEffectOp::ReadStRef => {
                            let (r, ty) = self.lower_value(&call_args[0])?;
                            let elem = match &ty {
                                ValueTy::Con(n, args) if n == "STRef" && args.len() == 2 => {
                                    concrete_or_double(args[1].clone())
                                }
                                _ => ValueTy::Double,
                            };
                            let read = self.fresh_value_reg();
                            self.emit_value(ValueInstr::ValueEffectOp {
                                dst: read,
                                op,
                                args: vec![r],
                            });
                            return Ok(self.finish_st_effect(read, elem));
                        }
                        _ => ValueTy::Void,
                    };
                    let mut regs = Vec::with_capacity(arity);
                    for a in call_args {
                        let (r, _) = self.lower_value(a)?;
                        regs.push(r);
                    }
                    let effect = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueEffectOp {
                        dst: effect,
                        op,
                        args: regs,
                    });
                    return Ok(self.finish_st_effect(effect, payload));
                }
                // STArray primitives (`builtin_sigs`): the flat mutable-array
                // operations mirror the STRef family — each lowers to a
                // primitive effect closure wrapped in the `ST s a` record.
                // Their INDEX math lives in the language (the `Ix` class lands
                // in a later task): `newArrayFlat` takes an already-computed
                // flat size and the offsets are plain `Int`s.
                //
                // These six names are PERMANENTLY RESERVED builtins (as are the
                // `STRef` ones above): a user definition with one of these names
                // is shadowed at every call site, in inference and here alike,
                // so it can never be reached. The reservation is deliberately
                // consistent across both passes — no silent disagreement about
                // which `getBounds` a call means.
                if let Some(op) = match name.as_str() {
                    "newArrayFlat" => Some(ValueEffectOp::NewArrayFlat),
                    "newArrayFromListFlat" => Some(ValueEffectOp::NewArrayFromListFlat),
                    "readArrayFlat" => Some(ValueEffectOp::ReadArrayFlat),
                    "writeArrayFlat" => Some(ValueEffectOp::WriteArrayFlat),
                    "modifyArrayFlat" => Some(ValueEffectOp::ModifyArrayFlat),
                    "getBounds" => Some(ValueEffectOp::GetArrayBounds),
                    "getElems" => Some(ValueEffectOp::GetArrayElems),
                    _ => None,
                } {
                    return self.lower_starray_effect(name, op, call_args, *span);
                }
                // io-streams primitives. Each action lowers to an `IO a`
                // record whose single `unIO` field is a primitive effect
                // closure (`Value::BuiltinCallable`). Applying it to the World
                // token performs the operation and returns `Pair World a`
                // (the interpreter's `exec_stream_effect`): `fromList` allocates
                // a memory input stream, `toList` drains one, `write` pushes (or
                // closes with `Nothing`) into an output stream such as `stdout`.
                if let Some(op) = match name.as_str() {
                    "fromList" => Some(ValueEffectOp::StreamFromList),
                    "toList" => Some(ValueEffectOp::StreamToList),
                    "write" => Some(ValueEffectOp::StreamWrite),
                    _ => None,
                } {
                    let arity = if matches!(op, ValueEffectOp::StreamWrite) {
                        2
                    } else {
                        1
                    };
                    if call_args.len() != arity {
                        return Err(CompileError::Type {
                            msg: format!(
                                "`{name}` expects {arity} argument(s), got {}",
                                call_args.len()
                            ),
                            span: *span,
                        });
                    }
                    let mut arg_regs = Vec::with_capacity(arity);
                    let mut arg_tys = Vec::with_capacity(arity);
                    for a in call_args {
                        let (r, t) = self.lower_value(a)?;
                        arg_regs.push(r);
                        arg_tys.push(t);
                    }
                    // The action's payload type (the `a` in `IO a`). The
                    // element type is read off the argument where it is known;
                    // an unresolved element falls back to the runtime's numeric
                    // value type.
                    let payload = match op {
                        ValueEffectOp::StreamFromList => {
                            let elem = arg_tys
                                .first()
                                .and_then(list_type_args)
                                .and_then(|inner| inner.first().cloned())
                                .unwrap_or(ValueTy::Double);
                            ValueTy::Con("InputStream".into(), vec![elem])
                        }
                        ValueEffectOp::StreamToList => {
                            let elem = match arg_tys.first() {
                                Some(ValueTy::Con(n, a)) if n == "InputStream" && !a.is_empty() => {
                                    a[0].clone()
                                }
                                _ => ValueTy::Double,
                            };
                            ValueTy::Con("List".into(), vec![elem])
                        }
                        _ => ValueTy::Void,
                    };
                    let effect = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueEffectOp {
                        dst: effect,
                        op,
                        args: arg_regs,
                    });
                    let rec = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueConstructRecord {
                        dst: rec,
                        fields: vec![effect],
                    });
                    return Ok((rec, ValueTy::Con("IO".into(), vec![payload])));
                }
                // The remaining io-streams operations (`read`, `atEnd`,
                // `unread`, `connect`, `makeInputStream`, `makeOutputStream`).
                // Same shape as the block above: an `IO a` record whose `unIO`
                // field is a primitive effect closure applied to the `World`
                // token by the interpreter. `makeInputStream`/
                // `makeOutputStream` additionally capture the program's closure
                // operand in that effect closure's env; the handler applies it.
                if let Some(op) = match name.as_str() {
                    "read" => Some(ValueEffectOp::StreamRead),
                    "atEnd" => Some(ValueEffectOp::StreamAtEnd),
                    "unread" => Some(ValueEffectOp::StreamUnread),
                    "connect" => Some(ValueEffectOp::StreamConnect),
                    "makeInputStream" => Some(ValueEffectOp::StreamMakeInput),
                    "makeOutputStream" => Some(ValueEffectOp::StreamMakeOutput),
                    _ => None,
                } {
                    let arity = if matches!(
                        op,
                        ValueEffectOp::StreamUnread | ValueEffectOp::StreamConnect
                    ) {
                        2
                    } else {
                        1
                    };
                    if call_args.len() != arity {
                        return Err(CompileError::Type {
                            msg: format!(
                                "`{name}` expects {arity} argument(s), got {}",
                                call_args.len()
                            ),
                            span: *span,
                        });
                    }
                    // The two `make*Stream` constructors take a program closure.
                    // Lower it against its real signature so a result-directed
                    // method inside it (`return`) selects the `IO` instance, and
                    // link the fresh element variable into the stream payload.
                    if matches!(
                        op,
                        ValueEffectOp::StreamMakeInput | ValueEffectOp::StreamMakeOutput
                    ) {
                        let v = self.next_tyvar;
                        self.next_tyvar += 1;
                        let elem = ValueTy::Var(v);
                        let maybe_elem = ValueTy::Con("Maybe".into(), vec![elem.clone()]);
                        let io_void = ValueTy::Con("IO".into(), vec![ValueTy::Void]);
                        let expected = match op {
                            ValueEffectOp::StreamMakeInput => {
                                ValueTy::Con("IO".into(), vec![maybe_elem])
                            }
                            _ => ValueTy::Thunk(vec![maybe_elem], vec![io_void]),
                        };
                        let (r, _) = self.lower_value_expected(&call_args[0], Some(&expected))?;
                        let effect = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueEffectOp {
                            dst: effect,
                            op,
                            args: vec![r],
                        });
                        let rec = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueConstructRecord {
                            dst: rec,
                            fields: vec![effect],
                        });
                        let name = match op {
                            ValueEffectOp::StreamMakeInput => "InputStream",
                            _ => "OutputStream",
                        };
                        let payload = ValueTy::Con(name.into(), vec![elem]);
                        return Ok((rec, ValueTy::Con("IO".into(), vec![payload])));
                    }
                    let mut arg_regs = Vec::with_capacity(arity);
                    let mut arg_tys = Vec::with_capacity(arity);
                    for a in call_args {
                        let (r, t) = self.lower_value(a)?;
                        arg_regs.push(r);
                        arg_tys.push(t);
                    }
                    // The stream element type when the lowered operand reveals
                    // it; the runtime's numeric value type otherwise.
                    let elem = arg_tys.first().and_then(|ty| match ty {
                        ValueTy::Con(n, args) if n == "InputStream" || n == "OutputStream" => {
                            args.first().cloned()
                        }
                        _ => None,
                    });
                    let payload = match op {
                        ValueEffectOp::StreamRead => {
                            ValueTy::Con("Maybe".into(), vec![elem.unwrap_or(ValueTy::Double)])
                        }
                        ValueEffectOp::StreamAtEnd => ValueTy::bool_ty(),
                        _ => ValueTy::Void,
                    };
                    let effect = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueEffectOp {
                        dst: effect,
                        op,
                        args: arg_regs,
                    });
                    let rec = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueConstructRecord {
                        dst: rec,
                        fields: vec![effect],
                    });
                    return Ok((rec, ValueTy::Con("IO".into(), vec![payload])));
                }
                // `smooth sig ms` — one-pole smoothing over a Buffer.
                if name == "smooth" {
                    if call_args.len() != 2 {
                        return Err(CompileError::Type {
                            msg: format!("smooth expects 2 arguments, got {}", call_args.len()),
                            span: *span,
                        });
                    }
                    let (x, ty) =
                        self.lower_value_expected(&call_args[0], Some(&self.env.buffer_vty()))?;
                    if !matches!(ty, ValueTy::Buffer(_)) {
                        return Err(CompileError::Type {
                            msg: "smooth signal argument must be a Buffer".into(),
                            span: call_args[0].span(),
                        });
                    }
                    let ms = self.caf_const(&call_args[1]).unwrap_or(0.0);
                    let y = self.emit_smooth(x, ms);
                    return Ok((y, self.env.buffer_vty()));
                }
                // Unary math over a Buffer (`sin i1`, `sqrt x`). These remain
                // BY NAME for now: they are `Floating` methods in Haskell 2010
                // (§6.4.4), and this stage adds the `Ord`/`Num`/`Fractional`
                // kernels but no `Floating` class — so there is no
                // `sin :: Floating a => a -> a` to resolve through and the
                // branch is the only surface they have.
                //
                // OPEN DEFECT (recorded, with a reproduction): the same shape
                // this task fixed for `abs`. `sin 1.0` is rejected with "`sin`
                // signal argument must be a Buffer" where GHC returns `0.841…`,
                // because a by-name branch in lowering is a buffer branch in
                // operator lowering (`AGENTS.md` forbids it). The fix is the
                // `Floating` class plus `Floating` kernels whose execution
                // dispatches on the operand, at which point this arm is deleted —
                // exactly what `abs` did here. Owner: the `Floating`-class stage.
                if let Some(op) = match name.as_str() {
                    "sin" => Some(UnOp::Sin),
                    "cos" => Some(UnOp::Cos),
                    "tan" => Some(UnOp::Tan),
                    "sqrt" => Some(UnOp::Sqrt),
                    "exp" => Some(UnOp::Exp),
                    "ln" => Some(UnOp::Ln),
                    "tanh" => Some(UnOp::Tanh),
                    _ => None,
                } {
                    if call_args.len() != 1 {
                        return Err(CompileError::Type {
                            msg: format!("`{name}` expects 1 argument, got {}", call_args.len()),
                            span: *span,
                        });
                    }
                    let (src, ty) =
                        self.lower_value_expected(&call_args[0], Some(&self.env.buffer_vty()))?;
                    if !matches!(ty, ValueTy::Buffer(_)) {
                        return Err(CompileError::Type {
                            msg: format!("`{name}` signal argument must be a Buffer"),
                            span: call_args[0].span(),
                        });
                    }
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueBufferUn { dst, op, src });
                    return Ok((dst, self.env.buffer_vty()));
                }
                // Binary math over buffers (`min i1 0.5`, `atan2 b a`): a
                // Buffer-typed operand makes the op element-wise over buffers.
                if let Some(op) = match name.as_str() {
                    "min" => Some(BinArith::Min),
                    "max" => Some(BinArith::Max),
                    "atan2" => Some(BinArith::Atan2),
                    _ => None,
                } {
                    if call_args.len() != 2 {
                        return Err(CompileError::Type {
                            msg: format!("`{name}` expects 2 arguments, got {}", call_args.len()),
                            span: *span,
                        });
                    }
                    let (a_reg, a_ty) = self.lower_value(&call_args[0])?;
                    let (b_reg, b_ty) = self.lower_value(&call_args[1])?;
                    let buffer_op =
                        matches!(a_ty, ValueTy::Buffer(_)) || matches!(b_ty, ValueTy::Buffer(_));
                    if buffer_op {
                        let a = self.buffer_operand(a_reg, &a_ty)?;
                        let b = self.buffer_operand(b_reg, &b_ty)?;
                        let dst = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueBufferBin { dst, op, a, b });
                        return Ok((dst, self.env.buffer_vty()));
                    }
                    // Scalar `atan2`/`min`/`max` over value-track scalars: not
                    // wired in v1 (the interpreter has no scalar atan2).
                    if name == "atan2" {
                        return Err(CompileError::Type {
                            msg: "`atan2` over scalar values is not supported; use buffers".into(),
                            span: *span,
                        });
                    }
                }
                // Boolean negation: `not b` — a prefix value-track builtin
                // (the `!` token stays the signal wire-cut combinator).
                if name == "not" {
                    if call_args.len() != 1 {
                        return Err(CompileError::Type {
                            msg: format!("`not` expects 1 argument, got {}", call_args.len()),
                            span: *span,
                        });
                    }
                    let (src, ty) = self.lower_value(&call_args[0])?;
                    if ty != ValueTy::bool_ty() {
                        return Err(CompileError::Type {
                            msg: format!("`not` expects a Bool argument, got {ty:?}"),
                            span: call_args[0].span(),
                        });
                    }
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueNot { dst, src });
                    return Ok((dst, ValueTy::bool_ty()));
                }
                // `error msg` — Haskell 2010 Prelude §9.1's `error :: String ->
                // a`. Out of line so this arm does not grow `lower_value`'s
                // stack frame (a large inline branch has pushed a nested-array
                // program over the limit before).
                //
                // The name is an ordinary BINDING, so every enclosing value
                // scope is consulted first, in the same order as
                // `free_vars_impl`/`note_callee_name`: a definition, a
                // λ-parameter (`value_locals`) OR a capture of the fragment we
                // are inside (`fragment_captures`). The capture set is what a
                // nested lambda sees — `f error = \y -> error y` captures the
                // parameter named `error`, and without this check the special
                // form below would answer the body with "`error` expects a
                // String message, got Double" (a valid H2010 program rejected).
                if name == "error"
                    && !self.defs.contains_key(name)
                    && !self.value_locals.iter().any(|s| s.contains_key(name))
                    && !self.fragment_captures.iter().any(|(n, _)| n == name)
                {
                    return self.lower_error_apply(name, call_args, span, expected);
                }
                // Collection operation (`length`, `cons`, `head`, `tail`, `map`,
                // `fold`, `filter`, `list`, `insert`, `lookup`, `member`,
                // `empty_map`, `empty_set`): a reserved name dispatched by
                // `ValueCallBuiltin`. Checked before the record/sum/newtype/
                // method/user-def resolution so a collection op can never be
                // shadowed by a definition of the same name.
                if let Some(op) = self.value_builtin(name, call_args.len()) {
                    let mut arg_regs = Vec::with_capacity(call_args.len());
                    let mut arg_tys = Vec::with_capacity(call_args.len());
                    // `map f xs` / `concat_map f xs`: lower the source list
                    // FIRST so its element type can type the closure's
                    // parameter — `map (fn x -> x * 0.5) (sine 440.0 0.5 0.0)`
                    // over a `List Buffer` keeps `x` a Buffer value and
                    // `x * 0.5` lowers to a Buffer op, not scalar.
                    // `concat_map` needs the same threading: its closure
                    // returns a LIST, and an untyped parameter would make the
                    // elements of that inner list default to `Double` (the 2-D
                    // `Ix` instance builds `Pair Int Int` cells through a
                    // lambda-in-lambda).
                    if (name == "map" || name == "concat_map") && call_args.len() == 2 {
                        let (lr, lt) = self.lower_value(&call_args[1])?;
                        let elem = list_type_args(&lt)
                            .and_then(|inner| inner.first().cloned())
                            .unwrap_or(ValueTy::Double);
                        let fexp = ValueTy::Thunk(vec![elem], vec![]);
                        let (fr, ft) = self.lower_value_expected(&call_args[0], Some(&fexp))?;
                        arg_regs.push(fr);
                        arg_regs.push(lr);
                        arg_tys.push(ft);
                        arg_tys.push(lt);
                    } else if name == "fold"
                        && call_args.len() == 3
                        && matches!(&call_args[0],
                            Expr::Ref(n, _) if self.env.class_of_method(n).is_some())
                    {
                        // Out of line: this arm lives inside the deeply
                        // recursive `lower_value` dispatch, whose stack frame
                        // must not grow (a large inline branch pushed an
                        // existing nested-map `do` program over the limit).
                        let (regs, tys) = self.lower_fold_with_method(call_args)?;
                        arg_regs.extend(regs);
                        arg_tys.extend(tys);
                    } else {
                        // Thread each argument's DECLARED parameter type from
                        // the builtin's `foreign fn` signature as its expected
                        // type. `show_double :: Double -> String` therefore
                        // lowers `show_double 9223372036854775808` as the
                        // `fromInteger` conversion of the literal, instead of
                        // the `Int` truncation `lower_value` would pick.
                        let declared = builtin_param_tys(self.env, name);
                        for (i, a) in call_args.iter().enumerate() {
                            let exp = declared.get(i).filter(|t| is_concrete_ty(t));
                            let (r, t) = match exp {
                                Some(e) => self.lower_value_expected(a, Some(e))?,
                                None => self.lower_value(a)?,
                            };
                            arg_regs.push(r);
                            arg_tys.push(t);
                        }
                    }
                    let ret = self.value_builtin_ty(name, &arg_tys, *span)?;
                    self.note_container(&ret);
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueCallBuiltin {
                        dst,
                        op,
                        args: arg_regs,
                    });
                    return Ok((dst, ret));
                }
                // Record constructor: `Point { x: 1.0 }`.
                if let Some(info) = self.env.data_types.get(name).cloned() {
                    match info {
                        DataInfo::Record(fields) => {
                            if call_args.len() != 1 {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "record constructor `{name}` expects one record literal, got {} arguments",
                                        call_args.len()
                                    ),
                                    span: *span,
                                });
                            }
                            // The single argument may be a record literal matched by field
                            // name or — for a single-field record — the field
                            // VALUE directly, newtype-style: `Box (fn x -> x *
                            // 2.0)` ≡ `Box { f: fn x -> x * 2.0 }`.
                            let fields_expr: Vec<(String, Expr)> = match &call_args[0] {
                                Expr::Record(f, _) => {
                                    f.iter().map(|(n, e)| (n.clone(), e.clone())).collect()
                                }
                                _ if fields.len() == 1 => {
                                    vec![(fields[0].0.clone(), call_args[0].clone())]
                                }
                                _ => {
                                    return Err(CompileError::Type {
                                        msg: format!(
                                            "record constructor `{name}` expects a record literal"
                                        ),
                                        span: *span,
                                    });
                                }
                            };
                            // Lower each field value in declared-field order so the
                            // runtime record matches the type's field layout. The
                            // record's concrete type args (threaded from the outer
                            // expected type) substitute each field type's
                            // placeholder positions; the substituted field type is
                            // then threaded as the field value's EXPECTED type so a
                            // result-directed method call inside the value
                            // (`Kleisli (fn x -> return (f x))` → `return`) resolves
                            // by the concrete monad.
                            let rec_args: Option<Vec<ValueTy>> = match expected {
                                Some(ValueTy::Con(n, a)) if n == name => Some(a.clone()),
                                Some(ValueTy::Con(n, a)) if n == name => Some(a.clone()),
                                _ => None,
                            };
                            let mut field_regs = Vec::with_capacity(fields.len());
                            let mut field_tys: Vec<ValueTy> = Vec::with_capacity(fields.len());
                            for (fname, _, strict) in &fields {
                                let fexpr = fields_expr
                                    .iter()
                                    .find(|(n, _)| n == fname)
                                    .map(|(_, e)| e)
                                    .ok_or_else(|| CompileError::Type {
                                        msg: format!(
                                            "missing field `{fname}` in `{name}` constructor"
                                        ),
                                        span: *span,
                                    })?;
                                let fty = fields
                                    .iter()
                                    .find(|(f, _, _)| f == fname)
                                    .map(|(_, t, _)| t.clone());
                                let field_expected = match (&rec_args, fty) {
                                    (Some(a), Some(ft)) if !a.is_empty() => {
                                        Some(crate::types::infer::substitute_field_params(&ft, a))
                                    }
                                    (Some(_), Some(ft)) => Some(ft),
                                    _ => None,
                                };
                                let (fr, ft) = match field_expected {
                                    Some(fe) => self.lower_value_expected(fexpr, Some(&fe))?,
                                    None => self.lower_value(fexpr)?,
                                };
                                // A strict field (`{ x: !Double }`, Haskell 2010)
                                // forces its argument to WHNF at construction.
                                let fr = if *strict {
                                    let forced = self.fresh_value_reg();
                                    self.emit_value(ValueInstr::ValueForce {
                                        dst: forced,
                                        src: fr,
                                    });
                                    forced
                                } else {
                                    fr
                                };
                                field_regs.push(fr);
                                field_tys.push(ft);
                            }
                            let dst = self.fresh_value_reg();
                            self.emit_value(ValueInstr::ValueConstructRecord {
                                dst,
                                fields: field_regs,
                            });
                            // Propagate the record's concrete type args when the
                            // outer expected carries them, so a projection /
                            // subsequent method call sees the parameterized type
                            // (`k1.unKleisli` of a `Kleisli Maybe Double Double`
                            // parameter is `Double -> Maybe Double`). Without an
                            // expected type, derive them from the fields so a
                            // head-arg-aware dispatch sees `Pair Int Int` rather
                            // than a bare `Pair`.
                            let ret = match rec_args {
                                Some(a) if !a.is_empty() => ValueTy::Con(name.clone(), a),
                                _ => ValueTy::Con(
                                    name.clone(),
                                    record_args_from_fields(&fields, &field_tys),
                                ),
                            };
                            return Ok((dst, ret));
                        }
                        DataInfo::Sum(_) => {
                            return Err(CompileError::Type {
                                msg: format!("`{name}` is a sum type; use one of its constructors"),
                                span: *span,
                            });
                        }
                        DataInfo::Opaque => {
                            return Err(CompileError::Type {
                                msg: format!("opaque type `{name}` has no constructors"),
                                span: *span,
                            });
                        }
                    }
                }
                // Sum constructor: `Circle 1.5`.
                //
                // A constructor is an ordinary CURRIED function (Haskell 2010
                // §4.2.1): `M3 :: Double -> Double -> Three`, so supplying
                // FEWER payload arguments than the constructor takes is a
                // PARTIAL application and a value — `f = M3 1.0`,
                // `map (Mk 1.0) xs`. Lower it exactly the way the bare-constructor
                // case does (see `lower_value_ref`'s sum arm): rewrite into
                // `\ $ctor_pK -> … C supplied… $ctor_pK` and let the closure
                // machinery handle arity, partial application and
                // re-application. More arguments than the payload is the arity
                // error; inference rejects it first, and lowering keeps its own
                // message for the paths that reach it directly.
                if let Some((sum_name, ctor_idx, payload)) = self.sum_ctor(name) {
                    if call_args.len() > payload.len() {
                        return Err(CompileError::Type {
                            msg: format!(
                                "constructor `{name}` expects {} argument(s), got {}",
                                payload.len(),
                                call_args.len()
                            ),
                            span: *span,
                        });
                    }
                    if call_args.len() < payload.len() {
                        return self.lower_under_applied(name, call_args, payload.len(), *span);
                    }
                    let bangs = self.ctor_payload_bangs(&sum_name, ctor_idx);
                    let mut payload_regs = Vec::with_capacity(call_args.len());
                    for (i, a) in call_args.iter().enumerate() {
                        let (pr, _) = self.lower_value(a)?;
                        // A strict payload (`C !Int`, Haskell 2010) forces its
                        // argument to WHNF at construction.
                        let pr = if bangs.get(i).copied().unwrap_or(false) {
                            let forced = self.fresh_value_reg();
                            self.emit_value(ValueInstr::ValueForce {
                                dst: forced,
                                src: pr,
                            });
                            forced
                        } else {
                            pr
                        };
                        payload_regs.push(pr);
                    }
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueConstructSum {
                        dst,
                        ctor: ctor_idx as u32,
                        payload: payload_regs,
                    });
                    return Ok((dst, ValueTy::Con(sum_name, vec![])));
                }
                // Newtype constructor: `Hz 440.0` wraps its single argument.
                if self.env.newtypes.contains_key(name) {
                    return self.lower_newtype_constructor(name, call_args, *span);
                }
                // Typeclass method call: the argument's static type selects the
                // instance, and the method body is inlined with the parameters
                // bound to the argument's registers — β-substitution at compile
                // time, zero runtime dispatch. User definitions shadow class
                // methods (a user `eq`/`lt` is a plain function), so this only
                // fires when no user def of that name exists. Constructor
                // classes (`Functor f`) resolve by the class-var-applied
                // argument's constructor head (`App("List", ..)` → `List`).
                // Typeclass method call: the argument's static type selects the
                // instance, and the method body is inlined with the parameters
                // bound to the argument's registers — β-substitution at compile
                // time, zero runtime dispatch. User definitions shadow class
                // methods (a user `eq`/`lt` is a plain function), so this only
                // fires when no user def of that name exists. Three resolution
                // modes: result-directed (`pure`/`return`/`mempty`), arity-0
                // (`Monoid mappend`, `Show show`), and constructor classes
                // (`Functor f`, by the class-var-applied argument).
                if !self.method_shadowed_by_def(name, call_args) {
                    if let Some(class_name) = self.env.class_of_method(name) {
                        let class_info = self.env.typeclasses.get(&class_name).cloned().unwrap();
                        let class_var = class_info.var.clone();
                        let sig = class_info
                            .methods
                            .iter()
                            .find(|(m, _)| m == name)
                            .map(|(_, s)| s.clone());
                        let sig = sig.as_ref().unwrap();
                        // Argument count: a `TFunc` signature carries its
                        // argument list; a bare class-var signature (`show: a`)
                        // takes exactly one selector argument. A `TForall`
                        // binder delegates to the inner type.
                        let n_sig_args = sig.sig_arg_count();
                        // Result-directed methods (`pure a`, `return x`): a
                        // function signature whose arguments never mention the
                        // class variable — the container comes only from the
                        // expected RESULT type. A bare signature (`show: a`,
                        // `mempty: m`) is NOT result-directed in an apply: it
                        // resolves by the selector argument (arity-0 path).
                        let result_directed = matches!(
                            sig,
                            crate::ast::TypeExpr::TFunc(args, _)
                                if !args.is_empty()
                                    && args
                                        .iter()
                                        .all(|a| !crate::types::infer::type_expr_mentions(a, &class_var))
                        );
                        if result_directed {
                            // `pure x` / `return x`: the container comes from the
                            // expected result type — or, when inference already
                            // recorded the decision for this call, from THAT.
                            // Re-asking for `expected` here made lowering repeat
                            // a judgement inference had made (`f :: IO Int; f =
                            // return 42` infers fine and was rejected with
                            // "expected type unknown").
                            let recorded =
                                self.recorded_selection(*span, &class_name)
                                    .map(|(key, subst)| match subst {
                                        crate::types::infer::ClassVarSubst::Ty(t) => t,
                                        _ => self.env.vty_of_name(&key).unwrap_or(ValueTy::Double),
                                    });
                            let exp = match expected.cloned().or(recorded) {
                                Some(t) => t,
                                None => {
                                    return Err(CompileError::Type {
                                        msg: format!(
                                            "cannot resolve method `{name}` of `{class_name}`: expected type unknown"
                                        ),
                                        span: *span,
                                    });
                                }
                            };
                            let cv_var = self.next_tyvar;
                            self.next_tyvar += 1;
                            let Some((ty_name, cv_subst)) =
                                crate::types::infer::method_dispatch_key(
                                    self.env,
                                    &class_name,
                                    &class_var,
                                    sig,
                                    &exp,
                                    cv_var,
                                    || {
                                        let v = self.next_tyvar;
                                        self.next_tyvar += 1;
                                        ValueTy::Var(v)
                                    },
                                )
                            else {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "cannot resolve method `{name}` of `{class_name}`: the expected type is not concrete"
                                    ),
                                    span: *span,
                                });
                            };
                            // Constraint instance: bind head args from the
                            // expected container's leading args and discharge
                            // the constraints before inlining the body.
                            self.bind_head_args_and_discharge(&class_name, &ty_name, &exp, *span)?;
                            // A primitive (kernel-backed) instance has no
                            // source body: dispatch it to the arithmetic /
                            // comparison kernel for (class, type, method) pHere
                            // instead of inlining equations.
                            if self.is_primitive_instance(&class_name, ty_name.as_str()) {
                                // The instance key solves the signature's
                                // class variable, so each parameter type is
                                // CONCRETE (`Num Integer` → `Integer -> Integer
                                // -> Integer`). Threading it into the argument
                                // lowering is what makes a bare literal in an
                                // `Integer` position lower as an `Integer`
                                // instead of an `Int`: without it the kernel
                                // received `[Integer, Int]` for
                                // `(2 :: Integer) == 2`.
                                //
                                // The caller's EXPECTATION is the other half of
                                // the judgement (§6.4.1): `g 0 + 2.5` makes
                                // `(+)`'s `a` `Double`, so the operand's own
                                // `Num b` is `Double` too.
                                let (ty_name, cv_subst) = self.instance_from_expectation(
                                    &class_name,
                                    expected,
                                    ty_name.as_str(),
                                    cv_subst,
                                );
                                let param_tys = crate::types::infer::signature_param_tys_conv(
                                    self.env,
                                    &class_var,
                                    &cv_subst,
                                    sig,
                                    || {
                                        let v = self.next_tyvar;
                                        self.next_tyvar += 1;
                                        ValueTy::Var(v)
                                    },
                                );
                                return self.lower_primitive_method(
                                    &class_name,
                                    name,
                                    ty_name.as_str(),
                                    call_args,
                                    *span,
                                    None,
                                    &param_tys,
                                );
                            }
                            let (params, body) = match self.instantiate_method(
                                &class_name,
                                name,
                                ty_name.as_str(),
                            ) {
                                Some(r) => r,
                                None => {
                                    return Err(CompileError::Type {
                                            msg: format!(
                                                "no instance of `{class_name}` for type `{ty_name}` and no default for `{name}`"
                                            ),
                                            span: *span,
                                        });
                                }
                            };
                            let key = (class_name, ty_name.clone(), name.to_string());
                            if self.method_lifting.contains(&key) {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "recursive typeclass method `{name}` for type `{ty_name}`"
                                    ),
                                    span: *span,
                                });
                            }
                            self.method_lifting.insert(key.clone());
                            let mut scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
                            let mut ctor_scope: HashMap<String, Option<String>> = HashMap::new();
                            // Bind every param to its lowered argument register
                            // (`pure x`/`return x` have one element arg). Lower
                            // each arg with its signature param type as expected
                            // so a lambda argument (`arr (fn p -> …)`) types its
                            // parameters structurally (`p` of the swap lambda
                            // stays a record, not Double).
                            let arg_param_tys = crate::types::infer::signature_param_tys_conv(
                                self.env,
                                &class_var,
                                &cv_subst,
                                sig,
                                || {
                                    let v = self.next_tyvar;
                                    self.next_tyvar += 1;
                                    ValueTy::Var(v)
                                },
                            );
                            // The signature's fresh variables are solved as the
                            // arguments arrive: `bind m f`'s second parameter is
                            // `a -> m b`, and `a` is only known from `m`'s
                            // payload. Without this the inlined body's lambda
                            // parameter stayed an unresolved variable, so a
                            // method call on it (`index b ix` where `b` is the
                            // bounds payload) could not dispatch and fell back
                            // to the definition.
                            let mut arg_var_bindings: HashMap<crate::types::TypeVarId, ValueTy> =
                                HashMap::new();
                            for (i, (p, a)) in params.iter().zip(call_args.iter()).enumerate() {
                                let exp = arg_param_tys
                                    .get(i)
                                    .map(|t| subst_value_vars(t, &arg_var_bindings));
                                let (r, t) = match exp {
                                    Some(e) => self.lower_value_expected(a, Some(&e))?,
                                    None => self.lower_value(a)?,
                                };
                                if let Some(pat) = arg_param_tys.get(i) {
                                    bind_pattern_vars(pat, &t, &mut arg_var_bindings);
                                }
                                scope.insert(p.clone(), (r, t));
                                ctor_scope.insert(p.clone(), self.static_scrutinee_ctor(a));
                            }
                            self.value_locals.push(scope);
                            self.value_local_ctors.push(ctor_scope);
                            let res = self.lower_value_expected(&body, Some(&exp));
                            self.value_locals.pop();
                            self.value_local_ctors.pop();
                            self.method_lifting.remove(&key);
                            return res;
                        }
                        if class_info.arity == 0 {
                            if call_args.len() != n_sig_args {
                                // A PRIMITIVE method is curried: a partial
                                // application yields the remaining function.
                                // Build `\ $pN -> (+) supplied… $pN`.
                                if call_args.len() < n_sig_args {
                                    let mut lambdas: Vec<String> = Vec::new();
                                    for i in call_args.len()..n_sig_args {
                                        lambdas.push(format!("$p{i}"));
                                    }
                                    // append the remaining params in order
                                    let mut args = call_args.to_vec();
                                    for l in &lambdas {
                                        args.push(Expr::Ref(l.clone(), *span));
                                    }
                                    let mut expr = Expr::Apply {
                                        name: name.to_string(),
                                        args,
                                        span: *span,
                                    };
                                    for l in lambdas.iter().rev() {
                                        expr = Expr::Lambda {
                                            params: vec![Param {
                                                name: l.clone(),
                                                span: *span,
                                                bang: false,
                                            }],
                                            body: Box::new(expr),
                                            span: *span,
                                        };
                                    }
                                    return self.lower_value(&expr);
                                }
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "method `{name}` of `{class_name}` expects {n_sig_args} argument(s), got {}",
                                        call_args.len()
                                    ),
                                    span: *span,
                                });
                            }
                            // Selector: the argument whose signature type mentions
                            // the class variable AND which is not a bare nullary
                            // method ref — both criteria must agree, because the
                            // same argument's concrete type solves the class
                            // variable (`pick: Bool -> Pair a a -> Double` selects
                            // on the `Pair`, `mappend mempty xs` on `xs`).
                            let Some(selector_idx) = crate::types::infer::class_var_selector_index(
                                sig,
                                &class_var,
                                call_args.len(),
                                |i| self.is_bare_nullary_method_ref(&call_args[i], &class_name),
                            ) else {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "cannot resolve method `{name}` of `{class_name}`: no argument can select the instance (every class-var argument is a nullary method reference)"
                                    ),
                                    span: *span,
                                });
                            };
                            let (arg_reg, arg_vty) = self.lower_value(&call_args[selector_idx])?;
                            // The instance the type checker selected at this call
                            // site, when it recorded one. It is authoritative:
                            // lowering's own static types are coarser than
                            // inference's substitution, and re-deriving the
                            // instance from them would reject a program that
                            // already type-checked.
                            let dispatch = self.recorded_selection(*span, &class_name);
                            let dispatch = match dispatch {
                                Some(d) => Some(d),
                                None => {
                                    // STAGE 3's default, mirrored (Haskell 2010
                                    // §4.3.4 + §4.5.5): an operand whose static
                                    // type lowering never resolved is the first
                                    // `default (Integer, Double)` candidate that
                                    // has the instance — `Integer` before
                                    // `Double`.
                                    //
                                    // Inference already decided this. Lowering
                                    // only arrives here when it has no recorded
                                    // selection (a position inference never
                                    // reached, or a polymorphic scheme), and
                                    // re-deriving the SAME first-satisfying
                                    // candidate is what keeps the two passes
                                    // agreeing. `Num`/`Fractional`/`Ord`/`Eq` are
                                    // all standard classes, so all of them default
                                    // (`\a -> \b -> a <= b` applied at two
                                    // literals is GHC's `Ord Integer`).
                                    let arg_dispatch_vty = if matches!(arg_vty, ValueTy::Var(_)) {
                                        ["Integer", "Double"]
                                            .into_iter()
                                            .filter_map(|n| self.env.vty_of_name(n))
                                            .find(|t| self.env.instance_exists(&class_name, t))
                                            .unwrap_or(ValueTy::Double)
                                    } else {
                                        arg_vty.clone()
                                    };
                                    let cv_var = self.next_tyvar;
                                    self.next_tyvar += 1;
                                    crate::types::infer::method_dispatch_key(
                                        self.env,
                                        &class_name,
                                        &class_var,
                                        sig,
                                        &arg_dispatch_vty,
                                        cv_var,
                                        || {
                                            let v = self.next_tyvar;
                                            self.next_tyvar += 1;
                                            ValueTy::Var(v)
                                        },
                                    )
                                }
                            };
                            let Some((ty_name, cv_subst)) = dispatch else {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "cannot resolve method `{name}` of `{class_name}`: the argument type is not concrete"
                                    ),
                                    span: call_args[selector_idx].span(),
                                });
                            };
                            // Primitive instance: dispatch to the kernel,
                            // reusing the selector register the dispatch above
                            // already emitted. The solved instance key makes
                            // every parameter type concrete, so a bare literal
                            // in an `Integer` position lowers as an `Integer`
                            // (see the result-directed path above).
                            if self.is_primitive_instance(&class_name, ty_name.as_str()) {
                                let (ty_name, cv_subst) = self.instance_from_expectation(
                                    &class_name,
                                    expected,
                                    ty_name.as_str(),
                                    cv_subst,
                                );
                                let param_tys = crate::types::infer::signature_param_tys_conv(
                                    self.env,
                                    &class_var,
                                    &cv_subst,
                                    sig,
                                    || {
                                        let v = self.next_tyvar;
                                        self.next_tyvar += 1;
                                        ValueTy::Var(v)
                                    },
                                );
                                return self.lower_primitive_method(
                                    &class_name,
                                    name,
                                    ty_name.as_str(),
                                    call_args,
                                    *span,
                                    Some((selector_idx, arg_reg, arg_vty)),
                                    &param_tys,
                                );
                            }
                            let (params, body) = match self.instantiate_method(
                                &class_name,
                                name,
                                ty_name.as_str(),
                            ) {
                                Some(r) => r,
                                None => {
                                    return Err(CompileError::Type {
                                            msg: format!(
                                                "no instance of `{class_name}` for type `{ty_name}` and no default for `{name}`"
                                            ),
                                            span: *span,
                                        });
                                }
                            };
                            let key = (class_name.clone(), ty_name.clone(), name.to_string());
                            if self.method_lifting.contains(&key) {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "recursive typeclass method `{name}` for type `{ty_name}`"
                                    ),
                                    span: *span,
                                });
                            }
                            self.method_lifting.insert(key.clone());
                            let (param_tys, res_ty) =
                                crate::types::infer::signature_param_and_result_tys_conv(
                                    self.env,
                                    &class_var,
                                    &cv_subst,
                                    sig,
                                    || {
                                        let v = self.next_tyvar;
                                        self.next_tyvar += 1;
                                        ValueTy::Var(v)
                                    },
                                );
                            let mut scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
                            let mut ctor_scope: HashMap<String, Option<String>> = HashMap::new();
                            for (i, p) in params.iter().enumerate() {
                                let (r, t) = if i == selector_idx {
                                    (arg_reg, arg_vty.clone())
                                } else if !self
                                    .is_bare_nullary_method_ref(&call_args[i], &class_name)
                                {
                                    self.lower_value(&call_args[i])?
                                } else {
                                    // Nullary arg (`mempty`): inline ITS
                                    // instance body with the signature param
                                    // type as expected so `list` stays open.
                                    let arg_name = match &call_args[i] {
                                        Expr::Ref(n, _) => n.as_str(),
                                        _ => unreachable!("nullary arg must be a Ref"),
                                    };
                                    // Instantiating a nullary method inlines ITS
                                    // body here, so its parameters must be
                                    // renamed too — the same chokepoint as every
                                    // other method-body path.
                                    let (_, nbody) = self
                                        .instantiate_method(&class_name, arg_name, ty_name.as_str())
                                        .unwrap();
                                    let exp = param_tys.get(i).cloned().unwrap_or(ValueTy::Double);
                                    self.lower_value_expected(&nbody, Some(&exp))?
                                };
                                scope.insert(p.clone(), (r, t));
                                ctor_scope
                                    .insert(p.clone(), self.static_scrutinee_ctor(&call_args[i]));
                            }
                            self.value_locals.push(scope);
                            self.value_local_ctors.push(ctor_scope);
                            // The inlined body is lowered against the METHOD's
                            // own result type: a result-directed form inside it
                            // (`error "…"`, which is bottom of any type) only
                            // resolves through that expectation, exactly as it
                            // would in the method's own signature.
                            let body_expected = expected.cloned().or(res_ty);
                            let res = match body_expected.as_ref() {
                                Some(t) => self.lower_value_expected(&body, Some(t)),
                                None => self.lower_value(&body),
                            };
                            self.value_locals.pop();
                            self.value_local_ctors.pop();
                            self.method_lifting.remove(&key);
                            return res;
                        }
                        // Constructor class: the class-var-applied argument's
                        // concrete type head selects the instance. Lower the
                        // container argument first (to find the constructor),
                        // then the rest with the signature's param type as
                        // expected so `bind mx (fn x -> return x)` resolves
                        // `return` by the monad.
                        let container_idx = self
                            .env
                            .class_var_arg_index(sig, &class_var)
                            .ok_or_else(|| CompileError::Type {
                            msg: format!(
                                "method `{name}` of `{class_name}` has no class-var-applied argument"
                            ),
                            span: *span,
                        })?;
                        if call_args.len() > n_sig_args {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "method `{name}` of `{class_name}` expects {n_sig_args} argument(s), got {}",
                                    call_args.len()
                                ),
                                span: *span,
                            });
                        }
                        if call_args.len() < n_sig_args {
                            // PARTIAL application of a constructor-class method
                            // is a value (Haskell 2010 §4.3.3) and inference
                            // already typed it as the curried remainder: the
                            // instance cannot be selected yet because the
                            // class-var-applied container is the missing
                            // argument. `fmap (+ 1.0)` is therefore the closure
                            // `\ $pK -> fmap supplied… $pK`, exactly the rewrite
                            // the primitive-method path uses.
                            return self.lower_under_applied(name, call_args, n_sig_args, *span);
                        }
                        // Phase 1: lower the container argument and resolve the
                        // constructor. The container arg may itself be a
                        // result-directed method call (`arr f` in
                        // `compose (arr f) (arr g)`) that needs an expected
                        // type to resolve. When the enclosing expression
                        // expects a concrete container (`Data("Kleisli", [..])`
                        // threaded from a field projection / match), derive the
                        // container arg's expected type from it: the instance
                        // head's args (`m`) from the expected's leading args,
                        // the remaining class-var slots freshened.
                        let container_expected: Option<ValueTy> = match &expected {
                            Some(ValueTy::Con(c, args)) => {
                                let inst = self
                                    .env
                                    .instances
                                    .get(&class_name)
                                    .and_then(|by_ty| by_ty.get(c));
                                match inst {
                                    Some(i) => {
                                        let head_len = i.head_args.len();
                                        let mut cargs: Vec<ValueTy> =
                                            args.iter().take(head_len).cloned().collect();
                                        while cargs.len() < head_len + class_info.arity {
                                            let v = self.next_tyvar;
                                            self.next_tyvar += 1;
                                            cargs.push(ValueTy::Var(v));
                                        }
                                        Some(match expected {
                                            Some(ValueTy::Con(..)) => {
                                                ValueTy::Con(c.clone(), cargs)
                                            }
                                            _ => ValueTy::Con(c.clone(), cargs),
                                        })
                                    }
                                    None => None,
                                }
                            }
                            _ => None,
                        };
                        let (container_reg, container_vty) = match container_expected {
                            Some(ce) => {
                                self.lower_value_expected(&call_args[container_idx], Some(&ce))?
                            }
                            None => self.lower_value(&call_args[container_idx])?,
                        };
                        // Constraint instance: bind head args from the
                        // container's LEADING args and discharge the
                        // constraints before resolving the constructor.
                        if let ValueTy::Con(c, _) = &container_vty {
                            self.bind_head_args_and_discharge(
                                &class_name,
                                c,
                                &container_vty,
                                *span,
                            )?;
                        }
                        let ctor = match &container_vty {
                            ValueTy::Con(c, _) => c.clone(),
                            _ => {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "cannot resolve method `{name}` of `{class_name}`: the argument type is not a constructor application"
                                    ),
                                    span: call_args[container_idx].span(),
                                });
                            }
                        };
                        let (params, body) = match self.instantiate_method(
                            &class_name,
                            name,
                            ctor.as_str(),
                        ) {
                            Some(r) => r,
                            None => {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "no instance of `{class_name}` for constructor `{ctor}` and no default for `{name}`"
                                    ),
                                    span: *span,
                                });
                            }
                        };
                        if params.len() != call_args.len() {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "method `{name}` of `{class_name}` expects {} argument(s), got {}",
                                    params.len(),
                                    call_args.len()
                                ),
                                span: *span,
                            });
                        }
                        // Phase 2: lower the remaining args with their signature
                        // param type as expected, then bind all params.
                        let mut param_tys = crate::types::infer::signature_param_tys_conv(
                            self.env,
                            &class_var,
                            &crate::types::infer::ClassVarSubst::Ctor(ctor.clone()),
                            sig,
                            || {
                                let v = self.next_tyvar;
                                self.next_tyvar += 1;
                                ValueTy::Var(v)
                            },
                        );
                        // A partial instance head (`Kleisli m` → head arg `m`):
                        // the signature-converted container type drops the head
                        // (`a b c` → `Data("Kleisli", [b, c])`). Prepend the head
                        // args bound at the call site (`m := Maybe`) so the arg
                        // expected types carry the full concrete container — a
                        // result-directed second arg (`arr g`) resolves `return`
                        // by the monad.
                        crate::types::infer::prepend_instance_head_args(
                            self.env,
                            &class_name,
                            &mut param_tys,
                            &ctor,
                            &self.head_arg_bindings,
                            || {
                                let v = self.next_tyvar;
                                self.next_tyvar += 1;
                                ValueTy::Var(v)
                            },
                        );
                        // The signature's fresh variables are solved as the
                        // arguments arrive. `bind m f`'s second parameter is
                        // `a -> m b`, and `a` is ONLY known from `m`'s payload:
                        // without binding it the inlined instance body's
                        // continuation parameter stayed an unresolved variable,
                        // so a call on it inside the callback (`index b ix`
                        // where `b` is the bounds payload) could not dispatch by
                        // type and fell back to the constrained definition.
                        let mut arg_var_bindings: HashMap<crate::types::TypeVarId, ValueTy> =
                            HashMap::new();
                        if let Some(pat) = param_tys.get(container_idx) {
                            bind_pattern_vars(pat, &container_vty, &mut arg_var_bindings);
                        }
                        let mut arg_regs = Vec::with_capacity(call_args.len());
                        let mut arg_vtys = Vec::with_capacity(call_args.len());
                        for (i, a) in call_args.iter().enumerate() {
                            if i == container_idx {
                                arg_regs.push(container_reg);
                                arg_vtys.push(container_vty.clone());
                            } else {
                                let exp = param_tys
                                    .get(i)
                                    .map(|t| subst_value_vars(t, &arg_var_bindings))
                                    .unwrap_or(ValueTy::Double);
                                let (r, t) = self.lower_value_expected(a, Some(&exp))?;
                                if let Some(pat) = param_tys.get(i) {
                                    bind_pattern_vars(pat, &t, &mut arg_var_bindings);
                                }
                                arg_regs.push(r);
                                arg_vtys.push(t);
                            }
                        }
                        let mut scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
                        let mut ctor_scope: HashMap<String, Option<String>> = HashMap::new();
                        for (p, (r, t)) in params.iter().zip(arg_regs.into_iter().zip(arg_vtys)) {
                            scope.insert(p.clone(), (r, t));
                        }
                        for (p, a) in params.iter().zip(call_args) {
                            ctor_scope.insert(p.clone(), self.static_scrutinee_ctor(a));
                        }
                        // The recursion guard is armed ONLY around the inlined
                        // BODY — not while the arguments are lowered. Arguments
                        // may legally inline other methods' defaults that
                        // reference this method (the method-lifting composition:
                        // `both`'s default `compose (first f) (second g)` reaches
                        // `compose` again through `second`'s default body while
                        // the outer `compose`'s SECOND argument is lowered).
                        // That is not recursion — `compose`'s own body never
                        // calls `compose`. Genuine recursion (a body that calls
                        // its own method name) is still caught: the key is held
                        // while the body lowers.
                        let key = (class_name.clone(), ctor.clone(), name.to_string());
                        if self.method_lifting.contains(&key) {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "recursive typeclass method `{name}` for type `{ctor}`"
                                ),
                                span: *span,
                            });
                        }
                        self.method_lifting.insert(key.clone());
                        self.value_locals.push(scope);
                        self.value_local_ctors.push(ctor_scope);
                        // Lower the inlined body with the method's expected
                        // result type so a result-directed call inside a DEFAULT
                        // body (`second`'s `arr swap`) resolves by the concrete
                        // container.
                        let res = match expected {
                            Some(exp) => self.lower_value_expected(&body, Some(exp)),
                            None => self.lower_value(&body),
                        };
                        self.value_locals.pop();
                        self.value_local_ctors.pop();
                        self.method_lifting.remove(&key);
                        return res;
                    }
                }
                // Foreign (FFI) application: `onepole 200.0 0.7 i1` — an ordinary
                // `Buffer -> … -> Buffer` arrow. Buffer args lower through the
                // value track (ValueReadInput / buffer arithmetic) into `srcs`;
                // scalar params are folded into the builtin (currying): a
                // constant → `BuiltinInstance.params`, a `?name`/param ref →
                // `param_bindings` (pushed via `set_param` before `process`).
                // The `Algorithm` stays a pure function over its baked params.
                if let Some(fsig) = self.env.foreign_sigs.get(name.as_str()).cloned() {
                    if let Some(sig) =
                        crate::types::ffi::ffi_sig_from_typeexpr(self.env, name, &fsig)
                    {
                        let mut param_values = Vec::new();
                        let mut param_bindings = Vec::new();
                        let mut signal_srcs = Vec::new();
                        let mut pos = 0usize;
                        // Signal slots with no explicit call arg: a first-class
                        // partial application (`onepole 200.0 0.7` → the scalars
                        // fold at build, the Buffer stays open). The resulting
                        // FFI arrow is a `Func` value applied on call.
                        let mut missing_signals = 0usize;
                        for p in &sig.params {
                            match p {
                                crate::types::ffi::FfiParam::Signal(_) => {
                                    if pos >= call_args.len() {
                                        // Partial application: the scalar params
                                        // before this slot were consumed; the
                                        // Buffer stays open for a later call.
                                        missing_signals += 1;
                                        pos += 1;
                                        continue;
                                    }
                                    let (r, ty) = self.lower_value_expected(
                                        &call_args[pos],
                                        Some(&self.env.buffer_vty()),
                                    )?;
                                    if !matches!(ty, ValueTy::Buffer(_)) {
                                        return Err(CompileError::Type {
                                            msg: format!(
                                                "signal argument {pos} of `{name}` must be a Buffer"
                                            ),
                                            span: call_args[pos].span(),
                                        });
                                    }
                                    signal_srcs.push(r);
                                    pos += 1;
                                }
                                crate::types::ffi::FfiParam::Scalar => {
                                    if pos >= call_args.len() {
                                        // Missing scalar before a trailing
                                        // signal slot — a partial application
                                        // gap; the arity check above rejected
                                        // this shape, so it is unreachable.
                                        break;
                                    }
                                    // An actor param / named
                                    // param ref is a LIVE value: record a
                                    // param binding (set_param per tick). A
                                    // constant folds into the builtin's params.
                                    if let Expr::Ref(ref_name, _) = &call_args[pos] {
                                        if let Some(&pidx) = self.param_names.get(ref_name) {
                                            param_values.push(0.0);
                                            param_bindings
                                                .push((param_values.len() - 1, pidx));
                                            pos += 1;
                                            continue;
                                        }
                                    }
                                    if let Expr::ActorParam {
                                        name: aname,
                                        default,
                                        span: aspan,
                                    } = &call_args[pos]
                                    {
                                        let default_val = if let Some(d) = default {
                                            const_f64(d).unwrap_or(0.0)
                                        } else {
                                            0.0
                                        };
                                        let idx = self.intern_param(
                                            aname.clone(),
                                            default_val,
                                            f64::NEG_INFINITY,
                                            f64::INFINITY,
                                            *aspan,
                                        )?;
                                        param_values.push(0.0);
                                        param_bindings.push((param_values.len() - 1, idx));
                                        pos += 1;
                                        continue;
                                    }
                                    let v = self.caf_const(&call_args[pos]).ok_or_else(|| {
                                        CompileError::Type {
                                            msg: format!(
                                                "param at position {pos} of `{name}` must be a constant or parameter reference"
                                            ),
                                            span: call_args[pos].span(),
                                        }
                                    })?;
                                    param_values.push(v);
                                    pos += 1;
                                }
                                crate::types::ffi::FfiParam::Record(rec_ty) => {
                                    if pos >= call_args.len() {
                                        break;
                                    }
                                    // Flatten the record literal's schema
                                    // fields into `param_values` (the factory
                                    // reads its config from the flat list),
                                    // mirroring the signal path.
                                    self.flatten_record_params(
                                        rec_ty,
                                        &call_args[pos],
                                        name,
                                        &mut param_values,
                                    )?;
                                    pos += 1;
                                }
                                _ => {
                                    return Err(CompileError::Type {
                                        msg: format!(
                                            "foreign `{name}` with variadic/record params is not yet supported in value position"
                                        ),
                                        span: *span,
                                    })
                                }
                            }
                        }
                        let instance = self.builtins.len();
                        self.builtins.push(BuiltinInstance {
                            name: name.clone(),
                            params: param_values,
                            resource: None,
                            tape_index: None,
                            kind: crate::builtin::BuiltinKind::Block,
                            signal_ins: signal_srcs.len() + missing_signals,
                            signal_outs: sig.signal_outs,
                            param_bindings,
                        });
                        // Partial application: the trailing Buffer slots are
                        // open (`onepole 200.0 0.7 : Buffer -> Buffer`). The
                        // scalars are baked into the instance; the FFI arrow is
                        // returned as a first-class `Func` value — its fragment
                        // reads the missing Buffer(s) from the call frame and
                        // runs the single `ValueCallForeign` on call.
                        if missing_signals > 0 {
                            let (callee_reg, _) =
                                self.build_ffi_arrow(instance, missing_signals, sig.signal_outs)?;
                            return Ok((
                                callee_reg,
                                ValueTy::Thunk(
                                    vec![self.env.buffer_vty(); missing_signals],
                                    vec![self.env.buffer_vty(); sig.signal_outs],
                                ),
                            ));
                        }
                        // Build the FFI arrow as a Callable fragment: its body
                        // extracts the Buffer args from the call frame and runs
                        // one `ValueCallForeign`. Applying it (ValueCallFunc)
                        // dispatches through run_call like any user function, so
                        // the arrow is a first-class value (HOF works for FFI).
                        let (callee_reg, call_reg) =
                            self.build_ffi_arrow(instance, signal_srcs.len(), sig.signal_outs)?;
                        // Apply the arrow to the Buffer args.
                        if sig.signal_outs == 1 {
                            self.emit_call(call_reg, callee_reg, signal_srcs);
                            return Ok((call_reg, self.env.buffer_vty()));
                        }
                        // Multi-out: build a tuple value (Buffer, …) from the
                        // consecutive output registers after the call result.
                        self.emit_call(call_reg, callee_reg, signal_srcs);
                        let dst = self.fresh_value_reg();
                        let fields: Vec<usize> =
                            (0..sig.signal_outs).map(|i| call_reg + i).collect();
                        self.emit_value(ValueInstr::ValueConstructRecord { dst, fields });
                        return Ok((dst, self.env.buffer_vty()));
                    }
                }
                // Value-function application: `add2 = adder 2.0` — the callee
                // is a definition whose body lowers to a closure value. Evaluate
                // named λ-parameter definitions are β-reduced by `reduce`
                // before lowering and never reach here.) Applying FEWER args
                // than the signature is partial application: the result is a
                // curry closure that captures the function and the applied
                // args, re-applying them with the remaining args on call.
                let callee_res = self.lower_value_ref(name, *span);
                let (callee_reg, callee_ty) = callee_res?;
                match &callee_ty {
                    ValueTy::Thunk(_, _) => {
                        let (arg_tys, ret_tys) = flatten_value_thunk(&callee_ty);
                        // A def's SCHEME is the declaration a call site needs:
                        // unifying its result with the caller's expected type
                        // and applying the substitution to its parameters is
                        // what makes `a -> a` pin BOTH (`f x = x; g y = f y`),
                        // at any chain length and through a λ indirection.
                        // Only when the callee has no scheme of this arity does
                        // the structural fallback run.
                        let (arg_tys, ret_tys) =
                            match self.scheme_pinned_params(name, arg_tys.len(), expected, *span) {
                                Some(params) => (params, ret_tys),
                                None => pin_ret_head_types(&arg_tys, &ret_tys, expected),
                            };
                        if call_args.len() > arg_tys.len() {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "`{name}` expects {} argument(s), got {}",
                                    arg_tys.len(),
                                    call_args.len()
                                ),
                                span: *span,
                            });
                        }
                        let mut arg_regs = Vec::with_capacity(call_args.len());
                        for (i, a) in call_args.iter().enumerate() {
                            // Thread the callee's parameter type as the
                            // argument's expected type: a result-directed /
                            // constructor-class method inside the argument
                            // (`arr f`, `compose …`) selects its instance by the
                            // container type the parameter demands
                            // (`apply (arr f) 3.0`).
                            let pt = arg_tys.get(i).cloned();
                            let ar = match pt {
                                Some(p) => self.lower_lazy_arg_expected(a, Some(&p))?,
                                None => self.lower_lazy_arg(a)?,
                            };
                            arg_regs.push(ar);
                        }
                        let ret_ty = ret_tys.first().cloned().unwrap_or(ValueTy::Double);
                        // A curried definition (`twice = \f -> \x -> …`, after the
                        // parser's currying pass emptied its parameter list)
                        // lowers to a NEST of single-argument closures, so its
                        // arguments are applied one at a time: `f a b` becomes
                        // `(f a) b`. Stopping after a partial application leaves
                        // the leftover closure value in place — no synthetic
                        // curry record is needed.
                        if is_curried_thunk(&callee_ty) {
                            let mut cur = callee_reg;
                            for ar in &arg_regs {
                                // In complete mode `emit_call` is a LAZY
                                // application: it builds a thunk (a callable
                                // with a memo cell). Applying the next argument
                                // needs the inner closure itself, so force the
                                // intermediate to WHNF first (forcing a closure
                                // is a no-op — only a thunk actually runs).
                                let callee = if self.mode == Mode::Complete {
                                    let forced = self.fresh_value_reg();
                                    self.emit_value(ValueInstr::ValueForce {
                                        dst: forced,
                                        src: cur,
                                    });
                                    forced
                                } else {
                                    cur
                                };
                                let dst = self.fresh_value_reg();
                                self.emit_call(dst, callee, vec![*ar]);
                                cur = dst;
                            }
                            if call_args.len() < arg_tys.len() {
                                let remaining = &arg_tys[call_args.len()..];
                                let leftover = nest_value_thunk(remaining, vec![ret_ty]);
                                return Ok((cur, leftover));
                            }
                            return Ok((cur, ret_ty));
                        }
                        if call_args.len() < arg_tys.len() {
                            // Partial application → a curry closure. Its env captures
                            // `[func, arg0, arg1, ...]` by value; on call it reads
                            // them out and dispatches `ValueCallFunc` with
                            // `(captured..., remaining...)`.
                            let remaining = arg_tys.len() - call_args.len();
                            let fragment_id =
                                self.apply_partial(arg_regs.len(), remaining, *span)?;
                            let env_reg = self.fresh_value_reg();
                            let mut fields = Vec::with_capacity(1 + arg_regs.len());
                            fields.push(callee_reg);
                            fields.extend(arg_regs.iter().copied());
                            self.emit_value(ValueInstr::ValueConstructRecord {
                                dst: env_reg,
                                fields,
                            });
                            let dst = self.fresh_value_reg();
                            self.emit_value(ValueInstr::ValueMakeClosure {
                                dst,
                                env: env_reg,
                                fragment: fragment_id,
                            });
                            let remaining_arg_tys: Vec<ValueTy> =
                                arg_tys[call_args.len()..].to_vec();
                            return Ok((dst, ValueTy::Thunk(remaining_arg_tys, vec![ret_ty])));
                        }
                        let dst = self.fresh_value_reg();
                        self.emit_call(dst, callee_reg, arg_regs);
                        Ok((dst, ret_ty))
                    }
                    _ => {
                        // A higher-order function parameter (a `Func` whose
                        // static signature inference kept structural, or an
                        // unresolved parameter) applied at runtime: inference
                        // already validated the arity against the real `Func`
                        // type, so this is a full-application dispatch. v1
                        // function results are Double scalars.
                        let mut arg_regs = Vec::with_capacity(call_args.len());
                        for a in call_args {
                            let (ar, _) = self.lower_value(a)?;
                            arg_regs.push(ar);
                        }
                        let dst = self.fresh_value_reg();
                        self.emit_call(dst, callee_reg, arg_regs);
                        Ok((dst, ValueTy::Double))
                    }
                }
            }
            Expr::ApplyExpr { callee, args, span } => {
                // Re-associate a name-based curried chain into a single Apply so
                // the name-dispatched arms below (collection ops, constructors,
                // FFI, user defs, IO primitives) see the full argument list.
                if let Some((name, mut all_args)) = uncurry_apply_chain(callee) {
                    all_args.extend(args.iter().cloned());
                    let apply = Expr::Apply {
                        name,
                        args: all_args,
                        span: *span,
                    };
                    return self.lower_value_expected(&apply, expected);
                }
                // Thread the call's expected result type into the callee as a
                // `Func` signature so a result-directed method chain in callee
                // position (`(compose …).unKleisli` in a `Just`-matched
                // scrutinee) resolves its monad from the concrete result type
                // before the callee is lowered.
                let callee_expected: Option<ValueTy> = expected.map(|exp| {
                    let fresh_args: Vec<ValueTy> = (0..args.len())
                        .map(|_| {
                            let v = self.next_tyvar;
                            self.next_tyvar += 1;
                            ValueTy::Var(v)
                        })
                        .collect();
                    ValueTy::Thunk(fresh_args, vec![exp.clone()])
                });
                let (cr, cty) = match &callee_expected {
                    Some(ce) => self.lower_value_expected(callee, Some(ce))?,
                    None => self.lower_value(callee)?,
                };
                let (arg_tys, ret_tys) = flatten_value_thunk(&cty);
                // Inference enforces exact arity (infer_apply_expr); this guard is defensive/unreachable via compilation.
                if args.len() > arg_tys.len() {
                    return Err(CompileError::Type {
                        msg: format!(
                            "expression expects {} argument(s), got {}",
                            arg_tys.len(),
                            args.len()
                        ),
                        span: *span,
                    });
                }
                let mut arg_regs = Vec::with_capacity(args.len());
                for a in args {
                    let (ar, _) = self.lower_value(a)?;
                    arg_regs.push(ar);
                }
                let ret_ty = ret_tys.first().cloned().unwrap_or(ValueTy::Double);
                // A closure value is a curried NEST of single-argument thunks
                // (`\a -> \b -> …`), so the arguments are applied one at a time:
                // `f a b` becomes `(f a) b`. A partial application leaves the
                // leftover closure value in place.
                if is_curried_thunk(&cty) {
                    let mut cur = cr;
                    for ar in &arg_regs {
                        let callee = if self.mode == Mode::Complete {
                            let forced = self.fresh_value_reg();
                            self.emit_value(ValueInstr::ValueForce {
                                dst: forced,
                                src: cur,
                            });
                            forced
                        } else {
                            cur
                        };
                        let dst = self.fresh_value_reg();
                        self.emit_call(dst, callee, vec![*ar]);
                        cur = dst;
                    }
                    if args.len() < arg_tys.len() {
                        return Ok((cur, nest_value_thunk(&arg_tys[args.len()..], vec![ret_ty])));
                    }
                    return Ok((cur, ret_ty));
                }
                let dst = self.fresh_value_reg();
                self.emit_call(dst, cr, arg_regs);
                Ok((dst, ret_ty))
            }
            Expr::Neg(inner, _span) => {
                // Negative literal / unary minus in value position. A numeric
                // literal folds to a negative constant; anything else lowers as
                // `zero - inner`.
                //
                // The result keeps the OPERAND's track: an integer literal
                // negates to a `Value::Int` (an `Expr::IntegerLit` is Int-track
                // unconditionally, so `-3` must be too) unless the expected
                // type is `Integer`, in which case it folds to a
                // `ValueConstInteger` — `-3 :: Integer` must be a typed-`Integer`
                // slot holding an `Integer`, the same invariant the literal
                // path keeps. A fractional literal to a `Value::Double`, and a
                // non-literal follows its lowered type. Without this, `-1` was
                // the one place a statically-`Int` slot held a `Value::Double` —
                // and an `Ix` offset computed from a negative lower bound then
                // failed the interpreter's "index is not an Int" check.
                if let Some(v) = const_bigint(inner) {
                    // A negated literal is ONE literal (Haskell 2010 §3.8), so it
                    // is the same `fromInteger` application its positive form is —
                    // with the sign already applied to the EXACT payload. Negating
                    // in arbitrary precision FIRST is what makes
                    // `f (-9223372036854775809)` the negated double of the
                    // literal's value rather than the negation of its low 64 bits.
                    let target = expected
                        .cloned()
                        .or_else(|| self.literal_instance(inner.span()))
                        .unwrap_or(ValueTy::Integer);
                    let exact = self.emitter_integer(&-v);
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueFromInteger {
                        dst,
                        src: exact,
                        target: self.conversion_tag(&target),
                    });
                    return Ok((dst, target));
                }
                // A negated FRACTIONAL literal is `fromRational` of the exact
                // negated ratio, for exactly the same reason.
                if let Expr::FractionalLit(r, _text, sp) = &**inner {
                    let neg = std::ops::Neg::neg(&**r);
                    return self.lower_fractional_literal(
                        &neg,
                        expected.cloned().or_else(|| self.literal_instance(*sp)),
                    );
                }
                match const_f64(inner) {
                    Some(v) => {
                        let dst = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueConstDouble { dst, value: -v });
                        Ok((dst, ValueTy::Double))
                    }
                    None => {
                        let (ir, inner_ty) = self.lower_value(inner)?;
                        let zero = self.fresh_value_reg();
                        match inner_ty {
                            ValueTy::Int => self.emit_value(ValueInstr::ValueConstInt {
                                dst: zero,
                                value: 0,
                            }),
                            ValueTy::Integer => self.emit_value(ValueInstr::ValueConstInteger {
                                dst: zero,
                                value: Box::new(0.into()),
                            }),
                            _ => self.emit_value(ValueInstr::ValueConstDouble {
                                dst: zero,
                                value: 0.0,
                            }),
                        }
                        let dst = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueSub {
                            dst,
                            a: zero,
                            b: ir,
                        });
                        Ok((dst, inner_ty))
                    }
                }
            }
            Expr::Lambda { params, body, span } => {
                // A lambda literal lowers to a FragmentIr (compiled once, shared
                // across every closure it creates) plus a by-value snapshot of
                // its free variables in an env Record. The closure references
                // the env; `ValueCallFunc` dispatches the fragment with the env
                // fields bound as a temporary cell frame.
                let param_names: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
                let free = self.free_vars(body, &param_names);
                // A named lambda-literal definition carries its resolved
                // parameter types from inference (set by `lower_value_ref`); an
                // anonymous lambda defaults to Double parameters — unless its
                // expected type is a `Func` signature, which types the params
                // structurally (the swap lambda `fn p -> Pair { first: p.second,
                // … }` of `arr (fn p -> …)` keeps `p` a `Pair`, not Double).
                // A curried definition's λ-parameter types arrive as one FLAT
                // list (`take = \n -> \xs -> …` → `[Double, List]`), so this
                // lambda consumes its own leading slice and leaves the rest for
                // its body's inner lambda.
                let saved_pending = self.pending_param_tys.take();
                let param_tys: Vec<ValueTy> = match saved_pending.clone() {
                    Some(mut pending) if pending.len() >= params.len() => {
                        let rest = pending.split_off(params.len());
                        if !rest.is_empty() {
                            self.pending_param_tys = Some(rest);
                        }
                        pending
                    }
                    Some(pending) => pending,
                    None => match expected {
                        Some(ValueTy::Thunk(arg_tys, _)) if arg_tys.len() == params.len() => {
                            arg_tys.clone()
                        }
                        _ => vec![ValueTy::Double; params.len()],
                    },
                };
                // The lambda's BODY is expected to produce the lambda's RESULT
                // type (not the whole `Func` type the caller expects) — a
                // result-directed method call inside (`bind mx (fn x -> return
                // x)`) resolves by that type.
                let body_expected = match expected {
                    Some(ValueTy::Thunk(_, rets)) => rets.first().cloned(),
                    _ => None,
                };
                let frag = self.lower_fragment(
                    params,
                    body,
                    &free,
                    &self.capture_tys(&free),
                    &param_tys,
                    body_expected.as_ref(),
                    *span,
                );
                self.pending_param_tys = saved_pending;
                let (fragment_id, ret_ty) = frag?;
                let (env_reg, _self_cell) = self.emit_env_snapshot(&free, *span, None)?;
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueMakeClosure {
                    dst,
                    env: env_reg,
                    fragment: fragment_id,
                });
                Ok((dst, ValueTy::Thunk(param_tys, vec![ret_ty])))
            }
            Expr::Void(_) => {
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstVoid { dst });
                Ok((dst, ValueTy::Void))
            }
            Expr::Str(s, _) => {
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstString {
                    dst,
                    value: s.clone(),
                });
                Ok((dst, ValueTy::String))
            }
            Expr::ListLit(elems, _) => {
                // In complete mode the empty literal `[]` is the lazy Nil.
                if elems.is_empty() && self.mode == Mode::Complete {
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueCallBuiltin {
                        dst,
                        op: ValueBuiltinOp::Nil,
                        args: Vec::new(),
                    });
                    return Ok((dst, ValueTy::Con("List".into(), vec![ValueTy::Double])));
                }
                let mut regs = Vec::with_capacity(elems.len());
                let mut elem_ty = ValueTy::Double;
                for e in elems {
                    let (r, t) = self.lower_value(e)?;
                    regs.push(r);
                    elem_ty = t;
                }
                let _ = regs.len();
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueListLit { dst, elems: regs });
                let ret = ValueTy::Con("List".into(), vec![elem_ty]);
                self.note_container(&ret);
                Ok((dst, ret))
            }
            Expr::MapLit(entries, _) => {
                let mut keys = Vec::with_capacity(entries.len());
                let mut vals = Vec::with_capacity(entries.len());
                // Inference already unified every entry's value type, so the
                // entries share one static value type; it is carried on the
                // result Map (a List/record value is NOT pinned to Double).
                let mut val_ty = ValueTy::Double;
                for (k, v) in entries {
                    let (kr, _) = self.lower_value(&Expr::Str(k.clone(), Span::new(0, 0)))?;
                    let (vr, vt) = self.lower_value(v)?;
                    keys.push(kr);
                    vals.push(vr);
                    val_ty = vt;
                }
                let _ = keys.len();
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueMapLit { dst, keys, vals });
                let ret = ValueTy::Con("Map".into(), vec![ValueTy::String, val_ty]);
                self.note_container(&ret);
                Ok((dst, ret))
            }
            Expr::Let { defs, body, span } => {
                // A bang binding forces its right-hand side AT the binding
                // point, so the RHS must be a value distinct from the binding
                // itself: `let !x = x` has nothing to force (the RHS is the
                // binding), and the thunk fragment cannot reference its own
                // name. Reject it with a bang-specific diagnostic instead of a
                // confusing "unknown `x`" (complete) or "recursive value
                // definition" (strict) error.
                if let Some(d) = defs
                    .iter()
                    .find(|d| d.is_bang() && bang_binding_refers_to_itself(d))
                {
                    return Err(CompileError::Type {
                        msg: format!(
                            "a bang binding cannot be recursive: `!{}` refers to itself",
                            d.name()
                        ),
                        span: *span,
                    });
                }
                if self.mode == Mode::Strict {
                    // Expression-level `let x = e in body` in value position:
                    // the defs are plain inlined bindings (v1 value defs are
                    // per-tick re-evaluated expressions), so lower each into
                    // the def table and lower the body.
                    let saved = self.defs.clone();
                    // A local FUNCTION binding is installed in the def table so
                    // the body's `g 1.0` dispatches to it through the ordinary
                    // definition path; a parameterless binding keeps its classic
                    // inlined form.
                    for d in defs {
                        self.defs.insert(d.name().to_string(), d.clone());
                    }
                    // A bang binding (`let !x = e`) is evaluated here, at the
                    // binding point, and its name reads the forced result.
                    let mut forced_scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
                    for d in defs.iter().filter(|d| d.is_bang()) {
                        let (reg, ty) = self.lower_value(d.body())?;
                        let forced = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueForce {
                            dst: forced,
                            src: reg,
                        });
                        forced_scope.insert(d.name().to_string(), (forced, ty));
                    }
                    self.value_locals.push(forced_scope);
                    let res = self.lower_value_expected(body, expected);
                    self.value_locals.pop();
                    self.defs = saved;
                    res
                } else {
                    // Complete mode: a `let` binding is call-by-need — the
                    // def is suspended as a thunk (a fragment computing `e`
                    // capturing its outer free variables) and the body reads
                    // the thunk, forcing it on demand and memoizing the
                    // result. The binding is shared: two uses of `x` force
                    // `e` once. A def that references the OTHER let names is
                    // not supported in v1 (an unknown-name error surfaces).
                    //
                    // The thunk's fragment starts with an EMPTY frame, so every
                    // enclosing binding its body mentions must be CAPTURED —
                    // including this fragment's own parameters (`\s -> let p =
                    // … in …` captures `s`). Only the let names themselves are
                    // bound here; naming a sibling def is the v1 limitation
                    // above (it is not captured and surfaces as unknown).
                    let mut bound = HashSet::new();
                    for d in defs {
                        bound.insert(d.name().to_string());
                    }
                    // A local FUNCTION binding (`g y = e`, which the currying
                    // pass spells `g = \y -> e`) is not a thunk: it is applied by
                    // name and has no standalone scheme, so lowering can only
                    // install it from here. Sibling bindings are mutually
                    // visible, so a function's body may reference a sibling
                    // VALUE binding (`h = x + 1.0`) and vice versa; installing
                    // the whole group keeps that resolution working inside the
                    // nested fragment compiles. Only the parameterless bindings
                    // take the call-by-need thunk path below.
                    let saved_defs = self.defs.clone();
                    for d in defs {
                        self.defs.insert(d.name().to_string(), d.clone());
                    }
                    let mut scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
                    for d in defs.iter().filter(|d| !is_local_function(d)) {
                        let e = d.body();
                        let free = self.free_vars(e, &bound);
                        let (fragment_id, _ret_ty) = self.lower_fragment(
                            &[],
                            e,
                            &free,
                            &self.capture_tys(&free),
                            &[],
                            None,
                            *span,
                        )?;
                        let (env_reg, _self_cell) = self.emit_env_snapshot(&free, *span, None)?;
                        let x_reg = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueLazy {
                            dst: x_reg,
                            env: env_reg,
                            fragment: fragment_id,
                        });
                        // A bang binding (`let !x = e`) forces the thunk here,
                        // at the binding point, and the name reads the forced
                        // value (WHNF only).
                        let x_reg = if d.is_bang() {
                            let forced = self.fresh_value_reg();
                            self.emit_value(ValueInstr::ValueForce {
                                dst: forced,
                                src: x_reg,
                            });
                            forced
                        } else {
                            x_reg
                        };
                        scope.insert(d.name().to_string(), (x_reg, _ret_ty));
                    }
                    self.value_locals.push(scope);
                    let res = self.lower_value_expected(body, expected);
                    self.value_locals.pop();
                    self.defs = saved_defs;
                    res
                }
            }
            Expr::ActorParam {
                name,
                default,
                span,
            } => {
                // An actor param in value position reads its named parameter
                // slot as a Double value.
                let default_val = if let Some(d) = default {
                    const_f64(d).unwrap_or(0.0)
                } else {
                    0.0
                };
                let idx = self.intern_param(
                    name.clone(),
                    default_val,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    *span,
                )?;
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueReadParam { dst, idx });
                Ok((dst, ValueTy::Double))
            }
            _ => Err(CompileError::Type {
                msg: "unsupported expression in value position".into(),
                span: e.span(),
            }),
        }
    }

    /// Resolve a collection-operation name to its IR op by arity. `insert`
    /// dispatches on argument count: 3 → map, 2 → set. Returns `None` for a
    /// non-collection name (the collection op names are reserved).
    /// Whether `name` is a builtin that resolves at lowering (a collection op,
    /// an FFI foreign function, or a math/logic primitive) and therefore must
    /// never be captured as a free variable.
    fn is_builtin_op(&self, name: &str) -> bool {
        let s = |lit: &str| lit.to_string();
        let collection = [
            "cons",
            "head",
            "tail",
            "length",
            "map",
            "fold",
            "filter",
            "list",
            "insert",
            "lookup",
            "member",
            "empty_map",
            "empty_set",
            "concat_map",
            "append_list",
            "concat_string",
            "show_double",
            "show_int",
            "show_integer",
            "show_bool",
            "not",
            "and",
            "or",
            "listRange",
            "arrayFromListChecked",
        ]
        .iter()
        .any(|n| name == s(n));
        collection
            || self.env.foreign_sigs.contains_key(name)
            || name == "runST"
            || name == "runSTArray"
            // The ST cell primitives are lowered to `ValueEffectOp`s in
            // `Expr::Apply` — never captured as free variables by a lambda.
            || matches!(
                name,
                "newSTRef"
                    | "readSTRef"
                    | "writeSTRef"
                    | "modifySTRef"
                    | "newArrayFlat"
                    | "newArrayFromListFlat"
                    | "readArrayFlat"
                    | "writeArrayFlat"
                    | "modifyArrayFlat"
                    | "getBounds"
                    | "getElems"
            )
            // Value-track builtins lowered in `Expr::Apply` (one-pole
            // smoothing, buffer elementwise ops): compile-time primitives, never
            // captured as free variables by a lambda. The io-streams operations
            // (`fromList`/`toList`/`write`/`read`/`atEnd`/`unread`/`connect`/
            // `makeInputStream`/`makeOutputStream`) and the stdio handles
            // (`stdin`/`stdout`/`stderr`) are likewise resolved at lowering.
            || matches!(
                name,
                "fromList"
                    | "toList"
                    | "write"
                    | "read"
                    | "atEnd"
                    | "unread"
                    | "connect"
                    | "makeInputStream"
                    | "makeOutputStream"
                    | "stdin"
                    | "stdout"
                    | "stderr"
                    | "smooth"
                    | "atan2"
                    | "min"
                    | "max"
            )
            || matches!(name, "sin" | "cos" | "tan" | "sqrt" | "exp" | "ln" | "tanh")
            // `error` is lowered to its own fault instruction (see the `Apply`
            // arm), so it is a builtin op for capture purposes too — unless a
            // user definition of the name shadows it. `__fault` is the
            // prelude-internal reserved SPELLING (compared literally, not a
            // lexer rule): the definition-site arms in the `Apply` lowering
            // place it first, so it is unconditional here too (see
            // `ir::VALUE_FAULT_PRIMITIVE`).
            || (name == "error" && !self.defs.contains_key(name))
            || name == crate::ir::VALUE_FAULT_PRIMITIVE
    }

    fn value_builtin(&mut self, name: &str, nargs: usize) -> Option<ValueBuiltinOp> {
        use ValueBuiltinOp::*;
        Some(match (name, nargs) {
            // In complete mode `cons` is the LAZY constructor (the tail is not
            // forced), which is what makes infinite sequences productive.
            ("cons", _) if self.mode == Mode::Complete => LazyCons,
            ("cons", _) => Cons,
            // In complete mode `list`/`[]` is the empty lazy list (Nil).
            ("list", _) if self.mode == Mode::Complete => Nil,
            ("list", _) => ListEmpty,
            ("head", _) => Head,
            ("tail", _) => Tail,
            ("length", _) => Length,
            ("map", _) => Map,
            ("fold", _) => Fold,
            ("filter", _) => Filter,
            ("insert", 3) => InsertMap,
            ("insert", 2) => InsertSet,
            ("lookup", _) => Lookup,
            ("member", _) => Member,
            ("empty_map", _) => MapEmpty,
            ("empty_set", _) => SetEmpty,
            ("concat_map", 2) => ConcatMap,
            ("append_list", 2) => AppendList,
            ("concat_string", 2) => ConcatString,
            ("show_double", 1) => ShowDouble,
            ("show_int", 1) => ShowInt,
            ("show_integer", 1) => ShowInteger,
            ("show_bool", 1) => ShowBool,
            // Immutable array storage (flat, type-agnostic). The arity is part
            // of the key: `arrayUpdate`'s assoc list is a distinct operand from
            // `arrayAccum`'s accumulator closure, so each name has exactly one
            // shape.
            ("arrayFromList", 2) => ArrayFromList,
            ("arrayBounds", 1) => ArrayBounds,
            ("arrayElems", 1) => ArrayElems,
            ("arrayIndexFlat", 2) => ArrayIndexFlat,
            ("arrayUpdate", 2) => ArrayUpdate,
            ("arrayAccum", 3) => ArrayAccum,
            ("arrayFromBuffer", 1) => ArrayFromBuffer,
            ("bufferFromArray", 1) => BufferFromArray,
            ("arrayFromAssocs", 2) => {
                // The M8 polymorphic constructor resolves against the bounds
                // shape in the interpreter; mark lowering so the assembly step
                // appends the synthetic hole fragment (one shared callable per
                // array, pointing at a single faulting body).
                self.uses_array_hole = true;
                ArrayFromAssocs
            }
            ("arraySeed", 2) => ArraySeed,
            // `listRange (l, u)` — the inclusive Int range (the only
            // list-GENERATING primitive; `Ix.range` is built on it).
            ("listRange", 1) => ListRange,
            // `listArray`'s lenient constructor: (bounds, rangeSize, elems) —
            // the interpreter pads a short list with the shared hole fragment,
            // so lowering must emit it (the same flag as `arrayFromAssocs`).
            ("arrayFromListChecked", 3) => {
                self.uses_array_hole = true;
                ArrayFromListChecked
            }
            _ => return None,
        })
    }

    /// Result static type of a collection op applied to the given argument
    /// types. Element/value types are read from the container argument. Open
    /// collections carry no capacity.
    fn value_builtin_ty(
        &self,
        name: &str,
        args: &[ValueTy],
        span: Span,
    ) -> Result<ValueTy, CompileError> {
        // Enforce the `Ord` constraint on Map/Set keys (and Set elements): every
        // concrete key type must have a derived `Ord` instance (see
        // `derive_eq_ord`). `Func` gets no instance, so a function-typed key is
        // a compile error. Mirrors the infer-side check; lowering runs after
        // inference, so this is defense-in-depth for direct lower-only paths.
        let check_ord = |ty: &ValueTy, what: &str| -> Result<(), CompileError> {
            match self.env.type_name_of_vty(ty) {
                Some(name) => {
                    let has = self
                        .env
                        .instances
                        .get("Ord")
                        .map(|by_ty| by_ty.contains_key(name.as_str()))
                        .unwrap_or(false);
                    if has {
                        Ok(())
                    } else {
                        Err(CompileError::Type {
                            msg: format!("no Ord instance for {what} type `{name}`"),
                            span,
                        })
                    }
                }
                None => Err(CompileError::Type {
                    msg: format!("no Ord instance for {what} type (function or unresolved type)"),
                    span,
                }),
            }
        };
        match name {
            "cons" if self.mode == Mode::Complete => {
                // LazyCons: the tail may be any value (a thunk), so the strict
                // List shape-check does not apply; the result is a List.
                Ok(ValueTy::Con("List".into(), vec![ValueTy::Double]))
            }
            "cons" | "filter" => match args.get(1).and_then(list_type_args) {
                Some(inner) => Ok(ValueTy::Con("List".into(), inner.clone())),
                _ => Err(CompileError::Type {
                    msg: format!("{name} expects a List, got {:?}", args.get(1)),
                    span,
                }),
            },
            "tail" => match args.first().and_then(list_type_args) {
                Some(inner) => Ok(ValueTy::Con("List".into(), inner.clone())),
                _ => Err(CompileError::Type {
                    msg: format!("{name} expects a List, got {:?}", args.first()),
                    span,
                }),
            },
            "head" => {
                let elem = match args.first().and_then(list_type_args) {
                    Some(inner) => inner.first().cloned().unwrap_or(ValueTy::Double),
                    _ => ValueTy::Double,
                };
                Ok(ValueTy::Con("Maybe".into(), vec![elem]))
            }
            "length" => Ok(ValueTy::Int),
            "map" => {
                // The result list's ELEMENT type is the closure's RETURN type
                // (`map : (a -> b) -> List a -> List b`), read from the lowered
                // `Func` signature of `args[0]`; a closure with no known return
                // falls back to the source element type.
                let elem = match args.first() {
                    Some(ValueTy::Thunk(_, rets)) => {
                        rets.first().cloned().unwrap_or(ValueTy::Double)
                    }
                    _ => match args.get(1).and_then(list_type_args) {
                        Some(inner) => inner.first().cloned().unwrap_or(ValueTy::Double),
                        _ => ValueTy::Double,
                    },
                };
                Ok(ValueTy::Con("List".into(), vec![elem]))
            }
            // fold's result is the accumulator/seed type (mirrors inference).
            "fold" => Ok(args.get(1).cloned().unwrap_or(ValueTy::Double)),
            "concat_map" => {
                // See `concat_map_result_elem`: `(a -> List b) -> List a -> List b`.
                let source_elem = match args.get(1).and_then(list_type_args) {
                    Some(inner) => inner.first().cloned().unwrap_or(ValueTy::Double),
                    _ => ValueTy::Double,
                };
                let closure_ty = args.first().unwrap_or(&ValueTy::Double);
                let elem = concat_map_result_elem(closure_ty, source_elem);
                Ok(ValueTy::Con("List".into(), vec![elem]))
            }
            "append_list" => match args.first().and_then(list_type_args) {
                Some(inner) => Ok(ValueTy::Con("List".into(), inner.clone())),
                _ => Err(CompileError::Type {
                    msg: format!("append_list expects a List, got {:?}", args.first()),
                    span,
                }),
            },
            "concat_string" => Ok(ValueTy::String),
            "show_double" => Ok(ValueTy::String),
            "show_int" => Ok(ValueTy::String),
            "show_integer" => Ok(ValueTy::String),
            "show_bool" => Ok(ValueTy::String),
            "list" => Ok(ValueTy::Con("List".into(), vec![ValueTy::Double])),
            "insert" => match args.len() {
                3 => {
                    check_ord(args.first().unwrap_or(&ValueTy::Double), "key")?;
                    Ok(ValueTy::Con(
                        "Map".into(),
                        vec![
                            args.first().cloned().unwrap_or(ValueTy::Double),
                            args.get(1).cloned().unwrap_or(ValueTy::Double),
                        ],
                    ))
                }
                _ => {
                    check_ord(args.first().unwrap_or(&ValueTy::Double), "element")?;
                    Ok(ValueTy::Con(
                        "Set".into(),
                        vec![args.first().cloned().unwrap_or(ValueTy::Double)],
                    ))
                }
            },
            "lookup" => {
                check_ord(args.first().unwrap_or(&ValueTy::Double), "key")?;
                let v = match args.get(1) {
                    Some(ValueTy::Con(name, inner)) if name == "Map" => {
                        inner.get(1).cloned().unwrap_or(ValueTy::Double)
                    }
                    _ => ValueTy::Double,
                };
                Ok(ValueTy::Con("Maybe".into(), vec![v]))
            }
            "member" => {
                check_ord(args.first().unwrap_or(&ValueTy::Double), "key")?;
                Ok(ValueTy::bool_ty())
            }
            "empty_map" => Ok(ValueTy::Con(
                "Map".into(),
                vec![ValueTy::String, ValueTy::Double],
            )),
            "empty_set" => Ok(ValueTy::Con("Set".into(), vec![ValueTy::Double])),
            // Immutable array storage (flat, type-agnostic): the static types
            // mirror the builtin signatures. Index types fall back to the
            // builtin `Int` (never a numeric `Double`), element types to the
            // runtime's numeric value type.
            "arrayFromList" => {
                let index = pair_index_ty(args.first());
                let elem = args
                    .get(1)
                    .and_then(list_type_args)
                    .and_then(|i| i.first().cloned());
                Ok(ValueTy::Con(
                    "Array".into(),
                    vec![index, concrete_or_double(elem.unwrap_or(ValueTy::Double))],
                ))
            }
            // Index-typed construction ops: the bounds pair gives the index
            // type `i`; `arrayFromAssocs` reads the element type off the
            // association-list element's `Pair` (second half), `arraySeed` off
            // its seed operand directly.
            "arrayFromAssocs" => {
                let index = pair_index_ty(args.first());
                let elem = args
                    .get(1)
                    .and_then(list_type_args)
                    .and_then(|i| i.first())
                    .map(|first| pair_second_ty(Some(first)));
                Ok(ValueTy::Con(
                    "Array".into(),
                    vec![index, concrete_or_double(elem.unwrap_or(ValueTy::Double))],
                ))
            }
            "arraySeed" => {
                let index = pair_index_ty(args.first());
                let elem = args.get(1).cloned();
                Ok(ValueTy::Con(
                    "Array".into(),
                    vec![index, concrete_or_double(elem.unwrap_or(ValueTy::Double))],
                ))
            }
            "arrayBounds" => {
                let index = array_index_ty(args.first());
                Ok(ValueTy::Con("Pair".into(), vec![index.clone(), index]))
            }
            "arrayElems" => Ok(ValueTy::Con(
                "List".into(),
                vec![array_elem_ty(args.first())],
            )),
            "arrayIndexFlat" | "arrayUpdate" | "arrayAccum" => {
                // The three array-CONSUMING flat ops validate the first
                // argument uniformly (defense-in-depth: inference already pins
                // it). `arrayIndexFlat` then reads its element type from the
                // same checked array type.
                let arr = match args.first() {
                    Some(ty @ ValueTy::Con(name, _)) if name == "Array" => ty.clone(),
                    other => {
                        return Err(CompileError::Type {
                            msg: format!("{name} expects an Array, got {other:?}"),
                            span,
                        })
                    }
                };
                if name == "arrayIndexFlat" {
                    Ok(array_elem_ty(Some(&arr)))
                } else {
                    Ok(arr)
                }
            }
            "arrayFromBuffer" => Ok(ValueTy::Con(
                "Array".into(),
                vec![ValueTy::Int, ValueTy::Double],
            )),
            "bufferFromArray" => Ok(self.env.buffer_vty()),
            // `listRange (l, u)`: the inclusive range as a flat list, whose
            // ELEMENT type is the bounds pair's own index type. It is not always
            // `Int`: `Ix Integer`'s `range` is `listRange b` over an `Integer`
            // pair, and `default (Integer, Double)` is what makes an
            // unconstrained bounds literal arrive there. Hardcoding `Int` here
            // made the lowered type disagree with inference (which instantiates
            // the signature per call) and with the interpreter (which builds the
            // elements on the operand's own track), so a constrained definition
            // whose body is `range (arrayBounds a)` was rejected against its own
            // signature.
            "listRange" => Ok(ValueTy::Con(
                "List".into(),
                vec![pair_index_ty(args.first())],
            )),
            // `arrayFromListChecked bounds rangeSize elems`: the element list is
            // the THIRD operand (the size sits between bounds and elements), so
            // the assembled type reads the index off the bounds and the element
            // type off `args[2]`.
            "arrayFromListChecked" => {
                let index = pair_index_ty(args.first());
                let elem = args
                    .get(2)
                    .and_then(list_type_args)
                    .and_then(|i| i.first().cloned());
                Ok(ValueTy::Con(
                    "Array".into(),
                    vec![index, concrete_or_double(elem.unwrap_or(ValueTy::Double))],
                ))
            }
            _ => Err(CompileError::Unsupported(format!(
                "unknown collection op {name}"
            ))),
        }
    }

    /// Whether `e` is a bare reference to a nullary method of `class_name` (e.g.
    /// `mempty` in `mappend xs mempty`) — such an argument resolves by the
    /// selector argument's concrete type.
    fn is_bare_nullary_method_ref(&self, e: &Expr, class_name: &str) -> bool {
        match e {
            Expr::Ref(name, _) => self.env.is_nullary_method(class_name, name.as_str()),
            _ => false,
        }
    }

    /// Bind a constraint instance's head args from the LEADING args of the
    /// call-site concrete container (`instance (Monad m) => Arrow (Kleisli m)`:
    /// `Kleisli Maybe Double Double` + head `Kleisli m` → `m := Maybe`), recording
    /// each binding in [`Self::head_arg_bindings`], then discharge every
    /// constraint by instance lookup of the bound type. A head arg that is not
    /// concrete at the call site is DEFERRED (recorded, not checked) — the
    /// concrete binding emerges when the surrounding expression's type is
    /// pinned. Returns the instance's `(head_args, constraints)` (both empty
    /// for a non-constraint instance).
    #[allow(clippy::type_complexity)]
    fn bind_head_args_and_discharge(
        &mut self,
        class_name: &str,
        ctor: &str,
        container_vty: &ValueTy,
        span: Span,
    ) -> Result<(Vec<String>, Vec<(String, String)>), CompileError> {
        let inst = self
            .env
            .instances
            .get(class_name)
            .and_then(|by_ty| by_ty.get(ctor))
            .cloned();
        let (head_args, constraints) = match &inst {
            Some(i) => (i.head_args.clone(), i.constraints.clone()),
            None => (vec![], vec![]),
        };
        let concrete_args = match container_vty {
            ValueTy::Con(_, a) => a.clone(),
            _ => vec![],
        };
        if concrete_args.is_empty() && !head_args.is_empty() {
            // The container's type args are unknown at the call site (the
            // lowerer's inlined record literal carries no parameterization).
            // Defer the head binding and constraint discharge — inference
            // already discharged them with the concrete args.
            return Ok((vec![], vec![]));
        }
        for (k, hv) in head_args.iter().enumerate() {
            if let Some(ca) = concrete_args.get(k) {
                self.head_arg_bindings.insert(hv.clone(), ca.clone());
            } else {
                return Err(CompileError::Type {
                    msg: format!(
                        "head argument `{hv}` of `{ctor}` is not concrete at the call site"
                    ),
                    span,
                });
            }
        }
        for (cclass, cv) in &constraints {
            let bound =
                self.head_arg_bindings
                    .get(cv)
                    .cloned()
                    .ok_or_else(|| CompileError::Type {
                        msg: format!("constraint `{cclass} {cv}` has an unbound type variable"),
                        span,
                    })?;
            let tname = self.env.type_name_of_vty(&bound);
            if let Some(tname) = tname {
                if !self
                    .env
                    .instances
                    .get(cclass.as_str())
                    .map(|m| m.contains_key(tname.as_str()))
                    .unwrap_or(false)
                {
                    return Err(CompileError::Type {
                        msg: format!(
                            "no instance of `{cclass}` for type `{tname}` (constraint of `{class_name}`)"
                        ),
                        span,
                    });
                }
            }
            // An unresolved type variable defers the discharge to the point
            // where the head arg becomes concrete (the body's own resolution).
        }
        Ok((head_args, constraints))
    }

    /// Lower `f supplied…` as a value by completing the call to `arity`
    /// arguments and rewriting it into `\ $pK -> … f supplied… $pK`.
    ///
    /// THE rewrite for an under-applied call whose lowerer only knows the FULL
    /// argument list (a foreign/builtin signature, a constructor, a
    /// constructor-class method whose instance is selected by a later
    /// argument). One helper, not three: the closure machinery already carries
    /// arity, partial application and RE-application, so the rewritten form
    /// needs no other support, and the `$`-prefixed parameter names cannot
    /// collide with a source identifier.
    ///
    /// Kept out of line: the calling arms sit inside the deeply recursive
    /// `lower_value` dispatch, where a few inline locals are enough to overflow
    /// the stack on a nested program.
    #[inline(never)]
    fn lower_under_applied(
        &mut self,
        name: &str,
        call_args: &[Expr],
        arity: usize,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        let names: Vec<String> = (call_args.len()..arity).map(|i| format!("$p{i}")).collect();
        let mut args = call_args.to_vec();
        args.extend(names.iter().map(|p| Expr::Ref(p.clone(), span)));
        let mut lam = Expr::Apply {
            name: name.to_string(),
            args,
            span,
        };
        for p in names.into_iter().rev() {
            lam = Expr::Lambda {
                params: vec![Param {
                    name: p,
                    span,
                    bang: false,
                }],
                body: Box::new(lam),
                span,
            };
        }
        self.lower_value(&lam)
    }

    /// Lower a call to a binding whose body is an UNDER-APPLIED constructor-class
    /// method — `g = fmap (+ 1.0)` applied as `g m` — by β-substituting the
    /// remaining arguments into the method call.
    ///
    /// A constructor-class method's instance is chosen by the class-var-applied
    /// CONTAINER argument (`fmap g m` selects on `m`). A partial application has
    /// not received that container yet, so it cannot select an instance — it is
    /// a bare function value, exactly as inference types it (`Functor f => f a
    /// -> f b`). But lowering cannot build a runtime dictionary: its only way to
    /// compile a method call is to inline the selected instance's body. The
    /// binding's use site is therefore the specialization point, and it is a
    /// legitimate one — a top-level value binding is INLINED at each use site
    /// (`lower_value_ref`), so every application is a distinct call site with
    /// its own container type, and `g (Just 1.0)` / `g [1.0]` each select their
    /// own instance.
    ///
    /// This is the same per-call-site specialization [`Self::lower_constrained_apply_site`]
    /// performs, and the same rule the reduce pass applies to a λ-parameter
    /// definition. It is the ONE place lowering needs it: every other position
    /// that could hold the bare method value (a λ body, an argument to a
    /// higher-order function) reaches a call site of its own.
    ///
    /// Returns `None` for every other callee — the common case.
    #[inline(never)]
    fn lower_applied_method_binding_site(
        &mut self,
        name: &str,
        call_args: &[Expr],
        expected: Option<&ValueTy>,
        span: Span,
    ) -> Option<Result<(usize, ValueTy), CompileError>> {
        if call_args.is_empty() {
            return None;
        }
        // A value local or fragment capture of the same spelling shadows the
        // definition, exactly as at every other definition lookup.
        if self.value_locals.iter().any(|s| s.contains_key(name))
            || self.fragment_captures.iter().any(|(n, _)| n == name)
        {
            return None;
        }
        let (under, missing) = self.under_applied_method_body(name)?;
        // The definition's own scheme says how many arguments it still wants;
        // applying at least that many reaches a FULL method call, which the
        // ordinary paths lower. Fewer is still a value.
        if call_args.len() < missing {
            return None;
        }
        let mut args = under;
        args.extend(call_args.iter().cloned());
        let inlined = Expr::Apply {
            name: self.under_applied_method_name(name)?,
            args,
            span,
        };
        Some(self.lower_value_expected(&inlined, expected))
    }

    /// The `(supplied arguments, missing count)` of a binding whose body is a
    /// single under-applied constructor-class method call, and nothing else.
    ///
    /// The test is deliberately strict — the body must BE that call, so the
    /// rewrite cannot duplicate or drop any other work. A binding that computes
    /// something around a partial method (`h = f . fmap g`) is an ordinary
    /// closure and keeps the runtime path.
    fn under_applied_method_body(&self, name: &str) -> Option<(Vec<Expr>, usize)> {
        let def @ Def::Eval { .. } = self.defs.get(name)? else {
            return None;
        };
        // Arity 0 only: a binding with parameters (`h x = …`) is a function
        // value, not a partial method application.
        if def.arity() != 0 {
            return None;
        }
        let body = def.body();
        let Expr::Apply {
            name: callee, args, ..
        } = body
        else {
            return None;
        };
        let class_name = self.env.class_of_method(callee)?;
        let class_info = self.env.typeclasses.get(&class_name)?;
        if class_info.arity == 0 {
            return None;
        }
        let sig = class_info
            .methods
            .iter()
            .find(|(m, _)| m == callee)
            .map(|(_, s)| s.clone())?;
        let declared = sig.sig_arg_count();
        (args.len() < declared).then(|| (args.clone(), declared - args.len()))
    }

    /// The method name [`Self::under_applied_method_body`] found, when the
    /// binding qualifies.
    fn under_applied_method_name(&self, name: &str) -> Option<String> {
        let Def::Eval { body, .. } = self.defs.get(name)? else {
            return None;
        };
        match body {
            Expr::Apply { name: callee, .. } => Some(callee.clone()),
            _ => None,
        }
    }

    /// Lower a call to a class-constrained definition (`mk :: Ix i => Pair i i
    /// -> Double`), if `name` names one.
    ///
    /// Returns `None` for every other callee — the common case — so the caller's
    /// arm can fall through to the ordinary application paths. A value local of
    /// the same name shadows the definition (and the call is not a
    /// specialization).
    ///
    /// The call site is where such a definition becomes a concrete function:
    ///
    /// 1. every argument is lowered to get its static type,
    /// 2. those types are matched against the signature's parameter patterns,
    ///    solving the class variables (`Pair Int Int` vs `Pair i i` → `i := Int`),
    /// 3. the class context is discharged against the registered instances (a
    ///    concrete type with no instance is a compile error — there is no
    ///    runtime dictionary to fall back to),
    /// 4. the body is inlined with the parameters bound to the argument
    ///    registers, so its method calls dispatch by the concrete
    ///    class-variable type.
    ///
    /// This is compile-time β-substitution: one specialization per call site,
    /// never a runtime dispatch. A call applied to FEWER arguments than the
    /// definition binds returns a closure over the applied ones (the remaining
    /// λ-chain is lowered as a lambda whose parameters the signature types).
    #[inline(never)]
    fn lower_constrained_apply_site(
        &mut self,
        name: &str,
        call_args: &[Expr],
        expected: Option<&ValueTy>,
        span: Span,
    ) -> Option<Result<(usize, ValueTy), CompileError>> {
        let env = self.env;
        let cd = env.constrained_defs.get(name)?.clone();
        // A value binding named like the definition shadows it: a λ-parameter
        // or a fragment capture (`\ listArray -> listArray 1.0`).
        if self.value_locals.iter().any(|s| s.contains_key(name))
            || self.fragment_captures.iter().any(|(n, _)| n == name)
        {
            return None;
        }
        // A DUAL name — one that is BOTH a class method and a
        // class-constrained definition. Today's prelude has none (`!` is the
        // accessor and `index` the `Ix` method, one role each), so this arm is
        // unreached by the prelude; it stays because a user program may declare
        // such a pair. When the first argument's static type selects a method
        // instance, DECLINE the definition so the method path below handles the
        // call. The first argument is probed in a sandbox (the instructions are
        // discarded), and the routing predicate is the SAME one inference uses
        // (`first_arg_solves_class_var`), so the passes cannot disagree.
        if env.class_of_method(name).is_some() {
            if let Some(first) = call_args.first() {
                if let Some(ty) = self.peek_value_ty(first) {
                    if crate::types::infer::first_arg_solves_class_var(env, name, &ty) {
                        return None;
                    }
                }
            }
        }
        Some(self.lower_constrained_apply(name, &cd, call_args, expected, span))
    }

    /// Whether `name` is shadowed by a DEFINITION at this call site, i.e. the
    /// call must NOT be a class-method dispatch.
    ///
    /// Mirrors inference's `method_shadowed_by_def`: a plain user definition
    /// shadows a method outright; a class-CONSTRAINED definition sharing its
    /// name with a method (a DUAL name — none exists in the prelude today)
    /// routes by the FIRST argument's static type — an argument that solves the
    /// method's class variable goes to the method, everything else to the
    /// definition.
    fn method_shadowed_by_def(&mut self, name: &str, args: &[Expr]) -> bool {
        // Lowering sees fragment captures; inference never does, so the
        // asymmetry is intentional and safe: captures only exist in lowering.
        if self.value_locals.iter().any(|s| s.contains_key(name))
            || self.fragment_captures.iter().any(|(n, _)| n == name)
        {
            return true;
        }
        let constrained = self.env.constrained_defs.contains_key(name);
        if !constrained && !self.defs.contains_key(name) {
            return false;
        }
        if constrained && self.env.class_of_method(name).is_some() {
            if let Some(first) = args.first() {
                if let Some(ty) = self.peek_value_ty(first) {
                    return !crate::types::infer::first_arg_solves_class_var(self.env, name, &ty);
                }
            }
        }
        // Untypeable or absent first arg: fall back to the definition (matches
        // inference's routing).
        true
    }

    /// Recover an expression's static value type by lowering it in a throwaway
    /// block, KEEPING the current locals visible.
    ///
    /// [`Self::sandbox_value_ty`] empties the local scopes because it types
    /// self-contained scrutinees; a dispatch probe asks about an argument that
    /// usually IS a local reference (`inRange b i` with `b` a λ-parameter), so
    /// clearing them would make every probe of a local fail and silently route
    /// every dual-name call to the definition.
    ///
    /// The emitted instructions, register allocation, `anchor_cells`, pending
    /// parameter types, and head-arg bindings are all restored. The monotone
    /// append-only tables (`container_tys`, `value_funcs`, `fragments`) are
    /// shared across probes; entries produced by a probe are dead IR and affect
    /// compile time only.
    fn peek_value_ty(&mut self, e: &Expr) -> Option<ValueTy> {
        let saved_blocks = std::mem::take(&mut self.value_blocks);
        let saved_cur = self.cur_value_block;
        self.cur_value_block = self.new_value_block();
        let saved_next = self.next_value_reg;
        // The probe must see the CURRENT scopes: a λ-parameter lives in
        // `value_locals`, and — inside a nested fragment — a capture lives in
        // `fragment_captures`. Taking either map away (as `sandbox_value_ty`
        // does for self-contained scrutinees) makes the probe of a plain
        // local reference fail, which silently routed every dual-name call
        // inside a λ body to the definition. Both maps are cloned and restored
        // instead, so a capture registered during the probe is discarded.
        let saved_captures_len = self.fragment_captures.len();
        let saved_inline = self.value_inline.clone();
        let saved_pending = self.pending_param_tys.clone();
        let saved_bindings = self.head_arg_bindings.clone();
        let saved_anchor_cells = self.anchor_cells.clone();
        let r = self.lower_value(e).ok();
        self.value_blocks = saved_blocks;
        self.cur_value_block = saved_cur;
        self.next_value_reg = saved_next;
        self.fragment_captures.truncate(saved_captures_len);
        self.value_inline = saved_inline;
        self.pending_param_tys = saved_pending;
        self.head_arg_bindings = saved_bindings;
        self.anchor_cells = saved_anchor_cells;
        r.map(|(_, ty)| ty)
    }

    /// [`Self::lower_constrained_apply_site`] for a known constrained
    /// definition.
    fn lower_constrained_apply(
        &mut self,
        name: &str,
        cd: &crate::types::ty::ConstrainedDef,
        call_args: &[Expr],
        expected: Option<&ValueTy>,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        let (arg_tes, _) = crate::types::infer::sig_func_parts(&cd.sig);
        if arg_tes.len() != cd.params.len() {
            return Err(CompileError::Type {
                msg: format!(
                    "signature of `{name}` declares {} argument(s), but the definition binds {}",
                    arg_tes.len(),
                    cd.params.len()
                ),
                span,
            });
        }
        if call_args.len() > cd.params.len() {
            return Err(CompileError::Type {
                msg: format!(
                    "`{name}` expects {} argument(s), got {}",
                    cd.params.len(),
                    call_args.len()
                ),
                span,
            });
        }
        // A constrained body cannot specialize itself eagerly: the recursive
        // call's class variable is only solved by the enclosing call site,
        // which is exactly the expansion in flight. The guard is applied when
        // the BODY is lowered (see [`Self::lower_constrained_body`]), not here:
        // the call's arguments are ordinary expressions and may legitimately
        // contain another use of the same definition.
        self.lower_constrained_apply_inner(name, cd, call_args, expected, span)
    }

    /// Lower a constrained definition's body under the recursion guard.
    ///
    /// The guard spans the BODY only. A nested use of the same definition is
    /// not recursion: `ixmap b f (ixmap b f a)` lowers the inner call as the
    /// outer call's ARGUMENT, before the outer body exists, and the accessor
    /// `index` over an `ixmap` reaches `ixmap`'s body (which calls `index` for
    /// its source read) before the outer accessor's body. Guarding the
    /// argument pass rejected both compositions as "recursive".
    fn lower_constrained_body(
        &mut self,
        name: &str,
        where_defs: &[Def],
        body: &Expr,
        expected: Option<&ValueTy>,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        if self.constrained_lifting.contains(name) {
            return Err(CompileError::Type {
                msg: format!("recursive class-constrained definition `{name}`"),
                span,
            });
        }
        self.constrained_lifting.insert(name.to_string());
        let result = self.lower_with_where_defs(where_defs, body, expected);
        self.constrained_lifting.remove(name);
        result
    }

    /// The body of [`Self::lower_constrained_apply`], run under its recursion
    /// guard.
    fn lower_constrained_apply_inner(
        &mut self,
        name: &str,
        cd: &crate::types::ty::ConstrainedDef,
        call_args: &[Expr],
        expected: Option<&ValueTy>,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        let env = self.env;
        let (arg_tes, ret_te) = crate::types::infer::sig_func_parts(&cd.sig);
        // Pass 1: lower the arguments and solve the signature's type variables
        // from their static types.
        //
        // A FUNCTION-shaped parameter (`(i -> j)` in `ixmap`) is the one case
        // where the expectation must be threaded: its argument is usually a
        // λ-literal, and a λ parameter is only typed structurally when the
        // expected type says what it is — otherwise `p` stayed the scalar
        // default and `p.second` had no record to project from. The class
        // variables of the preceding arguments are already solved here, so the
        // expectation is concrete. Scalar/record parameters keep the
        // expectation-free lowering (their argument's own form types them).
        let mut bindings: HashMap<String, ValueTy> = HashMap::new();
        // Haskell 2010 solves a signature's variables from the POSITION the
        // call flows into as well as from its arguments (measured: GHC passes
        // the same `$dNumDouble` into `g :: Num b => Int -> b` that `(+)`
        // selected for `g 0 + 2.5`). Binding here — BEFORE any argument is
        // lowered — is what makes the later `lower_value_expected` calls below
        // see the solved variable; binding after would leave the class
        // variable unsolved for the whole argument pass.
        //
        // The expectation is only a candidate SOURCE for one variable: it may
        // also be a variable itself, or a type the result merely CONTAINS
        // (`Pair i i` for an index pair). Binding it blindly replaced every
        // signature variable with whatever the position happened to say, which
        // turned `ixmap`'s index into `Double`. So the candidate is used only
        // when the result is the bare class variable (or a plain type variable)
        // — the `g :: Num b => Int -> b` shape, where the position genuinely IS
        // that variable's type.
        let exp_ty = expected.map(|e| match e {
            ValueTy::Thunk(_, rets) if rets.len() == 1 => rets[0].clone(),
            other => other.clone(),
        });
        if let Some(exp) = &exp_ty {
            let bare_var = match ret_te {
                crate::ast::TypeExpr::TName(n) => {
                    !crate::types::infer::is_declared_type_name(env, n)
                }
                // Look through the wrappers a signature may carry.
                crate::ast::TypeExpr::TForall(_, inner)
                | crate::ast::TypeExpr::TConstrained(_, inner) => match inner.as_ref() {
                    crate::ast::TypeExpr::TName(n) => {
                        !crate::types::infer::is_declared_type_name(env, n)
                    }
                    _ => false,
                },
                _ => false,
            };
            if bare_var && is_concrete_ty(exp) {
                crate::types::infer::bind_sig_type_vars(env, ret_te, exp, &mut bindings);
            }
        }
        let mut lowered: Vec<(usize, ValueTy)> = Vec::with_capacity(call_args.len());
        for (i, a) in call_args.iter().enumerate() {
            let expected_arg = match arg_tes.get(i) {
                Some(te) => {
                    let mut vars: HashMap<String, ValueTy> = HashMap::new();
                    let vty = crate::types::infer::sig_ty_to_vty(
                        env,
                        te,
                        &bindings,
                        &mut vars,
                        &mut || self.fresh_sig_var(),
                    )?;
                    // EVERY argument is lowered at its signature parameter
                    // type once the variables are solved — not only a
                    // function-shaped one. A scalar parameter that is now
                    // concrete is exactly what makes the operand instantiate at
                    // the right numeric type instead of defaulting: without it
                    // `g 0` lowered its body at `Integer` while the position
                    // demanded `Double`, and the kernel faulted on a mixed-kind
                    // pair. An unsolved parameter type passes no expectation, so
                    // a genuinely polymorphic argument keeps its own inference.
                    matches!(vty, ValueTy::Thunk(_, _)).then_some(vty)
                }
                None => None,
            };
            let (r, t) = match &expected_arg {
                Some(exp) => self.lower_value_expected(a, Some(exp))?,
                None => self.lower_value(a)?,
            };
            if let Some(pat) = arg_tes.get(i) {
                crate::types::infer::bind_sig_type_vars(env, pat, &t, &mut bindings);
            }
            lowered.push((r, t));
        }
        crate::types::infer::discharge_def_constraints(env, name, &cd.sig, &bindings, span)?;
        // The signature's RESULT type with the solved class variables
        // substituted. Variables the call site left unsolved (a genuinely
        // polymorphic result) become fresh lowerer type variables.
        let mut vars: HashMap<String, ValueTy> = HashMap::new();
        let ret =
            crate::types::infer::sig_ty_to_vty(env, ret_te, &bindings, &mut vars, &mut || {
                self.fresh_sig_var()
            })?;
        let mut scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
        let mut ctor_scope: HashMap<String, Option<String>> = HashMap::new();
        for (i, (r, t)) in lowered.iter().enumerate() {
            scope.insert(cd.params[i].name.clone(), (*r, t.clone()));
            ctor_scope.insert(
                cd.params[i].name.clone(),
                self.static_scrutinee_ctor(&call_args[i]),
            );
        }
        if call_args.len() < cd.params.len() {
            // Partial application: the remaining λ-parameters stay a function
            // value. The applied arguments are captured by value (they are in
            // scope as locals), and the remaining parameters are typed from the
            // signature, whose class variables are already solved — so method
            // dispatch inside the returned closure stays concrete.
            let remaining_tys: Vec<ValueTy> = arg_tes[call_args.len()..]
                .iter()
                .map(|te| {
                    crate::types::infer::sig_ty_to_vty(env, te, &bindings, &mut vars, &mut || {
                        self.fresh_sig_var()
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            self.value_locals.push(scope);
            self.value_local_ctors.push(ctor_scope);
            let remaining = Expr::Lambda {
                params: cd.params[call_args.len()..].to_vec(),
                body: Box::new(cd.body.clone()),
                span,
            };
            let saved_pending = self.pending_param_tys.take();
            self.pending_param_tys = Some(remaining_tys);
            let res = self.lower_constrained_body(name, &cd.where_defs, &remaining, expected, span);
            self.pending_param_tys = saved_pending;
            self.value_locals.pop();
            self.value_local_ctors.pop();
            return res;
        }
        // Full application: the definition body becomes the call's value,
        // typed by the signature's result. A concrete caller expectation wins
        // (inference pinned the signature result to it).
        let sig_ret = ret;
        let body_expected = expected.cloned().unwrap_or_else(|| sig_ret.clone());
        self.value_locals.push(scope);
        self.value_local_ctors.push(ctor_scope);
        let res =
            self.lower_constrained_body(name, &cd.where_defs, &cd.body, Some(&body_expected), span);
        self.value_locals.pop();
        self.value_local_ctors.pop();
        let (r, t) = res?;
        check_specialized_result(self.env, name, &t, &sig_ret, span)?;
        Ok((r, t))
    }

    /// Specialize a SIGNATURE-LESS curried definition — `at a i = a ! i`, which
    /// the parser drains into a bare `at = \a -> \i -> …` λ-nest — at its call
    /// site.
    ///
    /// Inference recovers the λ-parameter types of such a definition into
    /// [`Self::fn_param_tys`] (`Array i e`, `i`), but lowering has no
    /// substitution: a body that dispatches on a parameter's shape (`a ! i` →
    /// the constrained accessor `(!)`, whose `Ix` class variable is the array's
    /// INDEX type) reaches the typeclass dispatch with an unsolved variable and dies
    /// (`no instance of `Ix` for type `Pair``). Just as
    /// [`Self::lower_constrained_apply_site`] solves a class-constrained
    /// definition's variables from its arguments' static types, this site binds
    /// the λ-parameters to the argument registers' CONCRETE static types and
    /// lowers the uncurried body, so the body's method calls dispatch by the
    /// concrete types.
    ///
    /// Returns `None` (falling through to the ordinary application paths)
    /// unless the callee is exactly such a definition:
    ///
    /// - a value def in [`Self::defs`] that is NOT class-constrained (those
    ///   belong to [`Self::lower_constrained_apply_site`]) and carries no `::`
    ///   annotation;
    /// - an empty-parameter λ-nest whose recovered λ-parameter list holds at
    ///   least one non-`Double` type (a fully-`Double` def like `add a b = a + b`
    ///   keeps the ordinary eager path — nothing needs a concrete type) and
    ///   lines up with the λ-nest's depth;
    /// - and not a recursive definition: `take n xs = …`/`zipWith` must keep
    ///   their eager shared-cell path, because specializing a recursive body
    ///   would expand forever at compile time. A recursing DEFERRED def
    ///   (whose body also dispatches on a class method) is reported as a
    ///   compile error instead; the non-class recursion keeps its eager path.
    ///
    /// A value local or fragment capture shadowing the name, and a DUAL name
    /// (a method that is also a class-constrained definition — none in the
    /// prelude) whose first argument's static type selects a method instance,
    /// also return `None` (mirroring [`Self::lower_constrained_apply_site`]).
    #[inline(never)]
    fn lower_signature_less_apply_site(
        &mut self,
        name: &str,
        call_args: &[Expr],
        expected: Option<&ValueTy>,
        span: Span,
    ) -> Option<Result<(usize, ValueTy), CompileError>> {
        let env = self.env;
        // Class-constrained defs are specialized by the constrained site (which
        // runs first in the `Apply` arm); a def sharing a name with a method or
        // runtime primitive there must never reach this site twice.
        if env.constrained_defs.contains_key(name) {
            return None;
        }
        let def = self.defs.get(name)?.clone();
        let Def::Eval {
            body,
            where_defs: _,
            sig,
            ..
        } = &def
        else {
            return None;
        };
        // "Signature-less multi-parameter def": `at a i = …` parses to
        // `at = \a -> \i -> …` (Haskell 2010 §4.4.3.1), carries no `::`
        // annotation, and has more than one parameter for a call site to
        // specialize. A SINGLE-parameter definition is ordinary first-class
        // code (`f = double` binds it), and a bang parameter's force must
        // survive, so both keep the closure path.
        if sig.is_some() || !matches!(body, Expr::Lambda { .. }) {
            return None;
        }
        if def.arity() < 1 || crate::reduce::has_bang_param(&def) {
            return None;
        }
        let param_tys = self.fn_param_tys.get(name).cloned()?;
        // The recovered λ-parameter list must line up with the def's λ-nest
        // depth; a shape the currying pass did not normalize falls through to
        // the ordinary application paths instead of mis-aligning parameters
        // against the wrong body lambdas.
        if param_tys.len() != crate::types::infer::leading_lambda_count(body) {
            return None;
        }
        // At least one non-`Double` recovered λ-parameter: a type the eager
        // path cannot dispatch on with lowering's missing substitution. A
        // fully-`Double` def (`add a b = a + b`) has nothing to substitute and
        // stays on the eager path.
        if param_tys.is_empty() || param_tys.iter().all(|t| matches!(t, ValueTy::Double)) {
            return None;
        }
        // Recursion safety: a recursive signature-less curried def's body calls
        // the same def, which would specialize forever (its cycle only resolves
        // at runtime through the shared-cell closure path). Keep those on the
        // eager anchor path.
        if self.recursive_locals.contains(name) {
            // Specializing a recursive body would re-enter this site at every
            // step and expand forever, so a recursive definition always keeps
            // the eager path. Complete mode has a SHARED CELL to anchor the
            // cycle on (the STG model), so the eager path works there. Strict
            // mode has no cell: a body that ALSO needs per-call-site class
            // dispatch would have to specialize to resolve it, so report the
            // definition instead of looping.
            if self.mode != Mode::Complete && self.is_deferred_sigless_def(name) {
                return Some(Err(CompileError::Type {
                    msg: format!("recursive signature-less curried definition `{name}`"),
                    span,
                }));
            }
            return None;
        }
        // A value binding or fragment capture named like the definition shadows
        // it: a λ-parameter or a capture (`\ listArray -> listArray 1.0`).
        if self.value_locals.iter().any(|s| s.contains_key(name))
            || self.fragment_captures.iter().any(|(n, _)| n == name)
        {
            return None;
        }
        // A DUAL name (a method that is also a class-constrained definition —
        // none in the prelude): when the first argument's static type selects a
        // method instance, DECLINE the definition so the method path handles the
        // call.
        if env.class_of_method(name).is_some() {
            if let Some(first) = call_args.first() {
                if let Some(ty) = self.peek_value_ty(first) {
                    if crate::types::infer::first_arg_solves_class_var(env, name, &ty) {
                        return None;
                    }
                }
            }
        }
        // `param_tys` and `def` are owned clones, so the `&mut self` call below
        // is unencumbered by the guard's borrows.
        Some(self.lower_signature_less_apply(name, &def, call_args, &param_tys, expected, span))
    }

    /// The body of [`Self::lower_signature_less_apply_site`]: bind the applied
    /// arguments to the definition's λ-parameters and lower its uncurried body.
    fn lower_signature_less_apply(
        &mut self,
        name: &str,
        def: &Def,
        call_args: &[Expr],
        param_tys: &[ValueTy],
        expected: Option<&ValueTy>,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        let Def::Eval {
            body: def_body,
            where_defs,
            ..
        } = def
        else {
            unreachable!("the site only fires for `Def::Eval`");
        };
        // Recover the formal parameters from the λ-nest
        // (`\a -> \i -> a ! i` → params `[a, i]`, body `a ! i`): `arity` is the
        // number of recovered λ-parameter types, so exactly the def's head
        // lambdas are peeled off.
        let (params, uncurried_body) =
            crate::types::ty::split_curried_params(param_tys.len(), &[], def_body);
        if call_args.len() > params.len() {
            return Err(CompileError::Type {
                msg: format!(
                    "`{name}` expects {} argument(s), got {}",
                    params.len(),
                    call_args.len()
                ),
                span,
            });
        }
        // A call site's expectation reaches the arguments by UNIFYING the
        // callee's SCHEME (`a -> a` pins both sides of one bind), which is the
        // same rule the ordinary application path applies. The structural
        // fallback runs only when no scheme of this arity exists.
        let result_tys = self.def_result_tys.get(name).cloned().unwrap_or_default();
        let (arg_tys, _) = match self.scheme_pinned_params(name, param_tys.len(), expected, span) {
            Some(params) => (params, result_tys),
            None => pin_ret_head_types(param_tys, &result_tys, expected),
        };
        // Lower the applied arguments (each against its recovered parameter type
        // as the expectation) and bind each λ-parameter to its argument register
        // and CONCRETE static type, so the body's own method calls dispatch by
        // those types.
        let mut scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
        let mut ctor_scope: HashMap<String, Option<String>> = HashMap::new();
        let mut lowered: Vec<(usize, ValueTy)> = Vec::with_capacity(call_args.len());
        for (i, a) in call_args.iter().enumerate() {
            let (r, t) = match arg_tys.get(i).cloned() {
                Some(pt) => self.lower_value_expected(a, Some(&pt))?,
                None => self.lower_value(a)?,
            };
            lowered.push((r, t));
        }
        for (i, (p, (r, t))) in params.iter().zip(&lowered).enumerate() {
            scope.insert(p.name.clone(), (*r, t.clone()));
            ctor_scope.insert(p.name.clone(), self.static_scrutinee_ctor(&call_args[i]));
        }
        self.value_locals.push(scope);
        self.value_local_ctors.push(ctor_scope);
        let res = if call_args.len() < params.len() {
            // Partial application: the remaining λ-parameters stay a function
            // value, typed by the recovered parameter list (the same types the
            // ordinary path would use — the applied arguments are captured in
            // scope as locals).
            let remaining_tys: Vec<ValueTy> = param_tys[call_args.len()..].to_vec();
            let remaining = Expr::Lambda {
                params: params[call_args.len()..].to_vec(),
                body: Box::new(uncurried_body.clone()),
                span,
            };
            let saved_pending = self.pending_param_tys.take();
            self.pending_param_tys = Some(remaining_tys);
            let res = self.lower_value_expected(&remaining, expected);
            self.pending_param_tys = saved_pending;
            res
        } else {
            // Full application: the definition's uncurried body becomes the
            // call's value, compiled here with the parameters bound to the
            // argument registers — compile-time β-substitution, one
            // specialization per call site.
            self.lower_with_where_defs(where_defs, &uncurried_body, expected)
        };
        self.value_locals.pop();
        self.value_local_ctors.pop();
        res
    }

    /// Whether `name` is a DEFERRED signature-less curried definition — one
    /// whose per-call-site specialization
    /// ([`Self::lower_signature_less_apply`]) is the only way to compile it,
    /// so a bare reference or a recursive application carries no legal
    /// standalone value.
    ///
    /// The criterion is deliberately NARROWER than the specialization-site
    /// trigger: besides not being class-constrained, a MULTI-parameter λ-nest
    /// with no `::` annotation, and at least one recovered non-`Double`
    /// λ-parameter, the body must reference a call-site-dependent name — a class
    /// METHOD or a class-CONSTRAINED DEFINITION (`a ! i` lowers to `(!)`, which
    /// is constrained on `Ix i`). `nth xs k = head (tail xs)` references
    /// neither and stays a well-formed first-class value.
    ///
    /// A SINGLE-parameter definition is not deferred: `double x = x * 2.0` is
    /// first class like any other value (`f = double` binds it), and only a
    /// λ-nest the CURRYING pass produced — `f x y = …`, arity ≥ 2 — has
    /// per-call-site parameter shapes to recover.
    fn is_deferred_sigless_def(&self, name: &str) -> bool {
        if self.env.constrained_defs.contains_key(name) {
            return false;
        }
        let Some(Def::Eval { body, sig, .. }) = self.defs.get(name) else {
            return false;
        };
        if sig.is_some() || !matches!(body, Expr::Lambda { .. }) || self.defs[name].arity() < 2 {
            return false;
        }
        let Some(param_tys) = self.fn_param_tys.get(name) else {
            return false;
        };
        if param_tys.is_empty() || param_tys.iter().all(|t| matches!(t, ValueTy::Double)) {
            return false;
        }
        // The λ-parameters of THIS definition: a mention whose class variable is
        // solved by one of them is a call-site demand, one solved by the body
        // itself is not.
        let (params, _) = crate::types::ty::split_curried_params(param_tys.len(), &[], body);
        let param_names: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
        body_carries_class_demand(self.env, body, &param_names)
    }

    /// A fresh unification variable for signature-type conversion at a
    /// constrained definition's call site.
    fn fresh_sig_var(&mut self) -> ValueTy {
        let v = self.next_tyvar;
        self.next_tyvar += 1;
        ValueTy::Var(v)
    }

    /// Lower `e` with a definition's `where`-block definitions in scope,
    /// restoring the previous definition map afterwards.
    fn lower_with_where_defs(
        &mut self,
        where_defs: &[Def],
        e: &Expr,
        expected: Option<&ValueTy>,
    ) -> Result<(usize, ValueTy), CompileError> {
        if where_defs.is_empty() {
            return self.lower_value_expected(e, expected);
        }
        let saved = self.defs.clone();
        for d in where_defs {
            self.defs.insert(d.name().to_string(), d.clone());
        }
        let res = self.lower_value_expected(e, expected);
        self.defs = saved;
        res
    }

    /// Resolve a `Ref` in value position: a value local (match-arm binding) or a
    /// value definition (inlined at each use site).
    fn lower_value_ref(
        &mut self,
        name: &str,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        self.lower_value_ref_expected(name, span, None)
    }

    /// [`Self::lower_value_ref`] with an optional expected value type. A
    /// result-directed typeclass method (`mempty`) resolves by the expected
    /// type's concrete name.
    fn lower_value_ref_expected(
        &mut self,
        name: &str,
        span: Span,
        expected: Option<&ValueTy>,
    ) -> Result<(usize, ValueTy), CompileError> {
        // A class-constrained definition has no standalone value: its body is
        // specialized per CALL SITE, where the arguments — or the POSITION the
        // reference flows into — solve the class variable. A bare reference with
        // NEITHER (`f = mk`, no expectation) has no site at all, so report it
        // here instead of lowering the deferred body against an unsolved
        // variable, which would surface a misleading `no instance of … for
        // Double`. A `where`-local of the same name shadows the definition.
        //
        // The expectation IS such a site: `h :: Num b => b; h = 1` is a
        // constrained DEFINITION with no parameter list, and
        // `show_double (h :: Double)` fixes its `b` exactly as a call argument
        // would (Haskell 2010 §4.5.5 does not restrict a signed binding, and
        // measured GHC keeps `h :: forall b. Num b => b`). Refusing here made
        // every use of such a definition unreachable, however concrete its
        // position.
        // The class variable is SOLVED when inference recorded a selection for
        // THIS reference — that is the one judgement about the program
        // (`h + 2.5` decides `Double` for both `h` and `(+)`), and lowering only
        // has to read it. A concrete `expected` is the weaker, local form of the
        // same fact (an annotated reference).
        let recorded = self.dict_selections.iter().any(|((sp, _), _)| *sp == span);
        let position_solves = recorded
            || expected.is_some_and(|e| {
                let e = match e {
                    ValueTy::Thunk(_, rets) if rets.len() == 1 => rets[0].clone(),
                    other => other.clone(),
                };
                is_concrete_ty(&e)
            });
        if self.env.constrained_defs.contains_key(name)
            && !position_solves
            && !self.value_locals.iter().any(|s| s.contains_key(name))
        {
            return Err(CompileError::Type {
                msg: format!(
                    "cannot use class-constrained definition `{name}` without applying it: \
                     its class variable is solved per call site"
                ),
                span,
            });
        }
        // The position fixes the class variable: specialize the body HERE, the
        // same way a call site does. Only a NULLARY definition reaches this
        // (a reference carries no arguments, so a definition with parameters is
        // a partial application and keeps its function value).
        if self.env.constrained_defs.contains_key(name) && position_solves {
            let nullary = self
                .env
                .constrained_defs
                .get(name)
                .map(|cd| cd.params.is_empty())
                .unwrap_or(false);
            if nullary {
                if let Some(cd) = self.env.constrained_defs.get(name).cloned() {
                    if self.value_locals.iter().all(|s| !s.contains_key(name)) {
                        // Inference's decision for THIS reference wins: it is the
                        // one judgement about the program (`h + 2.5` → `Double`),
                        // and re-deriving it here from a coarser static type
                        // would be a second judgement that can disagree.
                        let inferred = self
                            .dict_selections
                            .iter()
                            .find(|((sp, _), _)| *sp == span)
                            .map(|(_, ty)| ty.clone());
                        let exp = inferred.as_ref().or(expected);
                        let (r, ty) =
                            self.lower_constrained_apply_inner(name, &cd, &[], exp, span)?;
                        return Ok((r, ty));
                    }
                }
            }
        }
        // A DEFERRED signature-less curried def has no standalone value either:
        // its body dispatches on a parameter's shape at a call site (`a ! i`
        // lowers to the constrained accessor `(!)`), so a bare
        // reference (`f = at`) would lower the absent body against an unsolved
        // class variable — the same misleading `no instance of … for Double`
        // the constrained guard reports instead. A value local or fragment
        // capture of the same name shadows the definition.
        //
        // EXCEPT in complete mode, where a recursive def lives in a shared cell
        // (the STG model): the cell's closure is compiled once from the body, so
        // the reference resolves there and needs no specialization.
        let anchored_cycle = self.mode == Mode::Complete && self.recursive_locals.contains(name);
        if self.is_deferred_sigless_def(name)
            && !anchored_cycle
            && !self.value_locals.iter().any(|s| s.contains_key(name))
            && !self.fragment_captures.iter().any(|(n, _)| n == name)
        {
            return Err(CompileError::Type {
                msg: format!(
                    "cannot use class-constrained definition `{name}` without applying it: \
                     its class variable is solved per call site"
                ),
                span,
            });
        }
        // A value binding shadows a class method — a λ-parameter in
        // `value_locals`, and equally a CAPTURE inside a nested fragment (a
        // `\ arr -> …` that names its handle `arr` must not resolve to the
        // `Arrow` class's `arr`).
        let captured = self.fragment_captures.iter().any(|(n, _)| n == name);
        if !self.defs.contains_key(name)
            && !captured
            && !self.value_locals.iter().any(|s| s.contains_key(name))
        {
            if let Some(class_name) = self.env.class_of_method(name) {
                let class_info = self.env.typeclasses.get(&class_name).cloned().unwrap();
                let class_var = class_info.var.clone();
                let sig = class_info
                    .methods
                    .iter()
                    .find(|(m, _)| m == name)
                    .map(|(_, s)| s.clone());
                let result_directed = match &sig {
                    Some(s) => crate::types::infer::sig_result_directed(s, &class_var),
                    None => false,
                };
                if let Some(sig) = sig.as_ref().filter(|_| result_directed) {
                    // Inference already decided this call's instance — that is
                    // the ONE judgement about the program, and lowering's job is
                    // to READ it. Asking for `expected` first (and failing when
                    // it is absent) made lowering re-derive a decision that had
                    // already been made: `f :: IO Int; f = return 42` infers
                    // `IO Int` correctly and then reported "expected type
                    // unknown" here, because the reference's own `expected` is
                    // not threaded through every enclosing form.
                    let recorded = self.recorded_selection(span, &class_name);
                    let exp = match recorded.as_ref().map(|(key, subst)| match subst {
                        crate::types::infer::ClassVarSubst::Ty(t) => t.clone(),
                        _ => match self.env.vty_of_name(key) {
                            Some(t) => t,
                            None => ValueTy::Double,
                        },
                    }) {
                        Some(t) => t,
                        None => match expected {
                            Some(t) => t.clone(),
                            None => {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "cannot resolve method `{name}` of `{class_name}`: expected type unknown"
                                    ),
                                    span,
                                });
                            }
                        },
                    };
                    // The full-type dispatch key: an arity-0 class with an
                    // applied-head instance (`C (Pair Int Int)`) must resolve by
                    // THAT key here too — the expected type's head name (`Pair`)
                    // names no instance.
                    let cv_var = self.next_tyvar;
                    self.next_tyvar += 1;
                    let Some((ty_name, cv_subst)) = crate::types::infer::method_dispatch_key(
                        self.env,
                        &class_name,
                        &class_var,
                        sig,
                        &exp,
                        cv_var,
                        || {
                            let v = self.next_tyvar;
                            self.next_tyvar += 1;
                            ValueTy::Var(v)
                        },
                    ) else {
                        return Err(CompileError::Type {
                            msg: format!(
                                "cannot resolve method `{name}` of `{class_name}`: the expected type is not concrete"
                            ),
                            span,
                        });
                    };
                    let (params, body) =
                        match self.instantiate_method(&class_name, name, ty_name.as_str()) {
                            Some(r) => r,
                            None => {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "no instance of `{class_name}` for type `{ty_name}`"
                                    ),
                                    span,
                                });
                            }
                        };
                    let key = (class_name, ty_name.clone(), name.to_string());
                    if self.method_lifting.contains(&key) {
                        return Err(CompileError::Type {
                            msg: format!(
                                "recursive typeclass method `{name}` for type `{ty_name}`"
                            ),
                            span,
                        });
                    }
                    self.method_lifting.insert(key.clone());
                    let mut scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
                    let mut ctor_scope: HashMap<String, Option<String>> = HashMap::new();
                    // Result-directed methods take no arguments (`mempty`) or a
                    // single element argument (`pure x`/`return x`) — bind any
                    // params to fresh registers typed from the signature (class
                    // var substituted by the dispatch key), so a body that uses
                    // a param sees the contract's type rather than a blanket
                    // `Double` slot.
                    let arg_reg = self.fresh_value_reg();
                    let param_tys = crate::types::infer::signature_param_tys_conv(
                        self.env,
                        &class_var,
                        &cv_subst,
                        sig,
                        || {
                            let v = self.next_tyvar;
                            self.next_tyvar += 1;
                            ValueTy::Var(v)
                        },
                    );
                    for (i, p) in params.iter().enumerate() {
                        let ty = param_tys.get(i).cloned().unwrap_or(ValueTy::Double);
                        scope.insert(p.clone(), (arg_reg, ty));
                        ctor_scope.insert(p.clone(), None);
                    }
                    self.value_locals.push(scope);
                    self.value_local_ctors.push(ctor_scope);
                    let res = self.lower_value_expected(&body, Some(&exp));
                    self.value_locals.pop();
                    self.value_local_ctors.pop();
                    self.method_lifting.remove(&key);
                    return res;
                }
                // A NON-result-directed method used as a first-class VALUE
                // (`fold mappend "" xs`) — Haskell 2010 §4.3.3: a class method
                // is an ordinary variable. Dispatch on the concrete type the
                // expected function signature carries (the same rule as
                // inference), then build the instance body's closure exactly
                // like a user definition. `$`-prefixed parameter names cannot
                // collide with source identifiers.
                if let (Some(sig), Some(exp)) = (sig.as_ref(), expected) {
                    if let Some((ty_name, cand_ty)) =
                        crate::types::infer::method_value_dispatch_env(self.env, exp)
                    {
                        return self.lower_method_value(
                            name,
                            &class_name,
                            &ty_name,
                            &cand_ty,
                            sig,
                            span,
                        );
                    }
                }
            }
        }
        self.lower_value_ref_inner(name, span)
    }

    /// Give an instance method's own parameters fresh reserved names and rename
    /// their bound occurrences through `body`.
    ///
    /// An instance body is spliced into the CALLER's expression, so a method
    /// parameter keeps whatever spelling the instance author chose. When that
    /// spelling is also a caller-visible name — the Prelude's `bind m f`
    /// against a user's top-level `f` — the spliced body's `f` refers to the
    /// METHOD PARAMETER but the caller's free-variable analysis and definition
    /// resolution see the user's definition, which is variable capture.
    ///
    /// Renaming to `$`-prefixed names fixes it at the source: `$` cannot occur
    /// in a source identifier, so a fresh name can never collide with a caller
    /// binding. The counter keeps every instantiation distinct, so a method
    /// instantiated twice (or a method body that instantiates another method)
    /// cannot alias either. Semantics are unchanged — only binding names move.
    /// The same technique the operator-as-value path uses for `$op_a`/`$op_b`.
    ///
    /// Returns the renamed parameter names in their original order (so callers
    /// can bind them to the argument registers they already lowered) and the
    /// renamed body.
    #[inline(never)]
    fn fresh_method_params(&mut self, body_params: &[String], body: &Expr) -> (Vec<String>, Expr) {
        let fresh = self.next_method_param;
        self.next_method_param += 1;
        let mut renames: HashMap<String, String> = HashMap::new();
        for (i, p) in body_params.iter().enumerate() {
            // `_` is the wildcard parameter (`Stmt`-style bodies): it binds
            // nothing, so renaming it would invent a binding the body never
            // refers to.
            if p == "_" {
                continue;
            }
            renames.insert(p.clone(), method_param_name(fresh, i));
        }
        let renamed = rename_bound(body, &renames);
        let names = body_params
            .iter()
            .map(|p| renames.get(p).cloned().unwrap_or_else(|| p.clone()))
            .collect();
        (names, renamed)
    }

    /// Resolve a typeclass method — the instance body, or the class DEFAULT body
    /// when the instance omits the method — and instantiate it
    /// capture-avoidingly.
    ///
    /// THE single place a method body enters the caller's expression. Returning
    /// the body with the method's own parameters already renamed means no
    /// inlining site can reintroduce the capture that
    /// [`Self::fresh_method_params`] exists to prevent: a future method-body
    /// path that calls this helper inherits the protection instead of having to
    /// remember it.
    ///
    /// Kept out of line (`#[inline(never)]`) because it sits underneath the
    /// deeply recursive `lower_value` dispatch: the extra binding it needs must
    /// not enlarge that frame.
    #[inline(never)]
    fn instantiate_method(
        &mut self,
        class_name: &str,
        name: &str,
        ty_name: &str,
    ) -> Option<(Vec<String>, Expr)> {
        let (params, body) = match self.env.resolve_method(name, ty_name) {
            Some((_, params, body)) => (params, body),
            None => self.env.class_default(class_name, name)?,
        };
        Some(self.fresh_method_params(&params, &body))
    }

    /// Lower a typeclass method used as a first-class value into a closure over
    /// its instance body, mirroring how a named definition with parameters is
    /// lowered.
    fn lower_method_value(
        &mut self,
        name: &str,
        class_name: &str,
        ty_name: &str,
        cand_ty: &ValueTy,
        sig: &crate::ast::TypeExpr,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        // A primitive instance used as a value has no source body: build the
        // curried kernel call `\ a -> \ b -> kernel a b` from the class
        // signature, so `fold (+)` receives a real function value.
        if self.is_primitive_instance(class_name, ty_name) {
            let a = "$prim_a".to_string();
            let b = "$prim_b".to_string();
            let lhs = Expr::Ref(a.clone(), span);
            let rhs = Expr::Ref(b.clone(), span);
            // Rebuild the operator call the kernel understands.
            let call = Expr::Apply {
                name: name.to_string(),
                args: vec![lhs, rhs],
                span,
            };
            let lam = Expr::Lambda {
                params: vec![Param {
                    name: a,
                    span,
                    bang: false,
                }],
                body: Box::new(Expr::Lambda {
                    params: vec![Param {
                        name: b,
                        span,
                        bang: false,
                    }],
                    body: Box::new(call),
                    span,
                }),
                span,
            };
            return self.lower_value(&lam);
        }
        let (body_params, body) = match self.instantiate_method(class_name, name, ty_name) {
            Some(r) => r,
            None => {
                return Err(CompileError::Type {
                    msg: format!("no instance of `{class_name}` for type `{ty_name}`"),
                    span,
                });
            }
        };
        let key = (
            class_name.to_string(),
            ty_name.to_string(),
            name.to_string(),
        );
        if self.method_lifting.contains(&key) {
            return Err(CompileError::Type {
                msg: format!("recursive typeclass method `{name}` for type `{ty_name}`"),
                span,
            });
        }
        // Synonymize the method's own parameters to fresh locals and build the
        // equivalent `\p1 -> \p2 -> body` lambda so the standard closure path
        // handles arity, partial application and dispatch. The class variable
        // is substituted by the CANDIDATE's full type: `instance Monoid List`
        // must see `List a`, not the arity-0 `List`.
        let cv_subst = crate::types::infer::ClassVarSubst::Ty(cand_ty.clone());
        let param_tys = crate::types::infer::signature_param_tys_conv(
            self.env,
            &self
                .env
                .typeclasses
                .get(class_name)
                .map(|c| c.var.clone())
                .unwrap_or_default(),
            &cv_subst,
            sig,
            || {
                let v = self.next_tyvar;
                self.next_tyvar += 1;
                ValueTy::Var(v)
            },
        );
        let mut lam = body;
        for p in body_params.into_iter().rev() {
            lam = Expr::Lambda {
                params: vec![Param {
                    name: p,
                    span,
                    bang: false,
                }],
                body: Box::new(lam),
                span,
            };
        }
        self.method_lifting.insert(key.clone());
        let saved_pending = self.pending_param_tys.take();
        self.pending_param_tys = Some(param_tys);
        let res = self.lower_value(&lam);
        self.pending_param_tys = saved_pending;
        self.method_lifting.remove(&key);
        res
    }

    /// Lower an arithmetic operator reference (`+`, `-`, `*`, `/`, `%`) as a
    /// first-class curried function value — Haskell 2010 Prelude: `(+)` is an
    /// ordinary function of two arguments.
    ///
    /// The operator is expanded into the equivalent `\a -> \b -> a <op> b` AST
    /// and lowered through the normal lambda path, so arity, partial
    /// application, capture and dispatch are all handled by machinery that
    /// already exists. The two parameters use the same reserved `$`-prefixed
    /// names as operator sections: `$` cannot occur in a source identifier, so
    /// they can never shadow a user binding.
    /// Lower `fold f z xs` when `f` is a bare METHOD reference
    /// (`fold mappend "" xs`): the seed and the source are lowered first so the
    /// method can be dispatched against its `b -> a -> b` signature. Kept out of
    /// line so the enclosing `lower_value` frame does not grow.
    fn lower_fold_with_method(
        &mut self,
        call_args: &[Expr],
    ) -> Result<(Vec<usize>, Vec<ValueTy>), CompileError> {
        let (zr, zt) = self.lower_value(&call_args[1])?;
        let (lr, lt) = self.lower_value(&call_args[2])?;
        let elem = list_type_args(&lt)
            .and_then(|inner| inner.first().cloned())
            .unwrap_or(ValueTy::Double);
        let fexp = ValueTy::Thunk(vec![zt.clone(), elem], vec![zt.clone()]);
        let (fr, ft) = self.lower_value_expected(&call_args[0], Some(&fexp))?;
        Ok((vec![fr, zr, lr], vec![ft, zt, lt]))
    }

    /// Whether `(class, ty)` is a PRIMITIVE instance: its method bodies are
    /// compiler kernels rather than source equations (the numeric primitives,
    /// the buffer arithmetic instances, the structural `Eq`/`Ord`).
    fn is_primitive_instance(&self, class_name: &str, ty_name: &str) -> bool {
        self.env
            .instances
            .get(class_name)
            .and_then(|m| m.get(ty_name))
            .map(|i| i.primitive)
            .unwrap_or(false)
    }

    /// Compile one dictionary method slot to a fragment: the instance's
    /// equation, or the class default when the instance omits it.
    ///
    /// The fragment is the method's curried body, taking the method's declared
    /// arguments as its leading value registers — exactly the shape
    /// [`Value::Callable`](crate::arena::Value::Callable) dispatches, so a
    /// projected slot needs no new call machinery.
    ///
    /// Returns `Err(reason)` when the body cannot be compiled as a standalone
    /// fragment. The one real limitation: a body with a FREE variable would
    /// need a creation site to snapshot the capture, and a dictionary is built
    /// once at seat with no enclosing scope. The caller records that as a
    /// [`DictGap`](crate::ir::DictGap) instead of emitting a slot that would
    /// read an absent cell.
    fn compile_dict_method(
        &mut self,
        plan: &crate::dicts::DictPlan,
        method: &str,
    ) -> Result<u32, String> {
        let Some((body_params, body)) =
            crate::dicts::body_of(self.env, &plan.class, &plan.ty_key, method)
        else {
            return Err(format!(
                "no body and no class default for method `{method}`"
            ));
        };
        // A slot must be a fragment taking the method's arguments in its leading
        // value registers — that is the shape `DictSlot::Fragment` promises
        // (design §3.1: "the same kind of index a `Value::Callable` uses").
        //
        // An instance EQUATION carries its parameters explicitly, but a class
        // DEFAULT body is stored as the parser curried it: a λ-nest with an
        // empty parameter list (`min = \x -> \y -> …`). Lowering that as-is
        // would produce a zero-argument fragment returning a closure, so the
        // λ-nest is peeled into the slot's parameters first, up to the method's
        // declared arity.
        let arity = self
            .env
            .typeclasses
            .get(&plan.class)
            .and_then(|c| c.methods.iter().find(|(m, _)| m == method))
            .map(|(_, s)| s.sig_arg_count())
            .unwrap_or(body_params.len());
        let (body_params, body) = if body_params.is_empty() {
            let (peeled, peeled_body) = crate::types::ty::split_curried_params(arity, &[], &body);
            let names: Vec<String> = peeled.iter().map(|p| p.name.clone()).collect();
            (names, peeled_body)
        } else {
            (body_params, body)
        };
        // Capture-avoiding rename, exactly as `instantiate_method` does: a
        // method's own parameter name must never collide with a name at the
        // (absent) call site.
        let (names, body) = self.fresh_method_params(&body_params, &body);
        let sig = self
            .env
            .typeclasses
            .get(&plan.class)
            .and_then(|c| c.methods.iter().find(|(m, _)| m == method))
            .map(|(_, s)| s.clone());
        // Which substitution the method's own signature is typed under mirrors
        // the registry's own dispatch: an arity-0 class solved its variable to
        // the FULL concrete type, a constructor class only to the head.
        let subst = if plan.class_arity == 0 {
            crate::types::infer::ClassVarSubst::Ty(plan.ty.clone())
        } else {
            crate::types::infer::ClassVarSubst::Ctor(plan.ty_key.clone())
        };
        let (param_tys, result_ty) = match &sig {
            Some(sig) => crate::types::infer::signature_param_and_result_tys_conv(
                self.env,
                &plan.class_var,
                &subst,
                sig,
                || {
                    let v = self.next_tyvar;
                    self.next_tyvar += 1;
                    ValueTy::Var(v)
                },
            ),
            None => (vec![ValueTy::Double; names.len()], None),
        };
        let span = crate::error::Span::new(0, 0);
        let params: Vec<Param> = names
            .iter()
            .map(|n| Param {
                name: n.clone(),
                span,
                bang: false,
            })
            .collect();
        let bound: HashSet<String> = names.iter().cloned().collect();
        let free = self.free_vars(&body, &bound);
        if !free.is_empty() {
            return Err(format!(
                "method `{method}`'s body captures {} — a dictionary is built at seat \
                 with no enclosing scope to snapshot the capture",
                free.join(", ")
            ));
        }
        let saved_pending = self.pending_param_tys.take();
        self.pending_param_tys = Some(param_tys.clone());
        let frag = self.lower_fragment(
            &params,
            &body,
            &free,
            &[],
            &param_tys,
            result_ty.as_ref(),
            span,
        );
        self.pending_param_tys = saved_pending;
        let (fragment, _ret) = frag.map_err(|e| format!("method `{method}`: {e}"))?;
        Ok(fragment as u32)
    }

    /// Turn the dictionary plan into IR descriptors: one
    /// [`DictDesc`](crate::ir::DictDesc) per buildable plan entry, in plan
    /// (topological) order, with each `Body` slot compiled to a fragment.
    ///
    /// A dictionary is emitted **whole or not at all**: if any slot cannot be
    /// compiled, or any superclass dictionary is missing, the pair becomes a
    /// [`DictGap`](crate::ir::DictGap) instead. A half-filled vtable would be a
    /// silent hole, which is exactly what this stage exists to prevent.
    fn build_dict_descriptors(
        &mut self,
        plan: &crate::dicts::Plan,
    ) -> (Vec<crate::ir::DictDesc>, Vec<crate::ir::DictGap>) {
        let mut gaps = plan.gaps.clone();
        let mut dicts: Vec<crate::ir::DictDesc> = Vec::new();
        // Plan index → descriptor index, for resolving `supers`.
        let mut desc_index: HashMap<usize, usize> = HashMap::new();
        for (plan_idx, entry) in plan.dicts.iter().enumerate() {
            // A dictionary is emitted whole or not at all. Code fragments
            // compiled for a descriptor that is then dropped are unreachable
            // (only its own slots could name them), so the fragment table is
            // rewound to here: leaving them would grow the arena/call-scratch
            // budget for code nothing can call.
            let frag_mark = self.fragments.len();
            // Each superclass is identified by the same `(class, registry key)`
            // pair the plan produced it from, so a superclass dictionary is
            // resolved by identity rather than by a second type comparison. The
            // plan orders supers first, so a missing one means that pair was
            // dropped as a gap.
            let super_indexes: Vec<usize> = entry
                .supers
                .iter()
                .filter_map(|sup| {
                    plan.dicts
                        .iter()
                        .position(|p| (&p.class, &p.ty_key) == (&sup.0, &sup.1))
                        .and_then(|i| desc_index.get(&i).copied())
                })
                .collect();
            if super_indexes.len() != entry.supers.len() {
                // Name the superclass that is missing, so the gap points at the
                // pair whose own gap entry explains why.
                let missing = entry
                    .supers
                    .iter()
                    .find(|(sup_class, sup_key)| {
                        plan.dicts
                            .iter()
                            .position(|p| (&p.class, &p.ty_key) == (sup_class, sup_key))
                            .and_then(|i| desc_index.get(&i))
                            .is_none()
                    })
                    .map(|(sup_class, sup_key)| format!("`{sup_class} {sup_key}`"))
                    .unwrap_or_else(|| "a superclass".to_string());
                gaps.push(crate::ir::DictGap {
                    class: entry.class.clone(),
                    ty_key: Some(entry.ty_key.clone()),
                    reason: format!(
                        "the superclass dictionary {missing} could not be built, so this \
                         dictionary's superclass field would dangle"
                    ),
                });
                continue;
            }
            let mut methods = Vec::with_capacity(entry.slots.len());
            let mut failure: Option<String> = None;
            for (slot, method) in entry.slots.iter().zip(&entry.method_names) {
                match slot {
                    crate::dicts::SlotKind::Kernel(kernel) => {
                        methods.push(crate::arena::DictSlot::Kernel(*kernel));
                    }
                    crate::dicts::SlotKind::Body => match self.compile_dict_method(entry, method) {
                        Ok(fragment) => {
                            methods.push(crate::arena::DictSlot::Fragment { fragment });
                        }
                        Err(reason) => {
                            failure = Some(reason);
                            break;
                        }
                    },
                }
            }
            if let Some(reason) = failure {
                self.fragments.truncate(frag_mark);
                gaps.push(crate::ir::DictGap {
                    class: entry.class.clone(),
                    ty_key: Some(entry.ty_key.clone()),
                    reason,
                });
                continue;
            }
            desc_index.insert(plan_idx, dicts.len());
            dicts.push(crate::ir::DictDesc {
                class: entry.class.clone(),
                ty_key: entry.ty_key.clone(),
                is_buffer: entry.is_buffer,
                supers: super_indexes,
                methods,
                method_names: entry.method_names.clone(),
            });
        }
        (dicts, gaps)
    }

    /// Lower a call to a primitive instance's method by emitting the
    /// corresponding kernel. This is the System FC "primitive dictionary": the
    /// instance has no equations, so the (class, type, method) triple selects
    /// the kernel.
    ///
    /// Covers the operators the typeclass layer dispatches today:
    /// `Eq`/`Ord` comparison over the structural order, `Num`/`Fractional`
    /// scalar arithmetic, and buffer arithmetic (which the existing buffer fork
    /// emitted before the class layer existed).
    /// `prefetched` carries an argument the dispatch step ALREADY lowered —
    /// `(index, register, static type)` — so it is not lowered a second time.
    /// Dispatch has to lower the selector argument to get its static type, and
    /// re-lowering it for the kernel would emit its instructions twice: a closed
    /// CAF operand (`osc = sine …`) would instantiate its generator twice in one
    /// expression, and an effectful operand would run its effects twice.
    /// The instance the type checker SELECTED for the method call at `span`, as
    /// the dispatch key and its substitution.
    ///
    /// The recorded EVIDENCE is the resolved class variable (`Pair Int Int` for
    /// `pick 1.0 ((0,0),(2,2))`), not the selector argument's type — so it is
    /// used as the class-variable substitution directly, and the registry key is
    /// the one that substitution renders as.
    ///
    /// Inference solves the class variable with its full substitution; lowering
    /// has none, so its own static type for the same operand can be coarser (`x`
    /// bound by `Just x` from a `Maybe a` scrutinee stays a `Var` here while
    /// inference solved it to `Double`). Re-deriving the instance from the
    /// coarser type would reject a program that type-checked, so the recorded
    /// selection is consulted FIRST — it is the same judgement, recorded once.
    ///
    /// Looked up by span AND class: two selections can share one operator span
    /// (`0 == (x + 0)`), and asking for `Eq` must not find the inner `Num`
    /// record.
    fn recorded_selection(
        &mut self,
        span: Span,
        class_name: &str,
    ) -> Option<(String, crate::types::infer::ClassVarSubst)> {
        let evidence = self
            .dict_selections
            .get(&(span, class_name.to_string()))?
            .clone();
        // A GIVEN witness (`Dict("Ord")`) has no concrete instance to lower
        // here; those flows keep their existing path.
        if matches!(evidence, ValueTy::Dict(_)) {
            return None;
        }
        let key = crate::types::ty::value_type_key(self.env, &evidence)
            .or_else(|| self.env.type_name_of_vty(&evidence))?;
        if !self
            .env
            .instances
            .get(class_name)
            .map(|by_ty| by_ty.contains_key(key.as_str()))
            .unwrap_or(false)
        {
            return None;
        }
        Some((key, crate::types::infer::ClassVarSubst::Ty(evidence)))
    }

    /// Lower a newtype constructor application (`Hz 440.0` → one
    /// `ValueNewtype` wrapper).
    ///
    /// The newtype's declared target type is the ONE argument's expected type:
    /// `newtype PBox = Point; PBox (Point { x: 9223372036854775808, … })` only
    /// reaches `Point`'s declared field types through it, exactly as a
    /// binding's own `::` does for its body. A target that resolves to nothing
    /// concrete (a type variable) supplies NO expectation, so no guessed type
    /// can convert the argument's literal.
    ///
    /// Kept out of line so the (already deep) `lower_value` dispatch frame does
    /// not grow — a nested 2-D `assocs`/`listArray` program overflowed the
    /// stack when this branch was inlined there.
    #[inline(never)]
    fn lower_newtype_constructor(
        &mut self,
        name: &str,
        call_args: &[Expr],
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        if call_args.len() != 1 {
            return Err(CompileError::Type {
                msg: format!(
                    "newtype constructor `{name}` expects 1 argument, got {}",
                    call_args.len()
                ),
                span,
            });
        }
        let target = self.env.newtypes.get(name).cloned().unwrap_or_default();
        let expected_arg = self.env.vty_of_name(&target).filter(is_concrete_ty);
        let (src, _) = match &expected_arg {
            Some(exp) => self.lower_value_expected(&call_args[0], Some(exp))?,
            None => self.lower_value(&call_args[0])?,
        };
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueNewtype { dst, src });
        Ok((dst, ValueTy::Newtype(name.to_string(), vec![])))
    }

    /// Rebind a primitive method's INSTANCE from the caller's expectation.
    ///
    /// The instance is normally selected by the selector argument's static type.
    /// For a DEFERRED operand that type may still be a variable, and lowering's
    /// defaulting then picks `Integer` while the position demands `Double` —
    /// the mixed-kind kernel fault `g 0 + 2.5` produced.
    ///
    /// Haskell 2010 gives the position a vote too (measured: GHC passes `g ::
    /// Num b => Int -> b` the very `$dNumDouble` that `(+)` selected), so a
    /// CONCRETE expectation that satisfies the class wins over a defaulted
    /// selector type. A non-satisfying expectation is not silently accepted —
    /// it simply does not override, and the selector's own judgement stands.
    fn instance_from_expectation(
        &self,
        class_name: &str,
        expected: Option<&ValueTy>,
        ty_name: &str,
        cv_subst: crate::types::infer::ClassVarSubst,
    ) -> (String, crate::types::infer::ClassVarSubst) {
        let Some(exp) = expected else {
            return (ty_name.to_string(), cv_subst);
        };
        // A function expectation carries its result in the LAST slot.
        let exp = match exp {
            ValueTy::Thunk(_, rets) if rets.len() == 1 => rets[0].clone(),
            other => other.clone(),
        };
        if !is_concrete_ty(&exp) || !self.env.instance_exists(class_name, &exp) {
            return (ty_name.to_string(), cv_subst);
        }
        let Some(key) = crate::types::ty::value_type_key(self.env, &exp)
            .or_else(|| self.env.type_name_of_vty(&exp))
        else {
            return (ty_name.to_string(), cv_subst);
        };
        (key, crate::types::infer::ClassVarSubst::Ty(exp))
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_primitive_method(
        &mut self,
        class_name: &str,
        name: &str,
        ty_name: &str,
        call_args: &[Expr],
        span: Span,
        prefetched: Option<(usize, usize, ValueTy)>,
        param_tys: &[ValueTy],
    ) -> Result<(usize, ValueTy), CompileError> {
        let (prefetched, param_tys) = (prefetched, param_tys);
        self.lower_primitive_method_inner(
            class_name, name, ty_name, call_args, span, prefetched, param_tys,
        )
    }

    /// [`Self::lower_primitive_method`]'s body.
    ///
    /// Split out so the caller can rebind the instance from the SOLVED
    /// parameter types before the kernel is chosen — see
    /// [`Self::lower_primitive_method_with_solved_instance`].
    #[allow(clippy::too_many_arguments)]
    fn lower_primitive_method_inner(
        &mut self,
        class_name: &str,
        name: &str,
        ty_name: &str,
        call_args: &[Expr],
        span: Span,
        prefetched: Option<(usize, usize, ValueTy)>,
        param_tys: &[ValueTy],
    ) -> Result<(usize, ValueTy), CompileError> {
        // A kernel consumes exactly the signature's arguments. A SURPLUS
        // application is an arity error (Haskell 2010 §4.3.3), never a silently
        // dropped argument: `fromInteger 1 2` must not lower a kernel that
        // takes one. Fewer arguments is a partial application, which the call
        // sites turn into a curried closure before reaching here.
        if call_args.len() > param_tys.len() {
            return Err(CompileError::Type {
                msg: format!(
                    "method `{name}` of `{class_name}` expects {} argument(s), got {}",
                    param_tys.len(),
                    call_args.len()
                ),
                span,
            });
        }
        // Lower the arguments once; a primitive kernel consumes registers. Each
        // argument is lowered with its SIGNATURE parameter type as the expected
        // type: the instance key already solved the class variable, so the
        // parameter type is concrete (`Num Integer` → `Integer -> Integer ->
        // Integer`). Without it a bare literal in an `Integer` position lowered
        // as a `ValueConstInt`, and the kernel then saw `[Integer, Int]` — the
        // comparison silently degraded to the cross-kind total order and the
        // arithmetic latched its mixed-kind fault.
        let mut arg_regs: Vec<usize> = Vec::with_capacity(call_args.len());
        let mut arg_tys: Vec<ValueTy> = Vec::with_capacity(call_args.len());
        for (i, a) in call_args.iter().enumerate() {
            let expected = param_tys.get(i);
            // The prefetched register is a fast path, NOT an authority: the
            // instance's solved parameter type decides the argument's
            // representation. A selector literal (`0 == x`) is lowered before
            // dispatch with no expected type, so its recorded type can disagree
            // with the parameter it flows into. The solved parameter types are
            // concrete here (`Num Integer` → `Integer -> Integer -> Integer`),
            // so a plain `==` is the whole test; there is no widening relation
            // between `Int` and `Integer` to consider.
            if let Some((idx, reg, ty)) = &prefetched {
                if *idx == i && expected.is_none_or(|e| e == ty) {
                    arg_regs.push(*reg);
                    arg_tys.push(ty.clone());
                    continue;
                }
            }
            let (r, t) = match expected {
                Some(exp) => self.lower_value_expected(a, Some(exp))?,
                None => self.lower_value(a)?,
            };
            arg_regs.push(r);
            arg_tys.push(t);
        }
        // The ONE `(class, method) → kernel` table also consulted by dictionary
        // construction; a second copy of this decision is the defect the
        // dictionary work removes.
        let Some(kernel) = crate::kernels::kernel_for_method(class_name, name) else {
            // `min`/`max` from `Ord`'s defaults are NOT primitive: they have
            // real default bodies and go through the ordinary method path.
            return Err(CompileError::Type {
                msg: format!(
                    "primitive instance `{class_name} {ty_name}` has no kernel for method `{name}`"
                ),
                span,
            });
        };
        match kernel {
            // Comparison: `==` `/=`, `<` `<=` `>` `>=` over the structural total
            // order.
            KernelId::Eq
            | KernelId::Ne
            | KernelId::Lt
            | KernelId::Gt
            | KernelId::Le
            | KernelId::Ge => {
                if arg_regs.len() < 2 {
                    return Err(CompileError::Type {
                        msg: format!("`{name}` expects 2 arguments, got {}", arg_regs.len()),
                        span,
                    });
                }
                let op = match kernel {
                    KernelId::Eq => CmpOp::Eq,
                    KernelId::Ne => CmpOp::Ne,
                    KernelId::Lt => CmpOp::Lt,
                    KernelId::Gt => CmpOp::Gt,
                    KernelId::Le => CmpOp::Le,
                    KernelId::Ge => CmpOp::Ge,
                    _ => unreachable!("the comparison kernels are the only ones in this arm"),
                };
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueCompare {
                    dst,
                    op,
                    a: arg_regs[0],
                    b: arg_regs[1],
                });
                Ok((dst, ValueTy::bool_ty()))
            }
            // Ring arithmetic: scalar (`ValueAdd` family) or whole-buffer
            // element-wise (`ValueBufferBin`), decided by the instance's kind.
            // A buffer is an ordinary value; the kernel is its dictionary entry.
            // Scalar operands materialize as buffer constants.
            arith_op @ (KernelId::Add | KernelId::Sub | KernelId::Mul | KernelId::Div) => {
                if arg_regs.len() < 2 {
                    return Err(CompileError::Type {
                        msg: format!("`{name}` expects 2 arguments, got {}", arg_regs.len()),
                        span,
                    });
                }
                // The instance key is the full applied head (`FixedBuffer
                // Double`), so the selector asks the registry whether the key
                // names a buffer rather than matching a spelling list.
                if self.env.instance_key_is_buffer(ty_name) {
                    let op = match arith_op {
                        KernelId::Add => BinArith::Add,
                        KernelId::Sub => BinArith::Sub,
                        KernelId::Mul => BinArith::Mul,
                        KernelId::Div => BinArith::Div,
                        _ => unreachable!("only the arithmetic kernels reach here"),
                    };
                    let a = self.buffer_operand(arg_regs[0], &arg_tys[0])?;
                    let b = self.buffer_operand(arg_regs[1], &arg_tys[1])?;
                    let dst = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueBufferBin { dst, op, a, b });
                    return Ok((dst, self.env.buffer_vty()));
                }
                // The result track follows the operands' ONE numeric type: two
                // `Int`s stay `Int`, two `Integer`s stay `Integer`, everything
                // else is `Double`. The two arguments are NOT typed
                // independently: the solved instance fixes the class variable,
                // so both are lowered with the SAME signature parameter type
                // (`Num a => a -> a -> a` applied at `Integer` gives
                // `Integer -> Integer -> Integer`). The pair is therefore
                // same-kind here, and the `Double` arm is the float track — not
                // a widening of a mixed pair.
                let ret = match (arg_tys.first(), arg_tys.get(1)) {
                    (Some(ValueTy::Int), Some(ValueTy::Int)) => ValueTy::Int,
                    (Some(ValueTy::Integer), Some(ValueTy::Integer)) => ValueTy::Integer,
                    _ => ValueTy::Double,
                };
                let dst = self.fresh_value_reg();
                let (a, b) = (arg_regs[0], arg_regs[1]);
                self.emit_value(match arith_op {
                    KernelId::Add => ValueInstr::ValueAdd { dst, a, b },
                    KernelId::Sub => ValueInstr::ValueSub { dst, a, b },
                    KernelId::Mul => ValueInstr::ValueMul { dst, a, b },
                    KernelId::Div => ValueInstr::ValueDiv { dst, a, b },
                    _ => unreachable!("only the arithmetic kernels reach here"),
                });
                Ok((dst, ret))
            }
            // The unary numeric kernels: `negate`, `abs`, `signum` and `recip`.
            // ONE instruction each, whose EXECUTION dispatches on the operand
            // kind — the same shape `+` has. A buffer operand lowers to the same
            // instruction (the interpreter runs the whole-block slice path); no
            // branch on the instance's kind is needed, and the by-name `abs`
            // branch is gone.
            unary @ (KernelId::Negate | KernelId::Abs | KernelId::Signum | KernelId::Recip) => {
                if arg_regs.is_empty() {
                    return Err(CompileError::Type {
                        msg: format!("`{name}` expects 1 argument, got 0"),
                        span,
                    });
                }
                let (src, src_ty) = (arg_regs[0], &arg_tys[0]);
                // A buffer instance's operand is a buffer; `buffer_operand`
                // materializes a scalar into a constant block when the register
                // arrived without a buffer handle, so the instruction always
                // receives a real `Value::Buffer`.
                let src = if self.env.instance_key_is_buffer(ty_name) {
                    self.buffer_operand(src, src_ty)?
                } else {
                    src
                };
                // `recip` is `Fractional`'s: its result is a `Double` (there is
                // no `Fractional Int`/`Integer`), and a buffer `Fractional`
                // instance does not exist yet, so a buffer operand here is a
                // type error rather than a silent scalar.
                let ret = if matches!(unary, KernelId::Recip) {
                    ValueTy::Double
                } else {
                    // The result keeps the OPERAND's track, and a buffer's track
                    // is the buffer.
                    match src_ty {
                        ValueTy::Int => ValueTy::Int,
                        ValueTy::Integer => ValueTy::Integer,
                        ValueTy::Buffer(_) => self.env.buffer_vty(),
                        _ => ValueTy::Double,
                    }
                };
                // The instance key is the authority for a buffer: the solved
                // instance fixes the type, and `arg_tys` may be a `Var` the
                // lowering could not resolve. `instance_key_is_buffer` asks the
                // registry, so no spelling list lives here.
                let ret = if self.env.instance_key_is_buffer(ty_name)
                    && !matches!(unary, KernelId::Recip)
                {
                    self.env.buffer_vty()
                } else {
                    ret
                };
                let dst = self.fresh_value_reg();
                self.emit_value(match unary {
                    KernelId::Negate => ValueInstr::ValueNegate { dst, src },
                    KernelId::Abs => ValueInstr::ValueAbs { dst, src },
                    KernelId::Signum => ValueInstr::ValueSignum { dst, src },
                    KernelId::Recip => ValueInstr::ValueRecip { dst, src },
                    _ => unreachable!("only the unary numeric kernels reach here"),
                });
                Ok((dst, ret))
            }
            // `compare a b`: the three-way order as an `Int` (recorded
            // divergence A8 — Haskell 2010 returns `Ordering`, which rill does
            // not have yet).
            KernelId::Compare => {
                if arg_regs.len() < 2 {
                    return Err(CompileError::Type {
                        msg: format!("`{name}` expects 2 arguments, got {}", arg_regs.len()),
                        span,
                    });
                }
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueCompare3 {
                    dst,
                    a: arg_regs[0],
                    b: arg_regs[1],
                });
                Ok((dst, ValueTy::Int))
            }
            // `fromInteger n`: widen the literal's EXACT `Integer` to the
            // instance's type. The instance decided the type, so `ty_name` is
            // authoritative — an unbound `dst` cannot tell `Int` from `Integer`
            // from `Double`.
            //
            // `Int` goes through the conversion too: Haskell 2010's
            // `fromInteger :: Integer -> Int` is a TRUNCATION, so
            // `123456789012345678901234567890 :: Int` must keep GHC's low-64-bit
            // answer. `literal_to_i64` remains the ONE truncation boundary; the
            // instruction is its single execution site (formerly the literal's
            // own `ValueConstInt` emission, which truncated before the target
            // was known).
            //
            // A `Ratio` target does not reach this kernel: its `fromInteger` is
            // a SOURCE equation, so a ratio literal is built by record
            // construction in the instance body.
            KernelId::FromInteger => {
                if arg_regs.is_empty() {
                    return Err(CompileError::Type {
                        msg: format!("`{name}` expects 1 argument, got 0"),
                        span,
                    });
                }
                let src = arg_regs[0];
                let dst = self.fresh_value_reg();
                let is_buffer = self.env.instance_key_is_buffer(ty_name);
                let target = if is_buffer {
                    crate::ir::ValueTyTag::Buffer
                } else {
                    match ty_name.split(' ').next().unwrap_or(ty_name) {
                        "Int" => crate::ir::ValueTyTag::Int,
                        "Integer" => crate::ir::ValueTyTag::Integer,
                        _ => crate::ir::ValueTyTag::Double,
                    }
                };
                let ret = match target {
                    crate::ir::ValueTyTag::Int => ValueTy::Int,
                    crate::ir::ValueTyTag::Integer => ValueTy::Integer,
                    crate::ir::ValueTyTag::Buffer => self.env.buffer_vty(),
                    crate::ir::ValueTyTag::Double => ValueTy::Double,
                };
                self.emit_value(ValueInstr::ValueFromInteger { dst, src, target });
                Ok((dst, ret))
            }
            // `fromRational r`: the exact `Rational` operand converted to the
            // instance's type. `Double` is the one builtin target (the buffer
            // case is the whole-block constant), and a `Ratio` target never
            // reaches a kernel — its `fromRational` is the identity source
            // equation, so the instance body runs instead.
            KernelId::FromRational => {
                if arg_regs.is_empty() {
                    return Err(CompileError::Type {
                        msg: format!("`{name}` expects 1 argument, got 0"),
                        span,
                    });
                }
                let src = arg_regs[0];
                let dst = self.fresh_value_reg();
                let target = if self.env.instance_key_is_buffer(ty_name) {
                    crate::ir::ValueTyTag::Buffer
                } else {
                    crate::ir::ValueTyTag::Double
                };
                let ret = match target {
                    crate::ir::ValueTyTag::Buffer => self.env.buffer_vty(),
                    _ => ValueTy::Double,
                };
                self.emit_value(ValueInstr::ValueFromRational { dst, src, target });
                Ok((dst, ret))
            } // No catch-all arm: this match is EXHAUSTIVE over `KernelId`, so a
              // future kernel variant fails to COMPILE here (non-exhaustive
              // pattern) instead of being silently mis-lowered at run time. That
              // is the same guarantee `has_instruction` gives with its
              // exhaustive match, and it is strictly earlier and louder than a
              // "no IR instruction yet" error. Design §3.9's gap list is empty,
              // so there is no kernel left to report.
        }
    }

    fn lower_operator_value(
        &mut self,
        op: &str,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        let a = "$op_a".to_string();
        let b = "$op_b".to_string();
        let lhs = Expr::Ref(a.clone(), span);
        let rhs = Expr::Ref(b.clone(), span);
        let arith = Expr::Apply {
            name: op.to_string(),
            args: vec![lhs, rhs],
            span,
        };
        let inner = Expr::Lambda {
            params: vec![Param {
                name: b,
                span,
                bang: false,
            }],
            body: Box::new(arith),
            span,
        };
        let outer = Expr::Lambda {
            params: vec![Param {
                name: a,
                span,
                bang: false,
            }],
            body: Box::new(inner),
            span,
        };
        self.lower_value(&outer)
    }

    /// The original [`Self::lower_value_ref`] body.
    fn lower_value_ref_inner(
        &mut self,
        name: &str,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        for scope in self.value_locals.iter().rev() {
            if let Some(&(reg, ref vty)) = scope.get(name) {
                return Ok((reg, vty.clone()));
            }
        }
        // An arithmetic operator used as a VALUE (`(+)`, `fold (+) 0 xs`,
        // `inc = (+) 1.0`): Haskell 2010 Prelude — `(+) :: a -> a -> a` is an
        // ordinary curried function, not only infix sugar. Build the equivalent
        // `\a -> \b -> a + b` — the SAME application the parser produces for the
        // infix spelling — so it flows through the closure machinery as any
        // other function value (arity, partial application and dispatch are then
        // already implemented). A USER definition of the same name wins: it is a
        // real binding, so it is handled below. (`%` is not here: it was removed
        // in `218a981` — Haskell 2010 has no `(%)` in the Prelude.)
        if matches!(name, "+" | "-" | "*" | "/") && !self.defs.contains_key(name) {
            return self.lower_operator_value(name, span);
        }
        // Bare zero-argument collection constructors: `list`, `empty_map`,
        // `empty_set` in value position are empty-container builtin calls (the
        // capacity argument was removed — open collections).
        if let Some(op) = match name {
            "list" => Some(ValueBuiltinOp::ListEmpty),
            "empty_map" => Some(ValueBuiltinOp::MapEmpty),
            "empty_set" => Some(ValueBuiltinOp::SetEmpty),
            _ => None,
        } {
            let dst = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueCallBuiltin {
                dst,
                op,
                args: vec![],
            });
            let ret = match op {
                ValueBuiltinOp::ListEmpty => ValueTy::Con("List".into(), vec![ValueTy::Double]),
                ValueBuiltinOp::MapEmpty => {
                    ValueTy::Con("Map".into(), vec![ValueTy::String, ValueTy::Double])
                }
                _ => ValueTy::Con("Set".into(), vec![ValueTy::Double]),
            };
            self.note_container(&ret);
            return Ok((dst, ret));
        }
        // The realized stdio handles: opaque stream handles the runner's World
        // owns. Programs cannot construct opaque values, so each handle is a
        // lowering-time constant (`stdout`/`stderr` are output sinks, `stdin`
        // the line-oriented console source).
        let stdio = match name {
            "stdout" => Some((
                crate::program::STDOUT_HANDLE,
                ValueTy::Con("OutputStream".into(), vec![ValueTy::String]),
            )),
            "stderr" => Some((
                crate::program::STDERR_HANDLE,
                ValueTy::Con("OutputStream".into(), vec![ValueTy::String]),
            )),
            "stdin" => Some((
                crate::program::STDIN_HANDLE,
                ValueTy::Con("InputStream".into(), vec![ValueTy::String]),
            )),
            _ => None,
        };
        if let Some((handle, ty)) = stdio {
            let dst = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueConstOpaque { dst, handle });
            return Ok((dst, ty));
        }
        if let Some(idx) = self.fragment_captures.iter().position(|(f, _)| f == name) {
            // A free variable of the enclosing lambda: read it from the call's
            // env frame. `idx` is the name's position in the fragment's capture
            // list, which `run_call` binds as frame cell `idx` (the env
            // Record field order == capture order). The static type is the
            // capture's type at the closure's definition site (a captured
            // record parameter stays its `Data` type, so a field projection /
            // method call inside the fragment resolves).
            let ty = self.fragment_captures[idx].1.clone();
            let dst = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueReadCell { dst, cell: idx });
            return Ok((dst, ty));
        }
        if let Some(fsig) = self.env.foreign_sigs.get(name).cloned() {
            if let Some(sig) = crate::types::ffi::ffi_sig_from_typeexpr(self.env, name, &fsig) {
                if sig
                    .params
                    .iter()
                    .all(|p| matches!(p, crate::types::ffi::FfiParam::Signal(_)))
                    && sig.signal_outs > 0
                {
                    // A bare FFI ref (`integrator`) is a first-class Buffer
                    // arrow (`Buffer -> … -> Buffer`): bake an instance with no
                    // scalars, all Buffer slots open, and return the Callable.
                    let instance = self.builtins.len();
                    self.builtins.push(BuiltinInstance {
                        name: name.to_string(),
                        params: Vec::new(),
                        resource: None,
                        tape_index: None,
                        kind: crate::builtin::BuiltinKind::Block,
                        signal_ins: sig.params.len(),
                        signal_outs: sig.signal_outs,
                        param_bindings: Vec::new(),
                    });
                    let (callee_reg, _) =
                        self.build_ffi_arrow(instance, sig.params.len(), sig.signal_outs)?;
                    return Ok((
                        callee_reg,
                        ValueTy::Thunk(
                            vec![self.env.buffer_vty(); sig.params.len()],
                            vec![self.env.buffer_vty(); sig.signal_outs],
                        ),
                    ));
                }
            }
        }
        if let Some(&idx) = self.param_names.get(name) {
            let dst = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueReadParam { dst, idx });
            return Ok((dst, ValueTy::Double));
        }
        if self.env.data_types.contains_key(name) {
            return Err(CompileError::Type {
                msg: format!("`{name}` is a data type, not a value"),
                span,
            });
        }
        if self.env.newtypes.contains_key(name) {
            return Err(CompileError::Type {
                msg: format!("`{name}` is a newtype; use its constructor `{name} <value>`"),
                span,
            });
        }
        // Nullary sum constructor: a bare `Nothing` / `Red` in value position is
        // a complete value of its sum type — construct the empty sum directly.
        if let Some((sum_name, ctor_idx, payload)) = self.sum_ctor(name) {
            if payload.is_empty() {
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstructSum {
                    dst,
                    ctor: ctor_idx as u32,
                    payload: vec![],
                });
                return Ok((dst, ValueTy::Con(sum_name, vec![])));
            }
            // A non-nullary constructor in value position is an ordinary CURRIED
            // function (Haskell 2010 §4.2.1): `Just :: a -> Maybe a`, so `g =
            // Just` and `map Just xs` are values. Build the equivalent
            // `\p0 -> … -> C p0 …` and let the closure machinery handle it —
            // arity, partial application and dispatch already live there. The
            // `$`-prefixed parameters cannot collide with a source identifier.
            let names: Vec<String> = (0..payload.len()).map(|i| format!("$ctor_p{i}")).collect();
            let construct = Expr::Apply {
                name: name.to_string(),
                args: names.iter().map(|p| Expr::Ref(p.clone(), span)).collect(),
                span,
            };
            let mut lam = construct;
            for p in names.into_iter().rev() {
                lam = Expr::Lambda {
                    params: vec![Param {
                        name: p,
                        span,
                        bang: false,
                    }],
                    body: Box::new(lam),
                    span,
                };
            }
            return self.lower_value(&lam);
        }
        let def = self
            .defs
            .get(name)
            .cloned()
            .ok_or_else(|| CompileError::Type {
                msg: format!("unknown `{name}` in value lowering"),
                span,
            })?;
        match def {
            Def::Eval { body, bang, .. } if def.arity() == 0 => {
                // Complete mode: a RECURSIVE local definition (`fibs = cons …
                // fibs …`) is a self-referential THUNK in a shared cell (the
                // STG model) — the same shared-cell mechanism as Anchor
                // closures. Detected by the def participating in a reference
                // cycle (self- OR mutual-recursion: `a = a`, `a = b; b = a`);
                // the recursion resolves through the cell, not inline.
                if self.mode == Mode::Complete && self.recursive_locals.contains(name) {
                    let reg = self.compile_anchor_closure(name, span)?;
                    let ty = self
                        .anchor_types
                        .get(name)
                        .cloned()
                        .unwrap_or(ValueTy::Double);
                    // A bang binding (`where !x = e`) forces its shared thunk
                    // here, per reference (the same binding-point force the
                    // inline path below emits). Dropping it would leave a
                    // recursive bang binding silently unforced. Forcing a
                    // self-cycle yields the memoized bottom, not a hang.
                    let reg = if bang {
                        let forced = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueForce {
                            dst: forced,
                            src: reg,
                        });
                        forced
                    } else {
                        reg
                    };
                    return Ok((reg, ty));
                }
                // v1 value bindings are per-tick re-evaluated expressions (no
                // shared state), so inlining the body at each use site is
                // semantically identical to lifting it once.
                if self.value_inline.contains(name) {
                    if self.mode == Mode::Strict {
                        return Err(CompileError::Type {
                            msg: format!("recursive value definition `{name}`"),
                            span,
                        });
                    }
                    // Complete mode: recursion is allowed (lazy value track).
                    // Break the cycle with a fresh unbound register — the
                    // recursive reference is bottom until thunk-emitting
                    // lowering arrives in a later task, and an unbound value
                    // register reads as a detectable `None`.
                    let dst = self.fresh_value_reg();
                    // Hardcode: the recursive value's inferred static type is
                    // unavailable here, so downstream static decisions (field
                    // indices / ctor selection / method dispatch) run against
                    // `Double`; exposure is currently low because recursive data
                    // types are still blocked by `check_acyclic`.
                    return Ok((dst, ValueTy::Double));
                }
                self.value_inline.insert(name.to_string());
                // A named lambda-literal definition lowers with its resolved
                // parameter types (threaded through `pending_param_tys`).
                let saved_pending = self.pending_param_tys.take();
                self.pending_param_tys = self.fn_param_tys.get(name).cloned();
                // A binding's OWN declared type is the expected type of its
                // body: `x :: Double; x = 9223372036854775808` must lower the
                // literal as the `Double` its signature declares, not on the
                // `Int` track (Haskell 2010 §6.4.1 `fromInteger`). This is the
                // same rule [`Self::closure_param_tys`] applies to a parameter:
                // only a DECLARED numeric leaf participates, never inference's
                // default, so a bare `x = 9223372036854775808` keeps the
                // `Int`-track fallback it had before.
                let declared = self.declared_binding_ty(name);
                let res = match declared {
                    Some(ty) => self.lower_value_expected(&body, Some(&ty)),
                    None => self.lower_value(&body),
                };
                self.pending_param_tys = saved_pending;
                self.value_inline.remove(name);
                // A bang binding (`!x = e`, `where !x = e`) forces its value to
                // WHNF. Value bindings are re-evaluated per use site here, so a
                // force at the inline site is the binding-point force.
                match res {
                    Ok((reg, ty)) if bang => {
                        let forced = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueForce {
                            dst: forced,
                            src: reg,
                        });
                        Ok((forced, ty))
                    }
                    other => other,
                }
            }
            Def::Eval { span, .. } => {
                let dspan = span;
                // The head λ-nest of the body IS the parameter list (Haskell
                // 2010 §4.4.3.1), so one split yields the parameters the
                // fragment binds and the body it lowers.
                let (def_params, body) = crate::types::infer::effective_def_params(&def);
                // A bare reference to a user definition with λ-parameters in
                // value position is a first-class function value: compile its
                // body into a real FragmentIr and capture free variables by
                // value (mirrors the lambda-literal path). The closure's
                // fragment dispatches the body with the value args bound to
                // the fragment's leading registers, so named definitions
                // participate in currying (`add5 = add2 5.0`) and dispatch
                // just like lambdas.
                //
                // Complete mode uses the STG model for mutual recursion
                // (`even`/`odd`): each top-level Anchor def's closure is
                // compiled EXACTLY ONCE (memoized in `anchor_closures`) and
                // stored in a SHARED cell; references capture the cell, not a
                // recompiled closure, so the recursion never re-enters this
                // arm at compile time.
                if self.mode == Mode::Complete {
                    let reg = self.compile_anchor_closure(name, span)?;
                    let ty = self
                        .anchor_types
                        .get(name)
                        .cloned()
                        .unwrap_or(ValueTy::Double);
                    return Ok((reg, ty));
                }
                // A λ parameter is bound inside its own fragment; leaving it out
                // would make it look like a free variable of that fragment
                // (`unknown \`y\``).
                let param_names: HashSet<String> =
                    def_params.iter().map(|p| p.name.clone()).collect();
                let free = self.free_vars(&body, &param_names);
                // A definition carries its λ-parameter types in its scheme
                // (rebuilt into `fn_param_tys`); the scalar default is only
                // the last resort. Without this a body that projects a field
                // of its parameter (`swap p :: Pair Int Int -> Pair Int Int =
                // Pair { first: p.second, … }`) had no record to project from
                // when the closure form was compiled instead of inlined.
                let param_tys: Vec<ValueTy> = self
                    .fn_param_tys
                    .get(name)
                    .filter(|tys| tys.len() == def_params.len())
                    .cloned()
                    .unwrap_or_else(|| vec![ValueTy::Double; def_params.len()]);
                let declared_result = self.declared_result_leaf(name);
                let (fragment_id, ret_ty) = self.lower_fragment(
                    &def_params,
                    &body,
                    &free,
                    &self.capture_tys(&free),
                    &param_tys,
                    declared_result.as_ref(),
                    dspan,
                )?;
                let (env_reg, self_cell) = self.emit_env_snapshot(&free, dspan, Some(name))?;
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueMakeClosure {
                    dst,
                    env: env_reg,
                    fragment: fragment_id,
                });
                // Strict mode: recursion is rejected, so self_cell is only
                // ever produced here for the complete-mode path above (which
                // writes into the shared cell instead). Keep the write for
                // safety if a self-cell is ever threaded through.
                if let Some(sc) = self_cell {
                    if self.mode == Mode::Complete {
                        self.emit_value(ValueInstr::ValueWriteCell { cell: sc, src: dst });
                    }
                }
                // The closure's signature carries the parameter types inference
                // actually PINNED, and a `Double` placeholder for one it did
                // not ([`Self::closure_param_tys`]). Advertising a defaulted
                // parameter as `Int` makes the call site truncate a literal GHC
                // converts from `Integer`; advertising a DECLARED one is what
                // makes `f 2` at `Integer -> Integer` an `Integer` constant in
                // the fragment slot instead of a `Double` one.
                let arg_tys = self.closure_param_tys(name, def_params.len());
                Ok((dst, ValueTy::Thunk(arg_tys, vec![ret_ty])))
            }
            _ => Err(CompileError::Type {
                msg: format!("`{name}` is not a value expression in v1"),
                span,
            }),
        }
    }

    /// The callee's parameter types after UNIFYING its SCHEME's result type with
    /// the caller's expected type.
    ///
    /// This is the rule a call site needs. A definition's scheme is a complete
    /// declaration: `f x = x` schemes as `a -> a`, `apply k x = k x` as
    /// `(a -> b) -> a -> b`. When the call site expects a type `T`, the way `T`
    /// reaches the ARGUMENTS is ordinary unification — unify the scheme's RESULT
    /// with `T`, then apply the resulting substitution to every one of the scheme's
    /// PARAMETER types. One bind therefore covers every position the scheme
    /// instantiates the same variable in, however many there are:
    ///
    /// - `a -> a` with `Double` expected pins BOTH the parameter and the result, so
    ///   a chain (`f x = x; g y = f y; h z = g z`) works at any length — each `y =`
    ///   `f y` link is the same `a -> a` scheme, so every hop repeats the same
    ///   unification, and
    /// - a λ indirection (`g = \y -> f y`) has the same scheme, so it is the same
    ///   mechanism, not a second one.
    ///
    /// It invents nothing: a scheme used at two DISTINCT types (`show_int (f 1)` and
    /// `show_double (f 1.0)`) gets a FRESH `Subst` per call site, so each site's
    /// bind is local and the other site's use is untouched. And when there is no
    /// expected type, or the result does not unify with it, `None` comes back and
    /// the caller keeps the callee's own signature — the literal then keeps its own
    /// exact conversion rather than being converted against a guess.
    ///
    /// `None` also means "no scheme of this arity", so a local/closure value whose
    /// signature lowering synthesized is never pinned by a def's scheme.
    fn scheme_pinned_params(
        &self,
        name: &str,
        arity: usize,
        expected: Option<&ValueTy>,
        span: Span,
    ) -> Option<Vec<ValueTy>> {
        let exp = expected.filter(|e| is_concrete_ty(e))?;
        let param_tys = self.fn_param_tys.get(name).filter(|t| t.len() == arity)?;
        let ret = self.def_result_tys.get(name)?.first()?;
        let mut subst = Subst::default();
        unify_value(ret, exp, &mut subst, span).ok()?;
        Some(param_tys.iter().map(|t| subst.resolve_value(t)).collect())
    }

    /// The parameter value types a closure's SIGNATURE advertises, from
    /// inference's resolved parameter list ([`Self::fn_param_tys`]).
    ///
    /// A default is not a declaration. `f x = x` schemes as `a -> a`, and
    /// inference resolves the unconstrained `a` to a residual type VARIABLE —
    /// but a variable is not the type `Int` either, and advertising
    /// `Int -> Int` for it makes every call site lower its argument as an
    /// `Int`, which truncates a literal that GHC converts from `Integer`
    /// (`f 9223372036854775808` printed `-9.223372036854776e18`). So a
    /// parameter whose inferred type is not CONCRETE keeps the pre-existing
    /// `Double` placeholder: it is the polymorphic slot lowering cannot name,
    /// and `Double` is the scalar default the rest of the value track uses.
    /// A parameter inference DID pin (`:: Integer -> Integer`, `first p = p.x`
    /// → `Point`, `x + 1.0` → `Double`) is used as written, which is what makes
    /// `f 2` at `Integer -> Integer` an `Integer` constant.
    fn closure_param_tys(&self, name: &str, arity: usize) -> Vec<ValueTy> {
        match self.fn_param_tys.get(name).filter(|tys| tys.len() == arity) {
            Some(tys) => tys.clone(),
            None => (0..arity)
                .map(|k| ValueTy::Var(u32::MAX - 1 - k as u32))
                .collect(),
        }
    }

    /// The numeric leaf a NULLARY binding's own signature declares, if any.
    ///
    /// A default is not a declaration: only the written `::` counts. This keeps
    /// `x :: Double; x = 2` lowering the literal as the declared `Double` while
    /// a bare `x = 2` (whose inferred type is a default) stays on the `Int`
    /// track, exactly as before.
    fn declared_binding_ty(&self, name: &str) -> Option<ValueTy> {
        let sig = self.defs.get(name)?.sig()?;
        if !crate::types::infer::sig_func_parts(sig).0.is_empty() {
            return None;
        }
        let mut vars: HashMap<String, ValueTy> = HashMap::new();
        let mut next = 0u32;
        let mut fresh = || {
            next += 1;
            ValueTy::Var(u32::MAX - next)
        };
        crate::types::infer::sig_ty_to_vty(self.env, sig, &HashMap::new(), &mut vars, &mut fresh)
            .ok()
    }

    /// The numeric leaf a definition's own signature declares as its RESULT
    /// type (`f :: Double -> Double` → `Double`), if any.
    ///
    /// A closure's body is lowered in its own fragment, where the call site's
    /// expected type cannot reach it. Without the declared result,
    /// `f :: Double -> Double; f p = 123456789012345678901234567890` lowered the
    /// literal on the `Int` track and printed the truncation `-4.362896299872286e18`.
    /// Only the written `::` counts — a defaulted result type is not a
    /// declaration.
    fn declared_result_leaf(&self, name: &str) -> Option<ValueTy> {
        let sig = self.defs.get(name)?.sig()?;
        let (_, ret) = crate::types::infer::sig_func_parts(sig);
        numeric_leaf_of(ret, self.env)
    }

    /// Free variables of `e`: `Ref` names not in `bound` and not shadowed by
    /// an inner `let`/`match`/`lambda`. Returned in deterministic (encounter)
    /// order, deduplicated — the order is the env Record field order, which
    /// doubles as the capture-cell indices in the call frame.
    fn free_vars(&self, e: &Expr, bound: &HashSet<String>) -> Vec<String> {
        let mut free = Vec::new();
        let mut seen = HashSet::new();
        self.free_vars_impl(e, bound, &mut free, &mut seen);
        free
    }

    /// The static types of a lambda's free variables, in the same order as
    /// [`Self::free_vars`]: resolved from the enclosing value scopes (a captured
    /// record parameter keeps its `Data` type, so a field projection / method
    /// call inside the fragment resolves), defaulting
    /// to Double for an unknown capture (matching the v1 scalar-capture default).
    fn capture_tys(&self, free: &[String]) -> Vec<ValueTy> {
        free.iter()
            .map(|n| {
                for scope in self.value_locals.iter().rev() {
                    if let Some((_, ty)) = scope.get(n) {
                        return ty.clone();
                    }
                }
                // A name captured by an ENCLOSING fragment (a lambda inside a
                // lambda) keeps its capture type (`k2` of the outer compose
                // lambda, itself a `Kleisli`).
                if let Some((_, ty)) = self.fragment_captures.iter().find(|(f, _)| f == n) {
                    return ty.clone();
                }
                // A shared-cell def (named function or self-recursive local)
                // captured by value: its static RESULT type from inference
                // (`fibs` : List, so `tail fibs` inside fibs' own body resolves).
                if let Some(ty) = self.shared_cell_result_tys.get(n) {
                    return ty.clone();
                }
                ValueTy::Double
            })
            .collect()
    }

    /// Whether `name` resolves at lowering time — a collection/math/FFI
    /// builtin, a data/newtype/sum constructor, a typeclass method, or a
    /// builtin-signature function. Such a name is a compile-time primitive,
    /// never a captured runtime value.
    fn resolves_at_lowering(&self, name: &str) -> bool {
        self.is_builtin_op(name)
            // `error` is lowered directly (a fault instruction), so it is a
            // compile-time primitive like the collection ops and must never be
            // captured as a free variable — unless a user definition shadows it.
            || (name == "error" && !self.defs.contains_key(name))
            // `__fault`, `__ratio_percent` and `__gcd` are the PRELUDE's
            // internal reserved spellings, lowered directly to instructions
            // unconditionally (see `ir::VALUE_FAULT_PRIMITIVE`,
            // `ir::RATIO_PERCENT_PRIMITIVE`, `ir::GCD_PRIMITIVE`), so they are
            // never runtime captures.
            || name == crate::ir::VALUE_FAULT_PRIMITIVE
            || name == crate::ir::RATIO_PERCENT_PRIMITIVE
            || name == crate::ir::GCD_PRIMITIVE
            || self.env.data_types.contains_key(name)
            || self.env.newtypes.contains_key(name)
            || self.sum_ctor(name).is_some()
            || self.env.class_of_method(name).is_some()
            || self.env.builtin_sigs.contains_key(name)
            // A class-constrained definition (`listArray`, `newArray`, …) is
            // specialized per CALL SITE at lowering, exactly like an inlined
            // top-level def: it has no standalone value, so it must never be
            // captured as one. Without this a `do`-block continuation lambda
            // captured `writeArray`/`modifyArray` as closures and the
            // specialized application was then shadowed by the capture.
            || self.env.constrained_defs.contains_key(name)
    }

    /// Record a name that appears in callee position of an application as a
    /// free variable when it is a genuine enclosing-scope closure: `fn x -> f x`
    /// applies a closure `f`, so the env snapshot must capture it. Type and
    /// constructor names (`Pair { ... }`, `Circle 1.5`) are NOT free variables
    /// — a lambda body constructs data types without capturing them.
    /// Typeclass methods (`pure`, `return`, `fmap`, `bind`, …) and builtins
    /// resolve at compile time via inlining, and top-level definitions are
    /// inlined at each use site (captured as a shared self-cell only in
    /// complete mode, for recursion).
    fn note_callee_name(
        &self,
        name: &str,
        bound: &HashSet<String>,
        out: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        if bound.contains(name) || !seen.insert(name.to_string()) {
            return;
        }
        // An enclosing value binding (λ-parameter or fragment capture) is
        // applied as a closure here, so it must be captured even when the name
        // also denotes a class method or a definition.
        if self.value_locals.iter().any(|s| s.contains_key(name))
            || self.fragment_captures.iter().any(|(n, _)| n == name)
        {
            out.push(name.to_string());
            return;
        }
        // A class-constrained definition is specialized per CALL SITE at
        // lowering: it has no standalone value, so it is never a capture —
        // not even in complete mode (where ordinary defs are captured as
        // shared self-cells for recursion).
        if self.env.constrained_defs.contains_key(name) {
            return;
        }
        if self.defs.contains_key(name) {
            if self.mode == Mode::Complete {
                out.push(name.to_string());
            }
        } else if !self.resolves_at_lowering(name) {
            out.push(name.to_string());
        }
    }

    fn free_vars_impl(
        &self,
        e: &Expr,
        bound: &HashSet<String>,
        out: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        match e {
            Expr::Ref(name, _) => {
                // A name bound by an ENCLOSING value scope (a λ-parameter of the
                // definition being compiled, or a capture of the fragment we are
                // already inside) is a runtime value HERE, whatever it may also
                // mean globally: a def named `arr`/`show`/`index` must still be
                // captured. This must be checked BEFORE the
                // `resolves_at_lowering` suppression — a parameter named `arr`
                // would otherwise be dropped as "the `Arrow` class method" and
                // the fragment body would resolve it as a method instead of a
                // capture.
                let enclosing = self.value_locals.iter().any(|s| s.contains_key(name))
                    || self.fragment_captures.iter().any(|(n, _)| n == name);
                if enclosing {
                    if seen.insert(name.clone()) {
                        out.push(name.clone());
                    }
                    return;
                }
                // Top-level value defs are inlined at each use site (see
                // `lower_value_ref_inner`), so they are never captured. A name
                // that resolves at lowering (a builtin, constructor, typeclass
                // method or builtin signature) is not a runtime value either.
                if !bound.contains(name)
                    && !self.resolves_at_lowering(name)
                    && (self.mode == Mode::Complete || !self.defs.contains_key(name))
                    && seen.insert(name.clone())
                {
                    out.push(name.clone());
                }
            }
            Expr::IntegerLit(_, _)
            | Expr::FractionalLit(_, _, _)
            | Expr::Void(_)
            | Expr::Str(_, _) => {}
            // A kernel is a compiler instruction, not a call to a named
            // definition: the callee contributes no capture (there is no name),
            // and only the arguments can carry free variables.
            Expr::Kernel { args, .. } => {
                for a in args {
                    self.free_vars_impl(a, bound, out, seen);
                }
            }
            // An annotation is transparent to free-variable analysis: the type
            // it names is compile-time only, so only the inner expression can
            // contribute a capture.
            Expr::Annot { expr, .. } => self.free_vars_impl(expr, bound, out, seen),
            Expr::Neg(inner, _) => self.free_vars_impl(inner, bound, out, seen),
            Expr::Apply { name, args, .. } => {
                self.note_callee_name(name, bound, out, seen);
                for a in args {
                    self.free_vars_impl(a, bound, out, seen);
                }
            }
            Expr::ApplyExpr { callee, args, .. } => {
                // After the currying pass a name-based application is a
                // right-nested chain of single-argument `ApplyExpr` nodes, so
                // the innermost CALLEE must be resolved exactly like
                // `Expr::Apply`'s name. Walking it as an ordinary expression
                // would capture `cons` from `cons n (list)` and fail the env
                // snapshot with "unknown `cons` in value lowering".
                match uncurry_apply_chain(callee) {
                    Some((name, mut inner)) => {
                        self.note_callee_name(&name, bound, out, seen);
                        inner.extend(args.iter().cloned());
                        for a in &inner {
                            self.free_vars_impl(a, bound, out, seen);
                        }
                    }
                    None => {
                        self.free_vars_impl(callee, bound, out, seen);
                        for a in args {
                            self.free_vars_impl(a, bound, out, seen);
                        }
                    }
                }
            }
            Expr::Let { defs, body, .. } => {
                // Inner `let` bindings shadow outer names inside the body, and
                // each binding's own parameters (a local FUNCTION `g y = e`, or
                // the λ-nest the currying pass leaves for it) shadow inside that
                // binding's body. The def bodies may still reference outer frees.
                let mut inner = bound.clone();
                for d in defs {
                    inner.insert(d.name().to_string());
                }
                for d in defs {
                    // A binding's own body sees its own parameters — the body's
                    // leading λ-nest, which is where `g y = e` keeps `y`
                    // (Haskell 2010 §4.4.3.1) — and its SIBLINGS (the block is
                    // mutually recursive, and a `where`-bound function routinely
                    // calls a sibling value binding).
                    let mut def_bound = inner.clone();
                    if let Expr::Lambda { params, .. } = d.body() {
                        for p in params {
                            def_bound.insert(p.name.clone());
                        }
                    }
                    self.free_vars_impl(d.body(), &def_bound, out, seen);
                }
                self.free_vars_impl(body, &inner, out, seen);
            }
            Expr::Record(fields, _) => {
                for (_, fe) in fields {
                    self.free_vars_impl(fe, bound, out, seen);
                }
            }
            Expr::FieldProject { record, .. } => self.free_vars_impl(record, bound, out, seen),
            Expr::FieldUpdate { record, value, .. } => {
                self.free_vars_impl(record, bound, out, seen);
                self.free_vars_impl(value, bound, out, seen);
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.free_vars_impl(scrutinee, bound, out, seen);
                for arm in arms {
                    let mut inner = bound.clone();
                    for v in crate::reduce::pattern_vars(&arm.pattern) {
                        inner.insert(v);
                    }
                    for (g, b) in &arm.guards {
                        if let Some(g) = g {
                            self.free_vars_impl(g, &inner, out, seen);
                        }
                        self.free_vars_impl(b, &inner, out, seen);
                    }
                }
            }
            Expr::If {
                cond, then, els, ..
            } => {
                self.free_vars_impl(cond, bound, out, seen);
                self.free_vars_impl(then, bound, out, seen);
                self.free_vars_impl(els, bound, out, seen);
            }
            Expr::Lambda { params, body, .. } => {
                let mut inner = bound.clone();
                for p in params {
                    inner.insert(p.name.clone());
                }
                self.free_vars_impl(body, &inner, out, seen);
            }
            Expr::ActorParam { default, .. } => {
                if let Some(d) = default {
                    self.free_vars_impl(d, bound, out, seen);
                }
            }
            Expr::ListLit(elems, _) => {
                for el in elems {
                    self.free_vars_impl(el, bound, out, seen);
                }
            }
            Expr::MapLit(entries, _) => {
                for (_, ve) in entries {
                    self.free_vars_impl(ve, bound, out, seen);
                }
            }
        }
    }

    /// Compile a lambda body into a [`FragmentIr`], returning the fragment's
    /// index and the body's value type.
    ///
    /// Fragment-local registers: the body's value instructions use registers
    /// `0..num_value_regs`, unrelated to the program's register numbering —
    /// the interpreter offsets them by a per-call base at dispatch. The
    /// program-level value-track state (`value_blocks`, `cur_value_block`,
    /// `next_value_reg`, `value_locals`, `fragment_captures`) is saved around
    /// the body compile and restored afterwards.
    ///
    /// Within the fragment: parameters bind to fragment-local registers
    /// `0..params.len()` (value args are copied in by `run_call`), typed
    /// with the λ-parameter types threaded from inference; a `Ref` to a free
    /// name emits `ValueReadCell { cell: capture_index }` where
    /// `capture_index` is the name's position in `free` (the env Record field
    /// order, bound by `run_call` as temp-frame cells).
    #[allow(clippy::too_many_arguments)]
    fn lower_fragment(
        &mut self,
        params: &[Param],
        body: &Expr,
        free: &[String],
        capture_tys: &[ValueTy],
        param_tys: &[ValueTy],
        expected_body: Option<&ValueTy>,
        _span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        let saved_blocks = std::mem::take(&mut self.value_blocks);
        let saved_cur = self.cur_value_block;
        self.cur_value_block = self.new_value_block();
        let saved_next = self.next_value_reg;
        let saved_locals = std::mem::take(&mut self.value_locals);
        let saved_captures = std::mem::take(&mut self.fragment_captures);
        // A fragment is a deferred computation: typeclass method calls inside
        // resolve at the fragment's own call sites, not against the enclosing
        // method-inlining path (clear the recursion guard).
        let saved_lifting = self.method_lifting.clone();
        self.method_lifting.clear();

        let mut scope: HashMap<String, (usize, ValueTy)> = HashMap::new();
        for (i, p) in params.iter().enumerate() {
            let ty = param_tys.get(i).cloned().unwrap_or(ValueTy::Double);
            scope.insert(p.name.clone(), (i, ty));
        }
        self.value_locals.push(scope);
        self.value_local_ctors.push(HashMap::new());
        self.fragment_captures = free
            .iter()
            .cloned()
            .zip(capture_tys.iter().cloned())
            .collect();
        self.next_value_reg = params.len();

        // Bang parameters force their incoming argument to WHNF in the
        // fragment's entry block, before the body binds/uses it.
        for (i, p) in params.iter().enumerate() {
            if !p.bang {
                continue;
            }
            let forced = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueForce {
                dst: forced,
                src: i,
            });
            let ty = param_tys.get(i).cloned().unwrap_or(ValueTy::Double);
            self.value_locals
                .last_mut()
                .unwrap()
                .insert(p.name.clone(), (forced, ty));
        }

        let res = self.lower_value_expected(body, expected_body);
        let body_result = match res {
            Ok(r) => r,
            Err(e) => {
                self.value_blocks = saved_blocks;
                self.cur_value_block = saved_cur;
                self.next_value_reg = saved_next;
                self.value_locals = saved_locals;
                self.value_local_ctors.pop();
                self.fragment_captures = saved_captures;
                return Err(e);
            }
        };
        let (result_reg, result_ty) = body_result;
        let frag = FragmentIr {
            value_blocks: std::mem::take(&mut self.value_blocks),
            entry: 0,
            num_value_regs: self.next_value_reg,
            output_value_regs: vec![result_reg],
            num_capture_cells: free.len(),
            sig: FuncSig {
                value_ins: params.len(),
                value_outs: 1,
                signal_ins: 0,
            },
        };
        let id = self.fragments.len();
        self.fragments.push(std::sync::Arc::new(frag));

        self.value_blocks = saved_blocks;
        self.cur_value_block = saved_cur;
        self.next_value_reg = saved_next;
        self.value_locals = saved_locals;
        self.value_local_ctors.pop();
        self.fragment_captures = saved_captures;
        self.method_lifting = saved_lifting;
        Ok((id, result_ty))
    }

    /// Snapshot the current values of `free` into a new env Record, returning
    /// the record's value register. Each free name is resolved through
    /// `lower_value_ref`, so a top-level definition inlines its value while a
    /// name captured by an enclosing lambda reads its env-frame cell.
    /// Build the env Record for a closure/lambda capture. A free variable that
    /// names the CURRENT def (`self_name == name`) is a recursive self-
    /// reference: instead of re-lowering the def (which would recurse at
    /// compile time), the field is a reference to a fresh cell that the caller
    /// fills with the closure AFTER it is created. Reading the field through
    /// the cell yields the closure — this is the live self-reference that makes
    /// recursion productive. In complete mode a top-level Anchor def uses its
    /// SHARED cell (the STG model: a binding is a thunk in a cell holding a
    /// closure or a pure value) so a mutually-recursive partner (`even`/`odd`)
    /// captures the CELL, not a recompiled closure — recompiling the partner
    /// is exactly what overflowed the compile-time stack. Returns
    /// `(env_reg, Option<self_cell_reg>)`.
    fn emit_env_snapshot(
        &mut self,
        free: &[String],
        span: Span,
        self_name: Option<&str>,
    ) -> Result<(usize, Option<usize>), CompileError> {
        let mut field_regs = Vec::with_capacity(free.len());
        let mut self_cell: Option<usize> = None;
        for name in free {
            // Inside a fragment, a shared-cell def is a CAPTURE (a
            // `ValueReadCell` into the frame), not a global cell register:
            // fragment registers are local, so `anchor_cell`'s main-block reg
            // would alias a fragment-local register. Only at the top level
            // (no fragment) is the shared cell the right reference.
            let in_fragment = !self.fragment_captures.is_empty()
                || self.fragment_captures.iter().any(|(f, _)| f == name);
            let is_shared_here =
                self.mode == Mode::Complete && self.is_shared_cell_def(name) && !in_fragment;
            if self_name == Some(name.as_str()) {
                if is_shared_here {
                    // Self-recursive top-level def: reference its SHARED cell
                    // (created by the Anchor arm before the fragment compiled),
                    // which the caller fills with the closure/thunk.
                    let cell_reg = self.anchor_cell(name);
                    field_regs.push(cell_reg);
                    self_cell = Some(cell_reg);
                } else {
                    // Self-reference: a cell that the caller writes the closure
                    // into.
                    let cell_reg = self.fresh_value_reg();
                    self.emit_value(ValueInstr::ValueBindCell { dst: cell_reg });
                    field_regs.push(cell_reg);
                    self_cell = Some(cell_reg);
                }
            } else if is_shared_here {
                // Mutual recursion (`even`/`odd`): capture the partner's SHARED
                // cell (created on first sight) and compile its closure/thunk
                // into it right now (eagerly) unless it is already compiled or
                // currently being compiled (the `anchor_compiling` guard). The
                // cell — not a recompiled closure — is the forward reference
                // that makes the compile-time recursion terminate.
                self.compile_anchor_closure(name, span)?;
                let cell_reg = self.anchor_cell(name);
                field_regs.push(cell_reg);
            } else {
                let (r, _) = self.lower_value_ref(name, span)?;
                field_regs.push(r);
            }
        }
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueConstructRecord {
            dst,
            fields: field_regs,
        });
        Ok((dst, self_cell))
    }

    /// Whether `name` is a top-level def that gets a SHARED cell in complete
    /// mode: a definition with parameters (a name bound to a function value,
    /// Haskell 2010 §4.4.3.1) OR a self-recursive local definition
    /// (`fibs = cons … fibs …`). Every reference — self or mutual — goes through
    /// the cell.
    fn is_shared_cell_def(&self, name: &str) -> bool {
        match self.defs.get(name) {
            Some(def @ Def::Eval { .. }) => def.arity() > 0 || self.recursive_locals.contains(name),
            _ => false,
        }
    }

    /// Compute the set of LOCAL defs that participate in a reference cycle
    /// (self- OR mutual-recursion): a def whose body transitively references
    /// its own name. `a = a` is a self-loop; `a = b; b = a` is a two-node
    /// cycle. Both must be shared-cell defs so the recursion resolves through
    /// the cell and forcing a divergent cycle yields `Bottom` (not an unbound
    /// register). Computed once at lowering start from the def-reference graph.
    fn compute_recursive_locals(defs: &HashMap<String, Def>) -> HashSet<String> {
        let mut refs_of: HashMap<String, HashSet<String>> = HashMap::new();
        for (name, def) in defs {
            let mut refs = HashSet::new();
            if !def.is_decl() {
                collect_def_ref_names(def.body(), &mut refs);
            }
            refs_of.insert(name.clone(), refs);
        }
        let mut recursive = HashSet::new();
        let names: Vec<String> = defs.keys().cloned().collect();
        for start in &names {
            // DFS from `start`'s body references; if we ever return to `start`,
            // it is part of a cycle.
            let mut visited = HashSet::new();
            let mut stack: Vec<String> = refs_of
                .get(start)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect();
            while let Some(cur) = stack.pop() {
                if cur == *start {
                    recursive.insert(start.clone());
                    break;
                }
                if !visited.insert(cur.clone()) {
                    continue;
                }
                if let Some(next) = refs_of.get(&cur) {
                    stack.extend(next.iter().cloned());
                }
            }
        }
        recursive
    }

    /// Get-or-create the SHARED cell for a top-level Anchor def (complete
    /// mode). The cell is a forward reference: its closure is compiled exactly
    /// once (eagerly, on first sight — see [`Self::compile_anchor_closure`])
    /// and written into the cell, so any number of references (including
    /// mutual recursion) capture the cell without recompiling. Returns the
    /// cell register.
    ///
    /// The `ValueBindCell` is emitted into the value-track ENTRY block (which
    /// always runs before any branch): a reference can first appear inside a
    /// branch arm, and the cell register must be bound on every execution path
    /// (the program-level cell is global, not per-block).
    fn anchor_cell(&mut self, name: &str) -> usize {
        if let Some(&c) = self.anchor_cells.get(name) {
            return c;
        }
        let saved_block = self.cur_value_block;
        self.cur_value_block = self.value_entry;
        let c = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBindCell { dst: c });
        self.cur_value_block = saved_block;
        self.anchor_cells.insert(name.to_string(), c);
        c
    }

    /// Compile the closure of a top-level Anchor def (complete mode) into its
    /// SHARED cell, unless it is already compiled or currently being compiled
    /// (the mutual-recursion guard: `even`'s compile reaching `odd` reaching
    /// `even` again uses `even`'s cell as the forward reference). The closure
    /// is memoized in [`Self::anchor_closures`], so each def's fragment is
    /// compiled exactly once. Returns the closure's value register.
    fn compile_anchor_closure(&mut self, name: &str, span: Span) -> Result<usize, CompileError> {
        if let Some(&reg) = self.anchor_closures.get(name) {
            return Ok(reg);
        }
        if self.anchor_compiling.contains(name) {
            // Mutual-recursion re-entry: the caller already captured our cell
            // (created by [`Self::anchor_cell`]); our closure is written into
            // it by the in-flight compile. Nothing to do here.
            return Ok(self.anchor_cells.get(name).cloned().unwrap());
        }
        let def = self
            .defs
            .get(name)
            .cloned()
            .ok_or_else(|| CompileError::Type {
                msg: format!("unknown `{name}` in value lowering"),
                span,
            })?;
        self.anchor_compiling.insert(name.to_string());
        // The shared cell must exist BEFORE the fragment compiles, so a
        // mutually-recursive partner's env snapshot captures the cell (not a
        // recompiled closure).
        //
        // The whole init sequence (bind cell + compile closure/thunk + write
        // cell) is hoisted into the value-track ENTRY block: the first
        // reference to a shared-cell def can appear inside a branch arm, and
        // the cell register must be bound and filled on every execution path
        // (block 0 always runs before any branch). Nested fragment compiles
        // save/restore their own blocks, so this wrap is re-entrant.
        let saved_block = self.cur_value_block;
        self.cur_value_block = self.value_entry;
        let cell = self.anchor_cell(name);
        // Seed the static result type BEFORE the fragment compiles, so a
        // recursive reference inside the body (`tail fibs`) resolves the def's
        // type (List) instead of the Double fallback. Computed later from the
        // lowered body if inference did not pin it (the seeded value is the
        // forward-declared type; the fragment ret type refines it).
        let seeded_ty = self.shared_cell_result_tys.get(name).cloned();
        // The head λ-nest of the body is the definition's parameter list
        // (Haskell 2010 §4.4.3.1: `f x y = e` IS `f = \x -> \y -> e`), so one
        // split yields both the parameters to bind and the body to lower. A
        // definition whose body builds no λ is a CAF and gets no parameters.
        let Def::Eval { body, span, .. } = &def else {
            self.anchor_compiling.remove(name);
            self.cur_value_block = saved_block;
            return Err(CompileError::Type {
                msg: format!("`{name}` is not a value expression in v1"),
                span,
            });
        };
        let head_arity = crate::types::infer::leading_lambda_count(body);
        let (def_params, body) = crate::types::ty::split_curried_params(head_arity, &[], body);
        let (body, dspan, is_anchor) = (body, *span, head_arity > 0);
        let param_names: HashSet<String> = def_params.iter().map(|p| p.name.clone()).collect();
        let free = self.free_vars(&body, &param_names);
        // The def's λ-parameter value types come from its scheme, rebuilt into
        // `fn_param_tys` by inference (`tail xs` → xs : List; a signature
        // `:: Pair … -> …` types `p` as the record). Falling back to Double
        // keeps unconstrained scalar params working.
        let param_tys: Vec<ValueTy> = if is_anchor {
            self.fn_param_tys
                .get(name)
                .cloned()
                .unwrap_or_else(|| vec![ValueTy::Double; def_params.len()])
        } else {
            Vec::new()
        };
        // Seed `anchor_types` with the inferred result type before the body
        // lowers, so a recursive reference inside (`tail fibs`) resolves the
        // def's static type via the Anchor arm.
        if let Some(seed) = seeded_ty.clone() {
            self.anchor_types.insert(name.to_string(), seed);
        }
        // A curried definition (`take = \n -> \xs -> …`) is niladic at the IR
        // level but its body is a nest of λs; their types come from inference's
        // flattened λ-parameter list (`take` → `[Double, List]`).
        let saved_pending = self.pending_param_tys.take();
        if !is_anchor {
            self.pending_param_tys = self.fn_param_tys.get(name).cloned();
        }
        // The fragment's body expected type is the def's DECLARED result leaf:
        // a closure body is its own fragment, so no call-site expected type can
        // reach it, and `f :: Double -> Double; f p = 1234…890` otherwise
        // lowered the literal on the `Int` track.
        let declared_result = self.declared_result_leaf(name);
        let frag = self.lower_fragment(
            &def_params,
            &body,
            &free,
            &self.capture_tys(&free),
            &param_tys,
            declared_result.as_ref(),
            dspan,
        );
        self.pending_param_tys = saved_pending;
        let (fragment_id, ret_ty) = frag?;
        let (env_reg, _) = self.emit_env_snapshot(&free, dspan, Some(name))?;
        let dst = self.fresh_value_reg();
        if is_anchor {
            self.emit_value(ValueInstr::ValueMakeClosure {
                dst,
                env: env_reg,
                fragment: fragment_id,
            });
        } else {
            // A recursive CAF (`fibs = cons 0.0 (cons 1.0 (... fibs ...))`) is
            // a THUNK over its body fragment, not a closure: the shared cell
            // holds the lazy computation, and the body's self-reference reads
            // the cell (forcing on demand, memoizing the result).
            self.emit_value(ValueInstr::ValueLazy {
                dst,
                env: env_reg,
                fragment: fragment_id,
            });
        }
        // Complete-mode recursion: fill the shared cell with the just-created
        // closure/thunk, so a recursive reference in the body reads it through
        // the cell.
        self.emit_value(ValueInstr::ValueWriteCell { cell, src: dst });
        let ty = if is_anchor {
            // The closure's signature carries the parameter types inference
            // actually PINNED, and a `Double` placeholder for one it did not
            // ([`Self::closure_param_tys`]). A defaulted parameter (`f x = x`
            // schemes as `a -> a`) must NOT become a declared `Int`: the call
            // site would then truncate a literal GHC converts from `Integer`,
            // and `f 9223372036854775808` printed `-9.223372036854776e18`
            // instead of `9.223372036854776e18`. A DECLARED parameter
            // (`f :: Integer -> Integer`) still reaches the call site, which is
            // what makes `f 2` an `Integer` constant rather than a `Double`
            // constant in an `Integer` fragment slot.
            let arg_tys = self.closure_param_tys(name, def_params.len());
            ValueTy::Thunk(arg_tys, vec![ret_ty])
        } else {
            ret_ty
        };
        self.anchor_closures.insert(name.to_string(), dst);
        self.anchor_types.insert(name.to_string(), ty);
        self.anchor_compiling.remove(name);
        self.cur_value_block = saved_block;
        Ok(dst)
    }

    /// Compile a partial-application (curry) fragment for a call that applies
    /// fewer args than the callee's signature.
    ///
    /// The curry fragment's env capture cells are `[func, arg0, arg1, ...]` —
    /// the function value followed by the already-applied args, in capture
    /// order. Its value registers start with the `remaining_arity` call-site
    /// args (the fragment's own params). On call it reads the captured function
    /// and applied args out of the env frame (`ValueReadCell` capture reads),
    /// then dispatches `ValueCallFunc` with `[captured args..., remaining args...]`.
    ///
    /// Mirrors `lower_fragment`'s register scheme: fragment-local registers
    /// `0..num_value_regs`, offset by the interpreter per call.
    /// Build the FFI arrow as a first-class `Value::Callable`: a fragment whose
    /// body reads the Buffer argument registers from the call frame and runs one
    /// `ValueCallForeign`. Returns `(callee_reg, call_dst, first_out_reg)`.
    ///
    /// The fragment is built in a temporary block context (like `apply_partial`),
    /// then restored. Applying the arrow through `ValueCallFunc` dispatches via
    /// `run_call`, so FFI arrows are ordinary values (HOF: `map (onepole 200.0
    /// 0.7) buffers`).
    fn build_ffi_arrow(
        &mut self,
        instance: usize,
        n_buf_args: usize,
        n_out: usize,
    ) -> Result<(usize, usize), CompileError> {
        let saved_blocks = std::mem::take(&mut self.value_blocks);
        let saved_cur = self.cur_value_block;
        self.cur_value_block = self.new_value_block();
        let saved_next = self.next_value_reg;
        let saved_locals = std::mem::take(&mut self.value_locals);
        let saved_captures = std::mem::take(&mut self.fragment_captures);

        // Fragment-local registers: Buffer args at 0..n_buf_args; the output(s)
        // follow.
        let fst = n_buf_args;
        for _ in 1..n_out {
            self.fresh_value_reg();
        }
        self.emit_value(ValueInstr::ValueCallForeign {
            dst: fst,
            srcs: (0..n_buf_args).collect(),
            instance,
        });
        let frag = FragmentIr {
            value_blocks: std::mem::take(&mut self.value_blocks),
            entry: 0,
            num_value_regs: fst + n_out,
            output_value_regs: (0..n_out).map(|i| fst + i).collect(),
            num_capture_cells: 0,
            sig: FuncSig {
                value_ins: n_buf_args,
                value_outs: n_out,
                signal_ins: 0,
            },
        };
        let fid = self.fragments.len();
        self.fragments.push(std::sync::Arc::new(frag));

        self.value_blocks = saved_blocks;
        self.cur_value_block = saved_cur;
        self.next_value_reg = saved_next;
        self.value_locals = saved_locals;
        self.fragment_captures = saved_captures;

        // Allocate the closure on the OUTER track.
        let env = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueConstructRecord {
            dst: env,
            fields: vec![],
        });
        let callee_reg = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueMakeClosure {
            dst: callee_reg,
            env,
            fragment: fid,
        });
        let call_dst = self.fresh_value_reg();
        Ok((callee_reg, call_dst))
    }

    fn apply_partial(
        &mut self,
        applied: usize,
        remaining_arity: usize,
        _span: Span,
    ) -> Result<usize, CompileError> {
        let saved_blocks = std::mem::take(&mut self.value_blocks);
        let saved_cur = self.cur_value_block;
        self.cur_value_block = self.new_value_block();
        let saved_next = self.next_value_reg;
        let saved_locals = std::mem::take(&mut self.value_locals);
        let saved_captures = std::mem::take(&mut self.fragment_captures);

        let func_reg = remaining_arity;
        let arg0_reg = remaining_arity + 1;
        let call_dst = remaining_arity + 1 + applied;
        let num_value_regs = call_dst + 1;
        self.next_value_reg = num_value_regs;
        // Capture index 0 = the captured function value; 1.. = the applied args.
        self.emit_value(ValueInstr::ValueReadCell {
            dst: func_reg,
            cell: 0,
        });
        for i in 0..applied {
            self.emit_value(ValueInstr::ValueReadCell {
                dst: arg0_reg + i,
                cell: 1 + i,
            });
        }
        let mut call_args: Vec<usize> = (arg0_reg..arg0_reg + applied).collect();
        call_args.extend(0..remaining_arity);
        self.emit_call(call_dst, func_reg, call_args);

        let frag = FragmentIr {
            value_blocks: std::mem::take(&mut self.value_blocks),
            entry: 0,
            num_value_regs,
            output_value_regs: vec![call_dst],
            num_capture_cells: 1 + applied,
            sig: FuncSig {
                value_ins: remaining_arity,
                value_outs: 1,
                signal_ins: 0,
            },
        };
        let id = self.fragments.len();
        self.fragments.push(std::sync::Arc::new(frag));

        self.value_blocks = saved_blocks;
        self.cur_value_block = saved_cur;
        self.next_value_reg = saved_next;
        self.value_locals = saved_locals;
        self.fragment_captures = saved_captures;
        Ok(id)
    }

    /// Resolve the constructor name of a scrutinee expression when it is
    /// statically known at compile time: a literal sum construction
    /// (`Circle 1.5`) or a `Ref` to an inlined value definition whose body is
    /// one. Returns `None` when the constructor cannot be determined statically
    /// (an unbound `_` wire, a field projection, a nested match, ...).
    fn static_scrutinee_ctor(&self, e: &Expr) -> Option<String> {
        self.static_scrutinee_ctor_impl(e, &mut HashSet::new())
    }

    fn static_scrutinee_ctor_impl(
        &self,
        e: &Expr,
        visited: &mut HashSet<String>,
    ) -> Option<String> {
        match e {
            Expr::Apply { name, args, .. } => {
                if self.sum_ctor(name).is_some() {
                    return Some(name.clone());
                }
                // A typeclass method call whose result constructor is statically
                // known: resolve the instance via the class-var-applied argument
                // (constructor classes) and inline the body.
                if let Some(class_name) = self.env.class_of_method(name) {
                    let class_info = self.env.typeclasses.get(&class_name).cloned().unwrap();
                    let sig = class_info
                        .methods
                        .iter()
                        .find(|(m, _)| m == name)
                        .map(|(_, s)| s.clone());
                    if class_info.arity >= 1 {
                        if let Some(container_idx) = self
                            .env
                            .class_var_arg_index(sig.as_ref().unwrap(), &class_info.var)
                        {
                            // The container argument's static constructor selects
                            // the instance: `fmap g (Just 1.0)` → `Just` → the
                            // `Maybe` instance.
                            let container_ctor =
                                self.static_scrutinee_ctor_impl(args.get(container_idx)?, visited)?;
                            let ty_name = self.instance_type_of_ctor(&container_ctor)?;
                            if let Some((_, params, body)) = self.env.resolve_method(name, &ty_name)
                            {
                                // β-substitute the call args into the body and
                                // resolve the result's constructor.
                                let mut subst: HashMap<String, Expr> = HashMap::new();
                                for (p, a) in params.iter().zip(args.iter()) {
                                    subst.insert(p.clone(), a.clone());
                                }
                                let inlined = crate::reduce::substitute(&body, &subst);
                                return self.static_scrutinee_ctor_impl(&inlined, visited);
                            }
                        }
                    }
                }
                // Builtin Maybe-returning ops resolve statically on concrete
                // inputs: `head` of a non-empty list and `lookup` of a present
                // key are `Just`; an empty list / absent key is `Nothing`.
                match name.as_str() {
                    "head" => args
                        .first()
                        .and_then(|xs| self.static_list_nonempty(xs, visited))
                        .map(|ne| {
                            if ne {
                                "Just".to_string()
                            } else {
                                "Nothing".to_string()
                            }
                        }),
                    "lookup" => {
                        if let (Some(Expr::Str(k, _)), Some(map)) = (args.first(), args.get(1)) {
                            self.static_map_has_key(map, k, visited).map(|present| {
                                if present {
                                    "Just".to_string()
                                } else {
                                    "Nothing".to_string()
                                }
                            })
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
            Expr::Ref(name, _) => {
                if visited.contains(name) {
                    return None;
                }
                if let Some(Def::Eval { body, .. }) = self.defs.get(name).cloned() {
                    visited.insert(name.clone());
                    let r = self.static_scrutinee_ctor_impl(&body, visited);
                    visited.remove(name);
                    return r;
                }
                // A value local bound to a statically-known constructor (a
                // method parameter bound to `Just 1.0`): a `match` over the
                // param's `Ref` selects its arm at compile time.
                for scope in self.value_local_ctors.iter().rev() {
                    if let Some(ctor) = scope.get(name) {
                        return ctor.clone();
                    }
                }
                // A bare nullary constructor (`Nothing`, `Red`) is a complete
                // sum value: its constructor is statically known even though it
                // is not a bare `Def::Eval` body.
                if let Some((_, _, payload)) = self.sum_ctor(name) {
                    if payload.is_empty() {
                        return Some(name.clone());
                    }
                }
                None
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                // A statically-known scrutinee constructor selects the arm at
                // compile time; the arm's body's constructor is the match's
                // result constructor.
                let scrut_ctor = self.static_scrutinee_ctor_impl(scrutinee, visited)?;
                for arm in arms {
                    if let Pattern::Ctor(c, _) = arm.pattern.strip_bang() {
                        if c == &scrut_ctor {
                            // A guarded arm's result constructor is NOT
                            // statically known (the guard may fail at runtime),
                            // so only a bare arm can claim a statically-known
                            // result.
                            return crate::reduce::unguarded_arm_body(arm)
                                .and_then(|body| self.static_scrutinee_ctor_impl(body, visited));
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// The type name bound to the class variable for a value constructor
    /// (`Just`/`Nothing` → `Maybe`; a builtin ctor like `List` maps to itself).
    fn instance_type_of_ctor(&self, ctor: &str) -> Option<String> {
        if let Some((sum_name, _, _)) = self.sum_ctor(ctor) {
            return Some(sum_name);
        }
        // A type constructor name used directly (`List`, `Box`, `Maybe`).
        if self.env.type_ctor_arity(ctor).is_some() {
            return Some(ctor.to_string());
        }
        None
    }

    /// Statically determine a list expression's emptiness: `Some(true)` when
    /// non-empty, `Some(false)` when empty, `None` when unknown. Follows
    /// inlined value definitions (the visit guard breaks cycles).
    fn static_list_nonempty(&self, e: &Expr, visited: &mut HashSet<String>) -> Option<bool> {
        match e {
            Expr::ListLit(elems, _) => Some(!elems.is_empty()),
            Expr::Apply { name, args, .. } => match name.as_str() {
                // `cons x xs` always yields a non-empty list; `list n` always an
                // empty one. `map` preserves the source length; `tail` of an
                // empty list stays empty (a non-empty source may still produce
                // an empty tail, so that case is unknown).
                "cons" => Some(true),
                "list" => Some(false),
                "map" => args
                    .get(1)
                    .and_then(|xs| self.static_list_nonempty(xs, visited)),
                "tail" => match args
                    .first()
                    .and_then(|xs| self.static_list_nonempty(xs, visited))
                {
                    Some(false) => Some(false),
                    _ => None,
                },
                _ => None,
            },
            Expr::Ref(name, _) => {
                if visited.contains(name) {
                    return None;
                }
                if let Some(Def::Eval { body, .. }) = self.defs.get(name).cloned() {
                    visited.insert(name.clone());
                    let r = self.static_list_nonempty(&body, visited);
                    visited.remove(name);
                    return r;
                }
                None
            }
            _ => None,
        }
    }

    /// Statically determine whether a map expression contains `key`:
    /// `Some(true)`/`Some(false)` when the keys are statically known, `None`
    /// otherwise. Follows inlined value definitions and `insert`/`empty_map`
    /// chains.
    fn static_map_has_key(
        &self,
        e: &Expr,
        key: &str,
        visited: &mut HashSet<String>,
    ) -> Option<bool> {
        match e {
            Expr::MapLit(entries, _) => {
                let present = entries.iter().any(|(k, _)| k == key);
                Some(present)
            }
            Expr::Apply { name, args, .. } if name == "insert" && args.len() == 3 => {
                // Resolve the insert key through the same Ref inlining used for
                // the map argument: a let-bound key (`k = "a"`) is a concrete
                // string the analysis can compare. A genuinely non-literal key
                // is unknown — the analysis must not claim it present/absent.
                match self.static_string_literal(&args[0], visited) {
                    Some(k) => {
                        if k == key {
                            Some(true)
                        } else {
                            self.static_map_has_key(&args[2], key, visited)
                        }
                    }
                    None => None,
                }
            }
            Expr::Apply { name, .. } if name == "empty_map" => Some(false),
            Expr::Ref(name, _) => {
                if visited.contains(name) {
                    return None;
                }
                if let Some(Def::Eval { body, .. }) = self.defs.get(name).cloned() {
                    visited.insert(name.clone());
                    let r = self.static_map_has_key(&body, key, visited);
                    visited.remove(name);
                    return r;
                }
                None
            }
            _ => None,
        }
    }

    /// Resolve an expression to a string literal, following inlined value
    /// definitions (cycle-guarded). `Some(s)` when the string is statically
    /// known, `None` otherwise.
    fn static_string_literal(&self, e: &Expr, visited: &mut HashSet<String>) -> Option<String> {
        match e {
            Expr::Str(s, _) => Some(s.clone()),
            Expr::Ref(name, _) => {
                if visited.contains(name) {
                    return None;
                }
                if let Some(Def::Eval { body, .. }) = self.defs.get(name).cloned() {
                    visited.insert(name.clone());
                    let r = self.static_string_literal(&body, visited);
                    visited.remove(name);
                    return r;
                }
                None
            }
            _ => None,
        }
    }

    /// Concrete type arguments of a builtin-sum scrutinee that is a direct
    /// constructor application (`Just x`, `Left x`, `Right x`), following
    /// inlined value definitions. Lowering gives such a scrutinee the static
    /// type `Data(name, [])`, so the placeholder payload types in the builtin
    /// shape must be resolved against the constructor arguments' concrete
    /// types (mirroring inference). The arguments are lowered in a sandbox —
    /// their instructions are discarded — to recover their static types.
    /// Returns an empty vector when the scrutinee is not a resolvable builtin
    /// construction (user sums already carry concrete payloads).
    fn builtin_sum_scrutinee_args(&mut self, scrutinee: &Expr) -> Vec<ValueTy> {
        self.builtin_sum_scrutinee_args_impl(scrutinee, &mut HashSet::new())
    }

    fn builtin_sum_scrutinee_args_impl(
        &mut self,
        e: &Expr,
        seen: &mut HashSet<String>,
    ) -> Vec<ValueTy> {
        match e {
            Expr::Ref(name, _) => {
                if !seen.insert(name.clone()) {
                    return vec![];
                }
                if let Some(Def::Eval { body, .. }) = self.defs.get(name).cloned() {
                    let r = self.builtin_sum_scrutinee_args_impl(&body, seen);
                    seen.remove(name);
                    return r;
                }
                vec![]
            }
            Expr::Apply { name, args, .. } => {
                let Some((sum_name, _, payload)) = self.sum_ctor(name) else {
                    return vec![];
                };
                if self.env.type_ctor_arity(&sum_name).is_none() {
                    // User sum — its payloads are already concrete.
                    return vec![];
                }
                // Placeholder payloads reference type-param positions
                // (`Var(k)` ↔ k-th type arg); the ctor's argument at that
                // payload slot supplies the concrete type.
                let mut out: Vec<ValueTy> = vec![];
                for (i, pt) in payload.iter().enumerate() {
                    let ValueTy::Var(k) = pt else {
                        continue;
                    };
                    let Some(arg) = args.get(i) else {
                        continue;
                    };
                    let Some(ty) = self.sandbox_value_ty(arg) else {
                        return vec![];
                    };
                    let idx = k.saturating_sub(1) as usize;
                    while out.len() <= idx {
                        out.push(ValueTy::Double);
                    }
                    out[idx] = ty;
                }
                out
            }
            _ => vec![],
        }
    }

    /// Recover an expression's static value type by lowering it in a sandbox:
    /// the emitted instructions are discarded. Used to resolve builtin-sum
    /// scrutinee payload types that `lower_value` does not carry on the
    /// returned scrutinee type (`Data(name, [])`).
    fn sandbox_value_ty(&mut self, e: &Expr) -> Option<ValueTy> {
        let saved_blocks = std::mem::take(&mut self.value_blocks);
        let saved_cur = self.cur_value_block;
        self.cur_value_block = self.new_value_block();
        let saved_next = self.next_value_reg;
        let saved_locals = std::mem::take(&mut self.value_locals);
        let saved_captures = std::mem::take(&mut self.fragment_captures);
        let saved_inline = std::mem::take(&mut self.value_inline);
        let saved_pending = std::mem::take(&mut self.pending_param_tys);
        let r = self.lower_value(e).ok();
        self.value_blocks = saved_blocks;
        self.cur_value_block = saved_cur;
        self.next_value_reg = saved_next;
        self.value_locals = saved_locals;
        self.fragment_captures = saved_captures;
        self.value_inline = saved_inline;
        self.pending_param_tys = saved_pending;
        r.map(|(_, ty)| ty.clone())
    }

    /// Resolve the sum type of a `match` from its arm constructors, intersecting
    /// the candidate sum types per constructor (mirrors inference). Literal,
    /// wildcard, and variable arms do not pin the sum (a scalar match, or a sum
    /// match with a fallback arm); the caller only resolves when at least one
    /// constructor arm exists.
    fn resolve_match_sum(&self, arms: &[MatchArm], span: Span) -> Result<String, CompileError> {
        let mut candidates: Option<Vec<String>> = None;
        for arm in arms {
            let ctor = match arm.pattern.strip_bang() {
                Pattern::Ctor(c, _) => c,
                _ => continue,
            };
            let per_ctor: Vec<String> = self
                .env
                .data_types
                .iter()
                .filter_map(|(tname, info)| match info {
                    DataInfo::Sum(ctors) if ctors.iter().any(|(c, _)| c == ctor) => {
                        Some(tname.clone())
                    }
                    _ => None,
                })
                .collect();
            if per_ctor.is_empty() {
                return Err(CompileError::Type {
                    msg: format!("unknown constructor `{ctor}`"),
                    span,
                });
            }
            candidates = Some(match candidates {
                None => per_ctor,
                Some(acc) => acc.into_iter().filter(|n| per_ctor.contains(n)).collect(),
            });
        }
        match candidates {
            Some(v) if v.len() == 1 => Ok(v[0].clone()),
            _ => Err(CompileError::Type {
                msg: "match arms use constructors of ambiguous or different sum types".into(),
                span,
            }),
        }
    }

    /// Lower the guard-free arm of a match whose scrutinee constructor is
    /// statically known. Only the selected arm is lowered (its pattern matched
    /// at compile time), preserving exact-capacity behavior. The arm that would
    /// match at runtime is the `Ctor` arm for `cname`, or the first `Var`/`Wild`
    /// arm. Returns `(body_reg, body_ty)`.
    #[allow(clippy::too_many_arguments)]
    fn lower_static_match_arm(
        &mut self,
        scrutinee_reg: usize,
        scrutinee_vty: &ValueTy,
        sum_name: &str,
        ctors: &[(String, Vec<ValueTy>)],
        scrutinee_args: &[ValueTy],
        is_builtin_sum: bool,
        arms: &[MatchArm],
        cname: &str,
        span: Span,
    ) -> Result<(usize, ValueTy), CompileError> {
        // The first arm that matches at runtime: the Ctor arm for `cname`, or a
        // Var/Wild arm (a Var binds the whole scrutinee, a Wild ignores it).
        let selected = arms
            .iter()
            .position(|arm| match arm.pattern.strip_bang() {
                Pattern::Ctor(c, _) => c == cname,
                Pattern::Var(_) | Pattern::Wild => true,
                _ => false,
            })
            .ok_or_else(|| CompileError::Type {
                msg: format!("match over `{cname}` has no matching arm"),
                span,
            })?;
        let arm = &arms[selected];
        // Reachable only from the guard-free static fast path (`!has_guards`),
        // so every arm here is a bare `pattern -> body`; a guarded arm is a
        // defensive error rather than a silent drop.
        let body = crate::reduce::unguarded_arm_body(arm).ok_or_else(|| CompileError::Type {
            msg: "guarded arm reached the static match fast path".into(),
            span,
        })?;
        // Bind the arm's pattern leaves in a fresh scope, visible to the body.
        self.value_locals.push(HashMap::new());
        self.value_local_ctors.push(HashMap::new());
        // A bang pattern forces the matched value before binding it (`!x`).
        // The static path matched the constructor syntactically, so this is the
        // only force the arm performs.
        let scrutinee_reg = self.force_bang_markers(&arm.pattern, scrutinee_reg);
        let result = match arm.pattern.strip_bang() {
            Pattern::Var(name) => {
                // A variable arm binds the whole scrutinee (its ctor is
                // statically known, so a nested match over it resolves too).
                self.value_locals
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), (scrutinee_reg, scrutinee_vty.clone()));
                self.value_local_ctors
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), Some(cname.to_string()));
                self.lower_value(body)
            }
            Pattern::Wild => self.lower_value(body),
            Pattern::Ctor(ctor, args) => {
                let ctor_idx = ctors.iter().position(|(c, _)| c == ctor).ok_or_else(|| {
                    CompileError::Type {
                        msg: format!("unknown constructor `{ctor}` for `{sum_name}`"),
                        span,
                    }
                })?;
                let payload_tys = &ctors[ctor_idx].1;
                let mut payload_regs = Vec::with_capacity(payload_tys.len());
                for _ in 0..payload_tys.len() {
                    payload_regs.push(self.fresh_value_reg());
                }
                self.emit_value(ValueInstr::ValueMatch {
                    dst: payload_regs.clone(),
                    slot: scrutinee_reg,
                    ctor: ctor_idx as u32,
                });
                // Bind the arm's payload vars to the match's payload regs.
                // Builtin-sum payload shapes carry placeholder type params;
                // resolve them against the scrutinee's concrete type args
                // (`Var(k)` → `scrutinee_args[k-1]`, mirroring inference).
                for (idx, p) in args.iter().enumerate() {
                    let Pattern::Var(name) = p.strip_bang() else {
                        continue;
                    };
                    // A bang sub-pattern (`Just !x`) forces its payload.
                    let payload_reg = self.force_bang_markers(p, payload_regs[idx]);
                    let raw = payload_tys.get(idx).cloned().unwrap_or(ValueTy::Double);
                    let pty = if is_builtin_sum {
                        match raw {
                            ValueTy::Var(k) => scrutinee_args
                                .get(k.saturating_sub(1) as usize)
                                .cloned()
                                .unwrap_or(ValueTy::Double),
                            t => t,
                        }
                    } else {
                        raw
                    };
                    self.value_locals
                        .last_mut()
                        .unwrap()
                        .insert(name.clone(), (payload_reg, pty));
                }
                self.lower_value(body)
            }
            _ => self.lower_value(body),
        };
        self.value_locals.pop();
        self.value_local_ctors.pop();
        result
    }

    /// Lower the runtime test for one pattern against `scrutinee_reg`, starting
    /// at `self.cur_value_block`. On a match, control flow reaches the returned
    /// block (where the arm's guards and body continue); on a mismatch it jumps
    /// to `else_t`. A `Ctor` pattern resolves its sum/ctor index by constructor
    /// name (`sum_ctor`) and extracts the payload via `ValueMatch`, then
    /// recurses over its argument patterns; `Lit` patterns emit a constant +
    /// `ValueCompare`/`Branch`. Variable leaves bind into the scope on top of
    /// `self.value_locals` (pushed by the caller around each arm). Builtin-sum
    /// payload placeholders (`Var(k)`) resolve against the scrutinee's concrete
    /// type args, falling back to `top_args` (recovered in the match arm) when
    /// `vty` carries none.
    fn lower_pattern_test(
        &mut self,
        pattern: &Pattern,
        scrutinee_reg: usize,
        vty: &ValueTy,
        top_args: &[ValueTy],
        else_t: usize,
        span: Span,
    ) -> Result<usize, CompileError> {
        // `!pat` forces the matched value to WHNF before the inner pattern is
        // tested; the stripped pattern then binds against the forced register.
        let scrutinee_reg = self.force_bang_markers(pattern, scrutinee_reg);
        let pattern = pattern.strip_bang();
        match pattern {
            Pattern::Var(name) => {
                // Binds the whole matched value; no test is emitted.
                self.value_locals
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), (scrutinee_reg, vty.clone()));
                Ok(self.cur_value_block)
            }
            Pattern::Wild => Ok(self.cur_value_block),
            Pattern::IntegerLit(v) => {
                // The pattern literal takes its representation from the
                // SCRUTINEE's type — the one expected type a pattern has. An
                // `Integer`/`Double` scrutinee therefore compares against an
                // `Integer`/`Double` constant built from the arbitrary-precision
                // value, not against a truncated `Int` (a cross-kind comparison
                // that silently never matched).
                let lit_reg = self.fresh_value_reg();
                match vty {
                    ValueTy::Integer => self.emit_value(ValueInstr::ValueConstInteger {
                        dst: lit_reg,
                        value: Box::new(v.clone()),
                    }),
                    ValueTy::Double => self.emit_value(ValueInstr::ValueConstDouble {
                        dst: lit_reg,
                        value: literal_to_f64(v),
                    }),
                    _ => self.emit_value(ValueInstr::ValueConstInt {
                        dst: lit_reg,
                        value: literal_to_i64(v),
                    }),
                }
                Ok(self.emit_literal_test(scrutinee_reg, lit_reg, else_t))
            }
            Pattern::FractionalLit(v, _) => {
                let lit_reg = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstDouble {
                    dst: lit_reg,
                    value: rational_to_f64(v),
                });
                Ok(self.emit_literal_test(scrutinee_reg, lit_reg, else_t))
            }
            Pattern::LitStr(v) => {
                let lit_reg = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueConstString {
                    dst: lit_reg,
                    value: v.clone(),
                });
                Ok(self.emit_literal_test(scrutinee_reg, lit_reg, else_t))
            }
            Pattern::Ctor(name, args) => {
                let (ctor_sum, ctor_idx, payload) =
                    self.sum_ctor(name).ok_or_else(|| CompileError::Type {
                        msg: format!("unknown constructor `{name}`"),
                        span,
                    })?;
                let is_builtin = self.env.type_ctor_arity(&ctor_sum).is_some();
                // The concrete type args of the scrutinee at this level: a
                // concrete `App`/`Data` type carries them; a bare construction
                // (`Data(name, [])`) or an unbound wire falls back to the
                // top-level recovered args.
                let scrutinee_args: Vec<ValueTy> = match vty {
                    ValueTy::Con(_, args) if !args.is_empty() => args.clone(),
                    _ => top_args.to_vec(),
                };
                let payload_regs: Vec<usize> =
                    (0..payload.len()).map(|_| self.fresh_value_reg()).collect();
                let bind_b = self.new_value_block();
                let cur = self.cur_value_block;
                self.set_value_term(
                    cur,
                    ValueTerm::BranchCtor {
                        slot: scrutinee_reg,
                        ctor: ctor_idx as u32,
                        then: bind_b,
                        els: else_t,
                    },
                );
                self.cur_value_block = bind_b;
                self.emit_value(ValueInstr::ValueMatch {
                    dst: payload_regs.clone(),
                    slot: scrutinee_reg,
                    ctor: ctor_idx as u32,
                });
                // Recurse per argument against its payload reg; the last
                // sub-test's matched block is where the arm continues.
                let mut pass = self.cur_value_block;
                for (i, arg) in args.iter().enumerate() {
                    let raw = payload.get(i).cloned().unwrap_or(ValueTy::Double);
                    let pty = if is_builtin {
                        match raw {
                            ValueTy::Var(k) => scrutinee_args
                                .get(k.saturating_sub(1) as usize)
                                .cloned()
                                .unwrap_or(ValueTy::Double),
                            t => t,
                        }
                    } else {
                        raw
                    };
                    // The payload's OWN type parameters belong to this level, so
                    // a nested pattern resolves its placeholders against them —
                    // not against the enclosing level's args. `Just (Just x)`
                    // binds `x` from the INNER `Maybe`'s argument; reusing the
                    // outer `[Maybe a]` typed `x` as a whole `Maybe`, and a
                    // method call on it then dispatched on the wrong type.
                    let sub_args: Vec<ValueTy> = match &pty {
                        ValueTy::Con(_, a) => a.clone(),
                        _ => Vec::new(),
                    };
                    pass = self.lower_pattern_test(
                        arg,
                        payload_regs[i],
                        &pty,
                        &sub_args,
                        else_t,
                        span,
                    )?;
                }
                Ok(pass)
            }
            // `strip_bang` removed every marker before this match.
            Pattern::Bang(_) => unreachable!("bang markers are stripped before the pattern test"),
        }
    }

    /// Emit one `ValueInstr::ValueForce` per bang marker on `pattern`,
    /// returning the register the stripped pattern should test/bind against.
    /// Forcing is to WHNF only — no deep strictness.
    fn force_bang_markers(&mut self, pattern: &Pattern, reg: usize) -> usize {
        let mut reg = reg;
        let mut p = pattern;
        while let Pattern::Bang(inner) = p {
            let forced = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueForce {
                dst: forced,
                src: reg,
            });
            reg = forced;
            p = inner;
        }
        reg
    }

    /// Emit the literal-equality test for a matched scalar: `ValueCompare { Eq }`
    /// against `lit_reg`, branching to a fresh pass block on equality and
    /// `else_t` otherwise. Returns the pass block.
    fn emit_literal_test(&mut self, scrutinee_reg: usize, lit_reg: usize, else_t: usize) -> usize {
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueCompare {
            dst,
            op: CmpOp::Eq,
            a: scrutinee_reg,
            b: lit_reg,
        });
        let pass_b = self.new_value_block();
        let cur = self.cur_value_block;
        // The comparison produced an ordinary `Bool` SUM value, so the test is
        // the same constructor-tag branch every other data match uses.
        self.set_value_term(
            cur,
            ValueTerm::BranchCtor {
                slot: dst,
                ctor: crate::arena::BOOL_TRUE_TAG,
                then: pass_b,
                els: else_t,
            },
        );
        self.cur_value_block = pass_b;
        pass_b
    }

    /// Lower an arm's guard/body alternatives starting at `self.cur_value_block`
    /// (the block where the pattern test succeeded). Each body runs in its own
    /// block and ends with `ValueMove { dst: out, src: body_reg }` +
    /// `Fallthrough(join)`; a failing guard falls through to the next
    /// alternative (or `else_t` after the last). Returns the first
    /// alternative's body type.
    fn lower_match_guards_and_body(
        &mut self,
        arm: &MatchArm,
        out: &usize,
        join: usize,
        else_t: usize,
        expected: Option<&ValueTy>,
    ) -> Result<ValueTy, CompileError> {
        let mut first_ty: Option<ValueTy> = None;
        let m = arm.guards.len();
        for (idx, (g, body)) in arm.guards.iter().enumerate() {
            let next = if idx + 1 < m {
                self.new_value_block()
            } else {
                else_t
            };
            let Some(g) = g else {
                // Bare `pattern -> body`: no guard to test. The body carries the
                // MATCH's expected type, exactly as an `if` branch does: an arm
                // body that is an integer literal takes its representation from
                // there (`f :: Integer -> Integer; f p = case p of { 2 -> 5; _ ->
                // p }`), instead of defaulting to `Int` and disagreeing with the
                // other arm's `Integer`.
                let (body_reg, body_ty) = match expected {
                    Some(exp) => self.lower_value_expected(body, Some(exp))?,
                    None => self.lower_value(body)?,
                };
                self.emit_value(ValueInstr::ValueMove {
                    dst: *out,
                    src: body_reg,
                });
                self.set_value_term(self.cur_value_block, ValueTerm::Fallthrough(join));
                if first_ty.is_none() {
                    first_ty = Some(body_ty);
                }
                break;
            };
            let (guard_reg, guard_ty) = self.lower_value(g)?;
            if guard_ty != ValueTy::bool_ty() {
                return Err(CompileError::Type {
                    msg: "match guard must be a Bool value".into(),
                    span: g.span(),
                });
            }
            let body_b = self.new_value_block();
            // A guard evaluates to a `Bool` SUM value, so the test is the
            // constructor-tag branch (`True` is tag 1).
            self.set_value_term(
                self.cur_value_block,
                ValueTerm::BranchCtor {
                    slot: guard_reg,
                    ctor: crate::arena::BOOL_TRUE_TAG,
                    then: body_b,
                    els: next,
                },
            );
            self.cur_value_block = body_b;
            let (body_reg, body_ty) = match expected {
                Some(exp) => self.lower_value_expected(body, Some(exp))?,
                None => self.lower_value(body)?,
            };
            self.emit_value(ValueInstr::ValueMove {
                dst: *out,
                src: body_reg,
            });
            self.set_value_term(self.cur_value_block, ValueTerm::Fallthrough(join));
            if first_ty.is_none() {
                first_ty = Some(body_ty);
            }
            self.cur_value_block = next;
        }
        Ok(first_ty.unwrap_or(ValueTy::Double))
    }

    /// Resolve a bare constructor name to `(sum type, ctor index, payload)`.
    /// Returns `None` when the name is not a constructor or is ambiguous
    /// (declared in multiple sum types — inference already rejected that).
    fn sum_ctor(&self, name: &str) -> Option<(String, usize, Vec<ValueTy>)> {
        let mut found: Option<(String, usize, Vec<ValueTy>)> = None;
        for (tname, info) in &self.env.data_types {
            if let DataInfo::Sum(ctors) = info {
                for (idx, (c, payload)) in ctors.iter().enumerate() {
                    if c == name {
                        if found.is_some() {
                            return None;
                        }
                        found = Some((
                            tname.clone(),
                            idx,
                            payload.iter().map(|(t, _)| t.clone()).collect(),
                        ));
                    }
                }
            }
        }
        found
    }

    /// Strictness flags per payload position of a sum constructor: `true`
    /// where the payload type carried a Haskell 2010 strictness flag
    /// (`data T = C !Int`), so lowering forces that argument to WHNF at
    /// construction. Empty for non-sum or out-of-range lookups.
    fn ctor_payload_bangs(&self, sum_name: &str, ctor_idx: usize) -> Vec<bool> {
        match self.env.data_types.get(sum_name) {
            Some(DataInfo::Sum(ctors)) => ctors
                .get(ctor_idx)
                .map(|(_, payload)| payload.iter().map(|(_, b)| *b).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Number of arena slots a value of the given static type occupies when
    /// fully allocated: the root slot plus its transitive field/payload
    /// subtree. Drives the value-arena capacity bound — a compound value
    /// output pins its whole subtree across ticks (the output keeps the root
    /// at rc >= 1, so `drop_ref` never reaches 0 on the children), so the
    /// bound must add the subtree size per output channel, not one slot.
    ///
    /// Shapes: a record is `1 + Σ field subtrees`; a sum is `1 + the largest
    /// ctor's payload subtree` (each ctor's payload subtree is the sum of its
    /// entries); a newtype is `1 + inner subtree`; scalars, func values and
    /// unbound variables occupy exactly one slot. Builtin collections are
    /// exact too: `List`/`Set` are `1 + cap × elem subtree`, `Map` is
    /// `1 + cap × (key subtree + value subtree)`, `Pair` is `1 + both
    /// subtrees`, `Maybe` is `1 + the inner subtree` and `Either` is
    /// `1 + the larger arm's subtree` (the smaller arm can hold no value at
    /// runtime). Recursive data types are cycle-guarded (a re-entered type
    /// contributes one slot) — v1 values are finite literal constructions, so
    /// the bound stays finite.
    fn subtree_size(&self, vty: &ValueTy) -> usize {
        self.subtree_size_impl(vty, &mut HashSet::new())
    }

    fn subtree_size_impl(&self, vty: &ValueTy, visiting: &mut HashSet<String>) -> usize {
        match vty {
            ValueTy::Int
            | ValueTy::Double
            // `Integer` is a leaf payload of the same representation cost as
            // `Int`: one arena slot, no nested subtree.
            | ValueTy::Integer
            | ValueTy::String
            | ValueTy::Buffer(_)
            | ValueTy::Void
            | ValueTy::Thunk(_, _)
            | ValueTy::Var(_)
            | ValueTy::TyConVar(_)
            // A dictionary is one arena slot: it is not a type whose shape
            // implies a nested value subtree.
            | ValueTy::Dict(_) => 1,
            ValueTy::TyConApp(_, args) => {
                1 + args
                    .iter()
                    .map(|t| self.subtree_size_impl(t, visiting))
                    .sum::<usize>()
            }
            ValueTy::Con(name, args) => match name.as_str() {
                "Maybe" => {
                    1 + args
                        .first()
                        .map(|t| self.subtree_size_impl(t, visiting))
                        .unwrap_or(1)
                }
                "Pair" => {
                    1 + args
                        .iter()
                        .map(|t| self.subtree_size_impl(t, visiting))
                        .sum::<usize>()
                }
                "Either" => {
                    1 + args
                        .iter()
                        .map(|t| self.subtree_size_impl(t, visiting))
                        .max()
                        .unwrap_or(1)
                }
                "List" | "Set" => {
                    // An empty argument list means the collection's element type
                    // was not known at this site (`list` as a bare value, a
                    // partial head). The budget is a static heuristic, so the
                    // element defaults to one slot rather than panicking.
                    let elem = args.first().unwrap_or(&ValueTy::Double);
                    1 + Self::ELEM_EST * self.subtree_size_impl(elem, visiting)
                }
                "Array" => {
                    // An immutable array holds a flat element Vec plus its
                    // stored bounds (a `Pair i i`). Unlike an open List —
                    // bounded by the ELEM_EST heuristic — an array can
                    // materialize a WHOLE signal block through the buffer
                    // bridge (`arrayFromBuffer` copies every live sample into
                    // its own arena slot).
                    //
                    // The bound below is the block size the DEFAULT entry
                    // points use (the `compile`/`compile_eval` family fixes
                    // `BUF` at 256). It is not a hard invariant: every entry is
                    // const-generic over `BUF` and `compile_to_ir` is
                    // block-size agnostic, so the same IR run with `BUF = 1024`
                    // would under-budget the array 4x and `arrayFromBuffer`
                    // would latch a clean exhaustion. Nothing breaks in-tree
                    // today (every caller uses 256).
                    // TODO: parameterise this estimate over `BUF` once lowering
                    // is told the block size.
                    let elem = args.last().unwrap_or(&ValueTy::Double);
                    1 + Self::ARRAY_ELEM_EST * self.subtree_size_impl(elem, visiting)
                }
                "Map" => {
                    // An unknown key/value type (a bare `empty_map` value, a
                    // partial head) defaults to one slot each; the budget is a
                    // static heuristic, never a type judgement.
                    let k = args.first().unwrap_or(&ValueTy::String);
                    let v = args.get(1).unwrap_or(&ValueTy::Double);
                    1 + Self::ELEM_EST
                        * (self.subtree_size_impl(k, visiting)
                            + self.subtree_size_impl(v, visiting))
                }
                _ => {
                    // A DECLARED data type (record, sum, or opaque token)
                    // resolves its shape through `data_types` — a question about
                    // the constructor, not a second type representation.
                    if !visiting.insert(name.clone()) {
                        return 1;
                    }
                    let s = match self.env.data_types.get(name) {
                        Some(DataInfo::Record(fields)) => {
                            1 + fields
                                .iter()
                                .map(|(_, t, _)| self.subtree_size_impl(t, visiting))
                                .sum::<usize>()
                        }
                        Some(DataInfo::Sum(ctors)) => {
                            // The constructed ctor is a runtime choice; the
                            // static bound is the largest ctor's full payload
                            // subtree.
                            1 + ctors
                                .iter()
                                .map(|(_, payload)| {
                                    payload
                                        .iter()
                                        .map(|(t, _)| self.subtree_size_impl(t, visiting))
                                        .sum::<usize>()
                                })
                                .max()
                                .unwrap_or(0)
                        }
                        // An opaque declaration's value is a handle: one slot.
                        Some(DataInfo::Opaque) | None => {
                            1 + args
                                .iter()
                                .map(|t| self.subtree_size_impl(t, visiting))
                                .sum::<usize>()
                        }
                    };
                    visiting.remove(name);
                    s
                }
            },
            ValueTy::Newtype(name, _) => {
                if !visiting.insert(name.clone()) {
                    return 1;
                }
                let inner = self
                    .env
                    .newtypes
                    .get(name)
                    .and_then(|n| self.env.vty_of_name(n))
                    .unwrap_or(ValueTy::Double);
                let s = 1 + self.subtree_size_impl(&inner, visiting);
                visiting.remove(name);
                s
            }
        }
    }

    fn lower(&mut self, e: &Expr, args: &[usize]) -> Result<Vec<usize>, CompileError> {
        match e {
            Expr::IntegerLit(v, _) => {
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueBufferConst {
                    dst,
                    value: literal_to_f64(v),
                });
                Ok(vec![dst])
            }
            Expr::FractionalLit(v, _, _) => {
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueBufferConst {
                    dst,
                    value: rational_to_f64(v),
                });
                Ok(vec![dst])
            }
            Expr::Void(span) => Err(CompileError::Type {
                msg: "the unit value `()` is a value expression; it cannot be used in a signal position".into(),
                span: *span,
            }),
            Expr::Ref(name, span) => self.lower_ref(name, args, *span),
            // A kernel is a VALUE-track operation: `__gcd`, `__ratio_percent`
            // and `__fault` all produce (or fault on) a value, never a signal
            // buffer. Reaching a signal position is a type error, reported here
            // rather than as a confusing "unknown name" later.
            Expr::Kernel { kernel, span, .. } => Err(CompileError::Type {
                msg: format!(
                    "`{}` is a value operation; a signal position requires a Buffer",
                    kernel.spelling()
                ),
                span: *span,
            }),
            Expr::Neg(inner, _) => {
                let outs = self.lower(inner, args)?;
                Ok(outs
                    .into_iter()
                    .map(|src| {
                        let dst = self.fresh_value_reg();
                        self.emit_value(ValueInstr::ValueBufferUn {
                            dst,
                            op: UnOp::Neg,
                            src,
                        });
                        dst
                    })
                    .collect())
            }
            Expr::Apply {
                name,
                args: call_args,
                span,
            } => {
                // A value-track builtin applied in a signal position
                // (`main = bufferFromArray (arrayFromBuffer buf)`): the body is
                // `Expr::Apply` because a ONE-argument application is not
                // rewritten by the currying pass, and a one-argument
                // application never consults `value_builtin`. Lower the whole
                // application as a value and require the single output to be a
                // Buffer.
                // `bufferFromArray` is the array bridge's buffer producer; every
                // other value builtin yields a value and is a type error here.
                if self.value_builtin(name, call_args.len()).is_some() {
                    let (reg, ty) = self.lower_value(e)?;
                    if matches!(ty, ValueTy::Buffer(_)) {
                        return Ok(vec![reg]);
                    }
                    return Err(CompileError::Type {
                        msg: format!(
                            "`{name}` produces {ty:?}; a signal position requires a Buffer"
                        ),
                        span: *span,
                    });
                }
                if name == "smooth" {
                    let x_regs = self.lower(&call_args[0], args)?;
                    let x = x_regs[0];
                    let ms = self.caf_const(&call_args[1]).unwrap_or(0.0);
                    return Ok(vec![self.emit_smooth(x, ms)]);
                }
                // A data constructor in a signal position is a type error: value
                // expressions lower on the value track (see `lower_value`).
                if self.env.data_types.contains_key(name.as_str()) || self.sum_ctor(name).is_some()
                {
                    return Err(CompileError::Type {
                        msg: format!(
                            "`{name}` is a value constructor; it cannot be used in a signal expression"
                        ),
                        span: *span,
                    });
                }
                // Foreign (FFI) declaration: walk the language-side signature,
                // folding scalar params and lowering signal args interleaved — a
                // single positional cursor over call_args mirrors inference
                // (`gain _ 0.5` → the Wire at [0], the constant at [1]).
                if let Some(fsig) = self.env.foreign_sigs.get(name.as_str()).cloned() {
                    if let Some(sig) = crate::types::ffi::ffi_sig_from_typeexpr(self.env, name, &fsig) {
                        let variadic_idx = sig
                            .params
                            .iter()
                            .position(|p| {
                                matches!(p, crate::types::ffi::FfiParam::VariadicSignal(_))
                            });
                        let mut param_values = Vec::new();
                        let mut param_bindings = Vec::new();
                        let mut signal_srcs = Vec::new();
                        let mut resource: Option<String> = None;
                        let mut tape_index: Option<usize> = None;
                        let mut pos = 0usize;
                        let mut signal_index = 0usize;
                        for p in &sig.params {
                            match p {
                                crate::types::ffi::FfiParam::Signal(_) => {
                                    let src = if pos >= call_args.len() {
                                        // Combinator-fed signal: no explicit
                                        // call arg — bind the next wiring
                                        // register (the `(_, _) :> dry_wet`
                                        // style feeds the channels via the
                                        // combinator).
                                        if signal_index >= args.len() {
                                            return Err(CompileError::Type {
                                                msg: format!(
                                                    "missing signal input for `{name}`"
                                                ),
                                                span: *span,
                                            });
                                        }
                                        args[signal_index]
                                    } else {
                                        let outs = self.lower(&call_args[pos], args)?;
                                        *outs.first().ok_or_else(|| CompileError::Type {
                                            msg: format!(
                                                "signal argument {pos} of `{name}` has no outputs"
                                            ),
                                            span: call_args[pos].span(),
                                        })?
                                    };
                                    // A foreign signal slot is a single channel in v1.
                                    signal_srcs.push(src);
                                    signal_index += 1;
                                    pos += 1;
                                }
                                crate::types::ffi::FfiParam::VariadicSignal(_) => {
                                    // The variadic's explicit signal span stops
                                    // before the trailing non-variadic params
                                    // (each consumes one trailing call arg —
                                    // mixer's trailing `Record` config).
                                    let trailing = variadic_idx.map(|i| sig.params.len() - i - 1)
                                    .unwrap_or(0);
                                    let end = call_args.len().saturating_sub(trailing);
                                    for arg in call_args[pos..end].iter() {
                                        // Each arg in the variadic span is a
                                        // signal expression feeding one channel.
                                        let outs = self.lower(arg, args)?;
                                        let src = *outs.first().ok_or_else(
                                            || CompileError::Type {
                                                msg: format!(
                                                    "signal argument of `{name}` has no outputs"
                                                ),
                                                span: arg.span(),
                                            },
                                        )?;
                                        signal_srcs.push(src);
                                    }
                                    signal_index += end.saturating_sub(pos);
                                    // Any remaining wiring registers are the
                                    // combinator-fed signal channels (the
                                    // `(_, _) :> mixer` style feeds the channels
                                    // via `Merge`, not as explicit call args).
                                    while signal_index < args.len() {
                                        signal_srcs.push(args[signal_index]);
                                        signal_index += 1;
                                    }
                                    pos = end;
                                }
                                crate::types::ffi::FfiParam::Scalar => {
                                    if pos >= call_args.len() {
                                        break;
                                    }
                                    if let Expr::Ref(ref_name, _) = &call_args[pos] {
                                        if let Some(&pidx) = self.param_names.get(ref_name) {
                                            param_values.push(0.0);
                                            param_bindings.push((param_values.len() - 1, pidx));
                                            pos += 1;
                                            continue;
                                        }
                                    }
                                    if let Expr::ActorParam {
                                        name,
                                        default,
                                        span,
                                    } = &call_args[pos]
                                    {
                                        let default_val = if let Some(d) = default {
                                            const_f64(d).unwrap_or(0.0)
                                        } else {
                                            0.0
                                        };
                                        let idx = self.intern_param(
                                            name.clone(),
                                            default_val,
                                            f64::NEG_INFINITY,
                                            f64::INFINITY,
                                            *span,
                                        )?;
                                        param_values.push(0.0);
                                        param_bindings.push((param_values.len() - 1, idx));
                                        pos += 1;
                                        continue;
                                    }
                                    let v = self.caf_const(&call_args[pos]).ok_or_else(|| {
                                        CompileError::Type {
                                            msg: format!(
                                                "param at position {pos} of `{name}` must be a \
                                                 constant or parameter reference"
                                            ),
                                            span: call_args[pos].span(),
                                        }
                                    })?;
                                    param_values.push(v);
                                    pos += 1;
                                }
                                crate::types::ffi::FfiParam::Record(rec_ty) => {
                                    if pos >= call_args.len() {
                                        break;
                                    }
                                    // Flatten the record literal's schema fields
                                    // into `param_values` in schema order (the
                                    // factory reads its config from the flat
                                    // list), mirroring the legacy record arm. A
                                    // `BandList` field (`bands: List EqBand`)
                                    // flattens each list element (a band record)
                                    // in the band schema's field order.
                                    if let Expr::Record(fields, field_span) = &call_args[pos] {
                                        let schema = crate::types::ffi::ffi_record_schema(self.env, rec_ty)
                                        .ok_or_else(|| CompileError::Type {
                                            msg: format!(
                                                "unknown record type `{rec_ty}` for foreign `{name}`"
                                            ),
                                            span: call_args[pos].span(),
                                        })?;
                                        let mut field_values: HashMap<&str, f64> = HashMap::new();
                                        for (field_name, field_expr) in fields {
                                            if let Some(val) = self.caf_const(field_expr) {
                                                field_values.insert(field_name.as_str(), val);
                                            }
                                        }
                                        for (fname, fscalar, default) in &schema.fields {
                                            match fscalar {
                                                crate::types::ffi::FfiScalar::Double
                                                | crate::types::ffi::FfiScalar::Int => {
                                                    let val = field_values
                                                        .get(fname.as_str())
                                                        .copied()
                                                        .unwrap_or(default.unwrap_or(0.0));
                                                    param_values.push(val);
                                                }
                                                crate::types::ffi::FfiScalar::BandList(
                                                    band_ty,
                                                ) => {
                                                    // The field's value is a list
                                                    // literal of band records.
                                                    let field_expr = fields
                                                        .iter()
                                                        .find(|(n, _)| n == fname)
                                                        .map(|(_, e)| e)
                                                        .cloned();
                                                    if let Some(
                                                        Expr::ListLit(elems, _)
                                                    ) = field_expr
                                                    {
                                                        let band_schema = crate::types::ffi::ffi_record_schema(self.env, band_ty)
                                                        .ok_or_else(|| CompileError::Type {
                                                            msg: format!(
                                                                "unknown band record type `{band_ty}` for foreign `{name}`"
                                                            ),
                                                            span: *field_span,
                                                        })?;
                                                        for elem in elems {
                                                            let Expr::Record(
                                                                band_fields,
                                                                _,
                                                            ) = elem
                                                            else {
                                                                return Err(CompileError::Type {
                                                                    msg: format!(
                                                                        "band element of `{name}` must be a record literal"
                                                                    ),
                                                                    span: elem.span(),
                                                                });
                                                            };
                                                            let mut band_values: HashMap<
                                                                String,
                                                                f64,
                                                            > = HashMap::new();
                                                            for (bf, be) in band_fields {
                                                                if let Some(val) =
                                                                    self.caf_const(&be)
                                                                {
                                                                    band_values.insert(
                                                                        bf.clone(),
                                                                        val,
                                                                    );
                                                                }
                                                            }
                                                            for (bfname, bfscalar, bdefault) in
                                                                &band_schema.fields
                                                            {
                                                                match bfscalar {
                                                                    crate::types::ffi::FfiScalar::Double
                                                                    | crate::types::ffi::FfiScalar::Int => {
                                                                        param_values.push(
                                                                            band_values
                                                                                .get(bfname.as_str())
                                                                                .copied()
                                                                                .unwrap_or(
                                                                                    bdefault
                                                                                        .unwrap_or(
                                                                                            0.0,
                                                                                        ),
                                                                                ),
                                                                        );
                                                                    }
                                                                    _ => {}
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        for (field_name, field_expr) in fields {
                                            if let Some(val) = self.caf_const(field_expr) {
                                                self.intern_param(
                                                    field_name.clone(),
                                                    val,
                                                    f64::NEG_INFINITY,
                                                    f64::INFINITY,
                                                    *field_span,
                                                )?;
                                            }
                                        }
                                    }
                                    pos += 1;
                                }
                                crate::types::ffi::FfiParam::Resource(_) => {
                                    if pos >= call_args.len() {
                                        break;
                                    }
                                    // A resource arg is either a symbolic `Ref`
                                    // to a declared tape name or an inline
                                    // `tape_loop <capacity>` constructor. The
                                    // new DSL path resolves both to a tape INDEX
                                    // (the program's `Vec<SharedCell>`); a `Ref`
                                    // absent from the declared-tape map stays a
                                    // name-based resource binding, which seating
                                    // rejects as an undeclared resource.
                                    match &call_args[pos] {
                                        Expr::Ref(res_name, _) => {
                                            match self.tape_decls.get(res_name.as_str()) {
                                                Some(&cap) => {
                                                    tape_index = Some(
                                                        self.resolve_named_tape(res_name, cap),
                                                    );
                                                }
                                                None => {
                                                    resource = Some(res_name.clone());
                                                }
                                            }
                                        }
                                        Expr::Apply { name: ctor, args: ctor_args, .. }
                                            if ctor == "tape_loop" =>
                                        {
                                            // `tape_loop : Int -> Tape Double` —
                                            // exactly one integer-constant arg.
                                            let cap = match ctor_args.as_slice() {
                                                [Expr::IntegerLit(v, _)] => {
                                                    literal_to_i64(v) as usize
                                                }
                                                _ => {
                                                    return Err(CompileError::Type {
                                                        msg: "tape_loop takes exactly one positive integer capacity"
                                                            .to_string(),
                                                        span: call_args[pos].span(),
                                                    })
                                                }
                                            };
                                            if cap == 0 {
                                                return Err(CompileError::Type {
                                                    msg: "tape_loop capacity must be > 0"
                                                        .to_string(),
                                                    span: call_args[pos].span(),
                                                });
                                            }
                                            tape_index = Some(self.resolve_tape(cap));
                                        }
                                        other => {
                                            return Err(CompileError::Type {
                                                msg: format!(
                                                    "resource argument of `{name}` must be a symbolic reference or a `tape_loop` constructor"
                                                ),
                                                span: other.span(),
                                            });
                                        }
                                    }
                                    pos += 1;
                                }
                            }
                        }
                        let instance = self.builtins.len();
                        self.builtins.push(BuiltinInstance {
                            name: name.clone(),
                            params: param_values,
                            resource,
                            tape_index,
                            kind: crate::builtin::BuiltinKind::Block,
                            signal_ins: signal_srcs.len(),
                            signal_outs: sig.signal_outs,
                            param_bindings,
                        });
                        let fst = self.fresh_value_reg();
                        for _ in 1..sig.signal_outs {
                            self.fresh_value_reg();
                        }
                        self.emit_value(ValueInstr::ValueCallForeign {
                            dst: fst,
                            srcs: signal_srcs,
                            instance,
                        });
                        return Ok((0..sig.signal_outs).map(|i| fst + i).collect());
                    }
                }
                let mut arg_regs = Vec::new();
                for a in call_args {
                    arg_regs.extend(self.lower(a, args)?);
                }
                // User-defined function: prepend λ-arguments to caller's args. The Anchor
                // splits by params().len() — first N entries fill the scope, remainder are
                // passed to the body. Builtins receive arg_regs directly (no λ-params).
                if self.defs.contains_key(name) {
                    let mut combined = arg_regs.clone();
                    combined.extend_from_slice(args);
                    self.lower_ref(name, &combined, *span)
                } else {
                    self.lower_ref(name, &arg_regs, *span)
                }
            }
            Expr::Str(_, span) => Err(CompileError::Type {
                msg: "string literal is only valid as a parameter name".into(),
                span: *span,
            }),
            Expr::Let {
                defs,
                body,
                span: _,
            } => {
                let saved = self.defs.clone();
                for d in defs {
                    self.defs.insert(d.name().to_string(), d.clone());
                }
                let result = self.lower(body, args);
                self.defs = saved;
                result
            }
            Expr::Record(..) => unreachable!("Record should be desugared before lowering"),
            Expr::ActorParam {
                name,
                default,
                span,
            } => {
                let default_val = if let Some(d) = default {
                    const_f64(d).unwrap_or(0.0)
                } else {
                    0.0
                };
                let full_name = self.actor_param_prefix(name);
                let idx = self.intern_param(
                    full_name,
                    default_val,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    *span,
                )?;
                let dst = self.fresh_value_reg();
                self.emit_value(ValueInstr::ValueBufferParamRead {
                    dst,
                    idx,
                });
                Ok(vec![dst])
            }
            Expr::FieldProject { span, .. } => Err(CompileError::Type {
                msg:
                    "field projection is a value expression; it cannot be used in a signal position"
                        .into(),
                span: *span,
            }),
            Expr::ApplyExpr { span, .. } => {
                // A Buffer-valued application (`sine 440.0 1.0 0.0`,
                // `lowpass 1000.0 0.7 (sine …)`) lowers through the value
                // track, where first-class Buffer values and FFI arrows live.
                // The single resulting wire feeds the block outputs like any
                // other signal expression.
                let (reg, ty) = self.lower_value(e)?;
                if matches!(ty, ValueTy::Buffer(_)) {
                    Ok(vec![reg])
                } else {
                    Err(CompileError::Type {
                        msg: format!(
                            "expression application in a signal position must produce a Buffer, got {ty:?}"
                        ),
                        span: *span,
                    })
                }
            }
            // `e :: T` erases at lowering (Haskell 2010 §3.12): the type is a
            // compile-time construct with no runtime presence, so the inner
            // expression is lowered in exactly the position the annotation
            // occupied. Passing the position's `args` through is what keeps a
            // top-level signal body (`main = sine 440.0 :: Buffer`) lowering
            // identically to its un-annotated spelling.
            Expr::Annot { expr, .. } => self.lower(expr, args),
            Expr::FieldUpdate { span, .. } => Err(CompileError::Type {
                msg: "field update is a value expression; it cannot be used in a signal position"
                    .into(),
                span: *span,
            }),
            Expr::Match { span, .. } => Err(CompileError::Type {
                msg: "match is a value expression; it cannot be used in a signal position".into(),
                span: *span,
            }),
            Expr::If { span, .. } => Err(CompileError::Type {
                msg: "if is a value expression; it cannot be used in a signal position".into(),
                span: *span,
            }),
            Expr::Lambda { span, .. } => Err(CompileError::Type {
                msg: "lambda is a value expression; it cannot be used in a signal position".into(),
                span: *span,
            }),
            Expr::ListLit(_, _) | Expr::MapLit(_, _) => Err(CompileError::Unsupported(
                "value expressions (list/map literals, comparisons) are not yet supported".into(),
            )),
        }
    }

    /// Intern a named parameter, returning its slot index. Repeated uses of the
    /// same name share one slot but must declare an identical default and range —
    /// a conflicting redeclaration is a compile error (avoids silent first-wins).
    #[allow(clippy::float_cmp)]
    fn intern_param(
        &mut self,
        name: String,
        default: f64,
        min: f64,
        max: f64,
        span: Span,
    ) -> Result<usize, CompileError> {
        if let Some(&idx) = self.param_names.get(&name) {
            let existing = &self.params[idx];
            if existing.default != default || existing.min != min || existing.max != max {
                return Err(CompileError::Type {
                    msg: format!(
                        "parameter `{name}` is redeclared with a different default/range; \
                         all uses of the same name must match"
                    ),
                    span,
                });
            }
            Ok(idx)
        } else {
            let idx = self.params.len();
            self.params.push(ParamDef {
                name: name.clone(),
                default,
                min,
                max,
            });
            self.param_names.insert(name, idx);
            Ok(idx)
        }
    }

    /// Build composit param name from current scope context + local name.
    fn actor_param_prefix(&self, local_name: &str) -> String {
        local_name.to_string()
    }

    /// Fold a compile-time parameter argument to a constant, resolving a
    /// reference to a closed CAF by const-folding its reduced body. Returns
    /// `None` when the argument is not a constant expression.
    /// Flatten a record literal's schema fields into `param_values` in schema
    /// order (the factory reads its config from the flat list). A `BandList`
    /// field (`bands: List EqBand`) flattens each list element in the band
    /// schema's field order. Shared by the signal and value FFI paths.
    fn flatten_record_params(
        &self,
        rec_ty: &str,
        arg: &Expr,
        name: &str,
        param_values: &mut Vec<f64>,
    ) -> Result<(), CompileError> {
        let Expr::Record(fields, field_span) = arg else {
            return Err(CompileError::Type {
                msg: format!("argument of `{name}` must be a record literal"),
                span: arg.span(),
            });
        };
        let schema = crate::types::ffi::ffi_record_schema(self.env, rec_ty).ok_or_else(|| {
            CompileError::Type {
                msg: format!("unknown record type `{rec_ty}` for foreign `{name}`"),
                span: arg.span(),
            }
        })?;
        let mut field_values: HashMap<&str, f64> = HashMap::new();
        for (field_name, field_expr) in fields {
            if let Some(val) = self.caf_const(field_expr) {
                field_values.insert(field_name.as_str(), val);
            }
        }
        for (fname, fscalar, default) in &schema.fields {
            match fscalar {
                crate::types::ffi::FfiScalar::Double | crate::types::ffi::FfiScalar::Int => {
                    let val = field_values
                        .get(fname.as_str())
                        .copied()
                        .unwrap_or(default.unwrap_or(0.0));
                    param_values.push(val);
                }
                crate::types::ffi::FfiScalar::BandList(band_ty) => {
                    // The field's value is a list literal of band records.
                    let field_expr = fields
                        .iter()
                        .find(|(n, _)| n == fname)
                        .map(|(_, e)| e)
                        .cloned();
                    if let Some(Expr::ListLit(elems, _)) = field_expr {
                        let band_schema = crate::types::ffi::ffi_record_schema(self.env, band_ty)
                            .ok_or_else(|| CompileError::Type {
                            msg: format!(
                                "unknown band record type `{band_ty}` for foreign `{name}`"
                            ),
                            span: *field_span,
                        })?;
                        for elem in elems {
                            let Expr::Record(band_fields, _) = elem else {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "band element of `{name}` must be a record literal"
                                    ),
                                    span: elem.span(),
                                });
                            };
                            let mut band_values: HashMap<String, f64> = HashMap::new();
                            for (bf, be) in band_fields {
                                if let Some(val) = self.caf_const(&be) {
                                    band_values.insert(bf.clone(), val);
                                }
                            }
                            for (bfname, bfscalar, bdefault) in &band_schema.fields {
                                match bfscalar {
                                    crate::types::ffi::FfiScalar::Double
                                    | crate::types::ffi::FfiScalar::Int => {
                                        param_values.push(
                                            band_values
                                                .get(bfname.as_str())
                                                .copied()
                                                .unwrap_or(bdefault.unwrap_or(0.0)),
                                        );
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn caf_const(&self, e: &Expr) -> Option<f64> {
        self.caf_const_impl(e, &mut HashSet::new())
    }

    /// Workhorse for [`caf_const`]: follows CAF→CAF reference chains
    /// transitively, tracking visited names so a cycle folds to `None` rather
    /// than recursing forever.
    fn caf_const_impl(&self, e: &Expr, visited: &mut HashSet<String>) -> Option<f64> {
        if let Expr::Ref(ref_name, _) = e {
            if self.cafs.contains(ref_name) {
                if visited.contains(ref_name) {
                    return None;
                }
                if let Some(Def::Eval { body, .. }) = self.defs.get(ref_name) {
                    visited.insert(ref_name.clone());
                    let v = self.caf_const_impl(body, visited);
                    visited.remove(ref_name);
                    return v;
                }
            }
        }
        const_f64(e)
    }

    fn lower_ref(
        &mut self,
        name: &str,
        args: &[usize],
        _span: Span,
    ) -> Result<Vec<usize>, CompileError> {
        if let Some(fsig) = self.env.foreign_sigs.get(name).cloned() {
            if let Some(sig) = crate::types::ffi::ffi_sig_from_typeexpr(self.env, name, &fsig) {
                let signal_ins = sig
                    .params
                    .iter()
                    .filter(|p| matches!(p, crate::types::ffi::FfiParam::Signal(_)))
                    .count();
                if sig.params.len() == signal_ins {
                    let instance = self.builtins.len();
                    self.builtins.push(BuiltinInstance {
                        name: name.to_string(),
                        params: Vec::new(),
                        resource: None,
                        tape_index: None,
                        kind: crate::builtin::BuiltinKind::Block,
                        signal_ins,
                        signal_outs: sig.signal_outs,
                        param_bindings: Vec::new(),
                    });
                    let fst = self.fresh_value_reg();
                    for _ in 1..sig.signal_outs {
                        self.fresh_value_reg();
                    }
                    let srcs = args.to_vec();
                    self.emit_value(ValueInstr::ValueCallForeign {
                        dst: fst,
                        srcs,
                        instance,
                    });
                    return Ok((0..sig.signal_outs).map(|i| fst + i).collect());
                }
            }
        }
        let bin = match name {
            "+" => Some(BinArith::Add),
            "-" => Some(BinArith::Sub),
            "*" => Some(BinArith::Mul),
            "/" => Some(BinArith::Div),
            "min" => Some(BinArith::Min),
            "max" => Some(BinArith::Max),
            "atan2" => Some(BinArith::Atan2),
            _ => None,
        };
        if let Some(op) = bin {
            let dst = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueBufferBin {
                dst,
                op,
                a: args[0],
                b: args[1],
            });
            return Ok(vec![dst]);
        }
        // Unary numeric CLASS methods (`abs`, `negate`, `signum`) in a signal
        // position: emit the SAME value-track kernel instruction the value track
        // emits, so the OPERAND dispatch decides the behaviour (a `Buffer` →
        // the whole-block `abs_slice` / element-wise path, a scalar → the scalar
        // operation). The lookup is the ONE `(class, method) → kernel` table, not
        // a spelling list — which is what keeps `abs` from being a
        // buffer-specific name branch in THIS lowering path too.
        if let Some(kernel) = crate::kernels::kernel_for_method("Num", name) {
            if matches!(
                kernel,
                crate::arena::KernelId::Abs
                    | crate::arena::KernelId::Negate
                    | crate::arena::KernelId::Signum
            ) {
                if args.len() != 1 {
                    return Err(CompileError::Type {
                        msg: format!("`{name}` expects 1 argument, got {}", args.len()),
                        span: crate::error::Span::new(0, 0),
                    });
                }
                let dst = self.fresh_value_reg();
                let src = args[0];
                self.emit_value(match kernel {
                    crate::arena::KernelId::Abs => ValueInstr::ValueAbs { dst, src },
                    crate::arena::KernelId::Negate => ValueInstr::ValueNegate { dst, src },
                    _ => ValueInstr::ValueSignum { dst, src },
                });
                return Ok(vec![dst]);
            }
        }
        let un = match name {
            "sin" => Some(UnOp::Sin),
            "cos" => Some(UnOp::Cos),
            "tan" => Some(UnOp::Tan),
            "sqrt" => Some(UnOp::Sqrt),
            "exp" => Some(UnOp::Exp),
            "ln" => Some(UnOp::Ln),
            "tanh" => Some(UnOp::Tanh),
            _ => None,
        };
        if let Some(op) = un {
            let dst = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueBufferUn {
                dst,
                op,
                src: args[0],
            });
            return Ok(vec![dst]);
        }
        if let Some(&idx) = self.param_names.get(name) {
            let dst = self.fresh_value_reg();
            self.emit_value(ValueInstr::ValueBufferParamRead { dst, idx });
            return Ok(vec![dst]);
        }
        for scope in self.locals.iter().rev() {
            if let Some(regs) = scope.get(name) {
                return Ok(regs.clone());
            }
        }
        let def = self
            .defs
            .get(name)
            .cloned()
            .ok_or_else(|| CompileError::Type {
                msg: format!("unknown `{name}` in value lowering"),
                span: crate::error::Span::new(0, 0),
            })?;
        match def {
            // A definition's parameters ARE its body's leading λ-nest (Haskell
            // 2010 §4.4.3.1: `f x = e` IS `f = \x -> e`), so there is no
            // separate parameter list to bind here — the λ arm below binds them.
            Def::Eval { ref body, .. } => {
                if let Expr::Lambda {
                    params: lp,
                    body: lbody,
                    span: lspan,
                } = body
                {
                    // A named lambda applied in signal position binds its
                    // parameters to the caller's block registers (value args
                    // materialise as their block regs; a trailing signal wire is
                    // the caller's input block) and lowers the body INLINE —
                    // v1 signal-lambda semantics are call-site wire-binding
                    // (macro-style), consistent with the spec's "signal wires
                    // bind per call" rule. No fragment is involved: the value
                    // args are already block regs (Const/ReadMainCell/param),
                    // so `x * g` lowers as a block-track `Bin`.
                    let n = lp.len();
                    if args.len() < n {
                        return Err(CompileError::Type {
                            msg: format!(
                                "lambda `{name}` applied in signal position expects at least \
                                 {n} argument(s), got {}",
                                args.len()
                            ),
                            span: *lspan,
                        });
                    }
                    let mut scope = HashMap::new();
                    for (idx, p) in lp.iter().enumerate() {
                        scope.insert(p.name.clone(), vec![args[idx]]);
                    }
                    self.locals.push(scope);
                    let out = self.lower(lbody, &args[n..]);
                    self.locals.pop();
                    return out;
                }
                if self.cafs.contains(name) {
                    // The cache is keyed by (name, call-site args): Task 5
                    // threads the caller's signal args into a value-CAF's body
                    // (a signal-lambda macro), so the lowered body is
                    // SITE-DEPENDENT. Two sites applying the same CAF to
                    // different wires must each lower with their own wire
                    // (`main = (amp2 _) , (amp2 _)`), not reuse the first
                    // site's registers. Genuine signal CAFs (empty signal-ins)
                    // never consume the threaded args, so identical args reuse
                    // the shared entry and only distinct-arg sites duplicate
                    // the (still correct) body.
                    let key = (name.to_string(), args.to_vec());
                    if let Some(regs) = self.caf_cache.get(&key) {
                        return Ok(regs.clone());
                    }
                    if self.caf_lifting.contains(name) {
                        if self.mode == Mode::Strict {
                            return Err(CompileError::Type {
                                msg: format!("recursive CAF definition `{name}`"),
                                span: _span,
                            });
                        }
                        // Complete mode: recursion is allowed (lazy value
                        // track). Break the cycle with a fresh placeholder
                        // register — thunk-emitting lowering arrives in a
                        // later task; until then the recursive reference
                        // reads as silence (an unbound buffer register).
                        let dst = self.fresh_value_reg();
                        return Ok(vec![dst]);
                    }
                    // Lift the closed body once: no signal inputs, so lower it
                    // with the caller's argument registers threaded through
                    // (a closed value def applied in signal position, e.g.
                    // `amp2 = amp 2.0; main = amp2 _`, is a signal-lambda macro:
                    // the value args are consumed positionally and the caller's
                    // signal args flow into the body). A genuine signal CAF has
                    // an empty signal-input set, so the threaded args are simply
                    // never consumed. The output registers are cached for sharing.
                    self.caf_lifting.insert(name.to_string());
                    let res = self.lower(body, args);
                    self.caf_lifting.remove(name);
                    let out = res?;
                    self.caf_cache.insert(key, out.clone());
                    return Ok(out);
                }
                self.lower(body, args)
            }
            _ => Err(CompileError::Type {
                msg: format!("`{name}` is a type declaration, not a signal definition"),
                span: _span,
            }),
        }
    }

    /// Lower a signal expression as a `Value::Buffer` value-track register, for an
    /// input-channel primitive. A bare reference to a program BUFFER input
    /// resolves to the input's buffer register; any
    /// other signal computation (`onepole 200.0 0.7 i1`) lowers through the signal
    /// path (`self.lower`), whose `ValueBuffer*` ops share the value track.
    /// Coerce a value-track operand into a `Buffer` operand for an element-wise
    /// op: a `Buffer` value passes through; a scalar register is broadcast into
    /// a buffer constant (`i1 * 0.5` → the constant fills a buffer).
    fn buffer_operand(&mut self, reg: usize, ty: &ValueTy) -> Result<usize, CompileError> {
        if matches!(ty, ValueTy::Buffer(_)) {
            return Ok(reg);
        }
        let dst = self.fresh_value_reg();
        self.emit_value(ValueInstr::ValueBufferFill { dst, src: reg });
        Ok(dst)
    }
}

/// The reserved spelling of a method-instantiation parameter: `$` cannot occur
/// in a source identifier, so a renamed binding can never shadow — or be
/// shadowed by — a user name.
fn method_param_name(fresh: usize, index: usize) -> String {
    format!("$m{fresh}_{index}")
}

/// Rename the bound occurrences of `renames` throughout `e`, capture-avoidingly.
///
/// `e` is an instance method body and `renames` maps the method's OWN parameter
/// names to their fresh `$`-prefixed replacements. The traversal stops at every
/// construct that rebinds a name, so a nested lambda, a `let`/`where` binding, a
/// pattern variable or a `do`-bound name that happens to share a spelling keeps
/// its own binding: only the method's own parameters are renamed, never a binder
/// the body introduces. That is what makes the substitution transparent.
///
/// `where` clauses reach lowering as `Expr::Let` (see `desugar::attach_where`),
/// so the `Expr::Let` arm covers them: the bound names are dropped from the
/// substitution while descending into their bodies, and the `where` bindings'
/// own right-hand sides are still renamed — exactly Haskell 2010 §4.4.3.3
/// scoping, where a `where` binding sees the definition's parameters.
///
/// Deliberately NOT [`crate::reduce::substitute`]: that one substitutes
/// arbitrary expressions for names and leaves an `Expr::Apply`'s callee name
/// alone (β-reduction never renames a callee). Instantiation renames a
/// BINDING, so a name applied by name (`f io_state.second` in the `Monad IO`
/// body, which is curried into an `ApplyExpr` but can also arrive as a plain
/// `Apply`) must be renamed in callee position too.
fn rename_bound(e: &Expr, renames: &HashMap<String, String>) -> Expr {
    match e {
        Expr::Ref(name, span) => match renames.get(name) {
            Some(fresh) => Expr::Ref(fresh.clone(), *span),
            None => e.clone(),
        },
        Expr::Apply { name, args, span } => Expr::Apply {
            name: renames.get(name).cloned().unwrap_or_else(|| name.clone()),
            args: args.iter().map(|a| rename_bound(a, renames)).collect(),
            span: *span,
        },
        Expr::ApplyExpr { callee, args, span } => Expr::ApplyExpr {
            callee: Box::new(rename_bound(callee, renames)),
            args: args.iter().map(|a| rename_bound(a, renames)).collect(),
            span: *span,
        },
        // A kernel node has no name to rename; only its arguments are walked.
        Expr::Kernel { kernel, args, span } => Expr::Kernel {
            kernel: *kernel,
            args: args.iter().map(|a| rename_bound(a, renames)).collect(),
            span: *span,
        },
        Expr::Neg(inner, span) => Expr::Neg(Box::new(rename_bound(inner, renames)), *span),
        Expr::Annot { expr, ty, span } => Expr::Annot {
            expr: Box::new(rename_bound(expr, renames)),
            ty: ty.clone(),
            span: *span,
        },
        Expr::Let { defs, body, span } => {
            let mut inner = renames.clone();
            for d in defs {
                inner.remove(d.name());
            }
            Expr::Let {
                defs: defs.iter().map(|d| rename_bound_def(d, renames)).collect(),
                body: Box::new(rename_bound(body, &inner)),
                span: *span,
            }
        }
        Expr::Record(fields, span) => Expr::Record(
            fields
                .iter()
                .map(|(n, fe)| (n.clone(), rename_bound(fe, renames)))
                .collect(),
            *span,
        ),
        Expr::ActorParam {
            name,
            default,
            span,
        } => Expr::ActorParam {
            name: name.clone(),
            default: default.as_ref().map(|d| Box::new(rename_bound(d, renames))),
            span: *span,
        },
        Expr::FieldProject {
            record,
            field,
            span,
        } => Expr::FieldProject {
            record: Box::new(rename_bound(record, renames)),
            field: field.clone(),
            span: *span,
        },
        Expr::FieldUpdate {
            record,
            field,
            value,
            span,
        } => Expr::FieldUpdate {
            record: Box::new(rename_bound(record, renames)),
            field: field.clone(),
            value: Box::new(rename_bound(value, renames)),
            span: *span,
        },
        Expr::Match {
            scrutinee,
            arms,
            span,
        } => Expr::Match {
            scrutinee: Box::new(rename_bound(scrutinee, renames)),
            arms: arms
                .iter()
                .map(|arm| {
                    // A match arm's pattern binds names of its own: they shadow
                    // the substitution inside that arm's guards and body.
                    let mut inner = renames.clone();
                    for v in crate::reduce::pattern_vars(&arm.pattern) {
                        inner.remove(&v);
                    }
                    MatchArm {
                        pattern: arm.pattern.clone(),
                        guards: arm
                            .guards
                            .iter()
                            .map(|(g, b)| {
                                (
                                    g.as_ref().map(|g| rename_bound(g, &inner)),
                                    rename_bound(b, &inner),
                                )
                            })
                            .collect(),
                        span: arm.span,
                    }
                })
                .collect(),
            span: *span,
        },
        Expr::If {
            cond,
            then,
            els,
            span,
        } => Expr::If {
            cond: Box::new(rename_bound(cond, renames)),
            then: Box::new(rename_bound(then, renames)),
            els: Box::new(rename_bound(els, renames)),
            span: *span,
        },
        Expr::Lambda { params, body, span } => {
            // A nested lambda rebinds its own parameters: drop them so an inner
            // binder of the same spelling as a method parameter keeps its own
            // binding (`\m -> …` inside `bind m f`'s body).
            let mut inner = renames.clone();
            for p in params {
                inner.remove(&p.name);
            }
            Expr::Lambda {
                params: params.clone(),
                body: Box::new(rename_bound(body, &inner)),
                span: *span,
            }
        }
        Expr::ListLit(elems, span) => Expr::ListLit(
            elems.iter().map(|el| rename_bound(el, renames)).collect(),
            *span,
        ),
        Expr::MapLit(entries, span) => Expr::MapLit(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), rename_bound(v, renames)))
                .collect(),
            *span,
        ),
        Expr::IntegerLit(_, _) | Expr::FractionalLit(_, _, _) | Expr::Void(_) | Expr::Str(_, _) => {
            e.clone()
        }
    }
}

/// Rename inside a definition's body and `where` bindings, used when descending
/// into a `let`/`where` block. The definition's own binders shadow the
/// substitution: `rename_bound`'s `Expr::Lambda`/`Expr::Let` arms already drop
/// them when it descends, so the body is renamed as a whole rather than
/// pre-peeling the currying λ-nest.
fn rename_bound_def(d: &Def, renames: &HashMap<String, String>) -> Def {
    match d {
        Def::Eval {
            name,
            body,
            where_defs,
            sig,
            bang,
            wrote_params,
            span,
        } => Def::Eval {
            name: name.clone(),
            body: rename_bound(body, renames),
            where_defs: where_defs
                .iter()
                .map(|w| rename_bound_def(w, renames))
                .collect(),
            sig: sig.clone(),
            bang: *bang,
            wrote_params: *wrote_params,
            span: *span,
        },
        other => other.clone(),
    }
}

/// Whether a bang binding's right-hand side mentions the bound name (a
/// self-recursive strict binding — `!x = x`, `!x = f x`). Such a binding has no
/// value to force at the binding point, so it is rejected rather than lowered.
fn bang_binding_refers_to_itself(d: &Def) -> bool {
    let mut refs = HashSet::new();
    collect_def_ref_names(d.body(), &mut refs);
    refs.contains(d.name())
}

/// Collect every user-definition name referenced by `e` (a `Ref` or an applied
/// callee) into `out`. Used by the recursion-cycle detection for shared-cell
/// locals, and by `reduce` to decide whether a `let`-bound function is still
/// needed after β-reduction.
pub(crate) fn collect_def_ref_names(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Ref(n, _) => {
            out.insert(n.clone());
        }
        Expr::Apply { name: n, args, .. } => {
            out.insert(n.clone());
            for a in args {
                collect_def_ref_names(a, out);
            }
        }
        // A kernel has NO callee name — that is what makes it
        // interception-proof — but its ARGUMENTS are ordinary expressions that
        // can name a binding. Without this arm the walk stops at the node, so
        // `reduce`'s binding pruning believes a `let`-bound helper reachable
        // only from `gcd`'s argument is unreferenced and drops it ("unknown
        // `g` in value lowering"), and `compute_recursive_locals` misses a
        // self-reference reachable only through a kernel argument.
        Expr::Kernel { args, .. } => {
            for a in args {
                collect_def_ref_names(a, out);
            }
        }
        Expr::ApplyExpr { callee, args, .. } => {
            collect_def_ref_names(callee, out);
            for a in args {
                collect_def_ref_names(a, out);
            }
        }
        Expr::Neg(inner, _) => collect_def_ref_names(inner, out),
        Expr::Annot { expr, .. } => collect_def_ref_names(expr, out),
        Expr::If {
            cond, then, els, ..
        } => {
            collect_def_ref_names(cond, out);
            collect_def_ref_names(then, out);
            collect_def_ref_names(els, out);
        }
        Expr::Let { defs, body, .. } => {
            for d in defs {
                if !d.is_decl() {
                    collect_def_ref_names(d.body(), out);
                }
            }
            collect_def_ref_names(body, out);
        }
        Expr::Lambda { body, .. } => collect_def_ref_names(body, out),
        Expr::Match {
            scrutinee, arms, ..
        } => {
            collect_def_ref_names(scrutinee, out);
            for arm in arms {
                for (g, b) in &arm.guards {
                    if let Some(g) = g {
                        collect_def_ref_names(g, out);
                    }
                    collect_def_ref_names(b, out);
                }
            }
        }
        Expr::Record(fields, _) => {
            for (_, f) in fields {
                collect_def_ref_names(f, out);
            }
        }
        Expr::ListLit(elems, _) => {
            for e in elems {
                collect_def_ref_names(e, out);
            }
        }
        Expr::MapLit(entries, _) => {
            for (_, v) in entries {
                collect_def_ref_names(v, out);
            }
        }
        Expr::ActorParam {
            default: Some(d), ..
        } => collect_def_ref_names(d, out),
        _ => {}
    }
}

/// Whether `e` raises a typing demand that only a CALL SITE can discharge.
///
/// Two kinds of mention qualify:
///
/// * a class METHOD (`inRange`, `+`) — its dictionary comes from the receiver's
///   type, never from the enclosing definition's own declaration;
/// * a class-CONSTRAINED DEFINITION (`(!)`, `listArray`) whose class variable is
///   still UNKNOWN after the mention's own arguments are taken into account,
///   i.e. the argument that must solve it names a λ-parameter of the enclosing
///   definition — so its type exists only at a call site.
///
/// The second clause has to be per-MENTION, not per-name, and this is exactly
/// what the earlier widening got wrong. `at a i = a ! i` is a demand: `a`'s
/// index type is the call's, so `g = at` has no standalone value. But
/// `mk p xs = Pair { first: p, second: elems (listArray (Pair { first: 0,
/// second: 1 }) xs) }` is NOT: `listArray`'s bounds are concrete, so the body
/// solves that `Ix i` itself and GHC gives `mk` the ordinary scheme `a -> [b] ->
/// (a, [b])`. Reading the mention as a demand refused that legal program.
///
/// `params` is the set of the enclosing definition's λ-parameters: a reference
/// to one is what makes an argument's type a call-site fact. A name that is not
/// a parameter is typed by the program's own declarations, so the mention is
/// self-solving.
fn body_carries_class_demand(env: &TypeEnv, e: &Expr, params: &HashSet<String>) -> bool {
    if let Some((name, args)) = application_spine(e) {
        if env.class_of_method(name).is_some() {
            return true;
        }
        if env.constrained_defs.contains_key(name) && mention_needs_a_site(env, name, &args, params)
        {
            return true;
        }
        // The mention itself is settled by the body; a demand may still hide in
        // its arguments (`listArray (Pair { … }) (map (\x -> x + 1.0) xs)`).
        return args
            .iter()
            .any(|a| body_carries_class_demand(env, a, params));
    }
    let mut kids: Vec<&Expr> = Vec::new();
    // Every child expression whose subtree can hold a demand.
    match e {
        Expr::Ref(..)
        | Expr::IntegerLit(..)
        | Expr::FractionalLit(..)
        | Expr::Void(..)
        | Expr::Str(..) => {}
        Expr::Apply { args, .. } => kids.extend(args.iter()),
        Expr::ApplyExpr { callee, args, .. } => {
            kids.push(callee);
            kids.extend(args.iter());
        }
        Expr::Kernel { args, .. } => kids.extend(args.iter()),
        Expr::Neg(inner, _) => kids.push(inner),
        Expr::Annot { expr, .. } => kids.push(expr),
        Expr::If {
            cond, then, els, ..
        } => {
            kids.push(cond);
            kids.push(then);
            kids.push(els);
        }
        Expr::Lambda { body, .. } => kids.push(body),
        Expr::Record(fields, _) => kids.extend(fields.iter().map(|(_, f)| f)),
        Expr::MapLit(entries, _) => kids.extend(entries.iter().map(|(_, v)| v)),
        Expr::ListLit(elems, _) => kids.extend(elems.iter()),
        Expr::FieldProject { record, .. } => kids.push(record),
        Expr::FieldUpdate { record, value, .. } => {
            kids.push(record);
            kids.push(value);
        }
        Expr::Let { defs, body, .. } => {
            for d in defs {
                if !d.is_decl() {
                    kids.push(d.body());
                }
            }
            kids.push(body);
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            kids.push(scrutinee);
            for arm in arms {
                for (g, b) in &arm.guards {
                    if let Some(g) = g {
                        kids.push(g);
                    }
                    kids.push(b);
                }
            }
        }
        Expr::ActorParam {
            default: Some(d), ..
        } => kids.push(d),
        _ => {}
    }
    kids.into_iter()
        .any(|k| body_carries_class_demand(env, k, params))
}

/// A curried application flattened to its CALLEE NAME and all applied arguments
/// in source order, when the callee spine ends in a name.
///
/// The parser nests a curried application one `ApplyExpr` per argument
/// (`(!) a i` is `ApplyExpr(ApplyExpr(Ref "!") [a]) [i]`), so a mention's
/// arguments are only visible after the spine is flattened.
fn application_spine(e: &Expr) -> Option<(&str, Vec<&Expr>)> {
    fn walk<'a>(e: &'a Expr, out: &mut Vec<&'a Expr>) -> Option<&'a str> {
        match e {
            Expr::Ref(name, _) => Some(name.as_str()),
            Expr::Apply { name, args, .. } => {
                out.extend(args.iter());
                Some(name.as_str())
            }
            Expr::ApplyExpr { callee, args, .. } => {
                let name = walk(callee, out)?;
                out.extend(args.iter());
                Some(name)
            }
            _ => None,
        }
    }
    let mut args = Vec::new();
    let name = walk(e, &mut args)?;
    Some((name, args))
}

/// Whether a mention of the class-constrained definition `name` applied to
/// `args` still needs a call site to solve its class variable.
///
/// The question is asked per constraint: the FIRST parameter that mentions the
/// class variable is the argument that solves it (`Pair i i` in `listArray`,
/// `Array i e` in `(!)`). If that argument is absent (an under-applied mention)
/// or names a λ-parameter of the enclosing definition, the body does not know
/// the type and the call site has to supply it.
fn mention_needs_a_site(
    env: &TypeEnv,
    name: &str,
    args: &[&Expr],
    params: &HashSet<String>,
) -> bool {
    let Some(cd) = env.constrained_defs.get(name) else {
        return false;
    };
    for var in signature_constraint_vars(&cd.sig) {
        let Some(idx) = crate::types::ty::class_var_param_pattern(&cd.sig, &var) else {
            // The variable occurs in no parameter: nothing at a call site can
            // solve it, so the mention adds no call-site dependency.
            continue;
        };
        match args.get(idx) {
            Some(arg) => {
                if expr_names_any_of(arg, params) {
                    return true;
                }
            }
            None => return true,
        }
    }
    false
}

/// The type-variable names a signature's class context constrains (`Ix i => …`
/// yields `i`), looking through `forall` binders and further contexts.
fn signature_constraint_vars(sig: &crate::ast::TypeExpr) -> Vec<String> {
    match sig {
        crate::ast::TypeExpr::TConstrained(cs, inner) => {
            let mut vars: Vec<String> = cs.iter().map(|(_, var)| var.clone()).collect();
            vars.extend(signature_constraint_vars(inner));
            vars
        }
        crate::ast::TypeExpr::TForall(_, inner) => signature_constraint_vars(inner),
        _ => Vec::new(),
    }
}

/// Whether `e` references any of `names` (`Ref` or applied callee).
fn expr_names_any_of(e: &Expr, names: &HashSet<String>) -> bool {
    let mut refs = HashSet::new();
    collect_def_ref_names(e, &mut refs);
    refs.iter().any(|n| names.contains(n))
}

/// The arbitrary-precision integer value of a constant expression, when it is
/// an integer literal (possibly nested under unary minus). Unlike [`const_f64`]
/// this does NOT accept fractional literals or arithmetic: it is used where the
/// constant must stay on the Int track (`Expr::Neg` of an integer literal) and
/// deciding whether a literal is an integer constant at all.
fn const_bigint(e: &Expr) -> Option<BigInt> {
    match e {
        Expr::IntegerLit(v, _) => Some(v.clone()),
        Expr::Neg(inner, _) => const_bigint(inner).map(|v| -v),
        _ => None,
    }
}

/// Reduce an arbitrary-precision integer literal to the `i64` the `Int` track
/// carries, reproducing GHC's `-Woverflowed-literals` behaviour exactly.
///
/// GHC accepts an out-of-range integer literal: it takes the literal's value
/// modulo `2^64` and reinterprets the low 64 bits as two's-complement `i64`,
/// emitting `Literal … is out of the Int range` as a warning. rill has no
/// warning channel, so only the VALUE is reproduced:
/// `123456789012345678901234567890` becomes `-4362896299872285998`, which is
/// what GHC 9.10.3 prints for the same literal at `Int` type.
///
/// This is THE conversion boundary. The AST and the tokeniser carry a
/// [`BigInt`]; every site that needs an `i64` (Ix offsets, array/tape
/// capacities, the `Int` value track, `Int`-track case patterns) calls this one
/// helper. Nothing below this point sees a `BigInt` on the `Int` track.
pub(crate) fn literal_to_i64(v: &BigInt) -> i64 {
    // The low 64 bits of the magnitude: the digit vector is little-endian, so
    // its first word IS the low word, and it is absent only for zero.
    let (sign, digits) = v.to_u64_digits();
    let low = digits.first().copied().unwrap_or(0);
    // Reinterpreting those bits as two's complement is `as i64`; a negative
    // literal reaches them through `wrapping_neg`, which keeps the low word.
    match sign {
        num_bigint::Sign::Minus => low.wrapping_neg() as i64,
        _ => low as i64,
    }
}

/// The `f64` value of an arbitrary-precision integer literal.
///
/// This is Haskell's `fromInteger :: Integer -> Double`: correctly rounded, and
/// saturating to `INFINITY` outside `f64`'s finite range. The exact decimal text
/// goes through Rust's correctly rounded `str::parse`; `num-bigint`'s own
/// `to_f64` truncates the mantissa instead of rounding, and `num-traits` is not
/// a direct dependency of this crate.
fn literal_to_f64(v: &BigInt) -> f64 {
    // UNREACHABLE failure: `to_str_radix(10)` yields an optionally-signed run of
    // ASCII digits, which is always a valid `f64` literal (out-of-range
    // magnitudes parse to `INFINITY` rather than `Err`). A silent `NAN` here
    // would poison arithmetic, so an unreachable case is an assertion failure.
    v.to_str_radix(10)
        .parse()
        .expect("decimal integer text always parses as f64")
}

/// The `f64` value of an exact fractional literal.
///
/// This is THE conversion boundary for fractional literals: the AST carries
/// the exact [`BigRational`] its digits spell, and the IR's
/// `ValueConstDouble` carries an `f64`, so exactly one rounding happens here —
/// the same discipline [`literal_to_i64`] keeps for integers. `0.1` lowers to
/// the nearest `f64` to `1/10` (it can only be an approximation), but the AST
/// and every consumer of the exact value still see `1/10`.
///
/// Correctly rounded, and saturating to `INFINITY` outside `f64`'s range:
/// `num-rational`'s `ToPrimitive::to_f64` rounds the quotient to 53 bits with
/// round-to-even and shifts by an exact power of two, so it is the correctly
/// rounded result of `numer / denom` rather than a mantissa truncation.
///
/// Kept out of line: `to_f64` is a large routine (big-integer shifts and a
/// 53-bit round-to-even loop), and inlining it into the deeply recursive
/// `lower_value` dispatch added enough locals to overflow the stack on a
/// nested program.
#[inline(never)]
fn rational_to_f64(v: &BigRational) -> f64 {
    v.to_f64()
        .expect("a non-zero rational always converts to an f64")
}

fn const_f64(e: &Expr) -> Option<f64> {
    match e {
        Expr::FractionalLit(v, _, _) => Some(rational_to_f64(v)),
        Expr::IntegerLit(v, _) => Some(literal_to_f64(v)),
        Expr::Neg(inner, _) => const_f64(inner).map(|v| -v),
        // Constant arithmetic: an infix operator IS an application of the
        // method (`a + b` ≡ `(+) a b`), so folding matches the same shape.
        Expr::Apply { name, args, .. } => {
            let [lhs, rhs] = args.as_slice() else {
                return None;
            };
            let a = const_f64(lhs)?;
            let b = const_f64(rhs)?;
            Some(match name.as_str() {
                "+" => a + b,
                "-" => a - b,
                "*" => a * b,
                "/" => a / b,
                _ => return None,
            })
        }
        _ => None,
    }
}

/// Lower a fully type-checked program into IR (the plain entry).
///
/// Lowering takes **no sample rate**: `?sample_rate` and the `smooth`
/// coefficient read the world's `Params::sample_rate` at run time, so the
/// compiler never sees a rate.
pub fn lower(tp: &TypedProgram) -> Result<Ir, CompileError> {
    lower_with(tp)
}

/// Like [`lower`] (retained as the named entry point).
pub fn lower_with(tp: &TypedProgram) -> Result<Ir, CompileError> {
    lower_with_cafs(tp, &HashSet::new())
}

/// Like [`lower_with`], but treats the given names as closed CAFs that are
/// lifted once and shared across reference sites.
pub fn lower_with_cafs(tp: &TypedProgram, cafs: &HashSet<String>) -> Result<Ir, CompileError> {
    lower_with_cafs_mode(tp, cafs, tp.mode)
}

/// Like [`lower_with_cafs`], carrying the evaluation mode. The mode gates
/// recursion: strict rejects recursive CAF/value definitions (the v1 acyclic
/// contract); complete allows them (thunk-emitting lowering in a later task —
/// for now a recursive reference breaks the cycle with a fresh placeholder
/// register).
pub fn lower_with_cafs_mode(
    tp: &TypedProgram,
    cafs: &HashSet<String>,
    mode: Mode,
) -> Result<Ir, CompileError> {
    lower_with_cafs_mode_entry(tp, cafs, mode, "main")
}

/// Like [`lower_with_cafs_mode`], with an explicit entry name. The entry
/// definition supplies the program's single value result; upstream,
/// [`infer_program_with_entry`](crate::types::infer::infer_program_with_entry)
/// recorded the entry's scheme in `tp.def_schemes`, which this function reads.
pub(crate) fn lower_with_cafs_mode_entry(
    tp: &TypedProgram,
    cafs: &HashSet<String>,
    mode: Mode,
    entry: &str,
) -> Result<Ir, CompileError> {
    lower_program_with_channels(tp, cafs, mode, 0, None, entry)
}

/// Like [`lower_with_cafs_mode`], with an explicit program input- and
/// output-channel count known before lowering. `input_channels` sizes the
/// program's input table in the IR; nothing drives those channels on the
/// retained value path.
pub fn lower_program_with_channels(
    tp: &TypedProgram,
    cafs: &HashSet<String>,
    mode: Mode,
    input_channels: usize,
    output_channels: Option<usize>,
    entry: &str,
) -> Result<Ir, CompileError> {
    let program: &Program = &tp.program;
    let main = program
        .defs
        .iter()
        .find(|d| d.name() == entry)
        .ok_or_else(|| {
            CompileError::Unsupported(format!("program must have a `{entry}` definition"))
        })?;

    // Definition lookup for the whole body walk. `program.defs` is the MERGED
    // top-level list inference settled on: Prelude definitions first, the user's
    // after, with any Prelude definition the user SHADOWS already dropped (see
    // `types::infer::merge_prelude_defs`). Building the map in that order makes
    // last-wins the deliberate expression of the shadowing rule rather than an
    // accident of insertion order — but the rule itself is enforced upstream,
    // so a shadowed Prelude body can never reach this map and be executed
    // against the scheme inference gave the user's name.
    //
    // Only `Def::Eval` names are unique in the merged list (declaration kinds
    // such as `instance` deliberately repeat their class name), so the guard
    // below scopes the uniqueness check to value definitions.
    let mut defs: HashMap<String, Def> = HashMap::new();
    for d in &program.defs {
        defs.insert(d.name().to_string(), d.clone());
        for wd in d.where_defs() {
            defs.insert(wd.name().to_string(), wd.clone());
        }
    }
    debug_assert!(
        {
            let evals: Vec<&str> = program
                .defs
                .iter()
                .filter(|d| matches!(d, Def::Eval { .. }))
                .map(|d| d.name())
                .collect();
            let unique: std::collections::HashSet<&&str> = evals.iter().collect();
            unique.len() == evals.len()
        },
        "merged definition list carries a duplicate value-definition name: \
         inference dropped no Prelude shadow, so lowering would run one body \
         while inference typed the other"
    );

    let recursive_locals = Lowerer::compute_recursive_locals(&defs);
    // `main` is an ordinary value definition; `?name` / `SetParameter` remains
    // for dynamic builtin params only.
    //
    // An explicit `input_channels` still sizes the program's input table in
    // the IR; the value-track body no longer reaches those channels (the
    // signal-transform surface is gone).
    let num_inputs = input_channels;
    let mut lw = Lowerer {
        defs,
        cafs,
        caf_cache: HashMap::new(),
        caf_lifting: HashSet::new(),
        block_state_slots: 0,
        delay_lens: Vec::new(),
        locals: Vec::new(),
        builtins: Vec::new(),
        params: Vec::new(),
        param_names: HashMap::new(),
        value_blocks: vec![ValueBlock::default()],
        cur_value_block: 0,
        value_entry: 0,
        next_value_reg: 0,
        next_tyvar: 0,
        dict_selections: tp.dict_selections.clone(),
        literal_tys: tp.literal_tys.clone(),
        literal_instances: tp.literal_instances.clone(),
        next_method_param: 0,
        value_regs_out: Vec::new(),
        value_out_tys: Vec::new(),
        container_tys: Vec::new(),
        value_funcs: Vec::new(),
        value_locals: Vec::new(),
        value_local_ctors: Vec::new(),
        env: &tp.type_env,
        mode,
        value_inline: HashSet::new(),
        method_lifting: HashSet::new(),
        constrained_lifting: HashSet::new(),
        head_arg_bindings: HashMap::new(),
        fragments: Vec::new(),
        fragment_captures: Vec::new(),
        fn_param_tys: tp.fn_param_tys.clone(),
        def_result_tys: tp
            .def_schemes
            .iter()
            .map(|(n, s)| (n.clone(), flatten_value_thunk(&s.ty.vty).1))
            .collect(),
        shared_cell_result_tys: tp.shared_cell_result_tys.clone(),
        pending_param_tys: None,
        anchor_cells: HashMap::new(),
        anchor_closures: HashMap::new(),
        anchor_types: HashMap::new(),
        anchor_compiling: HashSet::new(),
        recursive_locals,
        tape_decls: tp.tape_decls.iter().cloned().collect(),
        runst_payload_tys: tp.runst_payload_tys.clone(),
        tapes: Vec::new(),
        named_tapes: HashMap::new(),
        input_buffers: Vec::new(),
        uses_array_hole: false,
        array_hole_frag: None,
    };

    let mut main_args = Vec::with_capacity(num_inputs);
    for index in 0..num_inputs {
        let dst = lw.fresh_value_reg();
        lw.emit_value(ValueInstr::ValueBufferLoadInput { dst, index });
        lw.input_buffers.push(dst);
        main_args.push(dst);
    }
    // Value-output programs lower through the value track; signal-output
    // programs through the block track. The two tracks never mix — inference
    // guarantees a value-typed body has arity_in() == 0, so `main_args` is
    // empty on the value path.
    //
    // An IO program is just a value program: `main`'s value is the `IO ()`
    // action the runner applies to the realized `World`. Its value register is
    // the program's single value output (no special-casing here).
    let entry_scheme = tp
        .def_schemes
        .get(entry)
        .ok_or_else(|| CompileError::Unsupported(format!("no inferred scheme for `{entry}`")))?;
    let has_value_out = !matches!(entry_scheme.ty.vty, ValueTy::Buffer(_));
    let outs = if has_value_out {
        let (vr, vty) = lw.lower_value(main.body())?;
        lw.value_regs_out.push(vr);
        lw.value_out_tys.push(vty);
        Vec::new()
    } else {
        let outs = lw.lower(main.body(), &main_args)?;
        // Signal programs now lower to the VALUE track: the body's output
        // registers are `Value::Buffer` values (one per output channel), so
        // they feed the same `value_regs_out` path as value outputs.
        lw.value_regs_out = outs.clone();
        lw.value_out_tys = vec![lw.env.buffer_vty(); outs.len()];
        outs
    };
    if outs.is_empty() && lw.value_regs_out.is_empty() {
        return Err(CompileError::Unsupported(
            "body lowered to 0 outputs, expected at least 1".into(),
        ));
    }
    let num_outputs = output_channels.unwrap_or(outs.len());
    // The program's dictionary table (design §3.7 step 2). Built AFTER the main
    // body so every call site has recorded its selection; it emits no code into
    // the value track (a slot body is compiled into its own fragment), so it
    // cannot change the program's behaviour. Nothing reads the table yet.
    let mentioned: Vec<(String, ValueTy)> = tp
        .dict_selections
        .iter()
        .map(|((_, class), ty)| (class.clone(), ty.clone()))
        .collect();
    let dict_plan = crate::dicts::plan(&tp.type_env, &mentioned)?;
    let (dicts, dict_gaps) = lw.build_dict_descriptors(&dict_plan);
    // of allocations one tick can issue (value registers are per-tick scratch,
    // dropped at tick end), so the count of alloc-producing value instructions
    // is a strict upper bound on simultaneously-live per-tick slots. On top of
    // that:
    //   * each value output channel pins its WHOLE subtree across ticks — the
    //     output keeps the root at rc >= 1, so `drop_ref` never frees the
    //     children and the previous tick's output tree is still fully live
    //     while the next tick allocates. The bound therefore adds the static
    //     subtree size of each output's value type (a scalar adds 1).
    // Value-state slots (`ValueStateRead`/`ValueStateWrite`) are not emitted by
    // lowering in v1; when a future task wires `~`/`@` over values, its slot
    // count must join the bound the same way.
    //
    // Fragments allocate too: each fragment's own instructions allocate at most
    // one slot per alloc-producing instruction, each fragment call binds
    // `num_capture_cells` temp-frame cells (one independent copy per env field),
    // copies its `value_ins` argument values into the scratch slice, and copies
    // its `value_outs` results out. The per-fragment totals are summed
    // conservatively — an upper bound on any single call path (fragments may
    // nest, but the sum over all fragments bounds the deepest nesting as well).
    let fragment_capacity = lw
        .fragments
        .iter()
        .map(|f| {
            f.value_blocks
                .iter()
                .flat_map(|b| &b.instrs)
                .filter(|i| is_alloc_producing(i))
                .count()
                + f.num_capture_cells
                + f.sig.value_ins
                + f.sig.value_outs
        })
        .sum::<usize>();
    // Container-typed subexpressions pin their element slots while live. Open
    // collections have no type-level capacity, so `subtree_size` uses the
    // conservative `ELEM_EST`; the default RT mode applies `POOL_SAFETY_MULTIPLIER`
    // so legitimate growth does not exhaust the pool.
    let container_capacity = lw
        .container_tys
        .iter()
        .map(|t| lw.subtree_size(t))
        .sum::<usize>();
    let value_capacity = lw
        .value_blocks
        .iter()
        .flat_map(|b| &b.instrs)
        .filter(|i| is_alloc_producing(i))
        .count()
        + lw.value_out_tys
            .iter()
            .map(|t| lw.subtree_size(t))
            .sum::<usize>()
        + container_capacity
        + fragment_capacity
        // The dictionaries are ONE arena slot each, held for the world's whole
        // lifetime (not per tick), so they are charged once here. Their
        // superclass refs are extra owners of already-charged slots, not new
        // ones, and `methods`/`supers` are plain `Vec`s outside the arena.
        + dicts.len();
    let slot_capacity = value_capacity * Lowerer::POOL_SAFETY_MULTIPLIER;
    // Payload buffer budget: every live slot may hold one pooled payload
    // buffer (a Record/Sum/List/Set payload). `value_capacity` bounds the
    // peak live slots; `ELEM_EST` bounds each buffer's refs. Covers literals
    // (their subtree is already in `value_capacity`) and runtime match/ctor
    // payloads without separate tracking.
    let buffer_budget = value_capacity * Lowerer::ELEM_EST;
    // Measured behaviour of this accounting for a RECURSIVE PRELUDE definition
    // (B3): the count is a STATIC per-program bound over allocation sites, not
    // a per-call or per-element charge, so a recursive producer such as
    // `from n = cons n (from (n + 1.0))` contributes the same constant whether
    // it lives in the Prelude or is shadowed by an identical user body. Both
    // compile to capacity 400 / buffer budget 1600 for `main = from 1.0`.
    //
    // The budget is also independent of DEMAND — but the exact constant is a
    // property of the whole program, so each shape below has its own:
    //
    //   * `length (take n (from 1.0))` (a single builtin consumer, compiled via
    //     `compile_eval`) is CONSTANT at capacity 1104 / buffer budget 4416 for
    //     every `n` from 1 to 512. It has no demand wall to report: `length` is
    //     not yet `Cons`-aware (Task 5), so it walks nothing and returns
    //     `Int(0)` without forcing the chain.
    //   * The benchmark-shaped drain
    //     `fromList (take n (from 1.0)) |> toList |> fold (+) 0.0` is CONSTANT
    //     at capacity 2120 / buffer budget 8480 for every `n` from 1 to 256,
    //     and DOES force the chain, so it shows the real demand WALL: measured
    //     on this build the largest working `n` is 84 and `n = 85` latches
    //     "value-track resource exhausted: value arena capacity". `n = 64` (the
    //     GHC-parity benchmark size) sums to 2080.0 with 21 elements of
    //     headroom.
    //
    // The per-element runtime cost comes out of that fixed budget; it does not
    // enlarge it. Numbers above were measured with `compile_eval` reading
    // `Ir::value_state` (`capacity`, `buffer_budget`) and with `compile` +
    // `run` bisecting `n` for the walls.
    //
    // Complete mode (Stage A): the lazy value track keeps each thunk's memo
    // cell AND its forced result live for the thunk's lifetime (a strict
    // program would release the result at tick end), and a forced thunk may
    // suspend further thunks that outlive the forcing frame. Apply a
    // conservative multiplier so a complete-mode program cannot exhaust the
    // pre-allocated arena; exhaustion remains a latched `ProcessError`.
    let (slot_capacity, buffer_budget) = if mode == Mode::Complete {
        (slot_capacity * 2, buffer_budget * 2)
    } else {
        (slot_capacity, buffer_budget)
    };
    // Pre-allocated function-call scratch: the runtime call stack never holds
    // more than one frame per fragment (recursion is rejected at inference, so
    // no fragment can recur on a dispatch chain), so the total fragment count
    // is a strict upper bound on the deepest nesting. Each frame needs at most
    // `max_fragment_regs` value-register slots, giving the product below.
    // The M8 synthetic hole fragment (appended below, when an `arrayFromAssocs`
    // was lowered) must be counted in `max_call_depth`: forcing an unlisted
    // array position runs a real fragment frame.
    if lw.uses_array_hole {
        let hole_id = lw.fragments.len() as u32;
        lw.fragments.push(std::sync::Arc::new(FragmentIr {
            value_blocks: vec![ValueBlock {
                instrs: vec![ValueInstr::ValueArrayHoleFault],
                term: ValueTerm::Halt,
            }],
            entry: 0,
            num_value_regs: 1,
            output_value_regs: vec![],
            num_capture_cells: 0,
            sig: FuncSig {
                value_ins: 0,
                value_outs: 0,
                signal_ins: 0,
            },
        }));
        lw.array_hole_frag = Some(hole_id);
    }
    let max_call_depth = lw.fragments.len();
    let max_fragment_regs = lw
        .fragments
        .iter()
        .map(|f| f.num_value_regs)
        .max()
        .unwrap_or(0);
    // Recursion (complete mode) can nest the same fragment multiple times, so
    // the acyclic per-fragment bound is doubled as a conservative allowance.
    let call_depth_allowance = if mode == Mode::Complete {
        max_call_depth * 2
    } else {
        max_call_depth
    };
    let max_call_regs = call_depth_allowance * max_fragment_regs;
    // Terminate the program's value track: the entry block's fallthrough chain
    // ends here. Every existing program is a single block ending in `Halt`.
    lw.set_value_term(lw.cur_value_block, ValueTerm::Halt);
    // The runner contract: `main : IO ()`. The static type is the
    // only place the unit payload survives — the value track represents `()`
    // as `Double(0.0)` once it travels through a closure cell (see
    // `Ir::main_io_unit`).
    // The gate: `main` must be an `IO t` ACTION. `t` is free — the payload is
    // discarded at run time, exactly as GHC does.
    let main_is_io = main_is_io_action(&entry_scheme.ty.vty);
    // Whether the payload is statically `()`. This is NOT the gate; it only
    // resolves the value track's erasure ambiguity (`()` and `0.0` share the
    // `Double(0.0)` encoding inside a closure cell), so a `0.0` payload is
    // treated as unit only when the type says `IO ()`.
    let main_io_unit = main_is_io
        && match &entry_scheme.ty.vty {
            ValueTy::Con(_, args) => matches!(args.first(), Some(ValueTy::Void)),
            _ => false,
        };
    // The program's single result register: where the entry body's value
    // landed. `lw.value_regs_out[0]` — architecturally there are no signal
    // programs (no output channels), so the result is exactly one register
    // (buffer results are `Value::Buffer(idx)` arena values). The 7815 check
    // guarantees at least one output exists.
    let result_reg = lw.value_regs_out.first().copied().unwrap_or(0);
    Ok(Ir {
        main_is_io,
        main_io_unit,
        num_inputs,
        num_outputs,
        state: StateLayout {
            block_state_slots: lw.block_state_slots,
            delay_lens: lw.delay_lens,
            num_outputs,
        },
        builtins: lw.builtins,
        params: lw.params,
        value_blocks: std::mem::take(&mut lw.value_blocks),
        value_entry: lw.value_entry,
        num_value_regs: lw.next_value_reg,
        result_reg,
        value_funcs: lw.value_funcs,
        fragments: lw.fragments,
        array_hole_frag: lw.array_hole_frag,
        max_call_regs,
        value_state: ValueLayout {
            capacity: slot_capacity,
            buffer_budget,
            value_state_slots: 0,
        },
        num_buffers: lw.next_value_reg,
        input_buffers: lw.input_buffers,
        tapes: lw.tapes,
        dicts,
        dict_gaps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::BuiltinKind;
    use crate::ir::ValueInstr;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::reduce::reduce_with_cafs;
    use crate::types::infer::{infer_program, infer_program_with};

    fn ir_of(src: &str) -> Ir {
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        let tp = infer_program(&p, p.mode).unwrap();
        lower(&tp).unwrap()
    }

    fn ir_with_cafs(src: &str) -> Ir {
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        let typed = infer_program_with(&p, p.mode).unwrap();
        let cafs = typed.cafs.clone();
        let reduced = reduce_with_cafs(&typed.program, &cafs);
        let tp = crate::types::infer::TypedProgram {
            value_tys: HashMap::new(),
            literal_instances: HashMap::new(),
            program: reduced,
            def_schemes: typed.def_schemes.clone(),
            cafs,
            type_env: typed.type_env.clone(),
            fn_param_tys: typed.fn_param_tys.clone(),
            shared_cell_result_tys: typed.shared_cell_result_tys.clone(),
            tape_decls: Vec::new(),
            runst_payload_tys: typed.runst_payload_tys.clone(),
            mode: typed.mode,
            dict_selections: typed.dict_selections.clone(),
            literal_tys: typed.literal_tys.clone(),
        };
        lower_with_cafs(&tp, &tp.cafs).unwrap()
    }

    #[test]
    fn record_args_discard_a_partial_field_binding() {
        // Declared fields `first: Var(1)`, `second: Var(1)` — a homogeneous
        // record such as `Pair`. When a LATER field contradicts an earlier
        // binding (`first: Int` then `second: Bool`), the record contributes NO
        // type arguments: a leaked `first` binding would hand out `[Int]` and
        // let the dispatch key claim the record is a `Pair Int …`.
        let fields = [
            ("first".to_string(), ValueTy::Var(1), false),
            ("second".to_string(), ValueTy::Var(1), false),
        ];
        assert_eq!(
            record_args_from_fields(&fields, &[ValueTy::Int, ValueTy::Int]),
            vec![ValueTy::Int]
        );
        assert!(
            record_args_from_fields(&fields, &[ValueTy::Int, ValueTy::String]).is_empty(),
            "a contradictory field must void the whole record's type arguments"
        );
        // An unresolved field type keeps the parameterized type opaque.
        assert!(
            record_args_from_fields(&fields, &[ValueTy::Var(7), ValueTy::Var(7)]).is_empty(),
            "an unresolved field type must not be pinned as a type argument"
        );
    }

    /// Build a `Lowerer` for testing lowering helpers (`free_vars`) directly on
    /// a parsed AST, bypassing inference (which rejects higher-order programs
    /// that `free_vars` must still handle correctly).
    fn lw<'a>(env: &'a TypeEnv, cafs: &'a HashSet<String>) -> Lowerer<'a> {
        Lowerer {
            literal_instances: HashMap::new(),
            defs: HashMap::new(),
            cafs,
            caf_cache: HashMap::new(),
            caf_lifting: HashSet::new(),
            block_state_slots: 0,
            delay_lens: Vec::new(),
            locals: Vec::new(),
            builtins: Vec::new(),
            params: Vec::new(),
            param_names: HashMap::new(),
            value_blocks: vec![ValueBlock::default()],
            cur_value_block: 0,
            value_entry: 0,
            next_value_reg: 0,
            next_tyvar: 0,
            dict_selections: HashMap::new(),
            literal_tys: HashMap::new(),
            next_method_param: 0,
            value_regs_out: Vec::new(),
            value_out_tys: Vec::new(),
            container_tys: Vec::new(),
            value_funcs: Vec::new(),
            value_locals: Vec::new(),
            value_local_ctors: Vec::new(),
            env,
            mode: Mode::Strict,
            value_inline: HashSet::new(),
            method_lifting: HashSet::new(),
            constrained_lifting: HashSet::new(),
            head_arg_bindings: HashMap::new(),
            fragments: Vec::new(),
            fragment_captures: Vec::new(),
            fn_param_tys: HashMap::new(),
            def_result_tys: HashMap::new(),
            shared_cell_result_tys: HashMap::new(),
            pending_param_tys: None,
            anchor_cells: HashMap::new(),
            anchor_closures: HashMap::new(),
            anchor_types: HashMap::new(),
            anchor_compiling: HashSet::new(),
            recursive_locals: HashSet::new(),
            tape_decls: HashMap::new(),
            runst_payload_tys: HashMap::new(),
            tapes: Vec::new(),
            named_tapes: HashMap::new(),
            input_buffers: Vec::new(),
            uses_array_hole: false,
            array_hole_frag: None,
        }
    }

    /// Whether any main-block or fragment instruction matches the predicate. A
    /// lambda argument (`map (fn x -> …) xs`) compiles its body into a
    /// fragment, so the instruction under test may live outside `value_blocks`.
    fn has_instr(ir: &Ir, pred: impl Fn(&ValueInstr) -> bool) -> bool {
        let in_blocks = |blocks: &[ValueBlock]| blocks.iter().any(|b| b.instrs.iter().any(&pred));
        in_blocks(&ir.value_blocks) || ir.fragments.iter().any(|f| in_blocks(&f.value_blocks))
    }

    #[test]
    fn compute_recursive_locals_sees_a_kernel_argument() {
        // `compute_recursive_locals` asks `collect_def_ref_names` whether a def's
        // body references it. A kernel node has no callee name, but its
        // ARGUMENTS can carry the self-reference — and a def that participates in
        // a cycle needs a SHARED CELL, or forcing the cycle reads an unbound
        // register instead of yielding `Bottom`.
        //
        // MEASURED: no program in the suite distinguishes this at the public
        // surface (the local-recursion + kernel-argument shapes all fail earlier,
        // in inference, for unrelated `Int`/`Integer` reasons), so the walker is
        // pinned directly rather than through a golden value.
        let src = r"main = let { f = __gcd f 1; } in f";
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        let defs: HashMap<String, Def> = {
            let main = p.defs.iter().find(|d| d.name() == "main").unwrap();
            let Expr::Let { defs, .. } = main.body() else {
                panic!("main body must be a let");
            };
            defs.iter()
                .map(|d| (d.name().to_string(), d.clone()))
                .collect()
        };
        let recursive = Lowerer::compute_recursive_locals(&defs);
        assert!(
            recursive.contains("f"),
            "a self-reference inside a kernel argument must be detected: {recursive:?}"
        );
    }

    #[test]
    fn free_vars_walks_apply_callee() {
        // `outer = \ f -> \ x -> f x`: the inner lambda applies the closure
        // `f` from the ENCLOSING lambda. `f` appears only as the callee NAME of
        // an `Apply`, but it is a free variable of the body and must be
        // captured — otherwise the env snapshot misses it and lowering reports
        // "unknown `f` in value lowering" (Task 6 nested-HOF).
        let p = parse(
            &tokenize(r"outer = \f -> \x -> f x; main = 1.0").unwrap(),
            r"outer = \f -> \x -> f x; main = 1.0".as_bytes(),
        )
        .unwrap();
        let outer = p.defs.iter().find(|d| d.name() == "outer").unwrap();
        let Expr::Lambda { body: inner, .. } = outer.body() else {
            panic!("outer body must be a lambda");
        };
        let Expr::Lambda { body: apply, .. } = inner.as_ref() else {
            panic!("outer body must be a nested lambda");
        };
        let apply = apply.as_ref();
        assert!(matches!(apply, Expr::Apply { .. }));

        let env = TypeEnv::default();
        let empty = HashSet::new();
        let lw = lw(&env, &empty);
        let mut bound = HashSet::new();
        bound.insert("x".to_string());
        assert_eq!(
            lw.free_vars(apply, &bound),
            vec!["f".to_string()],
            "the applied closure name `f` must be captured as a free variable"
        );
    }

    /// `rename_bound` must rename ONLY the method's own parameters: every
    /// construct the body introduces that rebinds a name keeps its own binding.
    /// This is the property that makes method instantiation semantics-
    /// preserving, so it is pinned directly rather than only through the
    /// end-to-end values.
    #[test]
    fn rename_bound_stops_at_inner_binders() {
        // An INSTANCE METHOD BODY (not a definition's λ-nest): the method's
        // parameters are supplied separately, so the body itself carries no
        // binder for them. `\m -> m` inside it rebinds `m` locally; `f` is the
        // method's other parameter and is free in the body.
        let src = r"t = (\m -> m) 1.0 + 2.0; main = 1.0";
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        let def = p.defs.iter().find(|d| d.name() == "t").unwrap();
        let mut renames = HashMap::new();
        renames.insert("m".to_string(), "$m0_0".to_string());
        renames.insert("f".to_string(), "$m0_1".to_string());
        let renamed = rename_bound(def.body(), &renames);
        let text = format!("{renamed:?}");
        // The inner λ rebinds `m`, so neither its binder nor its body reference
        // is renamed: the shadowing rule holds.
        assert!(
            text.contains("params: [Param { name: \"m\""),
            "the inner λ must keep its own `m` binder: {text}"
        );
        assert!(
            !text.contains("$m0_0"),
            "a shadowed `m` must not be renamed: {text}"
        );
        // A genuinely free `f` IS renamed — that is the capture fix.
        let src2 = r"t = f + 1.0; main = 1.0";
        let p2 = parse(&tokenize(src2).unwrap(), src2.as_bytes()).unwrap();
        let def2 = p2.defs.iter().find(|d| d.name() == "t").unwrap();
        let renamed2 = rename_bound(def2.body(), &renames);
        assert!(
            format!("{renamed2:?}").contains("$m0_1"),
            "a free method parameter must be renamed: {renamed2:?}"
        );
    }

    #[test]
    fn gain_lowers_to_const_and_mul() {
        let ir = ir_of(r"main = sine 440.0 1.0 0.0 * 0.5;");
        assert_eq!(ir.num_inputs, 0);
        assert!(has_instr(&ir, |i| matches!(
            i,
            ValueInstr::ValueBufferBin {
                op: BinArith::Mul,
                ..
            }
        )));
    }

    #[test]
    fn actor_param_lowers_to_param_read() {
        // An actor param (`?gain`) is a named control parameter read as a
        // scalar VALUE (`ValueInstr::ValueReadParam`). `main` has no λ-params
        // (String launch arguments are gone), so dynamic control uses `?name`.
        let ir = ir_of(r"main = ?gain * 1.0;");
        assert!(
            has_instr(&ir, |i| matches!(i, ValueInstr::ValueReadParam { .. })),
            "expected a ValueReadParam instruction for `?gain`"
        );
    }

    #[test]
    fn onepole_lowers_to_callblock() {
        let ir = ir_of(r"main = onepole 200.0 0.5 (sine 440.0 1.0 0.0);");
        assert!(
            has_instr(&ir, |i| matches!(i, ValueInstr::ValueCallForeign { .. })),
            "expected a CallBlock instruction"
        );
        let bi = ir
            .builtins
            .iter()
            .find(|b| b.name == "onepole")
            .expect("onepole builtin");
        assert_eq!(bi.kind, BuiltinKind::Block);
        assert_eq!(bi.params, vec![200.0, 0.5]);
    }

    #[test]
    fn block_builtin_lowers_to_callblock() {
        let ir = ir_of(r"main = lowpass 1000.0 0.7 (sine 440.0 1.0 0.0);");
        assert!(
            has_instr(&ir, |i| matches!(i, ValueInstr::ValueCallForeign { .. })),
            "expected a CallBlock instruction"
        );
        let bi = ir
            .builtins
            .iter()
            .find(|b| b.name == "lowpass")
            .expect("lowpass builtin");
        assert_eq!(bi.kind, BuiltinKind::Block);
        assert_eq!(bi.params, vec![1000.0, 0.7]);
    }

    #[test]
    fn smooth_allocates_state() {
        let ir = ir_of(r"main = smooth (sine 440.0 1.0 0.0) 10.0;");
        assert_eq!(ir.state.block_state_slots, 1);
        assert!(has_instr(&ir, |i| matches!(
            i,
            ValueInstr::ValueBufferStateRead { .. }
        )));
        assert!(has_instr(&ir, |i| matches!(
            i,
            ValueInstr::ValueBufferStateWrite { .. }
        )));
    }

    #[test]
    fn open_block_stays_macro() {
        // Two independent `smooth` applications (each allocates a state slot)
        // must produce two state slots.
        let ir = ir_of(r"main = smooth (smooth (sine 440.0 1.0 0.0) 10.0) 20.0;");
        assert_eq!(ir.state.block_state_slots, 2);
    }

    #[test]
    fn caf_const_param_still_folds() {
        // cutoff = 1000.0; lowpass cutoff 0.7 -> param 1000.0
        let ir = ir_with_cafs(r"cutoff = 1000.0; main = lowpass cutoff 0.7 (sine 440.0 1.0 0.0);");
        let lp = ir.builtins.iter().find(|b| b.name == "lowpass").unwrap();
        assert!((lp.params[0] - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn transitive_caf_const_fold() {
        // b = 1000.0; a = b; main = lowpass a 0.7 -> a resolves through b to 1000.0
        let ir = ir_with_cafs(r"b = 1000.0; a = b; main = lowpass a 0.7 (sine 440.0 1.0 0.0);");
        let lp = ir.builtins.iter().find(|b| b.name == "lowpass").unwrap();
        assert!((lp.params[0] - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn non_folding_caf_in_builtin_param_is_error() {
        // osc = sine 440 0.5 0 is a CAF whose body is not a constant: using it in
        // a builtin param position must error, not silently become 0.
        let p = parse(
            &tokenize(r"osc = sine 440 0.5 0; main = lowpass osc 0.7 (sine 440.0 1.0 0.0);")
                .unwrap(),
            r"osc = sine 440 0.5 0; main = lowpass osc 0.7 (sine 440.0 1.0 0.0);".as_bytes(),
        )
        .unwrap();
        let typed = infer_program_with(&p, p.mode).unwrap();
        let cafs = typed.cafs.clone();
        let reduced = reduce_with_cafs(&typed.program, &cafs);
        let tp = crate::types::infer::TypedProgram {
            value_tys: HashMap::new(),
            literal_instances: HashMap::new(),
            program: reduced,
            def_schemes: typed.def_schemes.clone(),
            cafs: cafs.clone(),
            type_env: typed.type_env.clone(),
            fn_param_tys: typed.fn_param_tys.clone(),
            shared_cell_result_tys: typed.shared_cell_result_tys.clone(),
            tape_decls: Vec::new(),
            runst_payload_tys: typed.runst_payload_tys.clone(),
            mode: typed.mode,
            dict_selections: typed.dict_selections.clone(),
            literal_tys: typed.literal_tys.clone(),
        };
        let res = lower_with_cafs(&tp, &cafs);
        assert!(res.is_err());
    }

    #[test]
    fn mutually_recursive_cafs_are_errors() {
        // a = b; b = a  -> the reference cycle must surface as a compile error,
        // not a stack overflow.
        let p = parse(
            &tokenize("a = b; b = a; main = a").unwrap(),
            "a = b; b = a; main = a".as_bytes(),
        )
        .unwrap();
        let typed = infer_program(&p, p.mode).unwrap();
        let cafs = typed.cafs.clone();
        let reduced = reduce_with_cafs(&typed.program, &cafs);
        let tp = crate::types::infer::TypedProgram {
            value_tys: HashMap::new(),
            literal_instances: HashMap::new(),
            program: reduced,
            def_schemes: typed.def_schemes.clone(),
            cafs: cafs.clone(),
            type_env: typed.type_env.clone(),
            fn_param_tys: typed.fn_param_tys.clone(),
            shared_cell_result_tys: typed.shared_cell_result_tys.clone(),
            tape_decls: Vec::new(),
            runst_payload_tys: typed.runst_payload_tys.clone(),
            mode: typed.mode,
            dict_selections: typed.dict_selections.clone(),
            literal_tys: typed.literal_tys.clone(),
        };
        let res = lower_with_cafs(&tp, &cafs);
        assert!(res.is_err());
    }

    #[test]
    fn cyclic_caf_in_builtin_param_is_error() {
        // a = b; b = a; main = lowpass a 0.7
        // The cycle is reached through a BUILTIN PARAM, exercising caf_const_impl's
        // visited-set guard (not the caf_lifting guard in lower_ref).
        let p = parse(
            &tokenize(r"a = b; b = a; main = lowpass a 0.7 (sine 440.0 1.0 0.0);").unwrap(),
            r"a = b; b = a; main = lowpass a 0.7 (sine 440.0 1.0 0.0);".as_bytes(),
        )
        .unwrap();
        let typed = infer_program_with(&p, p.mode).unwrap();
        let cafs = typed.cafs.clone();
        let reduced = reduce_with_cafs(&typed.program, &cafs);
        let tp = crate::types::infer::TypedProgram {
            value_tys: HashMap::new(),
            literal_instances: HashMap::new(),
            program: reduced,
            def_schemes: typed.def_schemes.clone(),
            cafs: cafs.clone(),
            type_env: typed.type_env.clone(),
            fn_param_tys: typed.fn_param_tys.clone(),
            shared_cell_result_tys: typed.shared_cell_result_tys.clone(),
            tape_decls: Vec::new(),
            runst_payload_tys: typed.runst_payload_tys.clone(),
            mode: typed.mode,
            dict_selections: typed.dict_selections.clone(),
            literal_tys: typed.literal_tys.clone(),
        };
        let res = lower_with_cafs(&tp, &cafs);
        assert!(res.is_err());
    }

    #[test]
    fn unreferenced_caf_is_not_lowered() {
        // dead = sine 440 0.5 0; main = 0.5 * 0.5  -> no sine builtin
        let ir = ir_with_cafs(r"dead = sine 440 0.5 0; main = 0.5 * 0.5;");
        assert!(!ir.builtins.iter().any(|b| b.name == "sine"));
    }

    #[test]
    fn recursive_caf_is_error_not_overflow() {
        // a = a  -> compile error, not stack overflow
        let p = parse(
            &tokenize("a = a; main = a").unwrap(),
            "a = a; main = a".as_bytes(),
        )
        .unwrap();
        let typed = infer_program(&p, p.mode).unwrap();
        let cafs = typed.cafs.clone();
        let reduced = reduce_with_cafs(&typed.program, &cafs);
        let tp = crate::types::infer::TypedProgram {
            value_tys: HashMap::new(),
            literal_instances: HashMap::new(),
            program: reduced,
            def_schemes: typed.def_schemes.clone(),
            cafs: cafs.clone(),
            type_env: typed.type_env.clone(),
            fn_param_tys: typed.fn_param_tys.clone(),
            shared_cell_result_tys: typed.shared_cell_result_tys.clone(),
            tape_decls: Vec::new(),
            runst_payload_tys: typed.runst_payload_tys.clone(),
            mode: typed.mode,
            dict_selections: typed.dict_selections.clone(),
            literal_tys: typed.literal_tys.clone(),
        };
        let res = lower_with_cafs(&tp, &cafs);
        assert!(res.is_err());
    }

    #[test]
    fn record_construct_lowers_to_value_track() {
        let ir = ir_of("data Point = { x: Double, y: Double }; main = Point { x: 1.0, y: 2.0 }");
        assert!(ir.value_blocks[0]
            .instrs
            .iter()
            .any(|i| matches!(i, ValueInstr::ValueConstructRecord { .. })));
    }

    #[test]
    fn field_project_lowers_to_value_project() {
        let ir = ir_of(
            "data Point = { x: Double, y: Double }; p = Point { x: 1.0, y: 2.0 }; main = p.x",
        );
        assert!(ir.value_blocks[0]
            .instrs
            .iter()
            .any(|i| matches!(i, ValueInstr::ValueProject { .. })));
    }

    #[test]
    fn newtype_construct_lowers_to_value_newtype() {
        let ir = ir_of("newtype Hz = Double; main = Hz 440.0");
        assert!(ir.value_blocks[0]
            .instrs
            .iter()
            .any(|i| matches!(i, ValueInstr::ValueNewtype { .. })));
    }

    #[test]
    fn newtype_bare_ref_is_compile_error() {
        // `Hz` alone is not a value — the constructor requires its argument.
        let p = parse(
            &tokenize("newtype Hz = Double; main = Hz").unwrap(),
            "newtype Hz = Double; main = Hz".as_bytes(),
        )
        .unwrap();
        assert!(infer_program(&p, p.mode).is_err());
    }

    #[test]
    fn match_lowers_to_value_match() {
        let ir = ir_of(
            r"data Shape = Circle Double | Rect Double Double; s = Circle 1.5; main = case s of { Circle r -> r; Rect w h -> w; }",
        );
        assert!(ir.value_blocks[0]
            .instrs
            .iter()
            .any(|i| matches!(i, ValueInstr::ValueMatch { .. })));
    }

    #[test]
    fn match_selects_matching_arm_not_first() {
        // The matching arm is NOT first: static dispatch must select Circle's
        // arm, emitting a single ValueMatch for Circle's ctor index and routing
        // the output to the Circle arm's body register (not Rect's `w`, which
        // would be None at runtime).
        let ir = ir_of(
            r"data Shape = Circle Double | Rect Double Double; s = Circle 1.5; main = case s of { Rect w h -> w; Circle r -> r; }",
        );
        let matches = ir.value_blocks[0]
            .instrs
            .iter()
            .filter(|i| matches!(i, ValueInstr::ValueMatch { .. }))
            .count();
        assert_eq!(
            matches, 1,
            "static dispatch must emit exactly one ValueMatch"
        );
        let vm = ir.value_blocks[0]
            .instrs
            .iter()
            .find(|i| matches!(i, ValueInstr::ValueMatch { .. }))
            .unwrap();
        let ctor = match vm {
            ValueInstr::ValueMatch { ctor, .. } => *ctor,
            _ => u32::MAX,
        };
        assert_eq!(
            ctor, 0,
            "Circle is the first declared ctor of Shape (index 0)"
        );
        // The output must be the register the MATCH writes the selected arm's
        // payload into, not a register some other arm's binding owns. Expressed
        // against the instruction rather than a literal index: a numeric literal
        // now occupies two registers (its exact payload and its `fromInteger`
        // result), so absolute numbering is not the invariant — the routing is.
        let arms = match vm {
            ValueInstr::ValueMatch { dst, .. } => dst.clone(),
            _ => Vec::new(),
        };
        assert_eq!(arms.len(), 1, "one arm is statically selected");
        assert_eq!(
            ir.result_reg, arms[0],
            "the output must be the selected (Circle) arm's payload register"
        );
    }

    #[test]
    fn lower_match_runtime_dispatch_emits_branch_ctor() {
        // A guarded arm can fail its guard at runtime, so the match must lower
        // to runtime BranchCtor dispatch instead of the static fast path.
        let src = r"data Shape = Circle Double | Rect Double Double; main = case (Circle 1.5) of { Circle r | r > 0.0 -> r; Circle r -> r; Rect w h -> w; };";
        let toks = tokenize(src).unwrap();
        let p = parse(&toks, src.as_bytes()).unwrap();
        let tp = infer_program(&p, p.mode).unwrap();
        let ir = lower(&tp).unwrap();
        assert!(
            ir.value_blocks
                .iter()
                .any(|b| matches!(b.term, ValueTerm::BranchCtor { .. })),
            "a guarded match must lower to BranchCtor dispatch"
        );
    }

    #[test]
    fn lower_match_literal_and_wildcard() {
        let src = r"main = case 0 of { 0 -> 1.0; _ -> 2.0; };";
        let toks = tokenize(src).unwrap();
        let p = parse(&toks, src.as_bytes()).unwrap();
        let tp = infer_program(&p, p.mode).unwrap();
        let ir = lower(&tp).unwrap();
        assert!(
            ir.value_blocks
                .iter()
                .any(|b| matches!(b.term, ValueTerm::BranchCtor { .. })),
            "a literal match must lower to a constructor-tag branch"
        );
    }

    #[test]
    fn match_wire_scrutinee_dispatches_at_runtime() {
        // A match arm with a real (non-`true`) guard has no statically-known
        // outcome: the match must lower to runtime BranchCtor dispatch instead
        // of the static-only compile-time selection.
        let p = parse(
            &tokenize(
                r"data Shape = Circle Double | Rect Double Double; main = case (Circle 1.5) of { Circle r | r > 0.5 -> r; Circle r -> r; Rect w h -> w; }",
            )
            .unwrap(),
            r"data Shape = Circle Double | Rect Double Double; main = case (Circle 1.5) of { Circle r | r > 0.5 -> r; Circle r -> r; Rect w h -> w; }"
                .as_bytes(),
        )
        .unwrap();
        let tp = infer_program(&p, p.mode).unwrap();
        let ir = lower(&tp).unwrap();
        assert!(
            ir.value_blocks
                .iter()
                .any(|b| matches!(b.term, ValueTerm::BranchCtor { .. })),
            "a guarded match must lower to BranchCtor dispatch"
        );
    }

    #[test]
    fn lower_if_emits_branch_ctor() {
        // A non-constant Bool condition (here: a comparison over a value def,
        // whose result is a runtime value, not a literal) must lower to runtime
        // dispatch. The condition is an ordinary `Bool` SUM value, so the
        // terminator is the constructor-tag branch a `case` uses — there is no
        // separate boolean branch node.
        let ir = ir_of("g = 1.0; main = if g > 0.5 then 1.0 else 2.0;");
        assert!(
            ir.value_blocks
                .iter()
                .any(|b| matches!(b.term, ValueTerm::BranchCtor { .. })),
            "a non-constant if condition must lower to a BranchCtor"
        );
    }

    #[test]
    fn lower_if_static_literal_skips_branch() {
        // A literal Bool condition selects one branch at compile time: a single
        // block, no branch term at all.
        let ir = ir_of("main = if false then 1.0 else 2.0;");
        assert!(
            !ir.value_blocks
                .iter()
                .any(|b| matches!(b.term, ValueTerm::BranchCtor { .. })),
            "a literal-Bool if must not lower to a branch"
        );
    }

    #[test]
    fn collection_subtree_sizes_use_open_collection_estimate() {
        // Open collections: the estimator is ELEM_EST element slots per
        // container (element counts are data-dependent, not in the type).
        let env = TypeEnv::default();
        let empty = HashSet::new();
        let lw = lw(&env, &empty);
        assert_eq!(
            lw.subtree_size(&ValueTy::Con("List".into(), vec![ValueTy::Double])),
            1 + Lowerer::ELEM_EST
        );
        assert_eq!(
            lw.subtree_size(&ValueTy::Con(
                "Map".into(),
                vec![ValueTy::String, ValueTy::Double]
            )),
            1 + Lowerer::ELEM_EST * 2
        );
        assert_eq!(
            lw.subtree_size(&ValueTy::Con("Set".into(), vec![ValueTy::Int])),
            1 + Lowerer::ELEM_EST
        );
        assert_eq!(
            lw.subtree_size(&ValueTy::Con("Maybe".into(), vec![ValueTy::Double])),
            2
        );
        assert_eq!(
            lw.subtree_size(&ValueTy::Con(
                "Pair".into(),
                vec![ValueTy::Double, ValueTy::Int]
            )),
            3
        );
        assert_eq!(
            lw.subtree_size(&ValueTy::Con(
                "Either".into(),
                vec![ValueTy::Double, ValueTy::Int]
            )),
            2
        );
    }

    #[test]
    fn runst_lowers_to_runst_effect_op() {
        // A real `ST s a` body lowers to the region-running effect op: the
        // phantom `s` has no runtime representation, so `runST` consumes the
        // `ST` value and runs its region.
        let ir = ir_of(
            "{-# MODE complete #-} main = runST (\\s -> ST { unST: \\t -> Pair { first: t, second: 1.0 } });",
        );
        assert!(
            has_instr(&ir, |i| matches!(
                i,
                ValueInstr::ValueEffectOp {
                    op: ValueEffectOp::RunST,
                    ..
                }
            )),
            "expected a RunST effect instruction"
        );
    }

    #[test]
    fn stref_ops_lower_to_effect_closures() {
        // `newSTRef`/`writeSTRef`/`readSTRef` inside a `runST` region lower to
        // the STRef effect ops: a `BuiltinCallable` applied to the region's
        // STState token by the `Monad (ST s)` bind chain.
        let ir = ir_of(
            "{-# MODE complete #-} main = runST (\\s -> do { r <- newSTRef 0.0; writeSTRef r 42.0; readSTRef r });",
        );
        for op in [
            ValueEffectOp::NewStRef,
            ValueEffectOp::WriteStRef,
            ValueEffectOp::ReadStRef,
            ValueEffectOp::RunST,
        ] {
            assert!(
                has_instr(&ir, |i| matches!(
                    i,
                    ValueInstr::ValueEffectOp { op: o, .. } if *o == op
                )),
                "expected a {op:?} effect instruction"
            );
        }
    }

    #[test]
    fn literal_to_i64_reproduces_ghcs_truncation() {
        // Measured on GHC 9.10.3: `show (n :: Int)` for each literal below. GHC
        // accepts every one of them and warns `-Woverflowed-literals`; the
        // values are what must match.
        for (literal, ghc) in [
            ("123456789012345678901234567890", -4362896299872285998i64),
            ("9223372036854775808", i64::MIN),
            ("18446744073709551616", 0),
            ("18446744073709551621", 5),
            ("-18446744073709551621", -5),
            (
                "10000000000000000000000000000000000000000",
                -5047021154770878464,
            ),
            ("9223372036854775807", i64::MAX),
            ("-9223372036854775808", i64::MIN),
            // GHC parses this as `negate 9223372036854775809`: the literal
            // truncates to `-9223372036854775807` and the negation yields the
            // MAXIMUM, not the minimum. Negating before converting is what makes
            // this case right — converting first and negating twice is not.
            ("-9223372036854775809", i64::MAX),
            ("-18446744073709551616", 0),
            ("-18446744073709551617", -1),
            ("0", 0),
            ("1", 1),
            ("-1", -1),
        ] {
            let v: BigInt = literal.parse().unwrap();
            assert_eq!(
                literal_to_i64(&v),
                ghc,
                "literal {literal} must truncate to GHC's value"
            );
        }
    }

    #[test]
    fn an_in_range_literal_converts_exactly() {
        // The boundary is only a boundary: every value that FITS is untouched,
        // so ordinary arithmetic keeps working through the conversion.
        let v: BigInt = "1234567890123".parse().unwrap();
        assert_eq!(literal_to_i64(&v), 1234567890123);
        assert_eq!(literal_to_i64(&-v.clone()), -1234567890123);
        assert_eq!(literal_to_f64(&v), 1234567890123.0);
    }

    #[test]
    fn literal_to_f64_rounds_and_saturates_like_frominteger() {
        // Measured on GHC 9.10.3: `show (fromInteger n :: Double)` for every
        // value below. These pin the TWO properties `to_str_radix` +
        // `str::parse` promise and that `num-bigint`'s `ToPrimitive::to_f64`
        // would NOT satisfy: correct rounding of a mantissa-cut value, and
        // saturation to infinity instead of a wrap or a panic.
        for (literal, ghc) in [
            // Far beyond i64, and a value whose mantissa is 54 significant bits:
            // truncating (instead of rounding) gives a different double.
            ("123456789012345678901234567890", 1.2345678901234568e29f64),
            // 2^53 + 1: the first integer a double cannot represent exactly.
            // Round-to-nearest-EVEN must give 2^53, i.e. ...992, not ...994.
            ("9007199254740993", 9007199254740992.0f64),
            // The i64 boundary itself: 2^63 is exactly representable.
            ("9223372036854775808", 9.223372036854776e18f64),
            ("-9223372036854775809", -9.223372036854776e18f64),
        ] {
            let v: BigInt = literal.parse().unwrap();
            assert_eq!(
                literal_to_f64(&v),
                ghc,
                "literal {literal} must convert to GHC's Double"
            );
        }
        // A 400-digit integer is outside f64's finite range: GHC prints
        // `Infinity`, and the sign is preserved.
        let huge: BigInt = "9".repeat(400).parse().unwrap();
        assert_eq!(literal_to_f64(&huge), f64::INFINITY);
        assert_eq!(literal_to_f64(&-huge), f64::NEG_INFINITY);
    }

    #[test]
    fn rational_to_f64_is_the_one_conversion_boundary() {
        // Measured on GHC 9.10.3: `show (x :: Double)` for the same decimal
        // texts the lexer turns into exact ratios. `0.1` can only be
        // approximated here — the EXACT `1/10` lives in the AST — but the
        // approximation must be the correctly rounded nearest double.
        for (literal, ghc) in [
            ("0.1", 0.1f64),
            ("3.14", 3.14f64),
            ("1e3", 1000.0f64),
            ("1.5e-2", 0.015f64),
            // 18 digits whose exact rational is NOT exactly the double:
            // truncating the mantissa instead of rounding would differ.
            ("1.2345678901234567890123", 1.2345678901234567f64),
            ("9007199254740993.0", 9007199254740992.0f64),
        ] {
            let exact = crate::lexer::decimal_rational(literal).unwrap();
            assert_eq!(
                rational_to_f64(&exact),
                ghc,
                "`{literal}` must convert to GHC's Double"
            );
        }
        // Beyond f64's finite range the conversion saturates, as GHC prints
        // `Infinity` for `1e400 :: Double`.
        let huge = crate::lexer::decimal_rational("1e400").unwrap();
        assert_eq!(rational_to_f64(&huge), f64::INFINITY);
    }
}
