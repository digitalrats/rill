//! β-reduction: inline user-defined function calls after type inference.
//!
//! After this pass, the AST contains no `Apply` nodes targeting named
//! λ-parameter definitions — only builtins, `smooth`, `param`, combinators,
//! and runtime-dispatch `Apply` nodes over closure values (`add2 = adder 2.0`)
//! remain. This simplifies lowering: no Anchor handling in `lower_ref`.

use std::collections::{HashMap, HashSet};

use crate::ast::{ArithOp, Def, Expr, Program};
use crate::error::Span;

fn substitute(e: &Expr, subst: &HashMap<String, Expr>) -> Expr {
    match e {
        Expr::Ref(name, _) => {
            // CAF references are not inlined during substitution: they pass
            // through unchanged — `reduce_expr` keeps them once the β-reduction
            // completes.
            if let Some(replacement) = subst.get(name) {
                replacement.clone()
            } else {
                e.clone()
            }
        }
        Expr::Neg(inner, span) => Expr::Neg(Box::new(substitute(inner, subst)), *span),
        Expr::Apply { name, args, span } => {
            let reduced_args: Vec<Expr> = args.iter().map(|a| substitute(a, subst)).collect();
            Expr::Apply {
                name: name.clone(),
                args: reduced_args,
                span: *span,
            }
        }
        Expr::Seq(lhs, rhs, span) => Expr::Seq(
            Box::new(substitute(lhs, subst)),
            Box::new(substitute(rhs, subst)),
            *span,
        ),
        Expr::Par(lhs, rhs, span) => Expr::Par(
            Box::new(substitute(lhs, subst)),
            Box::new(substitute(rhs, subst)),
            *span,
        ),
        Expr::Split(lhs, rhs, span) => Expr::Split(
            Box::new(substitute(lhs, subst)),
            Box::new(substitute(rhs, subst)),
            *span,
        ),
        Expr::Merge(lhs, rhs, span) => Expr::Merge(
            Box::new(substitute(lhs, subst)),
            Box::new(substitute(rhs, subst)),
            *span,
        ),
        Expr::Loop(lhs, rhs, span) => Expr::Loop(
            Box::new(substitute(lhs, subst)),
            Box::new(substitute(rhs, subst)),
            *span,
        ),
        Expr::Delay(lhs, rhs, span) => Expr::Delay(
            Box::new(substitute(lhs, subst)),
            Box::new(substitute(rhs, subst)),
            *span,
        ),
        Expr::Arith { op, lhs, rhs, span } => Expr::Arith {
            op: *op,
            lhs: Box::new(substitute(lhs, subst)),
            rhs: Box::new(substitute(rhs, subst)),
            span: *span,
        },
        Expr::FieldProject {
            record,
            field,
            span,
        } => Expr::FieldProject {
            record: Box::new(substitute(record, subst)),
            field: field.clone(),
            span: *span,
        },
        Expr::FieldUpdate {
            record,
            field,
            value,
            span,
        } => Expr::FieldUpdate {
            record: Box::new(substitute(record, subst)),
            field: field.clone(),
            value: Box::new(substitute(value, subst)),
            span: *span,
        },
        Expr::Match {
            scrutinee,
            arms,
            span,
        } => {
            // A match-arm binding shadows an outer name of the same spelling:
            // drop it from the substitution while descending into the arm body.
            let reduced_arms: Vec<(String, Vec<crate::ast::Param>, Expr)> = arms
                .iter()
                .map(|(ctor, params, body)| {
                    let mut inner = subst.clone();
                    for p in params {
                        inner.remove(&p.name);
                    }
                    (ctor.clone(), params.clone(), substitute(body, &inner))
                })
                .collect();
            Expr::Match {
                scrutinee: Box::new(substitute(scrutinee, subst)),
                arms: reduced_arms,
                span: *span,
            }
        }
        Expr::Lambda { params, body, span } => {
            // A lambda rebinds its parameters inside the body: drop them from
            // the substitution so an outer binding of the same spelling is not
            // inlined into the lambda. Free names substituted before the lambda
            // become by-value captures (correct for the env-snapshot model).
            let mut inner = subst.clone();
            for p in params {
                inner.remove(&p.name);
            }
            Expr::Lambda {
                params: params.clone(),
                body: Box::new(substitute(body, &inner)),
                span: *span,
            }
        }
        Expr::Record(fields, span) => Expr::Record(
            fields
                .iter()
                .map(|(n, e)| (n.clone(), substitute(e, subst)))
                .collect(),
            *span,
        ),
        Expr::Bool(_, _) => e.clone(),
        Expr::ListLit(elems, span) => Expr::ListLit(
            elems.iter().map(|el| substitute(el, subst)).collect(),
            *span,
        ),
        Expr::MapLit(entries, span) => Expr::MapLit(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), substitute(v, subst)))
                .collect(),
            *span,
        ),
        Expr::Cmp { op, lhs, rhs, span } => Expr::Cmp {
            op: *op,
            lhs: Box::new(substitute(lhs, subst)),
            rhs: Box::new(substitute(rhs, subst)),
            span: *span,
        },
        Expr::Logic { op, lhs, rhs, span } => Expr::Logic {
            op: *op,
            lhs: Box::new(substitute(lhs, subst)),
            rhs: Box::new(substitute(rhs, subst)),
            span: *span,
        },
        _ => e.clone(),
    }
}

