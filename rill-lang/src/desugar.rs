//! The single desugaring pass that turns the parser's equation surface into
//! core AST nodes.
//!
//! Haskell 2010 gives a binding several surface spellings that all mean the
//! same thing once the sugar is removed:
//!
//! | Surface (Haskell 2010) | Core form produced here |
//! |---|---|
//! | `f p1 p2 \| g1 = e1 \| g2 = e2` (§4.4.3.1) | an [`Expr::Match`] arm holding one `(guard, body)` alternative per guard |
//! | `f 0.0 = e; f x = x` (§4.4.3.1) | a chain of [`Expr::Match`] nodes over the formal parameters |
//! | `f x = e where { d }` (§4.4.3.3) | `\x -> let { d } in e` |
//! | `f :: T; f x = e` (§4.4.1) | the signature attached to the binding |
//!
//! [`Expr::Match`] is the language's ONLY branching form, so guards and
//! equation sets reduce to it — exactly the System FC / GHC Core model, where
//! `case` is primitive and everything else is sugar over it. The pass runs
//! ONCE, between parsing and inference (`crate::compile_to_ir_entry`), so
//! inference and lowering never learn about guards, argument patterns or
//! `where`.
//!
//! This is also why the parser keeps a single equation record
//! ([`Def::Equations`]) for every head it cannot express as the plain
//! `f x = e` shape: the `case`-arm construction for equations lives HERE and
//! nowhere else. The `case` EXPRESSION parser builds its arms directly from
//! source text, so the two paths never overlap.

use std::collections::HashMap;

use crate::ast::{Def, Equation, Expr, MatchArm, Param, Pattern, Program, TypeExpr};
use crate::error::{CompileError, Span};

/// Rewrite every equation-surface construct in `program` into the core AST.
///
/// Runs after parsing and resource extraction (which only inspects top-level
/// `tape_loop` bindings) and before inference.
pub fn desugar_program(program: &mut Program) -> Result<(), CompileError> {
    program.defs = desugar_defs(std::mem::take(&mut program.defs))?;
    Ok(())
}

/// Desugar a definition list: a whole program, a `where` block, or a `let`
/// block.
///
/// Standalone signatures are gathered first and attached to the binding of the
/// same name, so a signature's position never matters to the rest of the
/// pipeline.
fn desugar_defs(defs: Vec<Def>) -> Result<Vec<Def>, CompileError> {
    let signatures = collect_signatures(&defs);
    let mut out = Vec::with_capacity(defs.len());
    for def in defs {
        match def {
            // A standalone signature declares no value of its own.
            Def::Signature { .. } => {}
            Def::Equations {
                name,
                equations,
                sig,
                span,
            } => {
                let sig = sig.or_else(|| signatures.get(&name).cloned());
                out.push(desugar_equations(&name, &equations, sig, span)?);
            }
            Def::Eval {
                name,
                params,
                body,
                where_defs,
                sig,
                bang,
                span,
            } => {
                let sig = sig.or_else(|| signatures.get(&name).cloned());
                // A CLASS-CONSTRAINED definition keeps its `where` block on the
                // node: its body is deferred to per-call-site specialization in
                // lowering, which reads the block from the registration
                // (`TypeEnv::constrained_defs`). Every other definition gets the
                // block as an `Expr::Let` inside its body, so it resolves in the
                // enclosing λ-environment (Haskell 2010 §4.4.3.3).
                let constrained = sig.as_ref().is_some_and(|s| s.is_constrained());
                let body = desugar_expr(body)?;
                let (body, where_defs) = if constrained {
                    (body, desugar_defs(where_defs)?)
                } else {
                    (attach_where(body, &where_defs)?, Vec::new())
                };
                out.push(Def::Eval {
                    name,
                    params,
                    body,
                    where_defs,
                    sig,
                    bang,
                    span,
                });
            }
            other => out.push(desugar_decl(other)?),
        }
    }
    Ok(out)
}

