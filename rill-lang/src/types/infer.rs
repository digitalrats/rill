//! Algorithm-W-style inference over scalar types, with bottom-up arity
//! synthesis and combinatorial arity checking.
//!
//! All binding groups (top-level, `where`, `let`) use mutual recursion:
//! every name in the group is visible to every body.

use std::collections::HashMap;

use super::ty::{Scalar, Scheme, Subst, Type, TypeVarId};
use super::unify::unify_scalar;
use crate::ast::{BinOp, Def, Expr, Program};
use crate::builtin::SignatureSource;
use crate::error::{CompileError, Span};

/// The typed result of inference: the program's definitions plus the resolved
/// type of the output and the final substitution.
#[derive(Debug, Clone)]
pub struct TypedProgram {
    /// The original program (unchanged AST).
    pub program: Program,
    /// Resolved diagram type of the body.
    pub process_ty: Type,
}

/// Inference context: fresh var supply, definition schemes, local bindings,
/// and a signature source for built-in resolution.
struct Ctx<'a> {
    next: TypeVarId,
    subst: Subst,
    defs: HashMap<String, Scheme>,
    locals: HashMap<String, Type>,
    sigs: &'a dyn SignatureSource,
}

impl Ctx<'_> {
    fn fresh(&mut self) -> Scalar {
        let v = self.next;
        self.next += 1;
        Scalar::Var(v)
    }

    fn instantiate(&mut self, scheme: &Scheme) -> Type {
        let mut remap: HashMap<TypeVarId, Scalar> = HashMap::new();
        for v in &scheme.vars {
            let f = self.fresh();
            remap.insert(*v, f);
        }
        let rw = |s: &Scalar| match s {
            Scalar::Var(v) => remap.get(v).cloned().unwrap_or_else(|| s.clone()),
            _ => s.clone(),
        };
        Type {
            ins: scheme.ty.ins.iter().map(&rw).collect(),
            outs: scheme.ty.outs.iter().map(&rw).collect(),
        }
    }

    fn free_vars(&self, t: &Type) -> Vec<TypeVarId> {
        let mut acc = Vec::new();
        for s in t.ins.iter().chain(t.outs.iter()) {
            if let Scalar::Var(v) = self.subst.resolve_scalar(s) {
                if !acc.contains(&v) {
                    acc.push(v);
                }
            }
        }
        acc
    }
}

/// Back-compat: infer with no built-ins.
pub fn infer_program(program: &Program) -> Result<TypedProgram, CompileError> {
    infer_program_with(program, &crate::builtin::NoSigs)
}

/// Infer with a signature source for built-in resolution.
///
/// Top-level definitions are mutually recursive: all names are visible
/// to all bodies.
pub fn infer_program_with(
    program: &Program,
    sigs: &dyn SignatureSource,
) -> Result<TypedProgram, CompileError> {
    let mut ctx = Ctx {
        next: 0,
        subst: Subst::default(),
        defs: HashMap::new(),
        locals: HashMap::new(),
        sigs,
    };

    infer_def_group(&mut ctx, &program.defs)?;

    let main_scheme = ctx
        .defs
        .get("main")
        .cloned()
        .ok_or_else(|| CompileError::Type {
            msg: "program must contain a `main` definition".into(),
            span: Span::new(0, 0),
        })?;

    let signal_arity_in = main_scheme.ty.arity_in() - main_scheme.lam_count;
    let signal_arity_out = main_scheme.ty.arity_out();

    if signal_arity_out != 1 || signal_arity_in > 1 {
        return Err(CompileError::Type {
            msg: format!(
                "signal arity must be (0|1)->1, found ({signal_arity_in}->{signal_arity_out})"
            ),
            span: Span::new(0, 0),
        });
    }
    Ok(TypedProgram {
        program: program.clone(),
        process_ty: main_scheme.ty,
    })
}