fn defs_map(defs: &[Def]) -> HashMap<String, Def> {
    defs.iter()
        .map(|d| (d.name().to_string(), d.clone()))
        .collect()
}

/// Whether `name` resolves to a closure-valued definition: a `Def::Local`
/// whose body is a lambda literal or a reference/application chain that
/// produces one (e.g. `add2 = adder 2.0`). Applying a closure value is a
/// RUNTIME dispatch (`ValueCallFunc`), so such `Apply` nodes must survive
/// reduction — inlining a closure body would drop the applied arguments.
/// Named λ-parameter definitions (`double x = ...`) remain β-reduced.
fn is_closure_def(ctx: &HashMap<String, Def>, name: &str, seen: &mut HashSet<String>) -> bool {
    if !seen.insert(name.to_string()) {
        return false;
    }
    match ctx.get(name) {
        Some(Def::Local { body, .. }) => match body {
            Expr::Lambda { .. } => true,
            Expr::Ref(next, _) => is_closure_def(ctx, next, seen),
            Expr::Apply { .. } => true,
            _ => false,
        },
        _ => false,
    }
}

fn reduce_def(def: &Def, ctx: &HashMap<String, Def>, cafs: &HashSet<String>) -> Def {
    if def.is_decl() {
        return def.clone();
    }
    let reduced_body = reduce_expr(def.body(), ctx, cafs);
    let reduced_where: Vec<Def> = def
        .where_defs()
        .iter()
        .map(|d| reduce_def(d, ctx, cafs))
        .collect();
    match def {
        Def::Anchor {
            name, params, span, ..
        } => Def::Anchor {
            name: name.clone(),
            params: params.clone(),
            body: reduced_body,
            where_defs: reduced_where,
            span: *span,
        },
        Def::Local { name, span, .. } => Def::Local {
            name: name.clone(),
            body: reduced_body,
            where_defs: reduced_where,
            span: *span,
        },
        _ => def.clone(),
    }
}