/// `name → signature` for the standalone `::` declarations in `defs`.
fn collect_signatures(defs: &[Def]) -> HashMap<String, TypeExpr> {
    defs.iter()
        .filter_map(|d| match d {
            Def::Signature { name, sig, .. } => Some((name.clone(), sig.clone())),
            _ => None,
        })
        .collect()
}

/// Desugar the bodies of a declaration that carries expressions
/// (`typeclass` defaults, `instance` method bodies). Every other declaration
/// passes through untouched.
fn desugar_decl(def: Def) -> Result<Def, CompileError> {
    Ok(match def {
        Def::Typeclass {
            name,
            var,
            supers,
            methods,
            defaults,
            span,
        } => {
            let mut out = Vec::with_capacity(defaults.len());
            for (mname, params, body) in defaults {
                out.push((mname, params, desugar_expr(body)?));
            }
            Def::Typeclass {
                name,
                var,
                supers,
                methods,
                defaults: out,
                span,
            }
        }
        Def::Instance {
            class,
            head,
            ty,
            constraints,
            head_args,
            mut method_bodies,
            span,
        } => {
            for (_, _, body) in method_bodies.iter_mut() {
                let taken = std::mem::replace(body, Expr::Void(span));
                *body = desugar_expr(taken)?;
            }
            Def::Instance {
                class,
                head,
                ty,
                constraints,
                head_args,
                method_bodies,
                span,
            }
        }
        other => other,
    })
}

/// Rewrite one equation group into a single `Def::Eval`.
///
/// Three shapes, in order:
///
/// 1. **Unconditional and variable-only** (`f x y = e` with no guard): plain
///    application, so the arguments stay λ-bound and `where` (if any) wraps the
///    body.
/// 2. **Guarded** (`f x | g1 = e1 | g2 = e2`): one [`Expr::Match`] arm whose
///    `guards` are the equation's alternatives. A guard can fail, so the match
///    is what expresses it — with no argument patterns it is a match over the
///    unit value, the same shape `case () of { _ | g -> e }` has.
/// 3. **Argument patterns** (`f 0.0 = …; f x = …`): per Haskell 2010 §4.4.3.1
///    the equations are tried in order, matching each argument position in turn,
///    so the group becomes a chain of [`Expr::Match`] nodes — one per matched
///    position — whose final leaf carries the equation's guards.
///
/// Every equation's `where` block wraps ITS OWN alternatives, so a `where`
/// written in one equation is invisible in the next (Haskell 2010 §4.4.3.3).
fn desugar_equations(
    name: &str,
    equations: &[Equation],
    sig: Option<TypeExpr>,
    span: Span,
) -> Result<Def, CompileError> {
    // Equations of one group must agree on arity (Haskell 2010 §4.4.3.1).
    let arity = equations.first().map(|e| e.patterns.len()).unwrap_or(0);
    if let Some(bad) = equations.iter().find(|e| e.patterns.len() != arity) {
        return Err(CompileError::Parse {
            msg: format!(
                "equations of `{name}` must all have {arity} argument pattern(s), this one has {}",
                bad.patterns.len()
            ),
            span: bad.span,
        });
    }

    // A single equation with variable-only arguments binds them directly:
    // nothing branches on a pattern, so no match is needed. Its `where` block
    // wraps the body and a guarded equation keeps the guard-match shape.
    if let [only] = equations {
        if all_variable_patterns(&only.patterns) {
            let params = variable_params(&only.patterns, span)?;
            let body = equation_leaf(only, &params)?;
            return Ok(curried_eval_def(name, params, body, sig, span));
        }
    }

    // The formal parameters. A variable-only argument keeps the user's own name
    // (a guard reads it: `f x | x > 0.0 = …`); any other argument gets a fresh
    // `$`-prefixed name, which no source identifier can spell, and the arm's
    // own pattern binds whatever the user wrote.
    let params = variable_params_or_fresh(&equations[0].patterns, arity, span)?;
    let body = equation_chain(equations, &params, 0)?;
    Ok(curried_eval_def(name, params, body, sig, span))
}

