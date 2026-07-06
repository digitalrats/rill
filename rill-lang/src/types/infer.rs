//! Algorithm-W-style inference over scalar types, with bottom-up arity
//! synthesis and combinator arity checking.

use std::collections::HashMap;

use super::ty::{Scalar, Scheme, Subst, Type, TypeVarId};
use super::unify::unify_scalar;
use crate::ast::{BinOp, Expr, Program};
use crate::builtin::SignatureSource;
use crate::error::{CompileError, Span};

/// The typed result of inference: the program's definitions plus the resolved
/// type of `process` and the final substitution.
#[derive(Debug, Clone)]
pub struct TypedProgram {
    /// The original program (unchanged AST).
    pub program: Program,
    /// Resolved diagram type of `process` (arities + scalars, substitution applied).
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

    let mut process_ty: Option<Type> = None;
    for def in &program.defs {
        ctx.locals.clear();
        for p in &def.params {
            let s = ctx.fresh();
            ctx.locals.insert(p.clone(), Type::uniform(1, 1, s));
        }
        let ty = infer_expr(&mut ctx, &def.body)?;
        let resolved = ctx.subst.apply(&ty);
        let vars = ctx.free_vars(&resolved);
        ctx.defs.insert(
            def.name.clone(),
            Scheme {
                vars,
                ty: resolved.clone(),
            },
        );
        if def.name == "process" {
            process_ty = Some(resolved);
        }
    }

    let pty = process_ty.ok_or_else(|| CompileError::Type {
        msg: "program has no `process` definition".into(),
        span: Span::new(0, 0),
    })?;
    let vars: Vec<TypeVarId> = (0..ctx.next).collect();
    for v in vars {
        ctx.subst.map.entry(v).or_insert(Scalar::Float);
    }
    let pty = ctx.subst.apply(&pty);

    if pty.arity_out() != 1 || pty.arity_in() > 1 {
        return Err(CompileError::Type {
            msg: format!(
                "`process` must have arity (0|1)->1, found ({}->{})",
                pty.arity_in(),
                pty.arity_out()
            ),
            span: process_span(program),
        });
    }
    Ok(TypedProgram {
        program: program.clone(),
        process_ty: pty,
    })
}

fn process_span(program: &Program) -> Span {
    program
        .defs
        .iter()
        .find(|d| d.name == "process")
        .map(|d| d.span)
        .unwrap_or(Span::new(0, 0))
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
            msg: "string literal is only valid as a `param` name".into(),
            span: *span,
        }),
        Expr::Bin { op, lhs, rhs, span } => {
            let a = infer_expr(ctx, lhs)?;
            let b = infer_expr(ctx, rhs)?;
            infer_bin(ctx, *op, &a, &b, *span)
        }
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
        if sig.num_params == 0 {
            return Ok(Type::uniform(
                sig.signal_ins,
                sig.signal_outs,
                Scalar::Float,
            ));
        }
    }
    if let Some(t) = ctx.locals.get(name) {
        return Ok(t.clone());
    }
    if let Some(scheme) = ctx.defs.get(name).cloned() {
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
    if let Some(sig) = ctx.sigs.builtin_sig(name) {
        let sig = sig.clone();
        if args.len() != sig.num_params {
            return Err(CompileError::Type {
                msg: format!(
                    "built-in `{name}` expects {} param(s), got {}",
                    sig.num_params,
                    args.len()
                ),
                span,
            });
        }
        for a in args {
            let at = infer_expr(ctx, a)?;
            if at.arity_in() != 0 || at.arity_out() != 1 {
                return Err(CompileError::Type {
                    msg: format!("param to `{name}` must be a constant expression"),
                    span: a.span(),
                });
            }
        }
        return Ok(Type::uniform(
            sig.signal_ins,
            sig.signal_outs,
            Scalar::Float,
        ));
    }
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
        infer_program(&parse(&tokenize(src).unwrap()).unwrap())
    }

    #[test]
    fn wire_is_1_to_1() {
        let t = ty_of("process = _;").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn gain_is_1_to_1() {
        let t = ty_of("process = _ * 0.5;").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn integrator_is_1_to_1() {
        let t = ty_of("process = + ~ _;").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn rejects_seq_arity_mismatch() {
        assert!(ty_of("process = (_ , _) : _;").is_err());
    }

    #[test]
    fn rejects_bad_process_arity() {
        assert!(ty_of("process = _ , _;").is_err());
    }

    #[test]
    fn split_then_merge_ok() {
        let t = ty_of("process = _ <: (_ , _) :> + ;").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn delay_constant_ok_variable_errors() {
        assert!(ty_of("process = _ @ 1;").is_ok());
        assert!(ty_of("process = _ @ _;").is_err());
    }

    #[test]
    fn user_def_alias_resolves() {
        let t = ty_of("gain = _ * 0.5; process = gain;").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn function_application_ok() {
        let t = ty_of("g(x) = x * 0.5; process = g(_);").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    struct TestSigs;
    impl crate::builtin::SignatureSource for TestSigs {
        fn builtin_sig(&self, name: &str) -> Option<&crate::builtin::BuiltinSig> {
            use crate::builtin::{BuiltinKind, BuiltinSig};
            match name {
                "lowpass" => Some(Box::leak(Box::new(BuiltinSig {
                    name: "lowpass",
                    signal_ins: 1,
                    signal_outs: 1,
                    num_params: 2,
                    kind: BuiltinKind::Block,
                }))),
                "onepole" => Some(Box::leak(Box::new(BuiltinSig {
                    name: "onepole",
                    signal_ins: 1,
                    signal_outs: 1,
                    num_params: 2,
                    kind: BuiltinKind::Sample,
                }))),
                _ => None,
            }
        }
    }

    fn ty_with(src: &str) -> Result<TypedProgram, CompileError> {
        infer_program_with(&parse(&tokenize(src).unwrap()).unwrap(), &TestSigs)
    }

    #[test]
    fn builtin_call_is_1_to_1() {
        let t = ty_with("process = _ : lowpass(1000.0, 0.7);").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn builtin_wrong_param_count_errors() {
        assert!(ty_with("process = _ : lowpass(1000.0);").is_err());
    }

    #[test]
    fn builtin_non_const_param_errors() {
        assert!(ty_with("process = _ : lowpass(_, 0.7);").is_err());
    }

    #[test]
    fn sample_builtin_in_feedback_typechecks() {
        assert!(ty_with("process = + ~ onepole(200.0, 0.5);").is_ok());
    }
}