fn reduce_expr(e: &Expr, ctx: &HashMap<String, Def>, cafs: &HashSet<String>) -> Expr {
    match e {
        Expr::Ref(name, _) => {
            if cafs.contains(name) {
                // Closed top-level definition (CAF): keep the shared reference so
                // lowering can lift it once instead of duplicating state here.
                e.clone()
            } else if let Some(def) = ctx.get(name) {
                if def.params().is_empty() && !def.is_decl() {
                    // Local binding with no params — inline the body
                    reduce_expr(def.body(), ctx, cafs)
                } else {
                    // Has unapplied λ-params — can't inline, leave as ref
                    e.clone()
                }
            } else {
                e.clone()
            }
        }
        Expr::Apply { name, args, span } => {
            let reduced_args: Vec<Expr> = args.iter().map(|a| reduce_expr(a, ctx, cafs)).collect();
            // Closure-valued applications (`add2 = adder 2.0`) are runtime
            // dispatches on closure values: leave the Apply in place so
            // lowering emits a `ValueCallFunc`. β-reducing a closure body would
            // drop the applied arguments.
            if is_closure_def(ctx, name, &mut HashSet::new()) {
                return Expr::Apply {
                    name: name.clone(),
                    args: reduced_args,
                    span: *span,
                };
            }
            if let Some(def) = ctx.get(name) {
                if def.is_decl() {
                    // Type declarations aren't signal definitions — keep the
                    // application as-is rather than inlining a sentinel body.
                    Expr::Apply {
                        name: name.clone(),
                        args: reduced_args,
                        span: *span,
                    }
                } else if reduced_args.len() < def.params().len() {
                    // Partial application (`add5 = add2 5.0` where `add2` takes
                    // two λ-params) is a RUNTIME dispatch over a closure value:
                    // keep the Apply so lowering emits a curry closure.
                    // β-reducing it would substitute the applied args and leave
                    // the remaining parameters dangling in the body.
                    Expr::Apply {
                        name: name.clone(),
                        args: reduced_args,
                        span: *span,
                    }
                } else {
                    // β-reduce: substitute args for params in the definition's body
                    let np = def.params().len();
                    let mut subst = HashMap::new();
                    for (idx, p) in def.params().iter().enumerate() {
                        if p.name != "_" {
                            subst.insert(p.name.clone(), reduced_args[idx].clone());
                        }
                    }
                    let inlined = substitute(def.body(), &subst);
                    // A func-value call (`f = double; main = f 21.0`) applies
                    // a definition with FEWER λ-params than arguments: the
                    // def's body is a `Ref` to the referenced definition, and
                    // the leftover arguments must be re-applied to it
                    // (`double 21.0`), not dropped. In v1 func values are
                    // named references only, so the inlined body is always a
                    // `Ref`; any other shape falls back to the pre-fix inline.
                    let wrapped = if reduced_args.len() > np {
                        match inlined {
                            Expr::Ref(inner_name, _) => Expr::Apply {
                                name: inner_name,
                                args: reduced_args[np..].to_vec(),
                                span: *span,
                            },
                            other => other,
                        }
                    } else {
                        inlined
                    };
                    // Recursively reduce the inlined body (may contain more calls)
                    reduce_expr(&wrapped, ctx, cafs)
                }
            } else {
                // Builtin, math, or unknown — leave as-is
                Expr::Apply {
                    name: name.clone(),
                    args: reduced_args,
                    span: *span,
                }
            }
        }
        Expr::Let {
            defs,
            body,
            span: _,
        } => {
            // Reduce let defs, build context, reduce body
            let reduced_defs: Vec<Def> = defs.iter().map(|d| reduce_def(d, ctx, cafs)).collect();
            let let_ctx = merge_contexts(ctx, &defs_map(&reduced_defs));
            reduce_expr(body, &let_ctx, cafs)
        }
        Expr::Loop(lhs, rhs, span) => {
            // Desugar the Faust-style integrator short forms to block built-ins:
            //   `+ ~ _`       → `integrator`
            //   `+ ~ (_ * k)` → `leaky_integrator k`
            if let Some(desugared) = desugar_integrator(lhs, rhs, *span) {
                return desugared;
            }
            Expr::Loop(
                Box::new(reduce_expr(lhs, ctx, cafs)),
                Box::new(reduce_expr(rhs, ctx, cafs)),
                *span,
            )
        }
        Expr::Seq(lhs, rhs, span) => Expr::Seq(
            Box::new(reduce_expr(lhs, ctx, cafs)),
            Box::new(reduce_expr(rhs, ctx, cafs)),
            *span,
        ),
        Expr::Par(lhs, rhs, span) => Expr::Par(
            Box::new(reduce_expr(lhs, ctx, cafs)),
            Box::new(reduce_expr(rhs, ctx, cafs)),
            *span,
        ),
        Expr::Split(lhs, rhs, span) => Expr::Split(
            Box::new(reduce_expr(lhs, ctx, cafs)),
            Box::new(reduce_expr(rhs, ctx, cafs)),
            *span,
        ),
        Expr::Merge(lhs, rhs, span) => Expr::Merge(
            Box::new(reduce_expr(lhs, ctx, cafs)),
            Box::new(reduce_expr(rhs, ctx, cafs)),
            *span,
        ),
        Expr::Delay(lhs, rhs, span) => Expr::Delay(
            Box::new(reduce_expr(lhs, ctx, cafs)),
            Box::new(reduce_expr(rhs, ctx, cafs)),
            *span,
        ),
        Expr::Arith { op, lhs, rhs, span } => Expr::Arith {
            op: *op,
            lhs: Box::new(reduce_expr(lhs, ctx, cafs)),
            rhs: Box::new(reduce_expr(rhs, ctx, cafs)),
            span: *span,
        },
        Expr::Neg(inner, span) => Expr::Neg(Box::new(reduce_expr(inner, ctx, cafs)), *span),
        Expr::Lambda { params, body, span } => {
            // A lambda rebinds its parameters inside the body: drop them from
            // the reduction context so an outer definition of the same spelling
            // is not inlined into the lambda (mirrors the shadowing in
            // `substitute`'s Lambda arm).
            let mut inner_ctx = ctx.clone();
            for p in params {
                inner_ctx.remove(&p.name);
            }
            Expr::Lambda {
                params: params.clone(),
                body: Box::new(reduce_expr(body, &inner_ctx, cafs)),
                span: *span,
            }
        }
        Expr::ListLit(elems, span) => Expr::ListLit(
            elems.iter().map(|el| reduce_expr(el, ctx, cafs)).collect(),
            *span,
        ),
        Expr::MapLit(entries, span) => Expr::MapLit(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), reduce_expr(v, ctx, cafs)))
                .collect(),
            *span,
        ),
        Expr::Cmp { op, lhs, rhs, span } => Expr::Cmp {
            op: *op,
            lhs: Box::new(reduce_expr(lhs, ctx, cafs)),
            rhs: Box::new(reduce_expr(rhs, ctx, cafs)),
            span: *span,
        },
        Expr::Logic { op, lhs, rhs, span } => Expr::Logic {
            op: *op,
            lhs: Box::new(reduce_expr(lhs, ctx, cafs)),
            rhs: Box::new(reduce_expr(rhs, ctx, cafs)),
            span: *span,
        },
        _ => e.clone(),
    }
}