/// The match chain for a group of equations, following Haskell 2010 §4.4.3.1.
///
/// At argument position `idx` the group branches on the DISTINCT patterns
/// written there, in first-appearance order. Each branch keeps every equation
/// that can still apply once the branch's pattern is chosen:
///
/// * the equations whose own pattern at `idx` IS that pattern, plus
/// * every equation whose pattern at `idx` is a VARIABLE — a variable accepts
///   any value, so those equations remain candidates too.
///
/// Both groups stay in source order. A variable arm is a wildcard (the value is
/// already in this position's parameter), so several equations starting with a
/// variable collapse into ONE arm whose body is the chain of all of them. That
/// is what makes `f 0.0 y = …; f x 0.0 = …; f x y = …` work: the second and
/// third equations share a variable first argument and must BOTH survive it.
///
/// Each nested match is total whenever its group is, so the ordinary totality
/// rule reports a genuinely partial GROUP (an equation set missing a
/// constructor or a Bool literal) and nothing else.
fn equation_chain(
    equations: &[Equation],
    params: &[Param],
    idx: usize,
) -> Result<Expr, CompileError> {
    if idx == params.len() {
        // Every argument of this equation matched: its guard alternatives.
        return equation_leaf(&equations[0], params);
    }
    let span = equations[0].span;
    let scrutinee = Expr::Ref(params[idx].name.clone(), span);
    // The equations a branch still admits, in source order: those matching its
    // pattern at this position, and every variable-pattern equation.
    let branch_of = |pattern: &Pattern, with_variables: bool| -> Vec<Equation> {
        equations
            .iter()
            .filter(|e| {
                let p = &e.patterns[idx];
                (with_variables && is_variable_pattern(p)) || p == pattern
            })
            .cloned()
            .collect()
    };
    // The distinct non-variable patterns, in first-appearance order, then the
    // single variable arm (if any equation has one here).
    let mut arms: Vec<MatchArm> = Vec::new();
    let mut seen: Vec<&Pattern> = Vec::new();
    let mut has_variable = false;
    for e in equations {
        let pattern = &e.patterns[idx];
        if is_variable_pattern(pattern) {
            has_variable = true;
            continue;
        }
        if seen.contains(&pattern) {
            continue;
        }
        seen.push(pattern);
        let body = equation_chain(&branch_of(pattern, true), params, idx + 1)?;
        arms.push(MatchArm {
            pattern: pattern.clone(),
            guards: vec![(Expr::Bool(true, span), body)],
            span: e.span,
        });
    }
    if has_variable {
        // Every equation whose pattern here is a variable survives the test
        // unchanged, so they all continue in ONE wildcard arm — in source order,
        // which keeps a more specific second position ahead of a general one.
        let body = equation_chain(&branch_of(&Pattern::Wild, true), params, idx + 1)?;
        arms.push(MatchArm {
            pattern: Pattern::Wild,
            guards: vec![(Expr::Bool(true, span), body)],
            span,
        });
    }
    Ok(Expr::Match {
        scrutinee: Box::new(scrutinee),
        arms,
        span,
    })
}

/// The leaf of an equation chain: the surviving equation's alternatives.
///
/// An unconditional equation is its own body. A guarded one becomes the match
/// shape `case () of { _ | g1 -> e1 | g2 -> e2 }`, so the existing guard and
/// totality rules decide whether it is exhaustive.
fn equation_leaf(equation: &Equation, params: &[Param]) -> Result<Expr, CompileError> {
    let body = if is_unconditional(equation) {
        wrap_equation_where(equation, desugar_expr(equation.guards[0].1.clone())?)?
    } else {
        let arm = equation_arm(equation)?;
        Expr::Match {
            scrutinee: Box::new(Expr::Void(equation.span)),
            arms: vec![MatchArm {
                pattern: Pattern::Wild,
                ..arm
            }],
            span: equation.span,
        }
    };
    // The equation's OWN variable names are bound to the shared position
    // parameters by a `let` alias. Naming the parameters after one equation
    // would make the others' bodies (and guards) reference names that are not
    // bound; a `let` gives each equation exactly the names it wrote, keeps
    // shadowing correct (an inner binding of the same name still wins), and
    // needs no reference-renaming walk.
    let mut body = body;
    for (i, pattern) in equation.patterns.iter().enumerate().rev() {
        let Some(param) = params.get(i) else {
            continue;
        };
        let (bang, inner) = match pattern {
            Pattern::Bang(inner) => (true, inner.as_ref()),
            other => (false, other),
        };
        let Pattern::Var(name) = inner else {
            continue;
        };
        if *name == param.name {
            continue;
        }
        let span = equation.span;
        body = Expr::Let {
            defs: vec![Def::Eval {
                name: name.clone(),
                params: Vec::new(),
                body: Expr::Ref(param.name.clone(), span),
                where_defs: Vec::new(),
                sig: None,
                bang,
                span,
            }],
            body: Box::new(body),
            span,
        };
    }
    Ok(body)
}