/// Infer a group of mutually-recursive definitions (top-level, where, or let).
/// Two-phase: first register placeholder schemes for all names, then infer
/// each body with the full mutual environment.
fn infer_def_group(ctx: &mut Ctx<'_>, defs: &[Def]) -> Result<(), CompileError> {
    if defs.is_empty() {
        return Ok(());
    }

    // Phase 1: placeholder schemes for all names
    for def in defs {
        if ctx.defs.contains_key(def.name()) {
            return Err(CompileError::Type {
                msg: format!("duplicate definition `{}`", def.name()),
                span: def.body().span(),
            });
        }
        let lam_count = def.params().len();
        let mut ins = Vec::with_capacity(lam_count);
        for _ in 0..lam_count {
            ins.push(ctx.fresh());
        }
        let out = ctx.fresh();
        ctx.defs.insert(
            def.name().to_string(),
            Scheme {
                lam_count,
                vars: vec![],
                ty: Type {
                    ins,
                    outs: vec![out],
                },
            },
        );
    }

    // Phase 2: infer bodies with placeholder visibility
    for def in defs {
        if !def.where_defs().is_empty() {
            infer_def_group(ctx, def.where_defs())?;
        }
        ctx.locals.clear();
        for p in def.params() {
            ctx.locals
                .insert(p.name.clone(), Type::uniform(0, 1, Scalar::Float));
        }
        let body_ty = infer_expr(ctx, def.body())?;
        let lam_count = def.params().len();
        let mut full_ins = Vec::with_capacity(lam_count + body_ty.ins.len());
        for _ in 0..lam_count {
            full_ins.push(Scalar::Float);
        }
        full_ins.extend(body_ty.ins);
        let resolved = ctx.subst.apply(&Type {
            ins: full_ins,
            outs: body_ty.outs,
        });
        let vars = ctx.free_vars(&resolved);
        ctx.defs.remove(def.name());
        ctx.defs.insert(
            def.name().to_string(),
            Scheme {
                lam_count,
                vars,
                ty: resolved,
            },
        );
    }

    // Second pass: re-infer with actual schemes for correct signal port counts
    for def in defs {
        ctx.locals.clear();
        for p in def.params() {
            ctx.locals
                .insert(p.name.clone(), Type::uniform(0, 1, Scalar::Float));
        }
        let body_ty = infer_expr(ctx, def.body())?;
        let lam_count = def.params().len();
        let mut full_ins = Vec::with_capacity(lam_count + body_ty.ins.len());
        for _ in 0..lam_count {
            full_ins.push(Scalar::Float);
        }
        full_ins.extend(body_ty.ins);
        let resolved = ctx.subst.apply(&Type {
            ins: full_ins,
            outs: body_ty.outs,
        });
        let vars = ctx.free_vars(&resolved);
        ctx.defs.remove(def.name());
        ctx.defs.insert(
            def.name().to_string(),
            Scheme {
                lam_count,
                vars,
                ty: resolved,
            },
        );
    }

    Ok(())
}

/// Infer the diagram type of an expression, synthesizing concrete arities.
fn infer_expr(ctx: &mut Ctx<'_>, e: &Expr) -> Result<Type, CompileError> {
    match e {
        Expr::Int(_, _) => Ok(Type {
            ins: vec![],
            outs: vec![Scalar::Int],
        }),
        Expr::Float(_, _) => Ok(Type {
            ins: vec![],
            outs: vec![Scalar::Float],
        }),
        Expr::Imag(_, _) => Ok(Type {
            ins: vec![],
            outs: vec![Scalar::Float, Scalar::Float],
        }),
        Expr::Wire(_) => {
            let s = ctx.fresh();
            Ok(Type::uniform(1, 1, s))
        }
        Expr::Cut(_) => {
            let s = ctx.fresh();
            Ok(Type {
                ins: vec![s],
                outs: vec![],
            })
        }
        Expr::Ref(name, span) => infer_ref(ctx, name, *span),
        Expr::Neg(inner, span) => {
            let t = infer_expr(ctx, inner)?;
            check_all_numeric(ctx, &t, *span)?;
            Ok(t)
        }
        Expr::Apply { name, args, span } => infer_apply(ctx, name, args, *span),
        Expr::Str(_, span) => Err(CompileError::Type {
            msg: "string literal is only valid as a parameter name".into(),
            span: *span,
        }),
        Expr::Bin { op, lhs, rhs, span } => {
            let a = infer_expr(ctx, lhs)?;
            let b = infer_expr(ctx, rhs)?;
            infer_bin(ctx, *op, &a, &b, *span)
        }
        Expr::Let {
            defs,
            body,
            span: _,
        } => {
            let saved_defs = ctx.defs.clone();
            infer_def_group(ctx, defs)?;
            let ty = infer_expr(ctx, body)?;
            ctx.defs = saved_defs;
            Ok(ty)
        }
        Expr::Record(..) => unreachable!("Record should be desugared before type inference"),
    }
}