/// Recognize the `+ ~ _` / `+ ~ (_ * k)` integrator short forms.
fn desugar_integrator(lhs: &Expr, rhs: &Expr, span: Span) -> Option<Expr> {
    let is_plus = matches!(lhs, Expr::Ref(name, _) if name == "+");
    if !is_plus {
        return None;
    }
    match rhs {
        Expr::Wire(_) => Some(Expr::Apply {
            name: "integrator".to_string(),
            args: vec![],
            span,
        }),
        Expr::Arith {
            op: ArithOp::Mul,
            lhs: w,
            rhs: k,
            ..
        } if matches!(w.as_ref(), Expr::Wire(_)) => Some(Expr::Apply {
            name: "leaky_integrator".to_string(),
            args: vec![(**k).clone()],
            span,
        }),
        _ => None,
    }
}

fn merge_contexts(
    outer: &HashMap<String, Def>,
    inner: &HashMap<String, Def>,
) -> HashMap<String, Def> {
    let mut merged = outer.clone();
    for (k, v) in inner {
        merged.insert(k.clone(), v.clone());
    }
    merged
}

/// β-reduce all user-defined function calls in the program, keeping references
/// to closed top-level definitions (`cafs`) un-inlined.
///
/// CAFs (closed top-level `Def::Local`s with zero signal inputs and zero
/// λ-parameters) are shared state: duplicating their body at each use site
/// would fork the stateful process. They are left as `Expr::Ref`s so lowering
/// can lift them once and route multiple consumers to the single instance.
/// Open blocks (macros) are still inlined as before.
pub fn reduce_with_cafs(program: &Program, cafs: &HashSet<String>) -> Program {
    let top_ctx: HashMap<String, Def> = program
        .defs
        .iter()
        .map(|d| (d.name().to_string(), d.clone()))
        .collect();

    let mut reduced_defs: Vec<Def> = Vec::new();
    for def in &program.defs {
        // Build context for this def: top-level defs + this def's where_defs
        let mut ctx = top_ctx.clone();
        for wd in def.where_defs() {
            ctx.insert(wd.name().to_string(), wd.clone());
        }
        let d = reduce_def(def, &ctx, cafs);
        reduced_defs.push(d);
    }
    // Re-reduce with the reduced defs to handle references between top-level defs
    let final_ctx: HashMap<String, Def> = reduced_defs
        .iter()
        .map(|d| (d.name().to_string(), d.clone()))
        .collect();
    let mut result = Program { defs: Vec::new() };
    for def in &reduced_defs {
        let mut ctx = final_ctx.clone();
        for wd in def.where_defs() {
            ctx.insert(wd.name().to_string(), wd.clone());
        }
        result.defs.push(reduce_def(def, &ctx, cafs));
    }
    result
}