/// The alternatives of one equation, each wrapped in that equation's own
/// `where` block.
fn equation_arm(equation: &Equation) -> Result<MatchArm, CompileError> {
    let mut guards = Vec::with_capacity(equation.guards.len());
    for (guard, body) in &equation.guards {
        // The `where` clause scopes over the WHOLE equation (Haskell 2010
        // §4.4.3.3), i.e. over every one of its guard alternatives.
        let body = wrap_equation_where(equation, desugar_expr(body.clone())?)?;
        guards.push((desugar_expr(guard.clone())?, body));
    }
    Ok(MatchArm {
        pattern: Pattern::Wild,
        guards,
        span: equation.span,
    })
}

/// Wrap `body` in the equation's `where` bindings, if it has any.
fn wrap_equation_where(equation: &Equation, body: Expr) -> Result<Expr, CompileError> {
    if equation.where_defs.is_empty() {
        return Ok(body);
    }
    let span = body.span();
    Ok(Expr::Let {
        defs: desugar_defs(equation.where_defs.clone())?,
        body: Box::new(body),
        span,
    })
}

/// Attach a definition-level `where` block to `body`.
///
/// A `where` clause sees the declaration's own formal parameters (Haskell 2010
/// §4.4.3.3), so the [`Expr::Let`] must sit INSIDE the curried λ-nest the parser
/// builds for a multi-parameter definition. With one parameter the parser keeps
/// the parameter on the definition, and with none there is no λ at all, so the
/// `let` wraps the body directly.
///
/// The parser marks each of its currying λs with the SPAN OF THE PARAMETER it
/// binds (`curry_defs_in`), which distinguishes them from a λ the user wrote in
/// the right-hand side: an explicit `\x -> …` gets the span of the whole
/// lambda. A `where` clause around such a λ therefore does NOT capture `x` —
/// exactly Haskell's scoping.
fn attach_where(body: Expr, where_defs: &[Def]) -> Result<Expr, CompileError> {
    if where_defs.is_empty() {
        return Ok(body);
    }
    let defs = desugar_defs(where_defs.to_vec())?;
    // Peel the parser's curried λ-nest. `curry_defs_in` rewrites every
    // multi-parameter definition into a λ-chain and gives each λ the SPAN OF THE
    // PARAMETER it binds; a λ the user wrote in the right-hand side carries the
    // span of the whole lambda instead. That is the only distinction needed:
    // the `let` belongs at the innermost point still inside the definition's own
    // parameters, which Haskell 2010 §4.4.3.3 requires to be visible to the
    // `where` bindings.
    let mut stack: Vec<Param> = Vec::new();
    let mut cur = body;
    while let Expr::Lambda {
        params: lam_params,
        body: inner,
        span,
    } = cur.clone()
    {
        if lam_params.len() != 1 || lam_params[0].span != span {
            break;
        }
        stack.push(lam_params[0].clone());
        cur = *inner;
    }
    let span = cur.span();
    let mut result = Expr::Let {
        defs,
        body: Box::new(cur),
        span,
    };
    for p in stack.into_iter().rev() {
        let span = p.span;
        result = Expr::Lambda {
            params: vec![p],
            body: Box::new(result),
            span,
        };
    }
    Ok(result)
}