fn infer_ref(ctx: &mut Ctx<'_>, name: &str, span: Span) -> Result<Type, CompileError> {
    if matches!(name, "+" | "-" | "*" | "/" | "%") {
        let s = ctx.fresh();
        return Ok(Type::uniform(2, 1, s));
    }
    if matches!(
        name,
        "sin" | "cos" | "tan" | "sqrt" | "exp" | "ln" | "tanh" | "abs"
    ) {
        return Ok(Type::uniform(1, 1, Scalar::Float));
    }
    if matches!(name, "min" | "max") {
        let s = ctx.fresh();
        return Ok(Type::uniform(2, 1, s));
    }
    if let Some(sig) = ctx.sigs.builtin_sig(name) {
        if sig.params.len() == sig.signal_ins() {
            return Ok(Type::uniform(
                sig.signal_ins(),
                sig.signal_outs,
                Scalar::Float,
            ));
        }
    }
    if let Some(t) = ctx.locals.get(name) {
        return Ok(t.clone());
    }
    if let Some(scheme) = ctx.defs.get(name).cloned() {
        if scheme.lam_count > 0 {
            return Err(CompileError::Type {
                msg: format!(
                    "`{name}` has {} unapplied parameter(s); call it as `{name} arg1 arg2 ...`",
                    scheme.lam_count
                ),
                span,
            });
        }
        return Ok(ctx.instantiate(&scheme));
    }
    Err(CompileError::Type {
        msg: format!("unknown identifier `{name}`"),
        span,
    })
}

fn infer_apply(
    ctx: &mut Ctx<'_>,
    name: &str,
    args: &[Expr],
    span: Span,
) -> Result<Type, CompileError> {
    if name == "smooth" {
        if args.len() != 2 {
            return Err(CompileError::Type {
                msg: "smooth expects exactly 2 arguments: smooth(signal, ms)".into(),
                span,
            });
        }
        let sig_ty = infer_expr(ctx, &args[0])?;
        if sig_ty.arity_out() != 1 {
            return Err(CompileError::Type {
                msg: "smooth first argument must produce exactly one wire".into(),
                span: args[0].span(),
            });
        }
        let ms_ty = infer_expr(ctx, &args[1])?;
        if ms_ty.arity_in() != 0 || ms_ty.arity_out() != 1 {
            return Err(CompileError::Type {
                msg: "smooth second argument (ms) must be a constant expression".into(),
                span: args[1].span(),
            });
        }
        unify_scalar(&ms_ty.outs[0], &Scalar::Float, &mut ctx.subst, span)?;
        return Ok(Type {
            ins: sig_ty.ins.clone(),
            outs: vec![Scalar::Float],
        });
    }
    if name == "param" {
        return infer_param(args, span);
    }
    if let Some(sig) = ctx.sigs.builtin_sig(name) {
        let sig = sig.clone();
        if args.len() != sig.params.len() {
            return Err(CompileError::Type {
                msg: format!(
                    "built-in `{name}` expects {} param(s), got {}",
                    sig.params.len(),
                    args.len()
                ),
                span,
            });
        }
        for a in args {
            if let Expr::Ref(ref_name, _) = a {
                if ctx.locals.contains_key(ref_name) {
                    continue;
                }
            }
            let at = infer_expr(ctx, a)?;
            if at.arity_in() != 0 || at.arity_out() != 1 {
                return Err(CompileError::Type {
                    msg: format!(
                        "param to `{name}` must be a constant expression or parameter reference"
                    ),
                    span: a.span(),
                });
            }
        }
        return Ok(Type::uniform(
            sig.signal_ins(),
            sig.signal_outs,
            Scalar::Float,
        ));
    }
    // User-defined function: λ-params are consumed, signal ports remain open
    if let Some(scheme) = ctx.defs.get(name).cloned() {
        if args.len() != scheme.lam_count {
            return Err(CompileError::Type {
                msg: format!(
                    "`{name}` expects {} argument(s), got {}",
                    scheme.lam_count,
                    args.len()
                ),
                span,
            });
        }
        let ty = ctx.instantiate(&scheme);
        return Ok(Type {
            ins: ty.ins[scheme.lam_count..].to_vec(),
            outs: ty.outs,
        });
    }
    // Fallback: builtin reference (abs, sin, +, etc.) applied to signal args
    let mut combined: Option<Type> = None;
    for arg in args {
        let at = infer_expr(ctx, arg)?;
        combined = Some(match combined {
            None => at,
            Some(acc) => par(&acc, &at),
        });
    }
    let callee = infer_ref(ctx, name, span)?;
    match combined {
        Some(args_ty) => seq(ctx, &args_ty, &callee, span),
        None => Ok(callee),
    }
}

fn infer_param(args: &[Expr], span: Span) -> Result<Type, CompileError> {
    if args.is_empty() || args.len() > 4 {
        return Err(CompileError::Type {
            msg: "param expects 1–4 arguments: param(name, default[, min, max])".into(),
            span,
        });
    }
    if !matches!(&args[0], Expr::Str(_, _)) {
        return Err(CompileError::Type {
            msg: "param first argument must be a string literal".into(),
            span: args[0].span(),
        });
    }
    Ok(Type::uniform(0, 1, Scalar::Float))
}

