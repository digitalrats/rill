//! β-reduction: inline user-defined function calls after type inference.
//!
//! After this pass, the AST contains no `Apply` nodes targeting user-defined
//! functions — only builtins, `smooth`, `param`, and combinators remain.
//! This simplifies lowering: no Anchor handling in `lower_ref`.

use std::collections::HashMap;

use crate::ast::{Def, Expr, Program};

fn substitute(e: &Expr, subst: &HashMap<String, Expr>) -> Expr {
    match e {
        Expr::Ref(name, _) => {
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
        Expr::Bin { op, lhs, rhs, span } => Expr::Bin {
            op: *op,
            lhs: Box::new(substitute(lhs, subst)),
            rhs: Box::new(substitute(rhs, subst)),
            span: *span,
        },
        Expr::Let { defs, body, span } => {
            let reduced_defs: Vec<Def> = defs
                .iter()
                .map(|d| reduce_def(d, &HashMap::new()))
                .collect();
            let reduced_body = reduce_expr(body, &defs_map(&reduced_defs));
            Expr::Let {
                defs: reduced_defs,
                body: Box::new(reduced_body),
                span: *span,
            }
        }
        _ => e.clone(),
    }
}

fn defs_map(defs: &[Def]) -> HashMap<String, Def> {
    defs.iter()
        .map(|d| (d.name().to_string(), d.clone()))
        .collect()
}

fn reduce_def(def: &Def, ctx: &HashMap<String, Def>) -> Def {
    let reduced_body = reduce_expr(def.body(), ctx);
    let reduced_where: Vec<Def> = def
        .where_defs()
        .iter()
        .map(|d| reduce_def(d, ctx))
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
    }
}

fn reduce_expr(e: &Expr, ctx: &HashMap<String, Def>) -> Expr {
    match e {
        Expr::Ref(name, _) => {
            if let Some(def) = ctx.get(name) {
                if def.params().is_empty() {
                    // Local binding with no params — inline the body
                    reduce_expr(def.body(), ctx)
                } else {
                    // Has unapplied λ-params — can't inline, leave as ref
                    e.clone()
                }
            } else {
                e.clone()
            }
        }
        Expr::Apply { name, args, span } => {
            let reduced_args: Vec<Expr> = args.iter().map(|a| reduce_expr(a, ctx)).collect();
            if let Some(def) = ctx.get(name) {
                // β-reduce: substitute args for params in the definition's body
                let mut subst = HashMap::new();
                for (idx, p) in def.params().iter().enumerate() {
                    if p.name != "_" {
                        subst.insert(p.name.clone(), reduced_args[idx].clone());
                    }
                }
                let inlined = substitute(def.body(), &subst);
                // Recursively reduce the inlined body (may contain more calls)
                reduce_expr(&inlined, ctx)
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
            let reduced_defs: Vec<Def> = defs.iter().map(|d| reduce_def(d, ctx)).collect();
            let let_ctx = merge_contexts(ctx, &defs_map(&reduced_defs));
            reduce_expr(body, &let_ctx)
        }
        Expr::Bin { op, lhs, rhs, span } => Expr::Bin {
            op: *op,
            lhs: Box::new(reduce_expr(lhs, ctx)),
            rhs: Box::new(reduce_expr(rhs, ctx)),
            span: *span,
        },
        Expr::Neg(inner, span) => Expr::Neg(Box::new(reduce_expr(inner, ctx)), *span),
        _ => e.clone(),
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

/// β-reduce all user-defined function calls in the program.
///
/// After this pass, no `Apply` node targets a user-defined function.
/// `let` blocks with all defs inlined collapse to their reduced body.
pub fn reduce(program: &Program) -> Program {
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
        let d = reduce_def(def, &ctx);
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
        result.defs.push(reduce_def(def, &ctx));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::BinOp;
    use crate::lexer::tokenize;
    use crate::parser;

    fn reduced_body(src: &str) -> Expr {
        let tokens = tokenize(src).unwrap();
        let program = parser::parse(&tokens, src.as_bytes()).unwrap();
        let reduced = reduce(&program);
        let main = reduced.main_def().unwrap();
        main.body().clone()
    }

    #[test]
    fn simple_apply_is_inlined() {
        // main = g 0.5 where { g x = _ * x; }  →  main = _ * 0.5
        let body = reduced_body("main = g 0.5 where { g x = _ * x; }");
        match &body {
            Expr::Bin {
                op: BinOp::Mul,
                lhs,
                rhs,
                ..
            } => {
                assert!(matches!(lhs.as_ref(), Expr::Wire(_)));
                assert!(matches!(rhs.as_ref(), Expr::Float(v, _) if *v == 0.5));
            }
            other => panic!("expected Bin(Mul), got {other:?}"),
        }
    }

    #[test]
    fn nested_apply_is_inlined() {
        // main = h where { f x = _ * x; g y = f y; h = g 0.5; }
        let body = reduced_body("main = h where { f x = _ * x; g y = f y; h = g 0.5; }");
        match &body {
            Expr::Bin {
                op: BinOp::Mul,
                lhs,
                rhs,
                ..
            } => {
                assert!(matches!(lhs.as_ref(), Expr::Wire(_)));
                assert!(matches!(rhs.as_ref(), Expr::Float(v, _) if *v == 0.5));
            }
            other => panic!("expected Bin(Mul), got {other:?}"),
        }
    }

    #[test]
    fn top_level_call_is_inlined() {
        let body = reduced_body("sq x = _ * x; main = sq 0.5");
        match &body {
            Expr::Bin { op: BinOp::Mul, .. } => {}
            other => panic!("expected Bin(Mul), got {other:?}"),
        }
    }

    #[test]
    fn builtin_not_reduced() {
        let body = reduced_body("main = _ : lowpass 1000.0 0.7");
        match &body {
            Expr::Bin {
                op: BinOp::Seq,
                rhs,
                ..
            } => {
                assert!(matches!(rhs.as_ref(), Expr::Apply { name, .. } if name == "lowpass"));
            }
            other => panic!("expected Bin(Seq), got {other:?}"),
        }
    }
}