/// `f = \x -> \y -> body` for two or more parameters, `f x = body` for one,
/// and `f = body` for none — the curried core shape the parser produces.
fn curried_eval_def(
    name: &str,
    params: Vec<Param>,
    body: Expr,
    sig: Option<TypeExpr>,
    span: Span,
) -> Def {
    if params.len() > 1 {
        let body = lambda_wrap(&params, body);
        return eval_def(name, Vec::new(), body, sig, span);
    }
    eval_def(name, params, body, sig, span)
}

fn eval_def(name: &str, params: Vec<Param>, body: Expr, sig: Option<TypeExpr>, span: Span) -> Def {
    Def::Eval {
        name: name.to_string(),
        params,
        body,
        where_defs: Vec::new(),
        sig,
        bang: false,
        span,
    }
}

fn lambda_wrap(params: &[Param], body: Expr) -> Expr {
    let mut lam = body;
    for p in params.iter().rev() {
        lam = Expr::Lambda {
            params: vec![p.clone()],
            body: Box::new(lam),
            span: p.span,
        };
    }
    lam
}

fn all_variable_patterns(patterns: &[Pattern]) -> bool {
    patterns.iter().all(is_variable_pattern)
}

fn is_variable_pattern(p: &Pattern) -> bool {
    match p {
        Pattern::Var(_) => true,
        Pattern::Bang(inner) => matches!(inner.as_ref(), Pattern::Var(_)),
        _ => false,
    }
}

/// The parameter list of a variable-only argument list.
fn variable_params(patterns: &[Pattern], span: Span) -> Result<Vec<Param>, CompileError> {
    patterns
        .iter()
        .map(|p| {
            let (bang, inner) = match p {
                Pattern::Bang(inner) => (true, inner.as_ref()),
                other => (false, other),
            };
            let Pattern::Var(name) = inner else {
                return Err(CompileError::Parse {
                    msg: "an argument pattern must be a variable here".into(),
                    span,
                });
            };
            Ok(Param {
                name: name.clone(),
                span,
                bang,
            })
        })
        .collect()
}

/// The formal parameters of a match chain: the user's own name where the
/// argument is a variable, a fresh `$argN` otherwise (so the arm's pattern can
/// bind whatever was written).
fn variable_params_or_fresh(
    patterns: &[Pattern],
    arity: usize,
    span: Span,
) -> Result<Vec<Param>, CompileError> {
    let mut params = Vec::with_capacity(arity);
    for i in 0..arity {
        match patterns.get(i) {
            Some(Pattern::Var(name)) => params.push(Param {
                name: name.clone(),
                span,
                bang: false,
            }),
            Some(Pattern::Bang(inner)) => match inner.as_ref() {
                Pattern::Var(name) => params.push(Param {
                    name: name.clone(),
                    span,
                    bang: true,
                }),
                _ => params.push(Param {
                    name: format!("$arg{i}"),
                    span,
                    bang: false,
                }),
            },
            _ => params.push(Param {
                name: format!("$arg{i}"),
                span,
                bang: false,
            }),
        }
    }
    Ok(params)
}

fn is_unconditional(equation: &Equation) -> bool {
    matches!(equation.guards.as_slice(), [(Expr::Bool(true, _), _)])
}