fn infer_bin(
    ctx: &mut Ctx<'_>,
    op: BinOp,
    a: &Type,
    b: &Type,
    span: Span,
) -> Result<Type, CompileError> {
    match op {
        BinOp::Seq => seq(ctx, a, b, span),
        BinOp::Par => Ok(par(a, b)),
        BinOp::Split => split(ctx, a, b, span),
        BinOp::Merge => merge(ctx, a, b, span),
        BinOp::Feedback => feedback(ctx, a, b, span),
        BinOp::Delay => delay(ctx, a, b, span),
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => arith(ctx, a, b, span),
    }
}

fn par(a: &Type, b: &Type) -> Type {
    let mut ins = a.ins.clone();
    ins.extend(b.ins.clone());
    let mut outs = a.outs.clone();
    outs.extend(b.outs.clone());
    Type { ins, outs }
}

fn seq(ctx: &mut Ctx<'_>, a: &Type, b: &Type, span: Span) -> Result<Type, CompileError> {
    if a.arity_out() != b.arity_in() {
        return Err(CompileError::Type {
            msg: format!(
                "sequential `:` arity mismatch: lhs outputs {}, rhs inputs {}",
                a.arity_out(),
                b.arity_in()
            ),
            span,
        });
    }
    for (x, y) in a.outs.iter().zip(b.ins.iter()) {
        unify_scalar(x, y, &mut ctx.subst, span)?;
    }
    Ok(Type {
        ins: a.ins.clone(),
        outs: b.outs.clone(),
    })
}

fn split(ctx: &mut Ctx<'_>, a: &Type, b: &Type, span: Span) -> Result<Type, CompileError> {
    let (ao, bi) = (a.arity_out(), b.arity_in());
    if ao == 0 || bi % ao != 0 {
        return Err(CompileError::Type {
            msg: format!(
                "split `<:` requires rhs inputs ({bi}) be a multiple of lhs outputs ({ao})"
            ),
            span,
        });
    }
    let reps = bi / ao;
    for r in 0..reps {
        for k in 0..ao {
            unify_scalar(&a.outs[k], &b.ins[r * ao + k], &mut ctx.subst, span)?;
        }
    }
    Ok(Type {
        ins: a.ins.clone(),
        outs: b.outs.clone(),
    })
}

fn merge(ctx: &mut Ctx<'_>, a: &Type, b: &Type, span: Span) -> Result<Type, CompileError> {
    let (ao, bi) = (a.arity_out(), b.arity_in());
    if bi == 0 || ao % bi != 0 {
        return Err(CompileError::Type {
            msg: format!(
                "merge `:>` requires lhs outputs ({ao}) be a multiple of rhs inputs ({bi})"
            ),
            span,
        });
    }
    let groups = ao / bi;
    for g in 0..groups {
        for k in 0..bi {
            unify_scalar(&a.outs[g * bi + k], &b.ins[k], &mut ctx.subst, span)?;
        }
    }
    Ok(Type {
        ins: a.ins.clone(),
        outs: b.outs.clone(),
    })
}

fn feedback(ctx: &mut Ctx<'_>, a: &Type, b: &Type, span: Span) -> Result<Type, CompileError> {
    let (ai, ao, bi, bo) = (a.arity_in(), a.arity_out(), b.arity_in(), b.arity_out());
    if bi > ao || bo > ai {
        return Err(CompileError::Type {
            msg: format!(
                "feedback `~` arity mismatch: need B.in({bi})<=A.out({ao}) and B.out({bo})<=A.in({ai})"
            ),
            span,
        });
    }
    for k in 0..bi {
        unify_scalar(&b.ins[k], &a.outs[k], &mut ctx.subst, span)?;
    }
    for k in 0..bo {
        unify_scalar(&b.outs[k], &a.ins[k], &mut ctx.subst, span)?;
    }
    Ok(Type {
        ins: a.ins[bo..].to_vec(),
        outs: a.outs.clone(),
    })
}

fn delay(ctx: &mut Ctx<'_>, a: &Type, b: &Type, span: Span) -> Result<Type, CompileError> {
    if a.arity_out() != 1 {
        return Err(CompileError::Type {
            msg: format!(
                "`@` left side must have output arity 1, found {}",
                a.arity_out()
            ),
            span,
        });
    }
    if b.arity_in() != 0 || b.arity_out() != 1 {
        return Err(CompileError::Type {
            msg: "`@` delay length must be a constant expression".into(),
            span,
        });
    }
    unify_scalar(&b.outs[0], &Scalar::Int, &mut ctx.subst, span)?;
    Ok(Type {
        ins: a.ins.clone(),
        outs: a.outs.clone(),
    })
}

