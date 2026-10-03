//! Faust-combinator sugar: a foreign fn used as an arrow in a combinator
//! (`Seq`/`Loop`/`Par`/`Split`/`Merge`) auto-binds its missing leading
//! `FixedBuffer` (signal) params to `Wire`. Legacy `_ : onepole 200.0 0.7`
//! desugars to `onepole _ 200.0 0.7` — the FFI model binds signals
//! positionally, and this pass keeps existing programs working.

use crate::ast::{Def, Expr, Program};
use crate::types::ffi::{ffi_sig_from_typeexpr, FfiParam};
use crate::types::ty::TypeEnv;

/// Rewrite a program's signal combinator operands so foreign fns with missing
/// leading signal params get `Wire` args prepended. Runs from inference
/// (`infer_program_with`) after `TypeEnv::foreign_sigs` is populated (the
/// builtin catalog + user `foreign fn` declarations), before any def body is
/// typed — reduce.rs has no `TypeEnv`.
pub(crate) fn desugar_foreign_combinators(program: &mut Program, env: &TypeEnv) {
    for def in &mut program.defs {
        rewrite_def(def, env);
    }
}

/// Rewrite a definition's body and (transitively) its `where` defs.
fn rewrite_def(def: &mut Def, env: &TypeEnv) {
    match def {
        Def::Local {
            body, where_defs, ..
        }
        | Def::Anchor {
            body, where_defs, ..
        } => {
            *body = rewrite(body, env);
            for wd in where_defs.iter_mut() {
                rewrite_def(wd, env);
            }
        }
        _ => {}
    }
}

/// Rewrite every combinator operand in `e`; a direct combinator operand that is
/// itself an `Apply`/`Ref` to a foreign fn gets its missing leading signal
/// args bound by [`bind_signal_args`].
fn rewrite(e: &Expr, env: &TypeEnv) -> Expr {
    match e {
        Expr::Seq(l, r, sp) => Expr::Seq(
            Box::new(bind_signal_args(&rewrite(l, env), env)),
            Box::new(bind_signal_args(&rewrite(r, env), env)),
            *sp,
        ),
        Expr::Loop(l, r, sp) => Expr::Loop(
            Box::new(bind_signal_args(&rewrite(l, env), env)),
            Box::new(bind_signal_args(&rewrite(r, env), env)),
            *sp,
        ),
        Expr::Par(l, r, sp) => Expr::Par(
            Box::new(bind_signal_args(&rewrite(l, env), env)),
            Box::new(bind_signal_args(&rewrite(r, env), env)),
            *sp,
        ),
        Expr::Split(l, r, sp) => Expr::Split(
            Box::new(bind_signal_args(&rewrite(l, env), env)),
            Box::new(bind_signal_args(&rewrite(r, env), env)),
            *sp,
        ),
        Expr::Merge(l, r, sp) => Expr::Merge(
            Box::new(bind_signal_args(&rewrite(l, env), env)),
            Box::new(bind_signal_args(&rewrite(r, env), env)),
            *sp,
        ),
        // Recurse into other expression kinds so a builtin call nested inside a
        // combinator operand that is NOT itself a top-level Apply is still
        // handled.
        Expr::Apply { name, args, span } => {
            let args = args.iter().map(|a| rewrite(a, env)).collect();
            Expr::Apply {
                name: name.clone(),
                args,
                span: *span,
            }
        }
        Expr::Let { defs, body, span } => {
            let defs = defs
                .iter()
                .map(|d| {
                    let mut d = d.clone();
                    rewrite_def(&mut d, env);
                    d
                })
                .collect();
            Expr::Let {
                defs,
                body: Box::new(rewrite(body, env)),
                span: *span,
            }
        }
        other => other.clone(),
    }
}

/// If `e` is an `Apply`/`Ref` to a foreign fn whose leading signal params are
/// NOT supplied positionally (the args are exactly the trailing scalar params),
/// prepend the missing `Wire` args. A positional call (`onepole _ 200.0 0.7`)
/// and an incomplete call (`onepole 200.0`) are left untouched — the former
/// already binds the signal, the latter is a genuine arity error surfaced by
/// the FFI inference arm.
fn bind_signal_args(e: &Expr, env: &TypeEnv) -> Expr {
    let (name, args_len, span) = match e {
        Expr::Apply { name, args, span } => (name, args.len(), *span),
        Expr::Ref(name, span) => (name, 0, *span),
        _ => return e.clone(),
    };
    let Some(te) = env.foreign_sigs.get(name.as_str()) else {
        return e.clone();
    };
    let Some(sig) = ffi_sig_from_typeexpr(te) else {
        return e.clone();
    };
    // The rule: the supplied args are exactly the trailing scalar params (the
    // legacy combinator style never passes the signal positionally), the signal
    // params lead the signature, and there is no variadic tail (record/variadic
    // builtins land in Task 4). Prepending the signal wires then yields the
    // positional FFI call.
    let n_sig = sig
        .params
        .iter()
        .filter(|p| matches!(p, FfiParam::Signal))
        .count();
    if n_sig == 0
        || sig
            .params
            .iter()
            .any(|p| matches!(p, FfiParam::VariadicSignal))
    {
        return e.clone();
    }
    let leading_signals = sig
        .params
        .iter()
        .take(n_sig)
        .all(|p| matches!(p, FfiParam::Signal));
    let trailing_scalars = sig
        .params
        .iter()
        .skip(n_sig)
        .all(|p| !matches!(p, FfiParam::Signal));
    let scalar_count = sig.params.len() - n_sig;
    if !leading_signals || !trailing_scalars || args_len != scalar_count {
        return e.clone();
    }
    let wires = vec![Expr::Wire(span); n_sig];
    match e {
        Expr::Apply { name, args, .. } => {
            let mut new_args = wires;
            new_args.extend(args.iter().cloned());
            Expr::Apply {
                name: name.clone(),
                args: new_args,
                span,
            }
        }
        Expr::Ref(name, _) => Expr::Apply {
            name: name.clone(),
            args: wires,
            span,
        },
        _ => unreachable!("checked Apply/Ref above"),
    }
}