/// Recursively desugar an expression: nested `let` blocks carry their own
/// equation surfaces.
fn desugar_expr(e: Expr) -> Result<Expr, CompileError> {
    Ok(match e {
        Expr::Let { defs, body, span } => Expr::Let {
            defs: desugar_defs(defs)?,
            body: Box::new(desugar_expr(*body)?),
            span,
        },
        Expr::Apply { name, args, span } => Expr::Apply {
            name,
            args: desugar_exprs(args)?,
            span,
        },
        Expr::ApplyExpr { callee, args, span } => Expr::ApplyExpr {
            callee: Box::new(desugar_expr(*callee)?),
            args: desugar_exprs(args)?,
            span,
        },
        Expr::Neg(inner, span) => Expr::Neg(Box::new(desugar_expr(*inner)?), span),
        Expr::Arith { op, lhs, rhs, span } => Expr::Arith {
            op,
            lhs: Box::new(desugar_expr(*lhs)?),
            rhs: Box::new(desugar_expr(*rhs)?),
            span,
        },
        Expr::Record(fields, span) => Expr::Record(
            fields
                .into_iter()
                .map(|(n, v)| Ok((n, desugar_expr(v)?)))
                .collect::<Result<Vec<_>, CompileError>>()?,
            span,
        ),
        Expr::ActorParam {
            name,
            default,
            span,
        } => Expr::ActorParam {
            name,
            default: match default {
                Some(d) => Some(Box::new(desugar_expr(*d)?)),
                None => None,
            },
            span,
        },
        Expr::FieldProject {
            record,
            field,
            span,
        } => Expr::FieldProject {
            record: Box::new(desugar_expr(*record)?),
            field,
            span,
        },
        Expr::FieldUpdate {
            record,
            field,
            value,
            span,
        } => Expr::FieldUpdate {
            record: Box::new(desugar_expr(*record)?),
            field,
            value: Box::new(desugar_expr(*value)?),
            span,
        },
        Expr::Match {
            scrutinee,
            arms,
            span,
        } => Expr::Match {
            scrutinee: Box::new(desugar_expr(*scrutinee)?),
            arms: arms
                .into_iter()
                .map(|arm| {
                    Ok(MatchArm {
                        pattern: arm.pattern,
                        guards: arm
                            .guards
                            .into_iter()
                            .map(|(g, b)| Ok((desugar_expr(g)?, desugar_expr(b)?)))
                            .collect::<Result<Vec<_>, CompileError>>()?,
                        span: arm.span,
                    })
                })
                .collect::<Result<Vec<_>, CompileError>>()?,
            span,
        },
        Expr::If {
            cond,
            then,
            els,
            span,
        } => Expr::If {
            cond: Box::new(desugar_expr(*cond)?),
            then: Box::new(desugar_expr(*then)?),
            els: Box::new(desugar_expr(*els)?),
            span,
        },
        Expr::Lambda { params, body, span } => Expr::Lambda {
            params,
            body: Box::new(desugar_expr(*body)?),
            span,
        },
        Expr::ListLit(elems, span) => Expr::ListLit(desugar_exprs(elems)?, span),
        Expr::MapLit(entries, span) => Expr::MapLit(
            entries
                .into_iter()
                .map(|(k, v)| Ok((k, desugar_expr(v)?)))
                .collect::<Result<Vec<_>, CompileError>>()?,
            span,
        ),
        Expr::Annot { expr, ty, span } => Expr::Annot {
            expr: Box::new(desugar_expr(*expr)?),
            ty,
            span,
        },
        Expr::Cmp { op, lhs, rhs, span } => Expr::Cmp {
            op,
            lhs: Box::new(desugar_expr(*lhs)?),
            rhs: Box::new(desugar_expr(*rhs)?),
            span,
        },
        Expr::Logic { op, lhs, rhs, span } => Expr::Logic {
            op,
            lhs: Box::new(desugar_expr(*lhs)?),
            rhs: Box::new(desugar_expr(*rhs)?),
            span,
        },
        other @ (Expr::Int(..)
        | Expr::Float(..)
        | Expr::Void(..)
        | Expr::Str(..)
        | Expr::Ref(..)
        | Expr::Bool(..)) => other,
    })
}

