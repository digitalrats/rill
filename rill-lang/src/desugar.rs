//! Faust-combinator sugar: a foreign fn used as an arrow in a combinator
//! (`Seq`/`Loop`/`Par`/`Split`/`Merge`/`Delay`) auto-binds its missing leading
//! `FixedBuffer` (signal) params to `Wire`. Legacy `_ : onepole 200.0 0.7`
//! desugars to `onepole _ 200.0 0.7` — the FFI model binds signals
//! positionally, and this pass keeps existing programs working.

use crate::ast::{Def, Expr, Param, Program};
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
        Expr::Seq(l, r, sp) => {
            let l = rewrite(l, env);
            let r = rewrite(r, env);
            // `:` overload — a List rhs makes `:` Haskell-style cons
            // (`x : xs` ≣ `cons x xs`), a Buffer rhs keeps Faust sequencing.
            // The parser builds `a : b : c` LEFT-associatively
            // (`(a : b) : c`); cons must chain right, so re-associate a
            // list-terminated chain into `a : (b : c)`.
            if is_list_expression(&r) {
                let mut elems = Vec::new();
                flatten_seq(&Expr::Seq(Box::new(l), Box::new(r), *sp), &mut elems);
                rebuild_right_chain(elems, *sp)
            } else {
                Expr::Seq(
                    Box::new(bind_signal_args(&l, env)),
                    Box::new(bind_signal_args(&r, env)),
                    *sp,
                )
            }
        }
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
        // `A @ n`: the lhs is the delayed signal arrow and needs the sugar; the
        // rhs is the constant delay-length literal and is left as-is.
        Expr::Delay(l, r, sp) => Expr::Delay(
            Box::new(bind_signal_args(&rewrite(l, env), env)),
            Box::new(rewrite(r, env)),
            *sp,
        ),
        // Recurse into Apply arguments and `let` defs/bodies so a builtin call
        // nested deeper than a direct combinator operand is still handled.
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

/// Whether `e` syntactically denotes a List value: a `[]` literal, a bare
/// `list`, a `cons` call, or a `:` cons-chain terminating in one. Used to
/// disambiguate the `:` overload BEFORE inference (List rhs → cons, else seq).
fn is_list_expression(e: &Expr) -> bool {
    match e {
        Expr::ListLit(..) => true,
        Expr::Ref(name, _) => matches!(name.as_str(), "list"),
        Expr::Apply { name, .. } => matches!(name.as_str(), "list" | "cons"),
        Expr::Seq(_, r, _) => is_list_expression(r),
        _ => false,
    }
}

/// Flatten a LEFT-associative `a : b : c` (`(a : b) : c`) into `[a, b, c]`.
fn flatten_seq(e: &Expr, out: &mut Vec<Expr>) {
    match e {
        Expr::Seq(l, r, _) => {
            flatten_seq(l, out);
            out.push((**r).clone());
        }
        other => out.push(other.clone()),
    }
}

/// Rebuild a flattened chain RIGHT-associatively: `[a, b, c]` →
/// `a : (b : c)` (the cons/`::`-like shape).
fn rebuild_right_chain(elems: Vec<Expr>, span: crate::error::Span) -> Expr {
    let mut it = elems.into_iter().rev();
    let mut acc = it.next().expect("non-empty chain");
    for e in it {
        acc = Expr::Seq(Box::new(e), Box::new(acc), span);
    }
    acc
}