/// β-reduce all user-defined function calls in the program.
///
/// After this pass, no `Apply` node targets a user-defined function.
/// `let` blocks with all defs inlined collapse to their reduced body.
/// With no CAF set, every closed top-level local is inlined (back-compat
/// for callers that do not distinguish CAFs).
pub fn reduce(program: &Program) -> Program {
    reduce_with_cafs(program, &HashSet::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ArithOp;
    use crate::lexer::tokenize;
    use crate::parser;
    use crate::types::infer::infer_program;

    fn reduced_body(src: &str) -> Expr {
        let tokens = tokenize(src).unwrap();
        let program = parser::parse(&tokens, src.as_bytes()).unwrap();
        let reduced = reduce(&program);
        let main = reduced.main_def().unwrap();
        main.body().clone()
    }

    fn caf_names(src: &str) -> std::collections::HashSet<String> {
        let tokens = tokenize(src).unwrap();
        let program = parser::parse(&tokens, src.as_bytes()).unwrap();
        let typed = infer_program(&program).unwrap();
        typed.cafs
    }

    fn reduced_with_cafs(src: &str) -> Program {
        let tokens = tokenize(src).unwrap();
        let program = parser::parse(&tokens, src.as_bytes()).unwrap();
        let cafs = caf_names(src);
        reduce_with_cafs(&program, &cafs)
    }

    fn contains_name(e: &Expr, name: &str) -> bool {
        match e {
            Expr::Ref(n, _) => n == name,
            Expr::Seq(l, r, _)
            | Expr::Par(l, r, _)
            | Expr::Split(l, r, _)
            | Expr::Merge(l, r, _) => {
                contains_name(l.as_ref(), name) || contains_name(r.as_ref(), name)
            }
            Expr::Loop(l, r, _) | Expr::Delay(l, r, _) => {
                contains_name(l.as_ref(), name) || contains_name(r.as_ref(), name)
            }
            Expr::Arith { lhs, rhs, .. } => {
                contains_name(lhs.as_ref(), name) || contains_name(rhs.as_ref(), name)
            }
            Expr::Apply { args, .. } => args.iter().any(|a| contains_name(a, name)),
            Expr::Neg(i, _) => contains_name(i.as_ref(), name),
            Expr::Let { defs, body, .. } => {
                defs.iter().any(|d| contains_name(d.body(), name))
                    || contains_name(body.as_ref(), name)
            }
            _ => false,
        }
    }

    #[test]
    fn caf_ref_is_not_inlined() {
        // c = 440.0 is a closed top-level local (CAF): main must keep the shared
        // reference `c` instead of inlining the constant at both use sites.
        let reduced = reduced_with_cafs("c = 440.0; main = c , c");
        let main = reduced.main_def().unwrap();
        assert!(contains_name(main.body(), "c"));
    }

    #[test]
    fn non_caf_local_is_still_inlined() {
        // gain = _ * 0.5 is open (1 signal input) — a macro. After reduce, no
        // `gain` reference may remain in main.
        let reduced = reduced_with_cafs("gain = _ * 0.5; main = gain");
        let main = reduced.main_def().unwrap();
        assert!(!contains_name(main.body(), "gain"));
    }

    #[test]
    fn simple_apply_is_inlined() {
        // main = g 0.5 where { g x = _ * x; }  →  main = _ * 0.5
        let body = reduced_body("main = g 0.5 where { g x = _ * x; }");
        match &body {
            Expr::Arith {
                op: ArithOp::Mul,
                lhs,
                rhs,
                ..
            } => {
                assert!(matches!(lhs.as_ref(), Expr::Wire(_)));
                assert!(matches!(rhs.as_ref(), Expr::Float(v, _) if *v == 0.5));
            }
            other => panic!("expected Arith(Mul), got {other:?}"),
        }
    }

    #[test]
    fn nested_apply_is_inlined() {
        // main = h where { f x = _ * x; g y = f y; h = g 0.5; }
        let body = reduced_body("main = h where { f x = _ * x; g y = f y; h = g 0.5; }");
        match &body {
            Expr::Arith {
                op: ArithOp::Mul,
                lhs,
                rhs,
                ..
            } => {
                assert!(matches!(lhs.as_ref(), Expr::Wire(_)));
                assert!(matches!(rhs.as_ref(), Expr::Float(v, _) if *v == 0.5));
            }
            other => panic!("expected Arith(Mul), got {other:?}"),
        }
    }

    #[test]
    fn top_level_call_is_inlined() {
        let body = reduced_body("sq x = _ * x; main = sq 0.5");
        match &body {
            Expr::Arith {
                op: ArithOp::Mul, ..
            } => {}
            other => panic!("expected Arith(Mul), got {other:?}"),
        }
    }

    #[test]
    fn builtin_not_reduced() {
        let body = reduced_body("main = _ : lowpass 1000.0 0.7");
        match &body {
            Expr::Seq(_, rhs, _) => {
                assert!(matches!(rhs.as_ref(), Expr::Apply { name, .. } if name == "lowpass"));
            }
            other => panic!("expected Seq, got {other:?}"),
        }
    }

    #[test]
    fn decl_ref_does_not_panic() {
        // A bare reference to a type declaration must not be inlined as a
        // signal body (declarations have no body to inline) — it stays a Ref.
        let tokens = tokenize("data Point = { x: Float }; main = Point").unwrap();
        let program = parser::parse(
            &tokens,
            "data Point = { x: Float }; main = Point".as_bytes(),
        )
        .unwrap();
        let reduced = reduce(&program);
        assert!(matches!(
            reduced.main_def().unwrap().body(),
            Expr::Ref(name, _) if name == "Point"
        ));
    }

    #[test]
    fn lambda_param_shadows_local_def() {
        // x = _ * 0.5; main = fn x -> x  →  the outer `x` must NOT be inlined
        // into the lambda body (the param shadows it).
        let body = reduced_body("x = _ * 0.5; main = fn x -> x");
        match &body {
            Expr::Lambda {
                params,
                body: inner,
                ..
            } => {
                assert_eq!(params.len(), 1);
                assert!(matches!(inner.as_ref(), Expr::Ref(name, _) if name == "x"));
            }
            other => panic!("expected Lambda, got {other:?}"),
        }
    }
}