fn desugar_exprs(exprs: Vec<Expr>) -> Result<Vec<Expr>, CompileError> {
    exprs.into_iter().map(desugar_expr).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    /// Desugar `src` and return the resulting definitions.
    fn desugared(src: &str) -> Vec<Def> {
        let mut program = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        desugar_program(&mut program).unwrap();
        program.defs
    }

    fn eval_def_of<'a>(defs: &'a [Def], name: &str) -> &'a Def {
        defs.iter()
            .find(|d| d.name() == name && matches!(d, Def::Eval { .. }))
            .unwrap_or_else(|| panic!("no `Def::Eval` named {name}"))
    }

    #[test]
    fn no_intermediate_forms_survive_the_pass() {
        // The pass is the ONLY consumer of the equation surface, so nothing but
        // core `Def::Eval` nodes may leave it.
        for src in [
            "f 0.0 = 1.0; f x = x;",
            "f x | x > 0.0 = 1.0 | otherwise = 2.0;",
            "f :: Float -> Float;\nf x = x;",
            "f x = g where { g = x; };",
        ] {
            let defs = desugared(src);
            assert!(
                defs.iter()
                    .all(|d| !matches!(d, Def::Equations { .. } | Def::Signature { .. })),
                "`{src}` left an intermediate form behind: {defs:?}"
            );
        }
    }

    #[test]
    fn guards_become_a_match_arm_with_one_alternative_per_guard() {
        let defs = desugared("f x | x > 0.0 = 1.0 | otherwise = 2.0;");
        let def = eval_def_of(&defs, "f");
        // `f x = …` keeps its parameter; the body is the guarded match.
        assert_eq!(def.params().len(), 1);
        let Expr::Match { arms, .. } = def.body() else {
            panic!("expected a match, got {:?}", def.body());
        };
        assert_eq!(arms.len(), 1);
        assert_eq!(arms[0].guards.len(), 2);
        // The final alternative is the `otherwise` guard, which is the Prelude
        // constant — an ordinary `Ref`, not a keyword.
        assert!(matches!(&arms[0].guards[1].0, Expr::Ref(n, _) if n == "otherwise"));
    }

    #[test]
    fn argument_patterns_become_a_match_chain_over_the_parameters() {
        let defs = desugared("f 0.0 = 1.0; f x = x;");
        let def = eval_def_of(&defs, "f");
        let Expr::Match { arms, .. } = def.body() else {
            panic!("expected a match, got {:?}", def.body());
        };
        // One arm for the literal equation and one for the variable catch-all.
        // The catch-all is a wildcard: its body re-binds the equation's own name
        // (`x`) to the shared argument parameter with a transparent `let`.
        assert_eq!(arms.len(), 2);
        assert!(matches!(arms[0].pattern, Pattern::LitFloat(v) if v == 0.0));
        assert!(matches!(arms[1].pattern, Pattern::Wild));
        assert!(
            matches!(&arms[1].guards[0].1, Expr::Let { defs, .. }
                if matches!(&defs[0], Def::Eval { name, .. } if name == "x")),
            "the catch-all must re-bind `x`: {:?}",
            arms[1].guards[0].1
        );
    }

    #[test]
    fn where_becomes_a_let_inside_the_curried_parameter_nest() {
        let defs = desugared("f x y = g where { g = x + y; };");
        let def = eval_def_of(&defs, "f");
        assert!(def.where_defs().is_empty(), "where must be consumed");
        // Two parameters curry into a λ-nest; the `let` sits at its centre so
        // both are in scope.
        let Expr::Lambda { body, .. } = def.body() else {
            panic!("expected a curried lambda, got {:?}", def.body());
        };
        let Expr::Lambda { body, .. } = body.as_ref() else {
            panic!("expected a second lambda, got {body:?}");
        };
        assert!(
            matches!(body.as_ref(), Expr::Let { .. }),
            "expected the `let` at the centre of the nest, got {body:?}"
        );
    }

    #[test]
    fn standalone_signature_attaches_to_the_binding() {
        let defs = desugared("f :: Float -> Float;\nf x = x;");
        assert_eq!(defs.len(), 1, "the signature is consumed: {defs:?}");
        assert!(
            eval_def_of(&defs, "f").sig().is_some(),
            "the binding must carry the signature"
        );
    }

    #[test]
    fn mismatched_equation_arities_are_rejected() {
        let err = desugar_program(
            &mut parse(
                &tokenize("f 0.0 = 1.0; f x y = x;").unwrap(),
                "f 0.0 = 1.0; f x y = x;".as_bytes(),
            )
            .unwrap(),
        )
        .expect_err("equations must agree on arity");
        assert!(
            err.to_string().contains("argument pattern"),
            "expected an arity diagnostic, got {err}"
        );
    }
}