/// If `e` is an `Apply`/`Ref` to a foreign fn whose trailing signal params are
/// NOT supplied positionally (the args are exactly the leading scalar params),
/// append the missing `Wire` args. A positional call (`onepole 200.0 0.7 _`)
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
    let Some(sig) = ffi_sig_from_typeexpr(name, te) else {
        return e.clone();
    };
    // The rule: the supplied args are exactly the leading scalar params (the
    // legacy combinator style never passes the signal positionally), the signal
    // params trail the signature, and there is no variadic tail. Appending the
    // signal wires then yields the positional FFI call.
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
    let scalar_count = sig.params.len() - n_sig;
    let leading_scalars = sig
        .params
        .iter()
        .take(scalar_count)
        .all(|p| !matches!(p, FfiParam::Signal));
    let trailing_signals = sig
        .params
        .iter()
        .skip(scalar_count)
        .all(|p| matches!(p, FfiParam::Signal));
    if !leading_scalars || !trailing_signals || args_len != scalar_count {
        return e.clone();
    }
    let wires = vec![Expr::Wire(span); n_sig];
    match e {
        Expr::Apply { name, args, .. } => {
            let mut new_args = args.to_vec();
            new_args.extend(wires);
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

/// IO-model auto-desugar: rewrite an old `main = <buffer-expr>` program into
/// the IO form `main i1 i2 = do { writeOutput 0 <out0>; …; writeOutput N-1
/// <outN-1>; return () }`. `_` wires become `main`'s λ-parameters (typed
/// `Buffer` inputs by inference); the buffer outputs of `<buffer-expr>` become
/// `writeOutput` calls. A `main` that is already an IO action (contains
/// `writeOutput`/`readInput`, or a `bind`/`do`) is left untouched.
pub(crate) fn desugar_main_io(program: &mut Program) -> bool {
    // Signal-lambda MACROS: user definitions whose body carries a `_` wire
    // (`integ = + ~ _`, `a x = _ * x`) — their wires live inside the body,
    // invisible at the call site. Only these block auto-desugar. Includes
    // `where`-block definitions (which are not top-level).
    let mut signal_macros: std::collections::HashSet<String> = std::collections::HashSet::new();
    fn collect_signal_macros<'a>(
        defs: impl Iterator<Item = &'a Def>,
        out: &mut std::collections::HashSet<String>,
    ) {
        for d in defs {
            if !d.is_decl() && contains_wire(d.body()) {
                out.insert(d.name().to_string());
            }
            for wd in d.where_defs() {
                collect_signal_macros(std::iter::once(wd), out);
            }
        }
    }
    collect_signal_macros(program.defs.iter(), &mut signal_macros);
    for def in &mut program.defs {
        if def.name() == "main" {
            let body = def.body().clone();
            // A bare `Ref` or `let`-bound result (`main = gain where { gain =
            // _ * 0.5 }`) hides the signal wires inside the referenced def —
            // auto-desugar cannot collect them as `main`'s inputs.
            if is_io_action(&body)
                || is_value_expression(&body)
                || has_internal_wire_feed(&body)
                || calls_user_def(&body, &signal_macros)
                || matches!(body, Expr::Ref(..) | Expr::Let { .. })
            {
                return false;
            }
            // Collect the wires in order of first appearance, replace each
            // with a fresh `i_k` reference, and gather the buffer outputs.
            let mut wire_names: Vec<String> = Vec::new();
            // Split top-level Par into separate outputs (each becomes a
            // `writeOutput` action), rewrite wires in each.
            let branches = split_par(&body);
            // Multi-output programs (`main = osc , osc`) would split a shared
            // CAF across `writeOutput` sites in separate fragments, breaking
            // CAF-sharing (one instance per tick); keep them on the legacy
            // signal path.
            if branches.len() != 1 {
                return false;
            }
            let mut outs: Vec<Expr> = Vec::new();
            for b in branches {
                outs.push(collect_and_rewrite(&b, &mut wire_names));
            }
            // Multi-INPUT programs (`conj _ _` — a single expression consuming
            // two wires) would need a `writeOutput` whose argument has >1
            // channel; not yet supported — keep those on the legacy signal
            // path.
            if wire_names.len() > 1 {
                return false;
            }
            if outs.is_empty() {
                outs.push(collect_and_rewrite(&body, &mut wire_names));
            }
            let params: Vec<Param> = wire_names
                .iter()
                .enumerate()
                .map(|(k, n)| Param {
                    name: n.clone(),
                    span: crate::error::Span::new(k, k + 1),
                })
                .collect();
            // Build: do { writeOutput 0 <out0>; …; return () }.
            let span = body.span();
            let mut stmts: Vec<Expr> = Vec::with_capacity(outs.len());
            for (k, o) in outs.iter().enumerate() {
                stmts.push(Expr::Apply {
                    name: "writeOutput".to_string(),
                    args: vec![Expr::Int(k as i64, span), o.clone()],
                    span,
                });
            }
            let ret = Expr::Apply {
                name: "return".to_string(),
                args: vec![Expr::Void(span)],
                span,
            };
            let io_body = wrap_do(stmts, ret, span);
            // Rebuild the Def: a `Local` (`main = expr`) becomes an `Anchor`
            // with the wire λ-parameters; an existing `Anchor` gains them.
            match def {
                Def::Local {
                    name,
                    where_defs,
                    span: dspan,
                    ..
                } => {
                    *def = Def::Anchor {
                        name: name.clone(),
                        params,
                        body: io_body,
                        where_defs: where_defs.clone(),
                        span: *dspan,
                    };
                }
                Def::Anchor {
                    body: b, params: p, ..
                } => {
                    *b = io_body;
                    p.extend(params);
                }
                _ => {}
            }
            return true;
        }
    }
    false
}

/// Whether `e` is already an IO action (has a `writeOutput`/`readInput` call,
/// or a `bind`/`do` desugar).
fn is_io_action(e: &Expr) -> bool {
    match e {
        Expr::Apply { name, args, .. } => {
            matches!(
                name.as_str(),
                "writeOutput" | "readInput" | "bind" | "return"
            ) || args.iter().any(is_io_action)
        }
        Expr::Let { defs, body, .. } => {
            defs.iter().any(|d| is_io_action(d.body())) || is_io_action(body)
        }
        _ => false,
    }
}

/// Whether `e` is a VALUE expression (types as a value channel, not a `Buffer`
/// signal). Auto-desugar is restricted to SIGNAL programs: a `main` whose body
/// builds a record/sum/list/map, matches, branches, or projects a field types
/// as a value and must NOT be wrapped in `writeOutput`. A pure signal
/// expression (wires, combinators, DSP applications, arithmetic over buffers)
/// contains none of these value constructs.
fn is_value_expression(e: &Expr) -> bool {
    match e {
        Expr::Match { .. }
        | Expr::Record(..)
        | Expr::If { .. }
        | Expr::Lambda { .. }
        | Expr::ListLit(..)
        | Expr::MapLit(..)
        | Expr::FieldProject { .. }
        | Expr::FieldUpdate { .. }
        | Expr::Cmp { .. }
        | Expr::Logic { .. } => true,
        Expr::Apply { args, .. } => args.iter().any(is_value_expression),
        Expr::ApplyExpr { callee, args, .. } => {
            is_value_expression(callee) || args.iter().any(is_value_expression)
        }
        Expr::Neg(inner, _) => is_value_expression(inner),
        Expr::Arith { lhs, rhs, .. } => is_value_expression(lhs) || is_value_expression(rhs),
        Expr::Seq(l, r, _)
        | Expr::Par(l, r, _)
        | Expr::Split(l, r, _)
        | Expr::Merge(l, r, _)
        | Expr::Loop(l, r, _)
        | Expr::Delay(l, r, _) => is_value_expression(l) || is_value_expression(r),
        Expr::Let { defs, body, .. } => {
            defs.iter().any(|d| is_value_expression(d.body())) || is_value_expression(body)
        }
        _ => false,
    }
}

/// Whether `e` contains a `_` (Wire) anywhere — a signal-lambda body's open
/// input.
fn contains_wire(e: &Expr) -> bool {
    match e {
        Expr::Wire(_) => true,
        Expr::Apply { args, .. } => args.iter().any(contains_wire),
        Expr::ApplyExpr { callee, args, .. } => {
            contains_wire(callee) || args.iter().any(contains_wire)
        }
        Expr::Neg(inner, _) => contains_wire(inner),
        Expr::Arith { lhs, rhs, .. } => contains_wire(lhs) || contains_wire(rhs),
        Expr::Seq(l, r, _)
        | Expr::Par(l, r, _)
        | Expr::Split(l, r, _)
        | Expr::Merge(l, r, _)
        | Expr::Loop(l, r, _)
        | Expr::Delay(l, r, _) => contains_wire(l) || contains_wire(r),
        Expr::Let { defs, body, .. } => {
            defs.iter().any(|d| contains_wire(d.body())) || contains_wire(body)
        }
        _ => false,
    }
}

/// Whether `e` is a SIGNAL combinator with internal wire feeding (`A : B`,
/// `A <: B`, `A :> B`, `A ~ B`, `A @ n`). Such an expression carries wires
/// that are NOT program inputs (they are fed by the left operand), so
/// auto-desugar cannot collect them as `main`'s λ-parameters without double
/// counting. These programs keep the legacy signal path (already functional).
fn has_internal_wire_feed(e: &Expr) -> bool {
    match e {
        Expr::Seq(_, _, _)
        | Expr::Split(_, _, _)
        | Expr::Merge(_, _, _)
        | Expr::Loop(_, _, _)
        | Expr::Delay(_, _, _) => true,
        Expr::Apply { args, .. } => args.iter().any(has_internal_wire_feed),
        Expr::ApplyExpr { callee, args, .. } => {
            has_internal_wire_feed(callee) || args.iter().any(has_internal_wire_feed)
        }
        Expr::Neg(inner, _) => has_internal_wire_feed(inner),
        Expr::Arith { lhs, rhs, .. } => has_internal_wire_feed(lhs) || has_internal_wire_feed(rhs),
        Expr::Par(l, r, _) => has_internal_wire_feed(l) || has_internal_wire_feed(r),
        Expr::Let { defs, body, .. } => {
            defs.iter().any(|d| has_internal_wire_feed(d.body())) || has_internal_wire_feed(body)
        }
        _ => false,
    }
}

/// Whether `e` applies or references a USER-DEFINED function (a signal-lambda
/// macro whose wire lives inside its body, invisible at the call site). Such a
/// `main` keeps the legacy signal path — auto-desugar cannot see the open
/// wires.
fn calls_user_def(e: &Expr, user_defs: &std::collections::HashSet<String>) -> bool {
    match e {
        Expr::Ref(name, _) => user_defs.contains(name),
        Expr::Apply { name, args, .. } => {
            user_defs.contains(name) || args.iter().any(|a| calls_user_def(a, user_defs))
        }
        Expr::ApplyExpr { callee, args, .. } => {
            calls_user_def(callee, user_defs) || args.iter().any(|a| calls_user_def(a, user_defs))
        }
        Expr::Neg(inner, _) => calls_user_def(inner, user_defs),
        Expr::Arith { lhs, rhs, .. } => {
            calls_user_def(lhs, user_defs) || calls_user_def(rhs, user_defs)
        }
        Expr::Seq(l, r, _)
        | Expr::Par(l, r, _)
        | Expr::Split(l, r, _)
        | Expr::Merge(l, r, _)
        | Expr::Loop(l, r, _)
        | Expr::Delay(l, r, _) => calls_user_def(l, user_defs) || calls_user_def(r, user_defs),
        Expr::Let { defs, body, .. } => {
            defs.iter().any(|d| calls_user_def(d.body(), user_defs))
                || calls_user_def(body, user_defs)
        }
        _ => false,
    }
}

/// Split a top-level `A , B` expression into its constituent buffer outputs
/// (recursively), preserving order. A non-Par expression is a single output.
fn split_par(e: &Expr) -> Vec<Expr> {
    match e {
        Expr::Par(l, r, _) => {
            let mut v = split_par(l);
            v.extend(split_par(r));
            v
        }
        _ => vec![e.clone()],
    }
}

/// Replace every `_` (Wire) in `e` in order of first appearance with
/// `Ref("i_k")`.
fn collect_and_rewrite(e: &Expr, wire_names: &mut Vec<String>) -> Expr {
    match e {
        Expr::Wire(span) => {
            let idx = wire_names.len();
            let name = format!("i{}", idx + 1);
            wire_names.push(name.clone());
            Expr::Ref(name, *span)
        }
        Expr::Seq(l, r, sp) => Expr::Seq(
            Box::new(collect_and_rewrite(l, wire_names)),
            Box::new(collect_and_rewrite(r, wire_names)),
            *sp,
        ),
        Expr::Split(l, r, sp) => Expr::Split(
            Box::new(collect_and_rewrite(l, wire_names)),
            Box::new(collect_and_rewrite(r, wire_names)),
            *sp,
        ),
        Expr::Merge(l, r, sp) => Expr::Merge(
            Box::new(collect_and_rewrite(l, wire_names)),
            Box::new(collect_and_rewrite(r, wire_names)),
            *sp,
        ),
        Expr::Loop(l, r, sp) => Expr::Loop(
            Box::new(collect_and_rewrite(l, wire_names)),
            Box::new(collect_and_rewrite(r, wire_names)),
            *sp,
        ),
        Expr::Delay(l, r, sp) => Expr::Delay(
            Box::new(collect_and_rewrite(l, wire_names)),
            Box::new(collect_and_rewrite(r, wire_names)),
            *sp,
        ),
        Expr::Par(l, r, sp) => Expr::Par(
            Box::new(collect_and_rewrite(l, wire_names)),
            Box::new(collect_and_rewrite(r, wire_names)),
            *sp,
        ),
        Expr::Apply { name, args, span } => Expr::Apply {
            name: name.clone(),
            args: args
                .iter()
                .map(|a| collect_and_rewrite(a, wire_names))
                .collect(),
            span: *span,
        },
        Expr::Arith { op, lhs, rhs, span } => Expr::Arith {
            op: *op,
            lhs: Box::new(collect_and_rewrite(lhs, wire_names)),
            rhs: Box::new(collect_and_rewrite(rhs, wire_names)),
            span: *span,
        },
        Expr::Neg(inner, span) => {
            Expr::Neg(Box::new(collect_and_rewrite(inner, wire_names)), *span)
        }
        Expr::Let { defs, body, span } => Expr::Let {
            defs: defs.clone(),
            body: Box::new(collect_and_rewrite(body, wire_names)),
            span: *span,
        },
        other => other.clone(),
    }
}

/// Build `do { <stmts>; <ret> }` as nested `bind` calls (the parser's do-block
/// desugar).
fn wrap_do(stmts: Vec<Expr>, ret: Expr, span: crate::error::Span) -> Expr {
    let mut rest = ret;
    for s in stmts.into_iter().rev() {
        let lam = Expr::Lambda {
            params: vec![Param {
                name: "_".to_string(),
                span,
            }],
            body: Box::new(rest),
            span,
        };
        rest = Expr::Apply {
            name: "bind".to_string(),
            args: vec![s, lam],
            span,
        };
    }
    rest
}