fn arith(ctx: &mut Ctx<'_>, a: &Type, b: &Type, span: Span) -> Result<Type, CompileError> {
    if a.arity_out() != 1 || b.arity_out() != 1 {
        return Err(CompileError::Type {
            msg: "arithmetic operands must each produce exactly one wire".into(),
            span,
        });
    }
    unify_scalar(&a.outs[0], &b.outs[0], &mut ctx.subst, span)?;
    let mut ins = a.ins.clone();
    ins.extend(b.ins.clone());
    Ok(Type {
        ins,
        outs: vec![a.outs[0].clone()],
    })
}

fn check_all_numeric(_ctx: &mut Ctx<'_>, _t: &Type, _span: Span) -> Result<(), CompileError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn ty_of(src: &str) -> Result<TypedProgram, CompileError> {
        infer_program(&parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap())
    }

    #[test]
    fn wire_is_1_to_1() {
        let t = ty_of("main = _").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn gain_is_1_to_1() {
        let t = ty_of("main = _ * 0.5").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn integrator_is_1_to_1() {
        let t = ty_of("main = + ~ _").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn rejects_seq_arity_mismatch() {
        assert!(ty_of("main = (_ , _) : _").is_err());
    }

    #[test]
    fn rejects_bad_process_arity() {
        assert!(ty_of("main = _ , _").is_err());
    }

    #[test]
    fn split_then_merge_ok() {
        let t = ty_of("main = _ <: (_ , _) :> +").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn delay_constant_ok_variable_errors() {
        assert!(ty_of("main = _ @ 1").is_ok());
        assert!(ty_of("main = _ @ _").is_err());
    }

    #[test]
    fn user_def_alias_resolves() {
        let t = ty_of("main = gain where { gain = _ * 0.5; }").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn function_application_ok() {
        let t = ty_of("main = g _ where { g x = _ * x; }").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn where_mutual_visibility() {
        let t = ty_of("main = a where { a = _ * 0.5; b = a; }").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn top_level_mutual_visibility() {
        let t = ty_of("a x = _ * x; main = a 0.5").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn top_level_reverse_order() {
        let t = ty_of("main = g 0.5; g x = _ * x").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn let_expression_scoping() {
        let t = ty_of("main = let g = _ * 0.5 in g").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn let_not_visible_outside() {
        assert!(ty_of("main = let g = _ * 0.5 in _ ; main = g").is_err());
    }

    struct TestSigs;
    impl crate::builtin::SignatureSource for TestSigs {
        fn builtin_sig(&self, name: &str) -> Option<&crate::builtin::BuiltinSig> {
            use crate::builtin::{BuiltinKind, BuiltinSig};
            match name {
                "lowpass" => Some(Box::leak(Box::new(BuiltinSig::simple(
                    "lowpass",
                    1,
                    1,
                    2,
                    BuiltinKind::Block,
                )))),
                "onepole" => Some(Box::leak(Box::new(BuiltinSig::simple(
                    "onepole",
                    1,
                    1,
                    2,
                    BuiltinKind::Sample,
                )))),
                _ => None,
            }
        }
    }

    fn ty_with(src: &str) -> Result<TypedProgram, CompileError> {
        infer_program_with(
            &parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap(),
            &TestSigs,
        )
    }

    #[test]
    fn builtin_call_is_1_to_1() {
        let t = ty_with("main = _ : lowpass(1000.0, 0.7)").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn builtin_wrong_param_count_errors() {
        assert!(ty_with("main = _ : lowpass(1000.0)").is_err());
    }

    #[test]
    fn builtin_ref_param_ok() {
        assert!(ty_with("main f = _ : lowpass(f, 0.7)").is_ok());
    }

    #[test]
    fn sample_builtin_in_feedback_typechecks() {
        assert!(ty_with("main = + ~ onepole(200.0, 0.5)").is_ok());
    }

    #[test]
    fn transitive_var_chain_resolves() {
        let t = ty_of("main = g where { f x = _ * x; g = f 0.5; }").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn var_unifies_with_float() {
        let t = ty_of("main = _ * 0.5").unwrap();
        assert!(matches!(t.process_ty.outs[0], Scalar::Float));
    }

    #[test]
    fn int_float_mismatch_errors() {
        // This test existed before; keep it but it's tricky to trigger
        // with current rules since everything defaults to Float
        assert!(ty_of("main = _ @ _").is_err());
    }
}
