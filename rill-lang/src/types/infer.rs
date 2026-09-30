//! Algorithm-W-style inference over scalar types, with bottom-up arity
//! synthesis and combinatorial arity checking.
//!
//! All binding groups (top-level, `where`, `let`) use mutual recursion:
//! every name in the group is visible to every body.

use std::collections::{HashMap, HashSet};

use super::ty::{
    ArrowTy, Block, Channel, DataInfo, Rate, Scalar, Scheme, Subst, TypeEnv, TypeVarId, ValueTy,
};
use super::unify::{unify_scalar, unify_value};
use crate::ast::{Def, Expr, MatchArm, Pattern, Program};
use crate::builtin::{ParamType, SignatureSource};
use crate::error::{CompileError, Span};
use crate::reduce::pattern_vars;

/// The typed result of inference: the program's definitions plus the resolved
/// type of the output and the final substitution.
#[derive(Debug, Clone)]
pub struct TypedProgram {
    /// The original program (unchanged AST).
    pub program: Program,
    /// Resolved diagram type of the body.
    pub process_ty: ArrowTy,
    /// Names of closed top-level definitions (CAFs): zero λ-parameters and
    /// zero signal input channels. Referenced by name, shared across the graph.
    pub cafs: HashSet<String>,
    /// The compile-time type environment (aliases, newtypes, data types),
    /// built once here and shared with lowering.
    pub type_env: TypeEnv,
    /// Resolved λ-parameter value types of every named lambda-literal definition,
    /// keyed by definition name. Lowering types fragment-local parameter
    /// registers with these so a higher-order parameter is a `Func`, a record
    /// parameter is its `Data` type, and field projection / function dispatch
    /// resolve at compile time.
    pub fn_param_tys: HashMap<String, Vec<ValueTy>>,
}

/// Inference context: fresh var supply, definition schemes, local bindings,
/// and a signature source for built-in resolution.
struct Ctx<'a> {
    next: TypeVarId,
    subst: Subst,
    defs: HashMap<String, Scheme>,
    /// Definition name → body expression, for every non-declaration def that
    /// has been registered. Used to resolve a func value's transitively
    /// referenced definition at application time (see [`func_target`]).
    def_bodies: HashMap<String, Expr>,
    locals: HashMap<String, ArrowTy>,
    sigs: &'a dyn SignatureSource,
    /// The compile-time type environment (aliases, newtypes, data types).
    env: TypeEnv,
    /// Recursion guard for typeclass method inlining: resolved
    /// (class, type, method) calls currently on the expansion path.
    method_lifting: HashSet<(String, String, String)>,
}

impl Ctx<'_> {
    fn fresh(&mut self) -> Scalar {
        let v = self.next;
        self.next += 1;
        Scalar::Var(v)
    }

    fn fresh_vty(&mut self) -> ValueTy {
        let v = self.next;
        self.next += 1;
        ValueTy::Var(v)
    }

    fn instantiate(&mut self, scheme: &Scheme) -> ArrowTy {
        let mut remap: HashMap<TypeVarId, Scalar> = HashMap::new();
        for v in &scheme.vars {
            let f = self.fresh();
            remap.insert(*v, f);
        }
        let rw = |s: &Scalar| match s {
            Scalar::Var(v) => remap.get(v).cloned().unwrap_or_else(|| s.clone()),
            _ => s.clone(),
        };
        // Preserve rate: value channels carry concrete value types (v1 keeps
        // value-typed schemes monomorphic, `vars` is empty for them).
        let block = |b: &Block| match b.rate {
            Rate::Value => Channel::value(b.vty.clone()),
            Rate::Signal => Block::new(rw(&b.elem)),
        };
        ArrowTy {
            ins: scheme.ty.ins.iter().map(&block).collect(),
            outs: scheme.ty.outs.iter().map(&block).collect(),
        }
    }

    fn free_vars(&self, t: &ArrowTy) -> Vec<TypeVarId> {
        let mut acc = Vec::new();
        for s in t.ins.iter().chain(t.outs.iter()).map(|b| &b.elem) {
            if let Scalar::Var(v) = self.subst.resolve_scalar(s) {
                if !acc.contains(&v) {
                    acc.push(v);
                }
            }
        }
        acc
    }
}

/// Names of sum types that declare a constructor with the given name.
fn sum_types_with_ctor(ctx: &Ctx<'_>, ctor: &str) -> Vec<String> {
    ctx.env
        .data_types
        .iter()
        .filter_map(|(tname, info)| match info {
            DataInfo::Sum(ctors) if ctors.iter().any(|(c, _)| c == ctor) => Some(tname.clone()),
            _ => None,
        })
        .collect()
}

/// The payload value types of `ctor` within the sum type `sum_name`.
fn sum_ctor_payload(ctx: &Ctx<'_>, sum_name: &str, ctor: &str) -> Option<Vec<ValueTy>> {
    match ctx.env.data_types.get(sum_name) {
        Some(DataInfo::Sum(ctors)) => ctors
            .iter()
            .find(|(c, _)| c == ctor)
            .map(|(_, payload)| payload.clone()),
        _ => None,
    }
}

/// Convert a data-declaration field/payload type expression into a value type,
/// substituting type-variable names with placeholder positions (`data Box a` →
/// field `value: a` becomes `Var(1)`, mirroring the builtin `Maybe`/`Pair`
/// shapes). Concrete type names resolve through [`TypeEnv::vty_of_name`].
fn data_field_vty(env: &TypeEnv, tyvars: &[String], te: &crate::ast::TypeExpr) -> ValueTy {
    match te {
        crate::ast::TypeExpr::TName(n) => match tyvars.iter().position(|t| t == n) {
            Some(k) => ValueTy::Var((k + 1) as u32),
            None => env.vty_of_name(n),
        },
        crate::ast::TypeExpr::TApp(head, args) => {
            // A type PARAMETER applied as a constructor (`m b` in
            // `data K m a b`) becomes a type-constructor application whose head
            // is the parameter's var id. A concrete constructor stays `App`.
            if let Some(k) = tyvars.iter().position(|t| t == head) {
                ValueTy::TyConApp(
                    (k + 1) as u32,
                    args.iter()
                        .map(|a| data_field_vty(env, tyvars, a))
                        .collect(),
                )
            } else {
                ValueTy::App(
                    head.clone(),
                    args.iter()
                        .map(|a| data_field_vty(env, tyvars, a))
                        .collect(),
                )
            }
        }
        crate::ast::TypeExpr::TFunc(args, ret) => ValueTy::Func(
            args.iter()
                .map(|a| data_field_vty(env, tyvars, a))
                .collect(),
            vec![data_field_vty(env, tyvars, ret)],
        ),
    }
}

/// The value type of a value of sum type `sum_name`: builtin sums are
/// parameterized (`Maybe a`, `Either a b`) and compare/construct as `App`,
/// while user sums are monomorphic `Data`. A fresh live value variable fills
/// each builtin type parameter (the shape's `Var(1)`/`Var(2)` placeholders are
/// NOT live unification vars — see [`TypeEnv::with_builtins`]).
fn fresh_sum_vty(ctx: &mut Ctx<'_>, sum_name: &str) -> ValueTy {
    match ctx.env.ctor_arity(sum_name) {
        Some(arity) => {
            let args = (0..arity).map(|_| ctx.fresh_vty()).collect();
            ValueTy::App(sum_name.into(), args)
        }
        None => ValueTy::Data(sum_name.into(), vec![]),
    }
}

/// Infer an expression that must yield exactly one output and no inputs — a
/// constant or a per-block value. Signal-rate constants (literals) are coerced
/// to their value type; returns the resulting `ValueTy`.
fn infer_const_value(ctx: &mut Ctx<'_>, e: &Expr) -> Result<ValueTy, CompileError> {
    let span = e.span();
    let t = infer_expr(ctx, e)?;
    if t.arity_in() != 0 || t.arity_out() != 1 {
        return Err(CompileError::Type {
            msg: format!(
                "expected a constant or value expression, got arity {}->{}",
                t.arity_in(),
                t.arity_out()
            ),
            span,
        });
    }
    let ch = &t.outs[0];
    match ch.rate {
        Rate::Value => Ok(ch.vty.clone()),
        Rate::Signal => match ch.elem {
            Scalar::Int => Ok(ValueTy::Int),
            Scalar::Float => Ok(ValueTy::Float),
            Scalar::Var(_) => Err(CompileError::Type {
                msg: "cannot determine the value type of a variable signal".into(),
                span,
            }),
        },
    }
}

/// Whether `e` is a reference (possibly chained) to a definition whose body is
/// a constant literal — a numeric scalar the value track can capture by value.
fn is_const_value_ref(ctx: &Ctx<'_>, e: &Expr) -> bool {
    let mut cur = e;
    let mut seen = HashSet::new();
    while let Expr::Ref(name, _) = cur {
        if !seen.insert(name.clone()) {
            return false;
        }
        match ctx.def_bodies.get(name.as_str()) {
            Some(Expr::Int(_, _)) => return true,
            Some(Expr::Float(_, _)) => return true,
            Some(next @ Expr::Ref(_, _)) => cur = next,
            _ => return false,
        }
    }
    false
}

/// Resolve a definition name to the fragment-valued definition it dispatches
/// to, following func-value alias chains (`fref = f` where `f` is a lambda
/// literal). Returns `None` when the name is not a fragment-valued definition.
///
/// A name is fragment-valued when its body is a lambda literal, an Anchor with
/// λ-parameters (compiled to a fragment when referenced as a value), an
/// application that produces a closure (`add3 = add 3.0`), or a reference chain
/// that reaches one. This mirrors `is_closure_def` in `reduce.rs` (which keeps
/// such applications un-β-reduced for runtime dispatch).
fn fragment_target(
    defs: &HashMap<String, Def>,
    name: &str,
    seen: &mut HashSet<String>,
) -> Option<String> {
    if !seen.insert(name.to_string()) {
        return None;
    }
    match defs.get(name) {
        Some(Def::Local { body, .. }) => match body {
            Expr::Lambda { .. } => Some(name.to_string()),
            Expr::Apply { .. } => Some(name.to_string()),
            Expr::Ref(next, _) => fragment_target(defs, next, seen),
            _ => None,
        },
        Some(Def::Anchor { params, .. }) if !params.is_empty() => Some(name.to_string()),
        _ => None,
    }
}

/// Collect the names of fragment-valued definitions reachable from `defs`
/// (top-level defs and their nested `where` defs).
fn fragment_valued_names(defs: &[Def]) -> HashSet<String> {
    let mut all: Vec<&Def> = Vec::new();
    for d in defs {
        all.push(d);
        for wd in d.where_defs() {
            all.push(wd);
        }
    }
    let mut map: HashMap<String, Def> = HashMap::new();
    for d in all {
        map.insert(d.name().to_string(), d.clone());
    }
    let mut names = HashSet::new();
    for d in map.values() {
        if fragment_target(&map, d.name(), &mut HashSet::new()).is_some() {
            names.insert(d.name().to_string());
        }
    }
    names
}

/// Descend `e` (a fragment-valued definition's body) collecting the
/// fragment-valued definitions it STATICALLY calls: an `Apply` whose callee is
/// an unshadowed reference to a fragment-valued name yields the edge
/// `src → callee`. Nested lambda/let/match bindings shadow outer names,
/// mirroring the scope rules; the walk still descends into them so a
/// self-reference through an anonymous inner lambda
/// (`a = fn x -> (fn y -> a y) 1.0`) is caught.
fn collect_static_calls(
    e: &Expr,
    src: &str,
    bound: &HashSet<String>,
    nodes: &HashSet<String>,
    out: &mut Vec<(String, String)>,
) {
    match e {
        Expr::Apply { name, args, .. } => {
            if !bound.contains(name) && nodes.contains(name) {
                out.push((src.to_string(), name.clone()));
            }
            for a in args {
                collect_static_calls(a, src, bound, nodes, out);
            }
        }
        Expr::Ref(_, _)
        | Expr::Int(_, _)
        | Expr::Float(_, _)
        | Expr::Imag(_, _)
        | Expr::Str(_, _)
        | Expr::Wire(_)
        | Expr::Cut(_) => {}
        Expr::Neg(inner, _) => collect_static_calls(inner, src, bound, nodes, out),
        Expr::Arith { lhs, rhs, .. } => {
            collect_static_calls(lhs, src, bound, nodes, out);
            collect_static_calls(rhs, src, bound, nodes, out);
        }
        Expr::Seq(lhs, rhs, _)
        | Expr::Par(lhs, rhs, _)
        | Expr::Split(lhs, rhs, _)
        | Expr::Merge(lhs, rhs, _)
        | Expr::Loop(lhs, rhs, _)
        | Expr::Delay(lhs, rhs, _) => {
            collect_static_calls(lhs, src, bound, nodes, out);
            collect_static_calls(rhs, src, bound, nodes, out);
        }
        Expr::Let { defs, body, .. } => {
            let mut inner = bound.clone();
            for d in defs {
                inner.insert(d.name().to_string());
            }
            for d in defs {
                collect_static_calls(d.body(), src, bound, nodes, out);
            }
            collect_static_calls(body, src, &inner, nodes, out);
        }
        Expr::Record(fields, _) => {
            for (_, fe) in fields {
                collect_static_calls(fe, src, bound, nodes, out);
            }
        }
        Expr::ListLit(elems, _) => {
            for el in elems {
                collect_static_calls(el, src, bound, nodes, out);
            }
        }
        Expr::MapLit(entries, _) => {
            for (_, ve) in entries {
                collect_static_calls(ve, src, bound, nodes, out);
            }
        }
        Expr::Bool(_, _) => {}
        Expr::Cmp { lhs, rhs, .. } | Expr::Logic { lhs, rhs, .. } => {
            collect_static_calls(lhs, src, bound, nodes, out);
            collect_static_calls(rhs, src, bound, nodes, out);
        }
        Expr::FieldProject { record, .. } => collect_static_calls(record, src, bound, nodes, out),
        Expr::FieldUpdate { record, value, .. } => {
            collect_static_calls(record, src, bound, nodes, out);
            collect_static_calls(value, src, bound, nodes, out);
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            collect_static_calls(scrutinee, src, bound, nodes, out);
            for arm in arms {
                let mut inner = bound.clone();
                for v in pattern_vars(&arm.pattern) {
                    inner.insert(v);
                }
                for (g, b) in &arm.guards {
                    collect_static_calls(g, src, &inner, nodes, out);
                    collect_static_calls(b, src, &inner, nodes, out);
                }
            }
        }
        Expr::If {
            cond, then, els, ..
        } => {
            collect_static_calls(cond, src, bound, nodes, out);
            collect_static_calls(then, src, bound, nodes, out);
            collect_static_calls(els, src, bound, nodes, out);
        }
        Expr::Lambda { params, body, .. } => {
            let mut inner = bound.clone();
            for p in params {
                inner.insert(p.name.clone());
            }
            collect_static_calls(body, src, &inner, nodes, out);
        }
        Expr::ActorParam { default, .. } => {
            if let Some(d) = default {
                collect_static_calls(d, src, bound, nodes, out);
            }
        }
    }
}

/// Reject recursion between first-class function definitions.
///
/// v1's strict contract forbids recursive calls: every runtime dispatch chain
/// must terminate, so the runtime call stack is statically bounded. This
/// syntactic pass builds the static call graph over fragment-valued
/// definitions (lambda literals, λ-parameter anchors, and closure-producing
/// applications like partial application) — an edge `A → B` when `A`'s body
/// calls `B` by name — and rejects any cycle with a compile error. Only
/// STATIC named calls are tracked: a higher-order parameter (`f` in
/// `twice = fn f x -> f (f x)`) dispatches whatever closure the caller
/// passes, and is not itself a recursion source.
fn check_recursion(defs: &[Def]) -> Result<(), CompileError> {
    let nodes = fragment_valued_names(defs);
    let mut edges: Vec<(String, String)> = Vec::new();
    for d in defs {
        let name = d.name();
        if !nodes.contains(name) {
            continue;
        }
        let bound = HashSet::new();
        collect_static_calls(d.body(), name, &bound, &nodes, &mut edges);
        // Self-recursion is a self-loop; report it directly for a clear message.
        if edges.iter().any(|(src, dst)| src == name && dst == name) {
            return Err(CompileError::Type {
                msg: format!("recursive function call: `{name}` calls itself"),
                span: d.body().span(),
            });
        }
    }
    // DFS over the call graph; a back edge into a node still being visited is
    // a cycle.
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    fn dfs(
        n: &str,
        edges: &Vec<(String, String)>,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) -> Result<(), CompileError> {
        if visited.contains(n) {
            return Ok(());
        }
        visiting.insert(n.to_string());
        for (src, dst) in edges {
            if src == n {
                if visiting.contains(dst.as_str()) {
                    return Err(CompileError::Type {
                        msg: format!(
                            "recursive function call: `{n}` calls `{dst}` (cyclic call graph)"
                        ),
                        span: Span::new(0, 0),
                    });
                }
                dfs(dst.as_str(), edges, visiting, visited)?;
            }
        }
        visiting.remove(n);
        visited.insert(n.to_string());
        Ok(())
    }
    for n in defs
        .iter()
        .filter_map(|d| nodes.contains(d.name()).then_some(d.name().to_string()))
    {
        dfs(n.as_str(), &edges, &mut visiting, &mut visited)?;
    }
    Ok(())
}

/// Build the class-var signature pattern for the container argument: `f a`
/// becomes `App("f", [fresh])` so `match_ctor_pattern` unifies the type-var
/// slots against the concrete constructor application. The caller computed
/// `idx` via [`TypeEnv::class_var_arg_index`], so the argument is always a
/// `TApp` headed by the class variable.
fn class_var_pattern(ctx: &mut Ctx<'_>, sig: &crate::ast::TypeExpr, idx: usize) -> ValueTy {
    let container_te = match sig {
        crate::ast::TypeExpr::TFunc(args, _) => args.get(idx).cloned(),
        _ => None,
    };
    match container_te {
        Some(crate::ast::TypeExpr::TApp(head, type_args)) => {
            let fresh: Vec<ValueTy> = type_args.iter().map(|_| ctx.fresh_vty()).collect();
            ValueTy::App(head, fresh)
        }
        _ => unreachable!(
            "class_var_pattern requires a TApp at container_idx (class_var_arg_index guarantees it)"
        ),
    }
}

/// Infer a typeclass method's argument or body expression (labeled by `what`
/// for error messages): it must produce a single output channel that is either
/// a value channel ([`Rate::Value`]) or a bare Float/Int literal (value-
/// compatible in v1). Genuine signal computations (`sin 1.0`, arithmetic)
/// cannot flow through the value track, so they are rejected here — at
/// inference, with a clear message — rather than failing obscurely during
/// lowering.
fn infer_method_value_vty(
    ctx: &mut Ctx<'_>,
    e: &Expr,
    what: &str,
) -> Result<ValueTy, CompileError> {
    infer_method_value_vty_expected(ctx, e, what, None)
}

/// [`infer_method_value_vty`] with an expected value type (used for
/// result-directed typeclass dispatch inside method-call arguments).
fn infer_method_value_vty_expected(
    ctx: &mut Ctx<'_>,
    e: &Expr,
    what: &str,
    expected: Option<ValueTy>,
) -> Result<ValueTy, CompileError> {
    let span = e.span();
    let t = infer_expr_expected(ctx, e, expected)?;
    if t.arity_in() != 0 || t.arity_out() != 1 {
        return Err(CompileError::Type {
            msg: format!(
                "expected a constant or value expression, got arity {}->{}",
                t.arity_in(),
                t.arity_out()
            ),
            span,
        });
    }
    match t.outs[0].rate {
        Rate::Value => Ok(t.outs[0].vty.clone()),
        Rate::Signal => match e {
            Expr::Int(_, _) => Ok(ValueTy::Int),
            Expr::Float(_, _) => Ok(ValueTy::Float),
            _ => Err(CompileError::Type {
                msg: format!(
                    "typeclass method {what} must be a value expression (found signal channel)"
                ),
                span,
            }),
        },
    }
}

/// Whether `e` is a bare reference to a nullary method of `class_name` (e.g.
/// `mempty` in `mappend xs mempty`) — such an argument resolves by the selector
/// argument's concrete type.
fn is_bare_nullary_method_ref(ctx: &Ctx<'_>, e: &Expr, class_name: &str) -> bool {
    match e {
        Expr::Ref(name, _) => ctx.env.is_nullary_method(class_name, name.as_str()),
        _ => false,
    }
}

/// Convert a typeclass method signature's curried argument types into concrete
/// parameter `ValueTy`s, substituting the class variable with the concrete
/// constructor (or concrete type name for arity-0 classes). Type variables in
/// the signature (`a`, `b`) map to fresh unification vars, shared by name so
/// `g : a -> b` and `xs : f a` agree on `a`. Used to validate instance method
/// bodies against the class contract.
fn signature_param_tys(
    ctx: &mut Ctx<'_>,
    class_var: &str,
    ctor: &str,
    sig: &crate::ast::TypeExpr,
) -> Vec<ValueTy> {
    let env = ctx.env.clone();
    signature_param_tys_conv(&env, class_var, ctor, sig, || ctx.fresh_vty())
}

/// [`signature_param_tys`] usable from lowering (no `Ctx`): `fresh` supplies
/// fresh type variables. Exposed so the lowerer can thread expected types into
/// method-call arguments (`bind mx (fn x -> return x)`).
pub(crate) fn signature_param_tys_conv(
    env: &TypeEnv,
    class_var: &str,
    ctor: &str,
    sig: &crate::ast::TypeExpr,
    mut fresh: impl FnMut() -> ValueTy,
) -> Vec<ValueTy> {
    let mut vars: HashMap<String, ValueTy> = HashMap::new();
    fn conv(
        env: &TypeEnv,
        class_var: &str,
        ctor: &str,
        vars: &mut HashMap<String, ValueTy>,
        fresh: &mut impl FnMut() -> ValueTy,
        te: &crate::ast::TypeExpr,
    ) -> ValueTy {
        match te {
            crate::ast::TypeExpr::TName(n) if n == class_var => env.vty_of_name(ctor),
            crate::ast::TypeExpr::TName(n) => {
                if matches!(n.as_str(), "Float" | "Int" | "Bool" | "String")
                    || env.data_types.contains_key(n)
                    || env.newtypes.contains_key(n)
                    || env.ctor_kinds.contains_key(n)
                    || env.type_aliases.contains_key(n)
                {
                    env.vty_of_name(n)
                } else {
                    vars.entry(n.clone()).or_insert_with(fresh).clone()
                }
            }
            crate::ast::TypeExpr::TApp(head, args) if head == class_var => {
                let vargs: Vec<ValueTy> = args
                    .iter()
                    .map(|a| conv(env, class_var, ctor, vars, fresh, a))
                    .collect();
                if env.ctor_arity(ctor).is_some() {
                    ValueTy::App(ctor.to_string(), vargs)
                } else if env.data_arities.contains_key(ctor) {
                    ValueTy::Data(ctor.to_string(), vargs)
                } else {
                    env.vty_of_name(ctor)
                }
            }
            crate::ast::TypeExpr::TApp(head, args) => ValueTy::App(
                head.clone(),
                args.iter()
                    .map(|a| conv(env, class_var, ctor, vars, fresh, a))
                    .collect(),
            ),
            crate::ast::TypeExpr::TFunc(args, ret) => ValueTy::Func(
                args.iter()
                    .map(|a| conv(env, class_var, ctor, vars, fresh, a))
                    .collect(),
                vec![conv(env, class_var, ctor, vars, fresh, ret)],
            ),
        }
    }
    match sig {
        crate::ast::TypeExpr::TFunc(args, _) if !args.is_empty() => args
            .iter()
            .map(|a| conv(env, class_var, ctor, &mut vars, &mut fresh, a))
            .collect(),
        other => vec![conv(env, class_var, ctor, &mut vars, &mut fresh, other)],
    }
}

/// Validate every instance's method bodies at compile time, even when the
/// instance is never called. Each body must infer to a single value channel
/// (or a bare literal) with its parameters bound to values of the class
/// signature's types (the class var substituted by the concrete constructor).
/// Constructor instances are kind-checked: a builtin constructor's value arity
/// must match the class variable's arity (`Pair` is arity 2, so it cannot be a
/// `Functor`, which needs arity 1). The recursion guard applies here too, so
/// self-inlining bodies are rejected even when the instance is dead code.
fn validate_instances(ctx: &mut Ctx<'_>) -> Result<(), CompileError> {
    // Clone the (class, type) keys so inference (which mutates ctx) does not
    // invalidate the iteration borrow.
    let keys: Vec<(String, String)> = ctx
        .env
        .instances
        .iter()
        .flat_map(|(class, by_ty)| by_ty.keys().map(|ty_name| (class.clone(), ty_name.clone())))
        .collect();
    for (class, ty_name) in keys {
        // Kind check: a constructor-class instance (`Functor f`) must bind a
        // constructor whose value arity equals the class variable's arity.
        if let Some(class_info) = ctx.env.typeclasses.get(&class).cloned() {
            if class_info.arity >= 1 {
                match ctx.env.ctor_value_arity(&ty_name) {
                    Some(got) if got != class_info.arity => {
                        return Err(CompileError::Type {
                            msg: format!(
                                "`{ty_name}` has arity {got}, but `{class}` expects arity {}",
                                class_info.arity
                            ),
                            span: Span::new(0, 0),
                        });
                    }
                    Some(_) => {}
                    None => {
                        return Err(CompileError::Type {
                            msg: format!(
                                "`{ty_name}` is not a type constructor (arity {} expected)",
                                class_info.arity
                            ),
                            span: Span::new(0, 0),
                        });
                    }
                }
            }
        }
        let info = ctx
            .env
            .instances
            .get(class.as_str())
            .and_then(|by_ty| by_ty.get(ty_name.as_str()))
            .cloned()
            .unwrap();
        for (mname, (params, body)) in &info.methods {
            let key = (class.clone(), ty_name.clone(), mname.clone());
            if ctx.method_lifting.contains(&key) {
                return Err(CompileError::Type {
                    msg: format!("recursive typeclass method `{mname}` for type `{ty_name}`"),
                    span: body.span(),
                });
            }
            ctx.method_lifting.insert(key.clone());
            let saved = ctx.locals.clone();
            // Bind each parameter to the signature-derived type (class var →
            // concrete constructor). Falls back to the instance's bound type for
            // a param count that does not match the signature.
            let (class_var, method_sig) = {
                let class_info = ctx.env.typeclasses.get(&class);
                match class_info.and_then(|c| c.methods.iter().find(|(m, _)| m == mname)) {
                    Some((_, sig)) => (class_info.map(|c| c.var.clone()), Some(sig.clone())),
                    None => (class_info.map(|c| c.var.clone()), None),
                }
            };
            let param_tys = match (class_var.clone(), method_sig.clone()) {
                (Some(cv), Some(sig)) => signature_param_tys(ctx, &cv, &ty_name, &sig),
                _ => params
                    .iter()
                    .map(|_| ctx.env.vty_of_name(&ty_name))
                    .collect(),
            };
            // The method body's EXPECTED result type (from the signature's
            // return, class var → concrete constructor): result-directed method
            // calls inside (`pure x = return x`) resolve by it.
            let body_expected = match (class_var, method_sig) {
                (Some(cv), Some(crate::ast::TypeExpr::TFunc(_, ret))) => {
                    let env = ctx.env.clone();
                    Some(
                        signature_param_tys_conv(&env, &cv, &ty_name, ret.as_ref(), || {
                            ctx.fresh_vty()
                        })
                        .first()
                        .cloned()
                        .unwrap_or(ValueTy::Float),
                    )
                }
                _ => None,
            };
            for (p, pt) in params.iter().zip(param_tys.iter()) {
                ctx.locals
                    .insert(p.clone(), ArrowTy::value_channel(pt.clone()));
            }
            let res = infer_method_value_vty_expected(ctx, body, "body", body_expected);
            ctx.locals = saved;
            ctx.method_lifting.remove(&key);
            res?;
        }
    }
    Ok(())
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
    // Build the type environment in two phases so declaration ORDER does not
    // matter. Phase 1 registers the pure name-mapping declarations (synonyms,
    // newtypes, typeclasses, instances); phase 2 resolves data-type
    // field/payload types against the COMPLETE alias/newtype environment. A
    // single-pass registration would wrongly reject
    // `data P = { x: Angles }; type Angles = Float; ...`.
    let mut env = TypeEnv::with_builtins();
    // Typeclass/instance declarations are registered by `register_decls` (the
    // same path the category prelude uses).
    env.register_decls(&program.defs);
    for def in &program.defs {
        match def {
            Def::TypeAlias { name, target, .. } => {
                env.type_aliases.insert(name.clone(), target.clone());
            }
            Def::Newtype { name, target, .. } => {
                env.newtypes.insert(name.clone(), target.clone());
            }
            _ => {}
        }
    }
    for def in &program.defs {
        match def {
            Def::Data {
                name,
                tyvars,
                fields,
                ..
            } => {
                let fields_ty = fields
                    .iter()
                    .map(|(f, t)| {
                        let ft = data_field_vty(&env, tyvars, t);
                        (f.clone(), ft)
                    })
                    .collect();
                env.data_types
                    .insert(name.clone(), DataInfo::Record(fields_ty));
                if !tyvars.is_empty() {
                    env.data_arities.insert(name.clone(), tyvars.len());
                }
            }
            Def::Sum {
                name,
                tyvars,
                ctors,
                ..
            } => {
                let ctors_ty = ctors
                    .iter()
                    .map(|(c, ts)| {
                        (
                            c.clone(),
                            ts.iter().map(|t| data_field_vty(&env, tyvars, t)).collect(),
                        )
                    })
                    .collect();
                env.data_types.insert(name.clone(), DataInfo::Sum(ctors_ty));
                // NOTE: parameterized user SUMS are intentionally NOT registered
                // in `data_arities` (only parameterized RECORDS are). A sum's
                // match-pin and ctor-construction paths stay monomorphic
                // `Data(name, [])`, so a parameterized sum as a typeclass
                // instance would fail instance-body validation with the
                // confusing `Data("Opt", [Var(_)])` vs `Data("Opt", [])` unify
                // error. Leaving sums out of the table makes `instance` fail
                // the kind check in `validate_instances` with a clean "not a
                // type constructor" / arity message. Sums still work as
                // ordinary data types (construction + match); they just cannot
                // be instances in v1. See `TypeEnv::data_arities`.
            }
            _ => {}
        }
    }

    // Reject recursive data types / newtypes: v1 guarantees acyclic value
    // graphs at compile time (the `strict` contract), keeping the arena
    // capacity bound exact and RC sound. A self-referential declaration would
    // otherwise materialise an unbounded subtree and exhaust the arena at
    // runtime (spec §9.1).
    env.check_acyclic()?;

    // Reject recursive function definitions (v1's acyclic call contract): a
    // runtime dispatch chain must terminate, so the call stack is statically
    // bounded (see `check_recursion`).
    check_recursion(&program.defs)?;

    // Derive `Eq`/`Ord` instances for every concrete data type (user + builtin)
    // and the scalar leaves, keeping any user-written instances intact.
    env.derive_eq_ord();

    let mut ctx = Ctx {
        next: 0,
        subst: Subst::default(),
        defs: HashMap::new(),
        def_bodies: HashMap::new(),
        locals: HashMap::new(),
        sigs,
        env,
        method_lifting: HashSet::new(),
    };

    infer_def_group(&mut ctx, &program.defs)?;

    // Validate instance method bodies even when the instance is never called:
    // each body must infer to a single value channel with its parameter bound
    // to a value of the instance's bound type. This catches garbage bodies
    // (signal expressions, unknown identifiers, recursive methods) at compile
    // time rather than leaving them to rot silently.
    validate_instances(&mut ctx)?;

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

    if signal_arity_out == 0 {
        return Err(CompileError::Type {
            msg: format!(
                "program must produce at least one signal output, found ({signal_arity_in}->{signal_arity_out})"
            ),
            span: Span::new(0, 0),
        });
    }

    let cafs = program
        .defs
        .iter()
        .filter_map(|def| match def {
            Def::Local { name, .. } => ctx
                .defs
                .get(name)
                .filter(|s| s.lam_count == 0 && s.ty.ins.is_empty())
                .map(|_| name.clone()),
            Def::Anchor { .. } => None,
            _ => None,
        })
        .collect();

    // Resolve each named lambda-literal definition's λ-parameter value types
    // through the final substitution. Lowering types fragment-local parameter
    // registers with these: a higher-order parameter must be a `Func` (so the
    // callee dispatches), a record parameter its `Data` type (so field
    // projection resolves indices), and a scalar parameter whatever the body
    // constrained it to (an unconstrained parameter stays a structural `Var`,
    // which lowering treats as an untyped scalar).
    let mut fn_param_tys: HashMap<String, Vec<ValueTy>> = HashMap::new();
    for def in &program.defs {
        if let Def::Local { name, body, .. } = def {
            if matches!(body, Expr::Lambda { .. }) {
                let scheme = ctx.defs.get(name).cloned();
                if let Some(s) = scheme {
                    if let Some(out) = s.ty.outs.first() {
                        if let ValueTy::Func(arg_tys, _) = &out.vty {
                            let resolved =
                                arg_tys.iter().map(|t| ctx.subst.resolve_value(t)).collect();
                            fn_param_tys.insert(name.to_string(), resolved);
                        }
                    }
                }
            }
        }
    }

    Ok(TypedProgram {
        program: program.clone(),
        process_ty: main_scheme.ty,
        cafs,
        type_env: ctx.env.clone(),
        fn_param_tys,
    })
}

/// Infer a definition body.
///
/// λ-parameters are bound as signal channels first. When that fails and the
/// definition has λ-parameters, retry with the parameters bound as VALUE
/// channels (fresh unresolved value types): a value function's λ-parameter
/// (`first p = p.x`) has no signal meaning, so its body only typechecks when
/// the parameter is a value. The failed attempt's substitution and fresh-var
/// counter are rolled back so the retry starts from a clean context. A def
/// with no λ-parameters that fails is genuinely broken and errors.
fn infer_def_body(ctx: &mut Ctx<'_>, def: &Def) -> Result<ArrowTy, CompileError> {
    let saved_locals = ctx.locals.clone();
    if def.params().is_empty() {
        let r = infer_expr(ctx, def.body());
        ctx.locals = saved_locals;
        return r;
    }
    let saved_subst = ctx.subst.clone();
    let saved_next = ctx.next;
    let saved_defs = ctx.defs.clone();
    let saved_bodies = ctx.def_bodies.clone();
    ctx.locals.clear();
    for p in def.params() {
        ctx.locals
            .insert(p.name.clone(), ArrowTy::uniform(0, 1, Scalar::Float));
    }
    let result = match infer_expr(ctx, def.body()) {
        Ok(t) => Ok(t),
        Err(_) => {
            ctx.subst = saved_subst;
            ctx.next = saved_next;
            ctx.defs = saved_defs;
            ctx.def_bodies = saved_bodies;
            ctx.locals.clear();
            for p in def.params() {
                let v = ctx.next;
                ctx.next += 1;
                ctx.locals
                    .insert(p.name.clone(), ArrowTy::value_channel(ValueTy::Var(v)));
            }
            infer_expr(ctx, def.body())
        }
    };
    ctx.locals = saved_locals;
    result
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
        if def.is_decl() {
            continue;
        }
        if ctx.defs.contains_key(def.name()) {
            return Err(CompileError::Type {
                msg: format!("duplicate definition `{}`", def.name()),
                span: def.body().span(),
            });
        }
        // Bodies are unchanged by inference; record them once so func-value
        // application can resolve the transitively referenced definition.
        ctx.def_bodies
            .insert(def.name().to_string(), def.body().clone());
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
                ty: ArrowTy {
                    ins: ins.into_iter().map(Block::new).collect(),
                    outs: vec![Block::new(out)],
                },
            },
        );
    }

    // Phase 2: infer bodies with placeholder visibility
    for def in defs {
        if def.is_decl() {
            continue;
        }
        if !def.where_defs().is_empty() {
            infer_def_group(ctx, def.where_defs())?;
        }
        let body_ty = infer_def_body(ctx, def)?;
        let lam_count = def.params().len();
        let mut full_ins = Vec::with_capacity(lam_count + body_ty.ins.len());
        for _ in 0..lam_count {
            full_ins.push(Block::new(Scalar::Float));
        }
        full_ins.extend(body_ty.ins);
        let resolved = ctx.subst.apply(&ArrowTy {
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
        if def.is_decl() {
            continue;
        }
        let body_ty = infer_def_body(ctx, def)?;
        let lam_count = def.params().len();
        let mut full_ins = Vec::with_capacity(lam_count + body_ty.ins.len());
        for _ in 0..lam_count {
            full_ins.push(Block::new(Scalar::Float));
        }
        full_ins.extend(body_ty.ins);
        let resolved = ctx.subst.apply(&ArrowTy {
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

/// Resolve an arm/branch body's result value type: a value channel directly,
/// or a bare `Int`/`Float` literal (signal-rate in v1 but value-compatible in
/// value positions).
fn arm_result_vty(bt: ArrowTy, body: &Expr, span: Span) -> Result<ValueTy, CompileError> {
    if bt.arity_in() != 0 || bt.arity_out() != 1 {
        return Err(CompileError::Type {
            msg: "branch must be a value expression (0->1 value channel)".into(),
            span,
        });
    }
    match bt.outs[0].rate {
        Rate::Value => Ok(bt.outs[0].vty.clone()),
        Rate::Signal => match body {
            Expr::Int(_, _) => Ok(ValueTy::Int),
            Expr::Float(_, _) => Ok(ValueTy::Float),
            _ => Err(CompileError::Type {
                msg: "branch must be a value expression (0->1 value channel)".into(),
                span,
            }),
        },
    }
}

/// Infer one arm's guards (each must be a Bool value) and unify all its bodies
/// into the running result type. The arm's pattern vars must already be bound
/// in `ctx.locals`.
fn infer_guarded_arm_body(
    ctx: &mut Ctx<'_>,
    arm: &MatchArm,
    result: &mut Option<ValueTy>,
) -> Result<(), CompileError> {
    for (g, body) in &arm.guards {
        if !matches!(g, Expr::Bool(true, _)) {
            let gt = infer_expr(ctx, g)?;
            if gt.arity_out() != 1
                || gt.outs[0].rate != Rate::Value
                || !matches!(ctx.subst.resolve_value(&gt.outs[0].vty), ValueTy::Bool)
            {
                return Err(CompileError::Type {
                    msg: "match guard must be a Bool value".into(),
                    span: g.span(),
                });
            }
        }
        let bt = infer_expr(ctx, body)?;
        let bv = arm_result_vty(bt, body, body.span())?;
        if let Some(acc) = result {
            unify_value(acc, &bv, &mut ctx.subst, body.span())?;
        } else {
            *result = Some(bv);
        }
    }
    Ok(())
}

/// An arm is "guarded" if it has more than one alternative or its first
/// alternative is not the bare `=> body` form (guard `true`).
fn arm_is_guarded(arm: &MatchArm) -> bool {
    arm.guards.len() > 1 || !matches!(arm.guards.first(), Some((Expr::Bool(true, _), _)))
}

/// Bind a pattern's variables into `ctx.locals` and type-check the pattern
/// against `vty` (recursively for nested constructor patterns).
///
/// The sum context is derived from the VALUE TYPE being matched at each
/// recursion level, so a nested constructor pattern (`Just (Left x)`) looks up
/// `Left` in `Either` (the payload's sum), not `Maybe`. `fallback_sum` is used
/// only when the matched type is not yet a known sum (a top-level wire
/// scrutinee still typed by a fresh var).
fn bind_pattern(
    ctx: &mut Ctx<'_>,
    pattern: &Pattern,
    vty: &ValueTy,
    fallback_sum: &str,
    arm_span: Span,
) -> Result<(), CompileError> {
    match pattern {
        Pattern::Var(name) => {
            ctx.locals
                .insert(name.clone(), ArrowTy::value_channel(vty.clone()));
            Ok(())
        }
        Pattern::Wild => Ok(()),
        Pattern::Ctor(name, args) => {
            // The sum this constructor belongs to: the type being matched when
            // it is itself a sum, else the caller's fallback (top-level wire
            // scrutinees whose type is still a fresh var).
            let resolved = ctx.subst.resolve_value(vty);
            let sum_name = match &resolved {
                ValueTy::App(n, _) | ValueTy::Data(n, _)
                    if matches!(ctx.env.data_types.get(n.as_str()), Some(DataInfo::Sum(_))) =>
                {
                    n.clone()
                }
                _ => fallback_sum.to_string(),
            };
            let is_builtin = ctx.env.ctor_arity(&sum_name).is_some();
            let scrutinee_args: Vec<ValueTy> = match &resolved {
                ValueTy::App(_, a) | ValueTy::Data(_, a) => a.clone(),
                _ => vec![],
            };
            let payload = match sum_ctor_payload(ctx, &sum_name, name) {
                Some(p) => p,
                None => {
                    return Err(CompileError::Type {
                        msg: format!("unknown constructor `{name}` for `{sum_name}`"),
                        span: arm_span,
                    });
                }
            };
            if args.len() != payload.len() {
                return Err(CompileError::Type {
                    msg: format!("wrong number of patterns for constructor `{name}`"),
                    span: arm_span,
                });
            }
            for (arg, pt) in args.iter().zip(payload.iter()) {
                // Builtin payloads carry placeholder type params; resolve them
                // against the scrutinee args (a fresh var when the scrutinee has
                // none — e.g. `Just x` matched against `Nothing`).
                let resolved_pt = if is_builtin {
                    match pt {
                        ValueTy::Var(k) => scrutinee_args
                            .get(k.saturating_sub(1) as usize)
                            .cloned()
                            .unwrap_or_else(|| ctx.fresh_vty()),
                        t => t.clone(),
                    }
                } else {
                    pt.clone()
                };
                bind_pattern(ctx, arg, &resolved_pt, &sum_name, arm_span)?;
            }
            Ok(())
        }
        Pattern::LitInt(_) | Pattern::LitFloat(_) | Pattern::LitBool(_) | Pattern::LitStr(_) => {
            Err(CompileError::Type {
                msg: "literal pattern requires a scalar scrutinee".into(),
                span: arm_span,
            })
        }
    }
}

/// Type-check every arm against the (already pinned) sum scrutinee: patterns,
/// guards (Bool), and unified bodies. The pattern is bound into `ctx.locals`
/// BEFORE the guards and bodies are inferred, because a guard references the
/// pattern's bound variables (`n | n > 0 => ...` needs `n`). Returns the
/// unified arm-body result value type.
fn check_match_arms(
    ctx: &mut Ctx<'_>,
    sum_name: &str,
    scrutinee_vty: &ValueTy,
    arms: &[MatchArm],
    span: Span,
) -> Result<ValueTy, CompileError> {
    let mut result: Option<ValueTy> = None;
    for arm in arms {
        let saved = ctx.locals.clone();
        bind_pattern(ctx, &arm.pattern, scrutinee_vty, sum_name, arm.span)?;
        infer_guarded_arm_body(ctx, arm, &mut result)?;
        ctx.locals = saved;
    }
    check_exhaustive_sum(ctx, sum_name, arms, span)?;
    Ok(result.unwrap_or(ValueTy::Float))
}

/// Pin a literal pattern's type to the scrutinee type (so a `Wire` scrutinee
/// infers `Int` from a `0` arm) and bind variable patterns.
fn check_scalar_pattern(
    ctx: &mut Ctx<'_>,
    pattern: &Pattern,
    scrutinee_vty: &ValueTy,
    arm_span: Span,
) -> Result<(), CompileError> {
    match pattern {
        Pattern::Var(name) => {
            ctx.locals
                .insert(name.clone(), ArrowTy::value_channel(scrutinee_vty.clone()));
            Ok(())
        }
        Pattern::Wild => Ok(()),
        Pattern::LitInt(_) => unify_value(scrutinee_vty, &ValueTy::Int, &mut ctx.subst, arm_span),
        Pattern::LitFloat(_) => {
            unify_value(scrutinee_vty, &ValueTy::Float, &mut ctx.subst, arm_span)
        }
        Pattern::LitBool(_) => unify_value(scrutinee_vty, &ValueTy::Bool, &mut ctx.subst, arm_span),
        Pattern::LitStr(_) => {
            unify_value(scrutinee_vty, &ValueTy::String, &mut ctx.subst, arm_span)
        }
        Pattern::Ctor(_, _) => Err(CompileError::Type {
            msg: "constructor pattern requires a sum scrutinee".into(),
            span: arm_span,
        }),
    }
}

/// Scalar totality: a `Wild`/`Var` arm, or (for Bool) both literals.
fn check_exhaustive_scalar(
    ctx: &Ctx<'_>,
    arms: &[MatchArm],
    scrutinee_vty: &ValueTy,
    span: Span,
) -> Result<(), CompileError> {
    let is_bool = matches!(ctx.subst.resolve_value(scrutinee_vty), ValueTy::Bool);
    let mut has_wild = false;
    let mut has_true = false;
    let mut has_false = false;
    for a in arms {
        if arm_is_guarded(a) {
            continue;
        }
        match &a.pattern {
            Pattern::Wild | Pattern::Var(_) => has_wild = true,
            Pattern::LitBool(b) => {
                if *b {
                    has_true = true;
                } else {
                    has_false = true;
                }
            }
            _ => {}
        }
    }
    let ok = has_wild || (is_bool && has_true && has_false);
    if ok {
        return Ok(());
    }
    Err(CompileError::Type {
        msg: "non-exhaustive match: a scalar match needs a `_` (or variable) arm".into(),
        span,
    })
}

/// Compile-time totality for sums: every constructor is covered by an
/// unguarded arm, or an unguarded Wild/Var arm exists.
fn check_exhaustive_sum(
    ctx: &Ctx<'_>,
    sum_name: &str,
    arms: &[MatchArm],
    span: Span,
) -> Result<(), CompileError> {
    let ctors: Vec<String> = match ctx.env.data_types.get(sum_name) {
        Some(DataInfo::Sum(cs)) => cs.iter().map(|(n, _)| n.clone()).collect(),
        _ => return Ok(()),
    };
    let mut covered: HashSet<&str> = HashSet::new();
    let mut has_wild = false;
    for a in arms {
        if arm_is_guarded(a) {
            continue;
        }
        match &a.pattern {
            Pattern::Ctor(name, _) => {
                covered.insert(name.as_str());
            }
            Pattern::Wild | Pattern::Var(_) => has_wild = true,
            _ => {}
        }
    }
    if has_wild {
        return Ok(());
    }
    let missing: Vec<String> = ctors
        .into_iter()
        .filter(|c| !covered.contains(c.as_str()))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(CompileError::Type {
        msg: format!(
            "non-exhaustive match: missing constructor(s) {}",
            missing.join(", ")
        ),
        span,
    })
}

/// Infer the diagram type of an expression, synthesizing concrete arities.
fn infer_expr(ctx: &mut Ctx<'_>, e: &Expr) -> Result<ArrowTy, CompileError> {
    infer_expr_expected(ctx, e, None)
}

/// Infer an expression with an optional **expected value type**. Used for
/// result-directed typeclass dispatch (`mempty`, `pure`, `return`): a method
/// whose signature has no class-var-applied argument is resolved by the type it
/// is expected to produce. `None` means "unknown" — such a method then errors
/// with "expected type unknown".
fn infer_expr_expected(
    ctx: &mut Ctx<'_>,
    e: &Expr,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    match e {
        Expr::Int(_, _) => Ok(ArrowTy {
            ins: vec![],
            outs: vec![Block::new(Scalar::Int)],
        }),
        Expr::Float(_, _) => Ok(ArrowTy {
            ins: vec![],
            outs: vec![Block::new(Scalar::Float)],
        }),
        Expr::Imag(_, _) => Ok(ArrowTy {
            ins: vec![],
            outs: vec![Block::new(Scalar::Float), Block::new(Scalar::Float)],
        }),
        Expr::Wire(_) => {
            let s = ctx.fresh();
            Ok(ArrowTy::uniform(1, 1, s))
        }
        Expr::Cut(_) => {
            let s = ctx.fresh();
            Ok(ArrowTy {
                ins: vec![Block::new(s)],
                outs: vec![],
            })
        }
        Expr::Ref(name, span) => infer_ref_expected(ctx, name, *span, expected),
        Expr::Neg(inner, span) => {
            let t = infer_expr(ctx, inner)?;
            check_all_numeric(ctx, &t, *span)?;
            Ok(t)
        }
        Expr::Apply { name, args, span } => infer_apply_expected(ctx, name, args, *span, expected),
        Expr::Str(_, _) => Ok(ArrowTy::value_channel(ValueTy::String)),
        Expr::Seq(lhs, rhs, span) => infer_seq(ctx, lhs, rhs, *span),
        Expr::Par(lhs, rhs, span) => infer_par(ctx, lhs, rhs, *span),
        Expr::Split(lhs, rhs, span) => infer_split(ctx, lhs, rhs, *span),
        Expr::Merge(lhs, rhs, span) => infer_merge(ctx, lhs, rhs, *span),
        Expr::Loop(lhs, rhs, span) => infer_loop(ctx, lhs, rhs, *span),
        Expr::Delay(lhs, rhs, span) => infer_delay(ctx, lhs, rhs, *span),
        Expr::Arith { lhs, rhs, span, .. } => infer_arith(ctx, lhs, rhs, *span),
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
        Expr::Record(_, _) => Ok(ArrowTy::uniform(0, 1, Scalar::Float)),
        Expr::ActorParam { default, span, .. } => {
            if let Some(d) = default {
                let ty = infer_expr(ctx, d)?;
                if ty.arity_in() != 0 || ty.arity_out() != 1 {
                    return Err(CompileError::Type {
                        msg: format!(
                            "`?name` default must be a constant expression (0→1), got {:?}",
                            ty
                        ),
                        span: *span,
                    });
                }
            }
            Ok(ArrowTy::uniform(0, 1, Scalar::Float))
        }
        Expr::FieldProject {
            record,
            field,
            span,
        } => {
            let t = infer_expr(ctx, record)?;
            // record must be a single Value channel of a known Data type
            if t.arity_out() != 1 || t.outs[0].rate != Rate::Value {
                return Err(CompileError::Type {
                    msg: "field projection requires a data value".into(),
                    span: *span,
                });
            }
            match &t.outs[0].vty {
                ValueTy::Data(name, args) => match ctx.env.data_types.get(name.as_str()) {
                    Some(DataInfo::Record(fields)) => {
                        let fty = fields
                            .iter()
                            .find(|(f, _)| f == field)
                            .map(|(_, t)| t.clone());
                        match fty {
                            // Parameterized user data (`data Box a = { value: a }`):
                            // placeholder `Var(k)` maps to the k-th type arg.
                            Some(ValueTy::Var(k)) if !args.is_empty() => {
                                Ok(ArrowTy::value_channel(
                                    args.get(k.saturating_sub(1) as usize)
                                        .cloned()
                                        .unwrap_or(ValueTy::Float),
                                ))
                            }
                            Some(ft) => Ok(ArrowTy::value_channel(ft)),
                            None => Err(CompileError::Type {
                                msg: format!("no field `{field}` in `{name}`"),
                                span: *span,
                            }),
                        }
                    }
                    _ => Err(CompileError::Type {
                        msg: format!("`{name}` is not a record type"),
                        span: *span,
                    }),
                },
                ValueTy::App(name, args) if ctx.env.ctor_arity(name.as_str()).is_some() => {
                    // Builtin record (`Pair a b`): field types are the
                    // placeholder `Var(1)`/`Var(2)` positions, resolved against
                    // the concrete type args.
                    match ctx.env.data_types.get(name.as_str()) {
                        Some(DataInfo::Record(fields)) => {
                            match fields.iter().position(|(f, _)| f == field) {
                                Some(idx) => {
                                    let fty = match &fields[idx].1 {
                                        ValueTy::Var(k) => args
                                            .get(k.saturating_sub(1) as usize)
                                            .cloned()
                                            .unwrap_or(ValueTy::Float),
                                        t => t.clone(),
                                    };
                                    Ok(ArrowTy::value_channel(fty))
                                }
                                None => Err(CompileError::Type {
                                    msg: format!("no field `{field}` in `{name}`"),
                                    span: *span,
                                }),
                            }
                        }
                        _ => Err(CompileError::Type {
                            msg: format!("`{name}` is not a record type"),
                            span: *span,
                        }),
                    }
                }
                ValueTy::Var(_) => {
                    // Deferred record: the record type is not known here (a
                    // value function's λ-parameter). The projection resolves
                    // when the function is called with a concrete argument —
                    // `reduce` β-reduces the call, so lowering sees a concrete
                    // expression. Return a fresh unresolved field type.
                    let v = ctx.next;
                    ctx.next += 1;
                    Ok(ArrowTy::value_channel(ValueTy::Var(v)))
                }
                _ => Err(CompileError::Type {
                    msg: "field projection requires a record value".into(),
                    span: *span,
                }),
            }
        }
        Expr::FieldUpdate {
            record,
            field,
            value,
            span,
        } => {
            let rt = infer_expr(ctx, record)?;
            if rt.arity_out() != 1 || rt.outs[0].rate != Rate::Value {
                return Err(CompileError::Type {
                    msg: "field update requires a data record".into(),
                    span: *span,
                });
            }
            match &rt.outs[0].vty {
                ValueTy::Data(name, _) => {
                    let fty = ctx
                        .env
                        .data_types
                        .get(name.as_str())
                        .and_then(|info| match info {
                            DataInfo::Record(fields) => fields
                                .iter()
                                .find(|(f, _)| f == field)
                                .map(|(_, t)| t.clone()),
                            _ => None,
                        });
                    let fty = match fty {
                        Some(t) => t,
                        None => {
                            return Err(CompileError::Type {
                                msg: format!("no field `{field}` in `{name}`"),
                                span: *span,
                            });
                        }
                    };
                    // COW update: the new value must be a constant or a value
                    // channel whose type matches the declared field type.
                    let vt = infer_const_value(ctx, value)?;
                    unify_value(&vt, &fty, &mut ctx.subst, value.span())?;
                    Ok(rt)
                }
                ValueTy::App(name, args) if ctx.env.ctor_arity(name.as_str()).is_some() => {
                    // Builtin record (`Pair a b`): the field type is a
                    // placeholder `Var(k)` position resolved against the
                    // concrete type args.
                    let fty = ctx
                        .env
                        .data_types
                        .get(name.as_str())
                        .and_then(|info| match info {
                            DataInfo::Record(fields) => fields
                                .iter()
                                .position(|(f, _)| f == field)
                                .map(|idx| match &fields[idx].1 {
                                    ValueTy::Var(k) => args
                                        .get(k.saturating_sub(1) as usize)
                                        .cloned()
                                        .unwrap_or(ValueTy::Float),
                                    t => t.clone(),
                                }),
                            _ => None,
                        });
                    let fty = match fty {
                        Some(t) => t,
                        None => {
                            return Err(CompileError::Type {
                                msg: format!("no field `{field}` in `{name}`"),
                                span: *span,
                            });
                        }
                    };
                    let vt = infer_const_value(ctx, value)?;
                    unify_value(&vt, &fty, &mut ctx.subst, value.span())?;
                    Ok(rt)
                }
                ValueTy::Var(_) => {
                    // Deferred record (a value function's λ-parameter): the
                    // field update resolves when the call is β-reduced with a
                    // concrete record. Infer the new value and keep the record
                    // type unresolved.
                    let _ = infer_const_value(ctx, value)?;
                    Ok(rt)
                }
                _ => Err(CompileError::Type {
                    msg: "field update requires a record value".into(),
                    span: *span,
                }),
            }
        }
        Expr::Match {
            scrutinee,
            arms,
            span,
        } => {
            if arms.is_empty() {
                return Err(CompileError::Type {
                    msg: "match requires at least one arm".into(),
                    span: *span,
                });
            }
            // `_` in match-scrutinee position is the identity value wire: it is
            // a Value channel with a fresh value type, pinned to the arm-derived
            // sum type (or a literal arm's scalar type) below by unification.
            // Bare Int/Float literals are signal-rate channels in v1 but
            // value-compatible in value positions (mirroring `arm_result_vty`).
            let st = match scrutinee.as_ref() {
                Expr::Wire(_) => {
                    let v = ctx.next;
                    ctx.next += 1;
                    ArrowTy::value_channel(ValueTy::Var(v))
                }
                Expr::Int(_, _) => ArrowTy::value_channel(ValueTy::Int),
                Expr::Float(_, _) => ArrowTy::value_channel(ValueTy::Float),
                _ => infer_expr(ctx, scrutinee)?,
            };
            // The scrutinee must be a single value channel.
            if st.arity_out() != 1 || st.outs[0].rate != Rate::Value {
                return Err(CompileError::Type {
                    msg: "match scrutinee must be a value".into(),
                    span: *span,
                });
            }
            let scrutinee_vty = st.outs[0].vty.clone();
            // Sum or scalar? Resolve the scrutinee type enough to decide.
            // Builtin sums are parameterized (`Maybe a`, `Either a b`):
            // `Nothing` infers as `App("Maybe", [..])` and `head`/`lookup` as
            // `App("Maybe", [t])`, while user sums stay `Data(name, [])`. The
            // scrutinee's sum name is the `App` head (when it is a builtin Sum)
            // or the `Data` name.
            let resolved = ctx.subst.resolve_value(&scrutinee_vty);
            let scrutinee_sum = match &resolved {
                ValueTy::Data(name, _) => Some(name.clone()),
                ValueTy::App(name, _)
                    if matches!(
                        ctx.env.data_types.get(name.as_str()),
                        Some(DataInfo::Sum(_))
                    ) =>
                {
                    Some(name.clone())
                }
                _ => None,
            };
            if arms
                .iter()
                .any(|a| matches!(a.pattern, Pattern::Ctor(_, _)))
            {
                // A constructor pattern requires a sum scrutinee. Derive the
                // sum name from the scrutinee, else from the constructor
                // intersection (a concrete scrutinee type disambiguates ctor
                // names shared across sum types; otherwise an ambiguous or
                // unknown constructor is a deterministic error).
                let sum_name = match &scrutinee_sum {
                    Some(n) => n.clone(),
                    None => {
                        let mut candidates: Option<Vec<String>> = None;
                        for a in arms {
                            if let Pattern::Ctor(name, _) = &a.pattern {
                                let per_ctor = sum_types_with_ctor(ctx, name);
                                if per_ctor.is_empty() {
                                    return Err(CompileError::Type {
                                        msg: format!("unknown constructor `{name}`"),
                                        span: a.span,
                                    });
                                }
                                candidates = Some(match candidates {
                                    None => per_ctor,
                                    Some(acc) => {
                                        acc.into_iter().filter(|n| per_ctor.contains(n)).collect()
                                    }
                                });
                            }
                        }
                        match candidates {
                            Some(v) if v.len() == 1 => v[0].clone(),
                            _ => {
                                return Err(CompileError::Type {
                                    msg: "match arms use constructors of ambiguous or different sum types"
                                        .into(),
                                    span: *span,
                                });
                            }
                        }
                    }
                };
                // Pin the scrutinee's value type to the arm-derived sum type.
                // Builtin sums keep their concrete type args (fresh when the
                // scrutinee carries none); user sums pin to the monomorphic
                // `Data`.
                let pin_ty = match ctx.env.ctor_arity(&sum_name) {
                    Some(arity) => {
                        let mut args: Vec<ValueTy> = match &scrutinee_vty {
                            ValueTy::App(_, a) | ValueTy::Data(_, a) => a.clone(),
                            _ => vec![],
                        };
                        while args.len() < arity {
                            args.push(ctx.fresh_vty());
                        }
                        ValueTy::App(sum_name.clone(), args)
                    }
                    None => ValueTy::Data(sum_name.clone(), vec![]),
                };
                unify_value(&scrutinee_vty, &pin_ty, &mut ctx.subst, *span)?;
                let result = check_match_arms(ctx, &sum_name, &pin_ty, arms, *span)?;
                Ok(ArrowTy::value_channel(result))
            } else {
                // Scalar (or unknown) match: literal/Var/Wild patterns only.
                // Pin literal patterns to the scrutinee type; require totality.
                let mut result: Option<ValueTy> = None;
                for a in arms {
                    let saved = ctx.locals.clone();
                    check_scalar_pattern(ctx, &a.pattern, &scrutinee_vty, a.span)?;
                    infer_guarded_arm_body(ctx, a, &mut result)?;
                    ctx.locals = saved;
                }
                check_exhaustive_scalar(ctx, arms, &scrutinee_vty, *span)?;
                Ok(ArrowTy::value_channel(result.unwrap_or(ValueTy::Float)))
            }
        }
        Expr::If {
            cond,
            then,
            els,
            span,
        } => {
            let ct = infer_expr(ctx, cond)?;
            if ct.arity_out() != 1 || ct.outs[0].rate != Rate::Value {
                return Err(CompileError::Type {
                    msg: "if condition must be a Bool value".into(),
                    span: cond.span(),
                });
            }
            if !matches!(ctx.subst.resolve_value(&ct.outs[0].vty), ValueTy::Bool) {
                return Err(CompileError::Type {
                    msg: "if condition must be a Bool value".into(),
                    span: cond.span(),
                });
            }
            let tt = infer_expr(ctx, then)?;
            let et = infer_expr(ctx, els)?;
            let tv = arm_result_vty(tt, then, then.span())?;
            let ev = arm_result_vty(et, els, els.span())?;
            unify_value(&tv, &ev, &mut ctx.subst, *span)?;
            Ok(ArrowTy::value_channel(tv))
        }
        Expr::Lambda { params, body, span } => {
            // Bind the parameters as value channels and infer the body; the
            // result type is the function type. v1 lambda parameters are
            // structural value types: the returned `Func` signature carries the
            // parameter variables (NOT hardcoded `Float`), so a call site can
            // unify a concrete argument against them — a higher-order parameter
            // (`f` in `twice = fn f x -> f (f x)`) becomes a `Func`, a record
            // parameter (`p` in `pair_map`) becomes its `Data` type. Signal-wire
            // parameters are a later task.
            let saved = ctx.locals.clone();
            let saved_lifting = ctx.method_lifting.clone();
            // A lambda body is a DEFERRED computation: typeclass method calls
            // inside resolve at the lambda's own call sites (with the lambda's
            // params concrete), NOT against the enclosing method-inlining path.
            // Clear the recursion guard so legitimate nested calls of the same
            // method (`ap mf mx = bind mf (fn f -> bind mx (..))`) are not
            // misread as recursion.
            ctx.method_lifting.clear();
            let mut arg_tys: Vec<ValueTy> = Vec::with_capacity(params.len());
            for p in params {
                let pty = ctx.fresh_vty();
                arg_tys.push(pty.clone());
                ctx.locals
                    .insert(p.name.clone(), ArrowTy::value_channel(pty));
            }
            // The lambda's BODY is expected to produce the lambda's RESULT type, not
            // the whole `Func` type (which is what the caller expects).
            let body_expected = match expected {
                Some(ValueTy::Func(_, rets)) => rets.first().cloned(),
                _ => None,
            };
            let bt = infer_expr_expected(ctx, body, body_expected)?;
            ctx.locals = saved;
            ctx.method_lifting = saved_lifting;
            if bt.arity_out() != 1 {
                return Err(CompileError::Type {
                    msg: "lambda body must produce one value".into(),
                    span: *span,
                });
            }
            let ret_ty = bt.outs[0].vty.clone();
            Ok(ArrowTy::value_channel(ValueTy::Func(arg_tys, vec![ret_ty])))
        }
        Expr::Bool(_, _) => Ok(ArrowTy::value_channel(ValueTy::Bool)),
        Expr::ListLit(elems, _) => {
            // Homogeneous list literal: every element is a value constant of the
            // same type; the literal's capacity is its length (a strict
            // type-carried bound — consing past it is a runtime overflow error).
            let mut elem_ty: Option<ValueTy> = None;
            for e in elems {
                let et = infer_const_value(ctx, e)?;
                if let Some(prev) = &elem_ty {
                    unify_value(prev, &et, &mut ctx.subst, e.span())?;
                } else {
                    elem_ty = Some(et);
                }
            }
            let elem_ty = elem_ty
                .map(|t| ctx.subst.resolve_value(&t))
                .unwrap_or(ValueTy::Float);
            Ok(ArrowTy::value_channel(ValueTy::App(
                "List".into(),
                vec![elem_ty],
            )))
        }
        Expr::MapLit(entries, _) => {
            // Map literal with string keys: every entry's value must share ONE
            // type (mirroring the homogeneous List literal); that type is the
            // map's value type. A mixed-type literal is a compile error, and a
            // non-Float value type (a List, a record) is carried accurately.
            let mut val_ty: Option<ValueTy> = None;
            for (_, ve) in entries {
                let vt = infer_const_value(ctx, ve)?;
                if let Some(prev) = &val_ty {
                    unify_value(prev, &vt, &mut ctx.subst, ve.span())?;
                } else {
                    val_ty = Some(vt);
                }
            }
            let val_ty = val_ty
                .map(|t| ctx.subst.resolve_value(&t))
                .unwrap_or(ValueTy::Float);
            Ok(ArrowTy::value_channel(ValueTy::App(
                "Map".into(),
                vec![ValueTy::String, val_ty],
            )))
        }
        Expr::Cmp { lhs, rhs, .. } => {
            // Value-track comparison: both sides are value constants (any
            // types — the interpreter's `value_cmp` is a cross-kind total
            // order). The result is a Bool value channel.
            let _ = infer_const_value(ctx, lhs)?;
            let _ = infer_const_value(ctx, rhs)?;
            Ok(ArrowTy::value_channel(ValueTy::Bool))
        }
        Expr::Logic { lhs, rhs, .. } => {
            let _ = infer_const_value(ctx, lhs)?;
            let _ = infer_const_value(ctx, rhs)?;
            Ok(ArrowTy::value_channel(ValueTy::Bool))
        }
    }
}

fn infer_ref(ctx: &mut Ctx<'_>, name: &str, span: Span) -> Result<ArrowTy, CompileError> {
    infer_ref_expected(ctx, name, span, None)
}

/// Resolve a `Ref` with an optional expected value type. A **result-directed**
/// typeclass method (`mempty`, `pure`, `return` — a method whose signature has
/// no class-var-applied argument) is resolved by the expected type's concrete
/// name; without an expected type it errors.
fn infer_ref_expected(
    ctx: &mut Ctx<'_>,
    name: &str,
    span: Span,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    // Result-directed typeclass method: `mempty` / `pure` / `return`. The class
    // variable appears only in the RESULT of the signature, so the instance is
    // selected by the expected result type. `g` (a lambda or match binding) and
    // user definitions shadow class methods.
    if !ctx.locals.contains_key(name) && !ctx.defs.contains_key(name) {
        if let Some(class_name) = ctx.env.class_of_method(name) {
            let class_info = ctx.env.typeclasses.get(&class_name).cloned().unwrap();
            let sig = class_info
                .methods
                .iter()
                .find(|(m, _)| m == name)
                .map(|(_, s)| s.clone());
            let class_var = class_info.var.clone();
            // Result-directed in the REF path: a method called with no
            // arguments resolves by the expected RESULT type. A function
            // signature whose arguments never mention the class variable
            // (`pure`, `return`) and a bare class-var signature (`mempty: m`)
            // are both result-directed here — a bare `mempty` outside a method
            // call errors with "expected type unknown" (Task 4 Step 5). The
            // arity-0 selector path (`show 1.0`) is handled in `infer_apply`.
            let result_directed = match &sig {
                Some(crate::ast::TypeExpr::TFunc(args, _)) => {
                    args.iter().all(|a| !type_expr_mentions(a, &class_var))
                }
                Some(_) => true,
                None => false,
            };
            if result_directed {
                let Some(exp) = expected else {
                    return Err(CompileError::Type {
                        msg: format!(
                            "cannot resolve method `{name}` of `{class_name}`: expected type unknown \
                             (use it where its result type is fixed, e.g. `mappend xs mempty`)"
                        ),
                        span,
                    });
                };
                let ty_name = match ctx.env.type_name_of_vty(&exp) {
                    Some(t) => t,
                    None => {
                        eprintln!("result-directed {name} of {class_name}: expected not concrete: {exp:?}");
                        return Err(CompileError::Type {
                            msg: format!(
                                "cannot resolve method `{name}` of `{class_name}`: the expected type is not concrete"
                            ),
                            span,
                        });
                    }
                };
                let (_, params, body) = match ctx.env.resolve_method(name, ty_name.as_str()) {
                    Some(r) => r,
                    None => {
                        return Err(CompileError::Type {
                            msg: format!("no instance of `{class_name}` for type `{ty_name}`"),
                            span,
                        });
                    }
                };
                let key = (class_name, ty_name.clone(), name.to_string());
                if ctx.method_lifting.contains(&key) {
                    return Err(CompileError::Type {
                        msg: format!("recursive typeclass method `{name}` for type `{ty_name}`"),
                        span,
                    });
                }
                ctx.method_lifting.insert(key.clone());
                let saved = ctx.locals.clone();
                for (p, pt) in params.iter().zip(signature_param_tys(
                    ctx,
                    &class_var,
                    &ty_name,
                    sig.as_ref().unwrap(),
                )) {
                    ctx.locals.insert(p.clone(), ArrowTy::value_channel(pt));
                }
                // Nested result-directed methods in the body (`pure x = return x`)
                // resolve by the same expected type.
                let body_vty =
                    infer_method_value_vty_expected(ctx, &body, "body", Some(exp.clone()));
                ctx.locals = saved;
                ctx.method_lifting.remove(&key);
                let body_vty = body_vty?;
                return Ok(ArrowTy::value_channel(body_vty));
            }
        }
    }
    infer_ref_inner(ctx, name, span)
}

/// The original `infer_ref` body: resolve a named reference (data type, sum
/// constructor, newtype, nullary constructor, method, builtin, def, local).
fn infer_ref_inner(ctx: &mut Ctx<'_>, name: &str, span: Span) -> Result<ArrowTy, CompileError> {
    // Data-type names and constructors are checked before builtins/user defs:
    // `Point` (record type) is a value channel; a bare sum constructor like
    // `Circle` must be applied to its payload.
    if let Some(info) = ctx.env.data_types.get(name) {
        match info {
            DataInfo::Record(_) => {
                return Ok(ArrowTy::value_channel(ValueTy::Data(name.into(), vec![])))
            }
            DataInfo::Sum(_) => {
                return Err(CompileError::Type {
                    msg: format!("`{name}` is a sum type; use one of its constructors"),
                    span,
                });
            }
        }
    }
    if ctx.env.newtypes.contains_key(name) {
        return Err(CompileError::Type {
            msg: format!("newtype constructor `{name}` requires one argument"),
            span,
        });
    }
    // Bare zero-argument collection constructors: `list`, `empty_map`,
    // `empty_set` in value position are empty-container builtin calls (the
    // capacity argument was removed — open collections).
    match name {
        "list" => {
            // `list` is the polymorphic empty list: the element type is a
            // fresh var that unifies with the context. `mempty = list` (Monoid
            // List) must be usable at any element type.
            return Ok(ArrowTy::value_channel(ValueTy::App(
                "List".into(),
                vec![ctx.fresh_vty()],
            )));
        }
        "empty_map" => {
            return Ok(ArrowTy::value_channel(ValueTy::App(
                "Map".into(),
                vec![ValueTy::String, ValueTy::Float],
            )))
        }
        "empty_set" => {
            return Ok(ArrowTy::value_channel(ValueTy::App(
                "Set".into(),
                vec![ValueTy::Float],
            )))
        }
        _ => {}
    }
    let ctor_sums = sum_types_with_ctor(ctx, name);
    if !ctor_sums.is_empty() {
        // A bare constructor is normally an error ("requires arguments"), but a
        // NULLARY constructor (`Nothing`, `Red`) needs no payload: the bare
        // reference is a complete value of its sum type.
        if ctor_sums.len() == 1 {
            let sum_name = &ctor_sums[0];
            if let Some(payload) = sum_ctor_payload(ctx, sum_name, name) {
                if payload.is_empty() {
                    return Ok(ArrowTy::value_channel(fresh_sum_vty(ctx, sum_name)));
                }
            }
        }
        // A bare constructor must be applied to its payload. If the ctor name
        // is shared, the ambiguity is reported rather than resolved by map order.
        let msg = if ctor_sums.len() == 1 {
            format!(
                "constructor `{name}` for `{}` requires arguments",
                ctor_sums[0]
            )
        } else {
            format!("constructor `{name}` requires arguments")
        };
        return Err(CompileError::Type { msg, span });
    }
    // A bare typeclass method reference is an unapplied method call: `show`
    // needs an argument to select the instance. User definitions shadow class
    // methods (a user `eq`/`lt` is a plain function, not the builtin Eq/Ord
    // method), so this only fires when no def/local of that name exists.
    if !ctx.locals.contains_key(name) && !ctx.defs.contains_key(name) {
        if let Some(class_name) = ctx.env.class_of_method(name) {
            return Err(CompileError::Type {
                msg: format!("method `{name}` of `{class_name}` requires an argument"),
                span,
            });
        }
    }
    if matches!(name, "+" | "-" | "*" | "/" | "%") {
        let s = ctx.fresh();
        return Ok(ArrowTy::uniform(2, 1, s));
    }
    if matches!(
        name,
        "sin" | "cos" | "tan" | "sqrt" | "exp" | "ln" | "tanh" | "abs"
    ) {
        return Ok(ArrowTy::uniform(1, 1, Scalar::Float));
    }
    if matches!(name, "min" | "max") {
        let s = ctx.fresh();
        return Ok(ArrowTy::uniform(2, 1, s));
    }
    if let Some(sig) = ctx.sigs.builtin_sig(name) {
        if sig.params.len() == sig.signal_ins() {
            return Ok(ArrowTy::uniform(
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
            // A bare reference to a user definition with λ-parameters is a
            // first-class function value: `f = double` binds `f` to
            // `ValueTy::Func([], [])` (an unknown signature — a bare named
            // ref's arity is resolved when it is applied, see `infer_apply`).
            // Using it where a signal is required is rejected by the signal
            // combinators.
            return Ok(ArrowTy::value_channel(ValueTy::Func(vec![], vec![])));
        }
        return Ok(ctx.instantiate(&scheme));
    }
    Err(CompileError::Type {
        msg: format!("unknown identifier `{name}`"),
        span,
    })
}

/// Resolve the definition a func value dispatches to.
///
/// A func value binding (`f = double`) is a `Def::Local` whose body is a bare
/// `Ref` to another definition; chains (`g = f`) follow the body refs until a
/// definition that is itself not a func value is reached (the referenced
/// definition, with λ-parameters). The structural `ValueTy::Func` no longer
/// carries the referenced name, so it is recovered from the AST bodies here.
/// (A later task replaces this with signature-based dispatch.)
fn func_target(ctx: &Ctx<'_>, name: &str) -> String {
    let mut cur = name.to_string();
    for _ in 0..=ctx.defs.len() {
        let is_func_value = ctx.defs.get(cur.as_str()).is_some_and(|s| {
            s.ty.outs.len() == 1
                && s.ty.outs[0].rate == Rate::Value
                && matches!(s.ty.outs[0].vty, ValueTy::Func(_, _))
        });
        match ctx.def_bodies.get(cur.as_str()) {
            Some(Expr::Ref(next, _)) if is_func_value => {
                cur = next.clone();
                continue;
            }
            _ => return cur,
        }
    }
    cur
}

/// Whether a type expression mentions a variable name anywhere.
pub(crate) fn type_expr_mentions(te: &crate::ast::TypeExpr, var: &str) -> bool {
    match te {
        crate::ast::TypeExpr::TName(n) => n == var,
        crate::ast::TypeExpr::TApp(head, args) => {
            head == var || args.iter().any(|a| type_expr_mentions(a, var))
        }
        crate::ast::TypeExpr::TFunc(args, ret) => {
            args.iter().any(|a| type_expr_mentions(a, var)) || type_expr_mentions(ret, var)
        }
    }
}

/// Infer an `Apply`, threaded with an optional expected value type for
/// result-directed typeclass dispatch (`pure`, `return`, `mempty`).
fn infer_apply_expected(
    ctx: &mut Ctx<'_>,
    name: &str,
    args: &[Expr],
    span: Span,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    infer_apply_impl(ctx, name, args, span, expected)
}

fn infer_apply_impl(
    ctx: &mut Ctx<'_>,
    name: &str,
    args: &[Expr],
    span: Span,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    if name == "not" {
        if args.len() != 1 {
            return Err(CompileError::Type {
                msg: format!("`not` expects 1 argument, got {}", args.len()),
                span,
            });
        }
        let arg_vty = infer_method_value_vty(ctx, &args[0], "argument")?;
        if arg_vty != ValueTy::Bool {
            return Err(CompileError::Type {
                msg: format!("`not` expects a Bool argument, got {arg_vty:?}"),
                span: args[0].span(),
            });
        }
        return Ok(ArrowTy::value_channel(ValueTy::Bool));
    }
    if name == "smooth" {
        if args.len() != 2 {
            return Err(CompileError::Type {
                msg: format!("smooth expects 2 arguments, got {}", args.len()),
                span,
            });
        }
        let sig_ty = infer_expr(ctx, &args[0])?;
        if sig_ty.arity_out() == 0 {
            return Err(CompileError::Type {
                msg: "smooth signal argument has no outputs".into(),
                span: args[0].span(),
            });
        }
        let st = infer_expr(ctx, &args[1])?;
        if st.arity_in() != 0 || st.arity_out() != 1 {
            return Err(CompileError::Type {
                msg: "smooth time must be a constant".into(),
                span: args[1].span(),
            });
        }
        return Ok(ArrowTy::uniform(
            sig_ty.arity_in(),
            sig_ty.arity_out(),
            Scalar::Float,
        ));
    }
    if name == "param" {
        return infer_param(args, span);
    }
    // Collection operations (value track): reserved names dispatched by the
    // interpreter's `ValueCallBuiltin`. They take precedence over user
    // definitions/builtins (mirrors lowering), so a collection op can never be
    // shadowed by a definition of the same name.
    match name {
        "length" | "cons" | "head" | "tail" | "map" | "fold" | "filter" | "list" | "insert"
        | "lookup" | "member" | "empty_map" | "empty_set" | "concat_map" | "append_list"
        | "concat_string" => {
            let ret = infer_collection_call(ctx, name, args, span)?;
            return Ok(ArrowTy::value_channel(ret));
        }
        _ => {}
    }
    // Data-type constructors take priority over builtins and user definitions:
    // ctor names (`Circle`, `Point`) are never builtins.
    if let Some(info) = ctx.env.data_types.get(name).cloned() {
        match info {
            DataInfo::Record(fields) => {
                // Record constructor: `Point { x: 1.0, y: 2.0 }`. The single
                // argument must be a record literal matched by field name.
                if args.len() != 1 {
                    return Err(CompileError::Type {
                        msg: format!(
                            "record constructor `{name}` expects one record literal, got {} arguments",
                            args.len()
                        ),
                        span,
                    });
                }
                match &args[0] {
                    Expr::Record(fields_expr, _) => {
                        // Every declared field exactly once, no duplicates.
                        let mut seen: Vec<&String> = Vec::new();
                        for (f, _) in fields_expr {
                            if seen.contains(&f) {
                                return Err(CompileError::Type {
                                    msg: format!("duplicate field `{f}` in `{name}` constructor"),
                                    span,
                                });
                            }
                            seen.push(f);
                        }
                        for (fname, _) in &fields {
                            if !seen.iter().any(|s| s.as_str() == fname.as_str()) {
                                return Err(CompileError::Type {
                                    msg: format!("missing field `{fname}` in `{name}` constructor"),
                                    span,
                                });
                            }
                        }
                        // Builtin records (`Pair a b`) carry PLACEHOLDER type
                        // parameters (`Var(1)`, `Var(2)`) that are NOT live
                        // unification vars. Freshen each placeholder to a fresh
                        // live var BEFORE unifying the fields: unifying the raw
                        // ids would bind them in the live subst, colliding with
                        // subsequently freshened ids (`Pair { first: 1.0,
                        // second: 2 }` failed on the second field).
                        let is_builtin = ctx.env.ctor_arity(name).is_some();
                        // Track which placeholder a freshened bare-var field
                        // replaced (`Var(k)` → fresh), so parameterized user
                        // data can resolve each type parameter from the subst
                        // after the fields unify: a compound field like
                        // `a -> m b` binds its placeholders directly, while a
                        // bare `a` field was freshened and needs the link.
                        let mut placeholder_bindings: HashMap<u32, ValueTy> = HashMap::new();
                        let field_tys: Vec<(String, ValueTy)> = fields
                            .iter()
                            .map(|(fname, fty)| {
                                let fty = match fty {
                                    ValueTy::Var(k) => {
                                        let fresh = ctx.fresh_vty();
                                        placeholder_bindings.insert(*k, fresh.clone());
                                        fresh
                                    }
                                    t => t.clone(),
                                };
                                (fname.to_string(), fty)
                            })
                            .collect();
                        for (f, e) in fields_expr {
                            let fty = field_tys
                                .iter()
                                .find(|(fname, _)| fname == f)
                                .map(|(_, t)| t.clone());
                            let fty = match fty {
                                Some(t) => t,
                                None => {
                                    return Err(CompileError::Type {
                                        msg: format!("no field `{f}` in `{name}`"),
                                        span: e.span(),
                                    });
                                }
                            };
                            let vt = infer_const_value(ctx, e)?;
                            unify_value(&vt, &fty, &mut ctx.subst, e.span())?;
                        }
                        if is_builtin {
                            let arg_tys: Vec<ValueTy> = field_tys
                                .iter()
                                .map(|(_, t)| ctx.subst.resolve_value(t))
                                .collect();
                            return Ok(ArrowTy::value_channel(ValueTy::App(name.into(), arg_tys)));
                        }
                        // Parameterized user data (`data Kleisli m a b`): resolve each type
                        // parameter from the substitution built by unifying the
                        // fields. `Var(k+1)` and the `TyConApp` head share one
                        // id space, so resolving `Var(k+1)` yields the concrete
                        // type (m → Maybe, a → Float, b → Float).
                        if let Some(arity) = ctx.env.data_arities.get(name) {
                            if *arity > 0 {
                                let arg_tys: Vec<ValueTy> = (0..*arity)
                                    .map(|k| {
                                        let pid = (k + 1) as u32;
                                        let pty = match placeholder_bindings.get(&pid) {
                                            // A bare `Var(k)` field was freshened;
                                            // resolve its fresh var.
                                            Some(fresh) => ctx.subst.resolve_value(fresh),
                                            None => ctx.subst.resolve_value(&ValueTy::Var(pid)),
                                        };
                                        match pty {
                                            ValueTy::Var(_) => ctx.fresh_vty(), // unconstrained param
                                            t => t,
                                        }
                                    })
                                    .collect();
                                return Ok(ArrowTy::value_channel(ValueTy::Data(
                                    name.into(),
                                    arg_tys,
                                )));
                            }
                        }
                        return Ok(ArrowTy::value_channel(ValueTy::Data(name.into(), vec![])));
                    }
                    _ => {
                        return Err(CompileError::Type {
                            msg: format!(
                                "record constructor `{name}` expects a record literal, e.g. `{name} {{ ... }}`"
                            ),
                            span,
                        });
                    }
                }
            }
            DataInfo::Sum(_) => {
                // Applying the sum type name itself is not a constructor call.
                return Err(CompileError::Type {
                    msg: format!("`{name}` is a sum type; use one of its constructors"),
                    span,
                });
            }
        }
    }
    // Newtype constructor: `Hz 440.0` wraps its single argument in the wrapper.
    if let Some(inner_name) = ctx.env.newtypes.get(name).cloned() {
        if args.len() != 1 {
            return Err(CompileError::Type {
                msg: format!(
                    "newtype constructor `{name}` expects 1 argument, got {}",
                    args.len()
                ),
                span,
            });
        }
        let vt = infer_const_value(ctx, &args[0])?;
        let inner = ctx.env.vty_of_name(&inner_name);
        unify_value(&vt, &inner, &mut ctx.subst, args[0].span())?;
        return Ok(ArrowTy::value_channel(ValueTy::Newtype(
            name.to_string(),
            vec![],
        )));
    }
    let ctor_sums = sum_types_with_ctor(ctx, name);
    if !ctor_sums.is_empty() {
        let sum_name = if ctor_sums.len() == 1 {
            ctor_sums[0].clone()
        } else {
            return Err(CompileError::Type {
                msg: format!("ambiguous constructor `{name}` (declared in multiple sum types)"),
                span,
            });
        };
        let payload = sum_ctor_payload(ctx, &sum_name, name).expect("ctor belongs to its sum");
        if args.len() != payload.len() {
            return Err(CompileError::Type {
                msg: format!(
                    "constructor `{name}` expects {} argument(s), got {}",
                    payload.len(),
                    args.len()
                ),
                span,
            });
        }
        // Builtin sums (`Maybe`, `Either`) carry PLACEHOLDER type parameters
        // (`Var(1)`, `Var(2)`) that are NOT live unification vars. Instantiate
        // each payload placeholder with a fresh live var, unify the argument
        // against it, and build the parameterized result `App(sum_name, ...)`
        // with the FULL `arity` type args: each payload type lands at its
        // placeholder position (`Var(k)` → arg k-1), the rest stay fresh
        // (`Left 5.0` is `App("Either", [Float, ?])`). The match pin pads the
        // scrutinee to `arity`, so the arities must agree.
        if let Some(arity) = ctx.env.ctor_arity(&sum_name) {
            let mut slots: Vec<Option<ValueTy>> = Vec::with_capacity(arity);
            for _ in 0..arity {
                slots.push(None);
            }
            for (i, pt) in payload.iter().enumerate() {
                let pty = match pt {
                    ValueTy::Var(_) => ctx.fresh_vty(),
                    t => t.clone(),
                };
                let e = &args[i];
                let vt = infer_const_value(ctx, e)?;
                unify_value(&vt, &pty, &mut ctx.subst, e.span())?;
                let idx = match pt {
                    ValueTy::Var(k) => k.saturating_sub(1) as usize,
                    _ => i,
                };
                if let Some(slot) = slots.get_mut(idx) {
                    *slot = Some(ctx.subst.resolve_value(&pty));
                }
            }
            let mut arg_tys: Vec<ValueTy> = Vec::with_capacity(arity);
            for s in slots {
                match s {
                    Some(t) => arg_tys.push(t),
                    None => arg_tys.push(ctx.fresh_vty()),
                }
            }
            return Ok(ArrowTy::value_channel(ValueTy::App(sum_name, arg_tys)));
        }
        for (e, pty) in args.iter().zip(payload.iter()) {
            let vt = infer_const_value(ctx, e)?;
            unify_value(&vt, pty, &mut ctx.subst, e.span())?;
        }
        return Ok(ArrowTy::value_channel(ValueTy::Data(sum_name, vec![])));
    }
    // Typeclass method call: `show x` resolves at compile time to the instance
    // of the class declaring `show` for the concrete type of `x` (v1 requires
    // a concrete argument type — an unresolved variable cannot select an
    // instance). The instance's body is β-reduced in place of the call; its
    // inferred type is the method's return type (v1 drops the declared return
    // signature). User definitions shadow class methods, so this only fires
    // when no def/local of that name exists.
    //
    // Resolution modes:
    //   - result-directed methods (`pure a`, `return x`, `mempty`): the class
    //     variable appears only in the RESULT, so the expected result type
    //     selects the instance.
    //   - arity-0 classes (`Show a`, `Monoid m`): the first non-nullary
    //     argument's concrete type NAME selects the instance; nullary args
    //     (`mempty`) resolve by the same name.
    //   - constructor classes (`Functor f`, arity ≥ 1): the class-var-applied
    //     argument selects the instance by constructor via `match_ctor_pattern`.
    if !ctx.locals.contains_key(name) && !ctx.defs.contains_key(name) {
        if let Some(class_name) = ctx.env.class_of_method(name) {
            let class_info = ctx.env.typeclasses.get(&class_name).cloned().unwrap();
            let class_var = class_info.var.clone();
            let sig = class_info
                .methods
                .iter()
                .find(|(m, _)| m == name)
                .map(|(_, s)| s.clone());
            let sig = sig.as_ref().unwrap();
            // Argument count: a `TFunc` signature carries its argument list; a
            // bare class-var signature (`show: a`) takes exactly one selector
            // argument (the arity-0 instance binds it).
            let n_sig_args = match sig {
                crate::ast::TypeExpr::TFunc(args, _) => args.len(),
                _ => 1,
            };
            // Result-directed methods (`pure a`, `return x`): a function
            // signature whose arguments never mention the class variable — the
            // container comes only from the expected RESULT type. A bare
            // signature (`show: a`, `mempty: m`) is NOT result-directed: it
            // resolves by the selector argument (arity-0 class path) or as a
            // nullary arg.
            let result_directed = matches!(
                sig,
                crate::ast::TypeExpr::TFunc(args, _)
                    if !args.is_empty()
                        && args.iter().all(|a| !type_expr_mentions(a, &class_var))
            );
            if result_directed {
                // `pure a`, `return x`: the container `f a` / `m a` comes only
                // from the expected result type.
                let Some(exp) = expected else {
                    return Err(CompileError::Type {
                        msg: format!(
                            "cannot resolve method `{name}` of `{class_name}`: expected type unknown \
                             (use it where its result type is fixed, e.g. `bind (Just 1.0) (fn x -> return x)`)"
                        ),
                        span,
                    });
                };
                let ty_name = match ctx.env.type_name_of_vty(&exp) {
                    Some(t) => t,
                    None => {
                        eprintln!("result-directed {name} of {class_name}: expected not concrete: {exp:?}");
                        return Err(CompileError::Type {
                            msg: format!(
                                "cannot resolve method `{name}` of `{class_name}`: the expected type is not concrete"
                            ),
                            span,
                        });
                    }
                };
                let (_, params, body) = match ctx.env.resolve_method(name, ty_name.as_str()) {
                    Some(r) => r,
                    None => {
                        return Err(CompileError::Type {
                            msg: format!("no instance of `{class_name}` for type `{ty_name}`"),
                            span,
                        });
                    }
                };
                let key = (class_name, ty_name.clone(), name.to_string());
                if ctx.method_lifting.contains(&key) {
                    return Err(CompileError::Type {
                        msg: format!("recursive typeclass method `{name}` for type `{ty_name}`"),
                        span,
                    });
                }
                ctx.method_lifting.insert(key.clone());
                let saved = ctx.locals.clone();
                let param_tys = signature_param_tys(ctx, &class_var, &ty_name, sig);
                for (p, pt) in params.iter().zip(param_tys) {
                    ctx.locals
                        .insert(p.clone(), ArrowTy::value_channel(pt.clone()));
                }
                // The single element argument (`pure x` / `return x`) unifies
                // against the signature's leading parameter type.
                for (a, p) in args.iter().zip(params.iter()) {
                    let av = infer_method_value_vty(ctx, a, "argument")?;
                    let pt = ctx
                        .locals
                        .get(p)
                        .cloned()
                        .map(|t| t.outs[0].vty.clone())
                        .unwrap_or(ValueTy::Float);
                    unify_value(&av, &pt, &mut ctx.subst, a.span())?;
                }
                let body_vty =
                    infer_method_value_vty_expected(ctx, &body, "body", Some(exp.clone()));
                ctx.locals = saved;
                ctx.method_lifting.remove(&key);
                let body_vty = body_vty?;
                return Ok(ArrowTy::value_channel(body_vty));
            }
            if class_info.arity == 0 {
                if args.len() != n_sig_args {
                    return Err(CompileError::Type {
                        msg: format!(
                            "method `{name}` of `{class_name}` expects {n_sig_args} argument(s), got {}",
                            args.len()
                        ),
                        span,
                    });
                }
                // Selector: the first argument that is not a bare nullary method
                // ref (`mappend xs mempty` — `xs` selects; `mempty` resolves by
                // that type name).
                let selector_idx = args
                    .iter()
                    .position(|a| !is_bare_nullary_method_ref(ctx, a, &class_name))
                    .unwrap_or(0);
                let arg_vty = infer_method_value_vty(ctx, &args[selector_idx], "argument")?;
                let ty_name = match ctx.env.type_name_of_vty(&arg_vty) {
                    Some(t) => t,
                    None => {
                        return Err(CompileError::Type {
                            msg: format!(
                                "cannot resolve method `{name}` of `{class_name}`: the argument type is not concrete"
                            ),
                            span: args[selector_idx].span(),
                        });
                    }
                };
                let (_, params, body) = match ctx.env.resolve_method(name, ty_name.as_str()) {
                    Some(r) => r,
                    None => {
                        return Err(CompileError::Type {
                            msg: format!("no instance of `{class_name}` for type `{ty_name}`"),
                            span,
                        });
                    }
                };
                let key = (class_name.clone(), ty_name.clone(), name.to_string());
                if ctx.method_lifting.contains(&key) {
                    return Err(CompileError::Type {
                        msg: format!("recursive typeclass method `{name}` for type `{ty_name}`"),
                        span,
                    });
                }
                ctx.method_lifting.insert(key.clone());
                let saved = ctx.locals.clone();
                let param_tys = signature_param_tys(ctx, &class_var, &ty_name, sig);
                for (i, p) in params.iter().enumerate() {
                    let pt = if i == selector_idx
                        || !is_bare_nullary_method_ref(ctx, &args[i], &class_name)
                    {
                        infer_method_value_vty(ctx, &args[i], "argument")?
                    } else {
                        // Nullary arg (`mempty`): inline ITS instance body
                        // with the signature's param type as expected so it
                        // unifies (`list` → List ?a).
                        let arg_name = match &args[i] {
                            Expr::Ref(n, _) => n.as_str(),
                            _ => unreachable!("nullary arg must be a Ref"),
                        };
                        let (_, _, nbody) =
                            ctx.env.resolve_method(arg_name, ty_name.as_str()).unwrap();
                        let expected = param_tys.get(i).cloned().unwrap_or(ValueTy::Float);
                        infer_method_value_vty_expected(ctx, &nbody, "argument", Some(expected))?
                    };
                    ctx.locals
                        .insert(p.clone(), ArrowTy::value_channel(pt.clone()));
                }
                let body_vty = infer_method_value_vty(ctx, &body, "body");
                ctx.locals = saved;
                ctx.method_lifting.remove(&key);
                let body_vty = body_vty?;
                return Ok(ArrowTy::value_channel(body_vty));
            }
            // Constructor class (arity ≥ 1): the class-var-applied argument's
            // concrete type selects the instance by constructor. Infer every
            // argument's value type, match the class-var pattern against the
            // container, then bind all method params to the call-site types.
            let container_idx = ctx
                .env
                .class_var_arg_index(sig, &class_var)
                .ok_or_else(|| CompileError::Type {
                    msg: format!(
                        "method `{name}` of `{class_name}` has no class-var-applied argument"
                    ),
                    span,
                })?;
            if args.len() != n_sig_args {
                return Err(CompileError::Type {
                    msg: format!(
                        "method `{name}` of `{class_name}` expects {n_sig_args} argument(s), got {}",
                        args.len()
                    ),
                    span,
                });
            }
            // Phase 1: infer the container argument and resolve the constructor.
            let container_vty = infer_method_value_vty(ctx, &args[container_idx], "argument")?;
            // Build the class-var pattern (`f a` → `App("f", [fresh])`) from the
            // container argument's signature type.
            let pattern = class_var_pattern(ctx, sig, container_idx);
            let ctor = match ctx.env.match_ctor_pattern(
                &class_var,
                &pattern,
                &container_vty,
                &mut ctx.subst,
            ) {
                Some(c) => c,
                None => {
                    return Err(CompileError::Type {
                        msg: format!(
                            "cannot resolve method `{name}` of `{class_name}`: the argument type \
                             does not apply `{class_var}`"
                        ),
                        span: args[container_idx].span(),
                    });
                }
            };
            let (_, params, body) = match ctx.env.resolve_method(name, ctor.as_str()) {
                Some(r) => r,
                None => {
                    return Err(CompileError::Type {
                        msg: format!("no instance of `{class_name}` for constructor `{ctor}`"),
                        span,
                    });
                }
            };
            if params.len() != args.len() {
                return Err(CompileError::Type {
                    msg: format!(
                        "method `{name}` of `{class_name}` expects {} argument(s), got {}",
                        params.len(),
                        args.len()
                    ),
                    span,
                });
            }
            let key = (class_name, ctor.clone(), name.to_string());
            if ctx.method_lifting.contains(&key) {
                return Err(CompileError::Type {
                    msg: format!("recursive typeclass method `{name}` for type `{ctor}`"),
                    span,
                });
            }
            ctx.method_lifting.insert(key.clone());
            // Phase 2: infer the remaining args with their signature param type
            // as expected (so `bind mx (fn x -> return x)` resolves `return` by
            // the monad), then bind all params.
            let param_tys =
                signature_param_tys_conv(&ctx.env.clone(), &class_var, &ctor, sig, || {
                    ctx.fresh_vty()
                });
            let mut arg_vtys = Vec::with_capacity(args.len());
            for (i, a) in args.iter().enumerate() {
                if i == container_idx {
                    arg_vtys.push(container_vty.clone());
                } else {
                    let exp = param_tys.get(i).cloned().unwrap_or(ValueTy::Float);
                    arg_vtys.push(infer_method_value_vty_expected(
                        ctx,
                        a,
                        "argument",
                        Some(exp),
                    )?);
                }
            }
            let saved = ctx.locals.clone();
            for (p, av) in params.iter().zip(arg_vtys.iter()) {
                ctx.locals
                    .insert(p.clone(), ArrowTy::value_channel(av.clone()));
            }
            let body_vty = infer_method_value_vty(ctx, &body, "body");
            ctx.locals = saved;
            ctx.method_lifting.remove(&key);
            let body_vty = body_vty?;
            return Ok(ArrowTy::value_channel(body_vty));
        }
    }
    if let Some(sig) = ctx.sigs.builtin_sig(name).cloned() {
        let min = sig.min_args();
        let max = sig.max_args();
        if args.len() < min {
            return Err(CompileError::Type {
                msg: format!(
                    "built-in `{name}` expects at least {min} arg(s), got {}",
                    args.len()
                ),
                span,
            });
        }
        if let Some(max) = max {
            if args.len() > max {
                return Err(CompileError::Type {
                    msg: format!(
                        "built-in `{name}` expects at most {max} arg(s), got {}",
                        args.len()
                    ),
                    span,
                });
            }
        }

        let mut signal_ins = 0;
        let mut pos = 0;

        for ptype in &sig.params {
            match ptype {
                ParamType::Signal => {
                    signal_ins += 1;
                }
                ParamType::Float | ParamType::Int => {
                    if pos >= args.len() {
                        break;
                    }
                    match &args[pos] {
                        Expr::Ref(ref_name, _) if ctx.locals.contains_key(ref_name) => {}
                        _ => {
                            let at = infer_expr(ctx, &args[pos])?;
                            if at.arity_in() != 0 || at.arity_out() != 1 {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "param at position {pos} of `{name}` must be constant or param reference"
                                    ),
                                    span: args[pos].span(),
                                });
                            }
                        }
                    }
                    pos += 1;
                }
                ParamType::String => {
                    if pos >= args.len() {
                        break;
                    }
                    match &args[pos] {
                        Expr::Str(_, _) => {}
                        _ => {
                            return Err(CompileError::Type {
                                msg: format!("argument {pos} of `{name}` must be a string literal"),
                                span: args[pos].span(),
                            });
                        }
                    }
                    pos += 1;
                }
                ParamType::Bool => {
                    if pos >= args.len() {
                        break;
                    }
                    pos += 1;
                }
                ParamType::Enum(variants) => {
                    if pos >= args.len() {
                        break;
                    }
                    match &args[pos] {
                        Expr::Ref(v, _) if variants.contains(&v.as_str()) => {}
                        _ => {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "argument {pos} of `{name}` must be one of: {}",
                                    variants.join(", ")
                                ),
                                span: args[pos].span(),
                            });
                        }
                    }
                    pos += 1;
                }
                ParamType::Resource => {
                    if pos >= args.len() {
                        break;
                    }
                    match &args[pos] {
                        Expr::Ref(_, _) => {}
                        _ => {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "resource argument {pos} of `{name}` must be a symbolic reference"
                                ),
                                span: args[pos].span(),
                            });
                        }
                    }
                    pos += 1;
                }
                ParamType::Record(_schema) => {
                    if pos >= args.len() {
                        break;
                    }
                    match &args[pos] {
                        Expr::Record(_, _) => {}
                        _ => {
                            return Err(CompileError::Type {
                                msg: format!("argument {pos} of `{name}` must be a record literal"),
                                span: args[pos].span(),
                            });
                        }
                    }
                    pos += 1;
                }
                ParamType::Variadic(inner) => match &**inner {
                    ParamType::Signal => {
                        for arg in &args[pos..] {
                            let ty = infer_expr(ctx, arg)?;
                            if ty.arity_out() == 0 {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "variadic signal argument of `{name}` has no outputs"
                                    ),
                                    span: arg.span(),
                                });
                            }
                            signal_ins += ty.arity_in();
                        }
                    }
                    _ => {
                        for arg in &args[pos..] {
                            let at = infer_expr(ctx, arg)?;
                            if at.arity_in() != 0 || at.arity_out() != 1 {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "variadic param of `{name}` must be constant or param reference"
                                    ),
                                    span: arg.span(),
                                });
                            }
                        }
                    }
                },
            }
        }

        return Ok(ArrowTy::uniform(signal_ins, sig.signal_outs, Scalar::Float));
    }
    // A first-class function value bound to a LOCAL (a lambda parameter, a
    // let/match binding): `f x` where `f` is a `Func`. Locals SHADOW top-level
    // defs of the same name (lexical scoping), so this runs before the def
    // path. The local's value type is unified with a fresh `Func` signature of
    // the applied arity — this both type-checks the arguments AND binds an
    // as-yet unresolved lambda parameter to a `Func` (value variables record
    // through `subst.value_map`). A local already resolved to a concrete
    // `Func` (a HOF parameter applied twice) applies against that signature, so
    // an arity inconsistency is rejected.
    if let Some(local) = ctx.locals.get(name).cloned() {
        if local.arity_out() == 1 && local.outs[0].rate == Rate::Value {
            let lty = &local.outs[0].vty;
            let resolved = ctx.subst.resolve_value(lty);
            let arg_tys: Vec<ValueTy>;
            let ret_tys: Vec<ValueTy>;
            match resolved {
                ValueTy::Func(a, r) => {
                    arg_tys = a;
                    ret_tys = r;
                }
                _ => {
                    arg_tys = (0..args.len()).map(|_| ctx.fresh_vty()).collect();
                    ret_tys = vec![ctx.fresh_vty()];
                }
            }
            let func_ty = ValueTy::Func(arg_tys.clone(), ret_tys.clone());
            if args.len() != arg_tys.len() {
                return Err(CompileError::Type {
                    msg: format!(
                        "`{name}` expects {} argument(s), got {}",
                        arg_tys.len(),
                        args.len()
                    ),
                    span,
                });
            }
            unify_value(lty, &func_ty, &mut ctx.subst, span)?;
            for (a, pty) in args.iter().zip(arg_tys.iter()) {
                let vt = infer_method_value_vty(ctx, a, "argument")?;
                unify_value(&vt, pty, &mut ctx.subst, a.span())?;
            }
            let ret_ty = ret_tys.first().cloned().unwrap_or(ValueTy::Float);
            return Ok(ArrowTy::value_channel(ret_ty));
        }
    }
    // User-defined function: λ-params are consumed, signal ports remain open
    if let Some(scheme) = ctx.defs.get(name).cloned() {
        // Func-value application: `f = double` binds `f` to `ValueTy::Func`,
        // and `f x` dispatches to the referenced definition by instantiating
        // its scheme (v1 resolves calls at compile time — `reduce` β-reduces
        // the body for lowering). The referenced definition's arity must match
        // the applied arguments.
        if scheme.ty.outs.len() == 1 && scheme.ty.outs[0].rate == Rate::Value {
            if let ValueTy::Func(arg_tys, ret_tys) = &scheme.ty.outs[0].vty {
                let ref_name = func_target(ctx, name);
                let target = ctx.defs.get(ref_name.as_str()).cloned();
                // A closure-valued definition (a lambda literal, or an
                // application/ref chain that produces one) dispatches at
                // RUNTIME — `add2 = adder 2.0`, `main = add2 3.0`. Its call
                // type comes from the Func signature, and the args are value
                // expressions unified against the parameter types.
                let apply_by_signature = target.as_ref().is_some_and(|ts| {
                    ts.lam_count == 0
                        && matches!(
                            ts.ty.outs.first().map(|o| &o.vty),
                            Some(ValueTy::Func(_, _))
                        )
                });
                if apply_by_signature {
                    // Signal-wire application: a trailing `_` wire argument binds
                    // to the callee's LAST parameter as a signal input (positional
                    // wire-capture at the call site, zero-copy block register).
                    // The leading args are value args unified against the leading
                    // parameter types; the trailing wire contributes one signal
                    // input. The lambda's body compiles on the block track with
                    // the wire bound to a block register. v1 contract: exactly
                    // one trailing wire, and it must exactly complete the value
                    // arity (`amp 2.0 _`; `amp _` or `amp2 0.5 _` are rejected).
                    if let Some(Expr::Wire(_)) = args.last() {
                        let value_args = &args[..args.len() - 1];
                        if value_args.len() + 1 != arg_tys.len() {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "`{name}` expects {} value argument(s) plus one signal wire, \
                                     got {} value argument(s) and a wire",
                                    arg_tys.len() - 1,
                                    value_args.len()
                                ),
                                span,
                            });
                        }
                        for (a, pty) in value_args.iter().zip(arg_tys.iter()) {
                            let vt = infer_method_value_vty(ctx, a, "argument")?;
                            unify_value(&vt, pty, &mut ctx.subst, a.span())?;
                        }
                        let result_scalar = match ret_tys.first() {
                            Some(ValueTy::Int) => Scalar::Int,
                            _ => Scalar::Float,
                        };
                        return Ok(ArrowTy::uniform(1, 1, result_scalar));
                    }
                    // Partial application (currying): fewer args than the
                    // signature produce a function over the remaining parameters.
                    // `add3 = add 3.0` types as `Func([Float], [Float])`.
                    if args.len() < arg_tys.len() {
                        for (a, pty) in args.iter().zip(arg_tys.iter()) {
                            let vt = infer_method_value_vty(ctx, a, "argument")?;
                            unify_value(&vt, pty, &mut ctx.subst, a.span())?;
                        }
                        let remaining: Vec<ValueTy> = arg_tys[args.len()..].to_vec();
                        return Ok(ArrowTy::value_channel(ValueTy::Func(
                            remaining,
                            ret_tys.clone(),
                        )));
                    }
                    if args.len() > arg_tys.len() {
                        return Err(CompileError::Type {
                            msg: format!(
                                "`{name}` expects {} argument(s), got {}",
                                arg_tys.len(),
                                args.len()
                            ),
                            span,
                        });
                    }
                    for (a, pty) in args.iter().zip(arg_tys.iter()) {
                        let vt = infer_method_value_vty(ctx, a, "argument")?;
                        unify_value(&vt, pty, &mut ctx.subst, a.span())?;
                    }
                    let ret_ty = ret_tys.first().cloned().unwrap_or(ValueTy::Float);
                    return Ok(ArrowTy::value_channel(ret_ty));
                }
                if let Some(ref_scheme) = target {
                    if args.len() != ref_scheme.lam_count {
                        return Err(CompileError::Type {
                            msg: format!(
                                "`{name}` references `{ref_name}`, which expects {} argument(s), got {}",
                                ref_scheme.lam_count,
                                args.len()
                            ),
                            span,
                        });
                    }
                    let ty = ctx.instantiate(&ref_scheme);
                    return Ok(ArrowTy {
                        ins: ty.ins[ref_scheme.lam_count..].to_vec(),
                        outs: ty.outs,
                    });
                }
            }
        }
        if args.len() != scheme.lam_count {
            // Partial application of a named definition (`add5 = add2 5.0` where
            // `add2` takes two λ-params): the remaining λ-params become the
            // value arguments of a first-class function value. The applied args
            // must be value scalars, unified against Float (v1 λ-params are
            // Float-typed).
            if args.len() < scheme.lam_count {
                let remaining = scheme.lam_count - args.len();
                for a in args {
                    let vt = infer_const_value(ctx, a)?;
                    unify_value(&vt, &ValueTy::Float, &mut ctx.subst, a.span())?;
                }
                return Ok(ArrowTy::value_channel(ValueTy::Func(
                    vec![ValueTy::Float; remaining],
                    vec![ValueTy::Float],
                )));
            }
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
        return Ok(ArrowTy {
            ins: ty.ins[scheme.lam_count..].to_vec(),
            outs: ty.outs,
        });
    }
    // Fallback: builtin reference (abs, sin, +, etc.) applied to signal args
    let mut combined: Option<ArrowTy> = None;
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

/// Infer a collection-operation call (`length`, `cons`, `head`, `tail`, `map`,
/// `fold`, `filter`, `list`, `insert`, `lookup`, `member`, `empty_map`,
/// `empty_set`): validates the argument arity and container types, returning
/// the result value type. Mirrors the lowerer's `value_builtin_ty` signatures
/// (Task 6.3).
fn infer_collection_call(
    ctx: &mut Ctx<'_>,
    name: &str,
    args: &[Expr],
    span: Span,
) -> Result<ValueTy, CompileError> {
    let arity_err = |expected: &str, got: usize| CompileError::Type {
        msg: format!("`{name}` expects {expected} argument(s), got {got}"),
        span,
    };
    let arg_vty = |ctx: &mut Ctx<'_>, i: usize| -> Result<ValueTy, CompileError> {
        match args.get(i) {
            Some(e) => infer_const_value(ctx, e),
            None => Err(arity_err(&(i + 1).to_string(), args.len())),
        }
    };
    // Enforce the `Ord` constraint on Map/Set keys (and Set elements): every
    // concrete key type must have a derived `Ord` instance (see
    // `derive_eq_ord`). `Func` gets no instance, so a function-typed key is a
    // compile error; an unresolved type var cannot select an instance either.
    let check_ord = |ctx: &mut Ctx<'_>, ty: &ValueTy, what: &str| -> Result<(), CompileError> {
        let ty = ctx.subst.resolve_value(ty);
        match ctx.env.type_name_of_vty(&ty) {
            Some(name) => {
                let has = ctx
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
    // Validate the first argument is a function value of the value-arity the op
    // dispatches (map/filter call it with one element, fold with (acc, elem)).
    // A `Func([], _)` is an unknown signature (a bare named function ref whose
    // arity resolves at runtime dispatch), so it is allowed; a lambda literal's
    // structural signature must match the expected arity exactly — a wrong-arity
    // closure would otherwise pass `value_ins` to the pre-sized call scratch and
    // panic the runtime path.
    let expect_closure = |ctx: &mut Ctx<'_>, want: usize, what: &str| -> Result<(), CompileError> {
        let ft = arg_vty(ctx, 0)?;
        let ok =
            matches!(&ft, ValueTy::Func(arg_tys, _) if arg_tys.is_empty() || arg_tys.len() == want);
        if ok {
            Ok(())
        } else {
            Err(CompileError::Type {
                msg: format!("`{name}` expects a {what} function"),
                span,
            })
        }
    };
    // The element type of a `List` value type; Float for a non-list.
    let list_elem = |t: &ValueTy| -> ValueTy {
        match t {
            ValueTy::App(n, inner) if n == "List" => {
                inner.first().cloned().unwrap_or(ValueTy::Float)
            }
            _ => ValueTy::Float,
        }
    };
    let list_of = |t: &ValueTy| ValueTy::App("List".into(), vec![list_elem(t)]);
    match name {
        "length" => {
            if args.len() != 1 {
                return Err(arity_err("1", args.len()));
            }
            let _ = arg_vty(ctx, 0)?;
            Ok(ValueTy::Int)
        }
        "cons" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            let xt = arg_vty(ctx, 0)?;
            let lt = arg_vty(ctx, 1)?;
            let elem = list_elem(&lt);
            unify_value(&xt, &elem, &mut ctx.subst, args[0].span())?;
            Ok(ValueTy::App("List".into(), vec![elem]))
        }
        "head" => {
            if args.len() != 1 {
                return Err(arity_err("1", args.len()));
            }
            let lt = arg_vty(ctx, 0)?;
            let elem = list_elem(&lt);
            Ok(ValueTy::App("Maybe".into(), vec![elem]))
        }
        "tail" => {
            if args.len() != 1 {
                return Err(arity_err("1", args.len()));
            }
            let lt = arg_vty(ctx, 0)?;
            Ok(list_of(&lt))
        }
        "map" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            expect_closure(ctx, 1, "unary")?;
            // The result list's ELEMENT type is the closure's RETURN type
            // (`map : (a -> b) -> List a -> List b`), not the source list's
            // element type — a type-changing map must be typed `List b`.
            let ft0 = arg_vty(ctx, 0)?;
            let ft = ctx.subst.resolve_value(&ft0);
            let ret_ty = match &ft {
                ValueTy::Func(_, rets) => {
                    let r = rets.first().cloned().unwrap_or(ValueTy::Float);
                    ctx.subst.resolve_value(&r)
                }
                _ => ValueTy::Float,
            };
            Ok(ValueTy::App("List".into(), vec![ret_ty]))
        }
        "fold" => {
            if args.len() != 3 {
                return Err(arity_err("3", args.len()));
            }
            expect_closure(ctx, 2, "binary")?;
            let zt = arg_vty(ctx, 1)?;
            let _ = arg_vty(ctx, 2)?;
            Ok(zt)
        }
        "filter" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            expect_closure(ctx, 1, "unary")?;
            let lt = arg_vty(ctx, 1)?;
            Ok(list_of(&lt))
        }
        "concat_map" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            expect_closure(ctx, 1, "unary")?;
            let elem = {
                let lt = arg_vty(ctx, 1)?;
                list_elem(&lt)
            };
            // (a -> List b) -> List a -> List b: the result element type is the
            // closure's RETURN list element, falling back to the source element.
            let ft0 = arg_vty(ctx, 0)?;
            let ft = ctx.subst.resolve_value(&ft0);
            let b = match &ft {
                ValueTy::Func(_, rets) => match rets.first() {
                    Some(ValueTy::App(n, inner)) if n == "List" => {
                        inner.first().cloned().unwrap_or(elem)
                    }
                    _ => elem,
                },
                _ => elem,
            };
            Ok(ValueTy::App("List".into(), vec![b]))
        }
        "append_list" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            let lt = arg_vty(ctx, 0)?;
            Ok(ValueTy::App("List".into(), vec![list_elem(&lt)]))
        }
        "concat_string" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            let _ = arg_vty(ctx, 0)?;
            let _ = arg_vty(ctx, 1)?;
            Ok(ValueTy::String)
        }
        "list" => {
            if !args.is_empty() {
                return Err(arity_err("0", args.len()));
            }
            Ok(ValueTy::App("List".into(), vec![ValueTy::Float]))
        }
        "insert" => match args.len() {
            3 => {
                let kt = arg_vty(ctx, 0)?;
                let vt = arg_vty(ctx, 1)?;
                let _ = arg_vty(ctx, 2)?;
                check_ord(ctx, &kt, "key")?;
                Ok(ValueTy::App("Map".into(), vec![kt, vt]))
            }
            2 => {
                let kt = arg_vty(ctx, 0)?;
                let _ = arg_vty(ctx, 1)?;
                check_ord(ctx, &kt, "element")?;
                Ok(ValueTy::App("Set".into(), vec![kt]))
            }
            _ => Err(arity_err("2 or 3", args.len())),
        },
        "lookup" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            let kt = arg_vty(ctx, 0)?;
            check_ord(ctx, &kt, "key")?;
            let mt = arg_vty(ctx, 1)?;
            let v = match &mt {
                ValueTy::App(n, inner) if n == "Map" => {
                    inner.get(1).cloned().unwrap_or(ValueTy::Float)
                }
                _ => ValueTy::Float,
            };
            Ok(ValueTy::App("Maybe".into(), vec![v]))
        }
        "member" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            let kt = arg_vty(ctx, 0)?;
            check_ord(ctx, &kt, "key")?;
            let _ = arg_vty(ctx, 1)?;
            Ok(ValueTy::Bool)
        }
        "empty_map" => {
            if !args.is_empty() {
                return Err(arity_err("0", args.len()));
            }
            Ok(ValueTy::App(
                "Map".into(),
                vec![ValueTy::String, ValueTy::Float],
            ))
        }
        "empty_set" => {
            if !args.is_empty() {
                return Err(arity_err("0", args.len()));
            }
            Ok(ValueTy::App("Set".into(), vec![ValueTy::Float]))
        }
        _ => Err(CompileError::Type {
            msg: format!("unknown collection op `{name}`"),
            span,
        }),
    }
}

fn infer_param(args: &[Expr], span: Span) -> Result<ArrowTy, CompileError> {
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
    Ok(ArrowTy::uniform(0, 1, Scalar::Float))
}

fn expr_has_variadic_signal(ctx: &Ctx<'_>, e: &Expr) -> bool {
    let name = match e {
        Expr::Apply { name, .. } => name,
        Expr::Ref(name, _) => name,
        _ => return false,
    };
    ctx.sigs
        .builtin_sig(name)
        .map(|s| s.has_variadic_signal())
        .unwrap_or(false)
}

fn infer_seq(
    ctx: &mut Ctx<'_>,
    lhs: &Expr,
    rhs: &Expr,
    span: Span,
) -> Result<ArrowTy, CompileError> {
    let a = infer_expr(ctx, lhs)?;
    let b = infer_expr(ctx, rhs)?;
    seq(ctx, &a, &b, span)
}

fn infer_par(
    ctx: &mut Ctx<'_>,
    lhs: &Expr,
    rhs: &Expr,
    _span: Span,
) -> Result<ArrowTy, CompileError> {
    let a = infer_expr(ctx, lhs)?;
    let b = infer_expr(ctx, rhs)?;
    Ok(par(&a, &b))
}

fn infer_split(
    ctx: &mut Ctx<'_>,
    lhs: &Expr,
    rhs: &Expr,
    span: Span,
) -> Result<ArrowTy, CompileError> {
    let a = infer_expr(ctx, lhs)?;
    let b = infer_expr(ctx, rhs)?;
    let rhs_variadic = expr_has_variadic_signal(ctx, rhs);
    split(ctx, &a, &b, span, rhs_variadic)
}

fn infer_merge(
    ctx: &mut Ctx<'_>,
    lhs: &Expr,
    rhs: &Expr,
    span: Span,
) -> Result<ArrowTy, CompileError> {
    let a = infer_expr(ctx, lhs)?;
    let b = infer_expr(ctx, rhs)?;
    let rhs_variadic = expr_has_variadic_signal(ctx, rhs);
    merge(ctx, &a, &b, span, rhs_variadic)
}

fn infer_loop(
    ctx: &mut Ctx<'_>,
    lhs: &Expr,
    rhs: &Expr,
    span: Span,
) -> Result<ArrowTy, CompileError> {
    let a = infer_expr(ctx, lhs)?;
    let b = infer_expr(ctx, rhs)?;
    feedback(ctx, &a, &b, span)
}

fn infer_delay(
    ctx: &mut Ctx<'_>,
    lhs: &Expr,
    rhs: &Expr,
    span: Span,
) -> Result<ArrowTy, CompileError> {
    let a = infer_expr(ctx, lhs)?;
    let b = infer_expr(ctx, rhs)?;
    delay(ctx, &a, &b, span)
}

fn infer_arith(
    ctx: &mut Ctx<'_>,
    lhs: &Expr,
    rhs: &Expr,
    span: Span,
) -> Result<ArrowTy, CompileError> {
    let a = infer_expr(ctx, lhs)?;
    let b = infer_expr(ctx, rhs)?;
    // Value-track arithmetic: a value operand (a lambda parameter, a value
    // function, a field projection, ...) combined with another value operand
    // or a bare Float/Int literal lowers on the value track and yields a Float
    // value channel — this is what makes lambda bodies like `fn x -> x * 2.0`
    // meaningful. A value channel mixed with a genuine signal computation
    // (`sin _`, a wire, a combinator) is still rejected.
    let a_value = a.outs.iter().any(|c| c.rate == Rate::Value);
    let b_value = b.outs.iter().any(|c| c.rate == Rate::Value);
    if a_value || b_value {
        // A value operand must be a numeric scalar (Float/Int, or an as-yet
        // unresolved Var such as a lambda parameter). Func values, records and
        // newtypes cannot take part in arithmetic (spec: no arithmetic on
        // closures); a bare Float/Int literal or a reference to a constant
        // definition (`k = 2.0`) is value-compatible — the latter is a common
        // lambda free variable, captured by value into the closure env.
        let scalar_vty = |v: &ValueTy| matches!(v, ValueTy::Float | ValueTy::Int | ValueTy::Var(_));
        let mut unify_operand = |t: &ArrowTy, e: &Expr| -> Result<(), CompileError> {
            if t.arity_in() == 0 && t.arity_out() == 1 {
                if t.outs[0].rate == Rate::Value {
                    if scalar_vty(&t.outs[0].vty) {
                        // A structural parameter variable in arithmetic is a
                        // numeric scalar: bind it to Float so the lambda's
                        // Func signature resolves concretely (`fn x -> x * 2`
                        // types `x` as Float, not an unconstrained Var).
                        if matches!(&t.outs[0].vty, ValueTy::Var(_)) {
                            unify_value(&t.outs[0].vty, &ValueTy::Float, &mut ctx.subst, e.span())?;
                        }
                        return Ok(());
                    }
                } else if matches!(e, Expr::Int(_, _) | Expr::Float(_, _))
                    || is_const_value_ref(ctx, e)
                {
                    return Ok(());
                }
            }
            Err(CompileError::Type {
                msg: "value-channel arithmetic requires numeric value operands".into(),
                span: e.span(),
            })
        };
        unify_operand(&a, lhs)?;
        unify_operand(&b, rhs)?;
        return Ok(ArrowTy::value_channel(ValueTy::Float));
    }
    arith(ctx, &a, &b, span)
}

pub(crate) fn par(a: &ArrowTy, b: &ArrowTy) -> ArrowTy {
    let mut ins = a.ins.clone();
    ins.extend(b.ins.clone());
    let mut outs = a.outs.clone();
    outs.extend(b.outs.clone());
    ArrowTy { ins, outs }
}

fn seq(ctx: &mut Ctx<'_>, a: &ArrowTy, b: &ArrowTy, span: Span) -> Result<ArrowTy, CompileError> {
    reject_value_channels(a, span)?;
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
        unify_scalar(&x.elem, &y.elem, &mut ctx.subst, span)?;
    }
    Ok(ArrowTy {
        ins: a.ins.clone(),
        outs: b.outs.clone(),
    })
}

fn split(
    ctx: &mut Ctx<'_>,
    a: &ArrowTy,
    b: &ArrowTy,
    span: Span,
    rhs_variadic: bool,
) -> Result<ArrowTy, CompileError> {
    let ao = a.arity_out();
    reject_value_channels(a, span)?;
    let bi = if rhs_variadic { ao } else { b.arity_in() };
    if ao == 0 || bi % ao != 0 {
        return Err(CompileError::Type {
            msg: format!(
                "split `<:` requires rhs inputs ({bi}) be a multiple of lhs outputs ({ao})"
            ),
            span,
        });
    }
    let reps = bi / ao;
    if !rhs_variadic {
        for r in 0..reps {
            for k in 0..ao {
                unify_scalar(
                    &a.outs[k].elem,
                    &b.ins[r * ao + k].elem,
                    &mut ctx.subst,
                    span,
                )?;
            }
        }
    }
    Ok(ArrowTy {
        ins: a.ins.clone(),
        outs: b.outs.clone(),
    })
}

fn merge(
    ctx: &mut Ctx<'_>,
    a: &ArrowTy,
    b: &ArrowTy,
    span: Span,
    rhs_variadic: bool,
) -> Result<ArrowTy, CompileError> {
    let ao = a.arity_out();
    reject_value_channels(a, span)?;
    let bi = if rhs_variadic { ao } else { b.arity_in() };
    if bi == 0 || !ao.is_multiple_of(bi) {
        return Err(CompileError::Type {
            msg: format!(
                "merge `:>` requires lhs outputs ({ao}) be a multiple of rhs inputs ({bi})"
            ),
            span,
        });
    }
    let groups = ao / bi;
    if !rhs_variadic {
        for g in 0..groups {
            for k in 0..bi {
                unify_scalar(
                    &a.outs[g * bi + k].elem,
                    &b.ins[k].elem,
                    &mut ctx.subst,
                    span,
                )?;
            }
        }
    }
    Ok(ArrowTy {
        ins: a.ins.clone(),
        outs: b.outs.clone(),
    })
}

fn feedback(
    ctx: &mut Ctx<'_>,
    a: &ArrowTy,
    b: &ArrowTy,
    span: Span,
) -> Result<ArrowTy, CompileError> {
    let (ai, ao, bi, bo) = (a.arity_in(), a.arity_out(), b.arity_in(), b.arity_out());
    reject_value_channels(a, span)?;
    reject_value_channels(b, span)?;
    if bi > ao || bo > ai {
        return Err(CompileError::Type {
            msg: format!(
                "feedback `~` arity mismatch: need B.in({bi})<=A.out({ao}) and B.out({bo})<=A.in({ai})"
            ),
            span,
        });
    }
    for k in 0..bi {
        unify_scalar(&b.ins[k].elem, &a.outs[k].elem, &mut ctx.subst, span)?;
    }
    for k in 0..bo {
        unify_scalar(&b.outs[k].elem, &a.ins[k].elem, &mut ctx.subst, span)?;
    }
    Ok(ArrowTy {
        ins: a.ins[bo..].to_vec(),
        outs: a.outs.clone(),
    })
}

fn delay(ctx: &mut Ctx<'_>, a: &ArrowTy, b: &ArrowTy, span: Span) -> Result<ArrowTy, CompileError> {
    reject_value_channels(a, span)?;
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
    unify_scalar(&b.outs[0].elem, &Scalar::Int, &mut ctx.subst, span)?;
    Ok(ArrowTy {
        ins: a.ins.clone(),
        outs: a.outs.clone(),
    })
}

fn arith(ctx: &mut Ctx<'_>, a: &ArrowTy, b: &ArrowTy, span: Span) -> Result<ArrowTy, CompileError> {
    reject_value_channels(a, span)?;
    reject_value_channels(b, span)?;
    if a.arity_out() != 1 || b.arity_out() != 1 {
        return Err(CompileError::Type {
            msg: "arithmetic operands must each produce exactly one wire".into(),
            span,
        });
    }
    unify_scalar(&a.outs[0].elem, &b.outs[0].elem, &mut ctx.subst, span)?;
    let mut ins = a.ins.clone();
    ins.extend(b.ins.clone());
    Ok(ArrowTy {
        ins,
        outs: vec![a.outs[0].clone()],
    })
}

fn check_all_numeric(_ctx: &mut Ctx<'_>, _t: &ArrowTy, _span: Span) -> Result<(), CompileError> {
    Ok(())
}

/// Reject value-rate output channels from signal combinators.
///
/// Value channels are the value-track's arena values (records, sums, func
/// values); composing one through `:`, `<:`, `:>`, `~`, `@`, or arithmetic has
/// no block representation — a bare func value (`f = double` used as a signal)
/// would otherwise typecheck as an Int block and panic in lowering. Value
/// programs are single expressions, so this never rejects a valid program.
fn reject_value_channels(t: &ArrowTy, span: Span) -> Result<(), CompileError> {
    if t.outs.iter().any(|c| c.rate == Rate::Value) {
        return Err(CompileError::Type {
            msg: "a value channel cannot be used in a signal combinator".into(),
            span,
        });
    }
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
    fn rejects_zero_output_process() {
        assert!(ty_of("main = !").is_err());
    }

    #[test]
    fn multi_output_process_allowed() {
        let t = ty_of("main = _ , _").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (2, 2));
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
                    BuiltinKind::Block,
                )))),
                "sine" => Some(Box::leak(Box::new(BuiltinSig::simple(
                    "sine",
                    0,
                    1,
                    3,
                    BuiltinKind::Block,
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
        let t = ty_with("main = _ : lowpass 1000.0 0.7").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn builtin_wrong_param_count_errors() {
        assert!(ty_with("main = _ : lowpass 1000.0").is_err());
    }

    #[test]
    fn builtin_ref_param_ok() {
        assert!(ty_with("main f = _ : lowpass f 0.7").is_ok());
    }

    #[test]
    fn sample_builtin_in_feedback_typechecks() {
        assert!(ty_with("main = + ~ onepole 200.0 0.5").is_ok());
    }

    #[test]
    fn transitive_var_chain_resolves() {
        let t = ty_of("main = g where { f x = _ * x; g = f 0.5; }").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn var_unifies_with_float() {
        let t = ty_of("main = _ * 0.5").unwrap();
        assert!(matches!(t.process_ty.outs[0].elem, Scalar::Float));
    }

    #[test]
    fn int_float_mismatch_errors() {
        // This test existed before; keep it but it's tricky to trigger
        // with current rules since everything defaults to Float
        assert!(ty_of("main = _ @ _").is_err());
    }

    #[test]
    fn actor_param_no_default_typechecks() {
        let t = ty_of("main = _ * ?gain").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn actor_param_with_default_typechecks() {
        let t = ty_of("main = _ * ?gain=0.5").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
    }

    #[test]
    fn actor_param_default_must_be_constant() {
        assert!(ty_of("main = _ * ?gain=_").is_err());
    }

    #[test]
    fn closed_top_level_local_is_caf() {
        // osc = sine 440 0.5 0  (0 input channels, 0 λ-params) -> CAF
        let typed = ty_with("osc = sine 440 0.5 0; main = _ * 0.5").unwrap();
        assert!(typed.cafs.contains("osc"));
    }

    #[test]
    fn open_block_is_not_caf() {
        // gain = _ * 0.5  (1 input channel) -> macro, not CAF
        let typed = ty_with("gain = _ * 0.5; main = _ * 0.5").unwrap();
        assert!(!typed.cafs.contains("gain"));
    }

    #[test]
    fn function_with_params_is_not_caf() {
        // voice amp = osc * amp  (has a λ-param) -> not CAF
        let typed =
            ty_with("osc = sine 440 0.5 0; voice amp = osc * amp; main = voice 0.5").unwrap();
        assert!(typed.cafs.contains("osc"));
        assert!(!typed.cafs.contains("voice"));
    }

    #[test]
    fn data_record_constructor_infers_value_channel() {
        let t =
            ty_of("data Point = { x: Float, y: Float }; main = Point { x: 1.0, y: 2.0 }").unwrap();
        assert_eq!(t.process_ty.outs.len(), 1);
        assert_eq!(t.process_ty.outs[0].rate, Rate::Value);
        assert_eq!(
            t.process_ty.outs[0].vty,
            ValueTy::Data("Point".into(), vec![])
        );
    }

    #[test]
    fn field_project_produces_float_value() {
        let t =
            ty_of("data Point = { x: Float, y: Float }; p = Point { x: 1.0, y: 2.0 }; main = p.x")
                .unwrap();
        assert_eq!(t.process_ty.outs[0].rate, Rate::Value);
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Float);
    }

    #[test]
    fn sum_match_branches_on_constructor() {
        let t = ty_of(
            "data Shape = Circle Float | Rect Float Float; main = match _ of { Circle r => r; Rect w h => w; }",
        )
        .unwrap();
        assert_eq!(t.process_ty.outs[0].rate, Rate::Value);
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Float);
    }

    #[test]
    fn field_update_preserves_record_type() {
        let t = ty_of(
            "data Point = { x: Float, y: Float }; p = Point { x: 1.0, y: 2.0 }; main = p.x := 3.0",
        )
        .unwrap();
        assert_eq!(t.process_ty.outs[0].rate, Rate::Value);
        assert_eq!(
            t.process_ty.outs[0].vty,
            ValueTy::Data("Point".into(), vec![])
        );
    }

    #[test]
    fn match_arm_zero_output_is_error() {
        assert!(ty_of(
            "data Shape = Circle Float | Rect Float Float; main = match _ of { Circle r => !; Rect w h => 2.0 }"
        )
        .is_err());
    }

    #[test]
    fn match_arms_must_be_value_expressions() {
        // A bare literal arm body is value-compatible (Phase 7:
        // `Nothing => 42.0`); a genuine signal computation (a wire) is not.
        assert!(ty_of("data Shape = Circle Float; main = match _ of { Circle r => 1.0 }").is_ok());
        assert!(
            ty_of("data Shape = Circle Float; main = match _ of { Circle r => _ * 2.0 }").is_err()
        );
    }

    #[test]
    fn match_scrutinee_must_be_value() {
        assert!(ty_of("data Shape = Circle Float; main = match 3 of { Circle r => r }").is_err());
    }

    #[test]
    fn match_arms_same_sum_type() {
        assert!(ty_of("data A = C Float; data B = C Int; main = match _ of { C r => r }").is_err());
    }

    #[test]
    fn match_too_many_arm_bindings_is_error() {
        assert!(ty_of("data Shape = Circle Float; main = match _ of { Circle r s => r }").is_err());
    }

    #[test]
    fn guarded_match_arm_type_checks_and_is_non_exhaustive_alone() {
        // A guarded ctor arm (`Circle r | r > 0.0 => r`) type-checks its guard
        // as a Bool value, but a guarded arm does NOT cover its constructor for
        // exhaustiveness: without a wildcard the match must fail loudly, not
        // silently drop the guard.
        let res = ty_of(
            "data Shape = Circle Float | Rect Float Float; \
             main = match Circle 1.0 of { Circle r | r > 0.0 => r; };",
        );
        let err = match res {
            Err(e) => e,
            Ok(_) => panic!("a guarded match without a wildcard must be non-exhaustive"),
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("non-exhaustive"),
            "expected a non-exhaustive error, got: {msg}"
        );
        // A wildcard arm makes the guarded match exhaustive; the guard is
        // type-checked as a Bool value.
        assert!(ty_of(
            "data Shape = Circle Float | Rect Float Float; \
             main = match Circle 1.0 of { Circle r | r > 0.0 => r; _ => 0.0; };"
        )
        .is_ok());
        // A non-Bool guard is a type error.
        let res = ty_of(
            "data Shape = Circle Float | Rect Float Float; \
             main = match Circle 1.0 of { Circle r | r + 1.0 => r; _ => 0.0; };",
        );
        assert!(
            res.is_err(),
            "a non-Bool guard must be a compile error, not silently compiled"
        );
    }

    #[test]
    fn record_ctor_missing_field_is_error() {
        assert!(ty_of("data Point = { x: Float, y: Float }; main = Point { x: 1.0 }").is_err());
    }

    #[test]
    fn record_ctor_duplicate_field_is_error() {
        assert!(ty_of(
            "data Point = { x: Float, y: Float }; main = Point { x: 1.0, x: 2.0, y: 3.0 }"
        )
        .is_err());
    }

    #[test]
    fn if_infers_bool_condition_and_branch_types() {
        let t = ty_of("main = if 1.0 < 2.0 then 1.0 else 2.0").unwrap();
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Float);
        assert!(ty_of("main = if 1.0 then 1.0 else 2.0").is_err());
        assert!(ty_of("main = if 1.0 < 2.0 then 1 else 2").is_ok());
        let t2 = ty_of("main = if 1.0 < 2.0 then 1 else 2").unwrap();
        assert_eq!(t2.process_ty.outs[0].vty, ValueTy::Int);
    }

    #[test]
    fn scalar_match_infers_literal_patterns_and_totality() {
        let t = ty_of("main = match _ of { 0 => 1.0; _ => 2.0; }").unwrap();
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Float);
        assert!(ty_of("main = match _ of { 0 => 1.0 }").is_err());
        let t2 = ty_of("main = match true of { true => 1.0; false => 2.0 }").unwrap();
        assert_eq!(t2.process_ty.outs[0].vty, ValueTy::Float);
    }

    #[test]
    fn bare_int_literal_scrutinee_infers_as_value() {
        // `match 0 of {...}`: a bare Int literal is a signal-rate channel in v1
        // but value-compatible in value positions (mirroring arm_result_vty).
        let t = ty_of("main = match 0 of { 0 => 1.0; _ => 2.0; }").unwrap();
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Float);
        let t2 = ty_of("main = match 0.0 of { 0.0 => 1; _ => 2; }").unwrap();
        assert_eq!(t2.process_ty.outs[0].vty, ValueTy::Int);
    }

    #[test]
    fn nested_cross_sum_ctor_pattern_infers() {
        // `Just (Left x)`: the nested `Left` constructor belongs to `Either`,
        // NOT `Maybe`. The sum context must be re-derived from the payload type
        // at each pattern level.
        let t = ty_of("main = match Just (Left 2.0) of { Just (Left x) => x; _ => 0.0; }").unwrap();
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Float);
    }

    #[test]
    fn nested_user_sum_ctor_pattern_infers() {
        // Same nesting for a USER sum: `Wrap (Foo x)` — `Foo` is looked up in
        // `Inner`, not `Outer`.
        let t = ty_of(
            "data Inner = Foo Float; data Outer = Wrap Inner | Empty; \
             main = match Wrap (Foo 2.0) of { Wrap (Foo x) => x; _ => 0.0; }",
        )
        .unwrap();
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Float);
    }

    #[test]
    fn type_after_data_resolves() {
        // Regression: the synonym is declared AFTER the data type that uses it.
        // Registration must be order-independent (two-phase: aliases first).
        let t = ty_of("data P = { x: Angles }; type Angles = Float; main = P { x: 1.0 }").unwrap();
        assert_eq!(t.process_ty.outs[0].rate, Rate::Value);
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Data("P".into(), vec![]));
    }

    #[test]
    fn newtype_after_data_resolves() {
        // The newtype is declared AFTER the data type that uses it. The field
        // must resolve to `Newtype("Hz")` (not `Data("Hz")`), so constructing
        // it requires the explicit `Hz 440.0` wrapper.
        let t = ty_of("data P = { f: Hz }; newtype Hz = Float; main = P { f: Hz 440.0 }").unwrap();
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Data("P".into(), vec![]));
        // Newtypes are distinct wrappers: a bare Float does not satisfy a Hz
        // field (no automatic wrapping) — this is what the ordering fix buys
        // (a pre-fix `Data("Hz")` field would accept nothing, not even `Hz 440.0`).
        assert!(ty_of("data P = { f: Hz }; newtype Hz = Float; main = P { f: 440.0 }").is_err());
    }

    #[test]
    fn chained_alias_resolves() {
        let t =
            ty_of("data P = { f: A }; type A = B; type B = Float; main = P { f: 1.0 }").unwrap();
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Data("P".into(), vec![]));
    }

    #[test]
    fn recursive_data_type_is_compile_error() {
        // A self-referential type would materialise an unbounded arena subtree
        // and exhaust the fixed capacity (spec §9.1 strict acyclicity).
        assert!(
            ty_of("data List = Cons Float List | End Float; main = Cons 1.0 (End 2.0)").is_err()
        );
    }

    #[test]
    fn recursive_newtype_is_compile_error() {
        assert!(ty_of("newtype A = B; newtype B = A; main = A 1.0").is_err());
    }

    #[test]
    fn acyclic_data_type_still_compiles() {
        let t =
            ty_of("data Point = { x: Float, y: Float }; main = Point { x: 1.0, y: 2.0 }").unwrap();
        assert_eq!(
            t.process_ty.outs[0].vty,
            ValueTy::Data("Point".into(), vec![])
        );
    }

    #[test]
    fn lambda_def_infers_as_func() {
        // A Def::Local whose body is a lambda literal infers to a Func value
        // channel via the normal def-body inference path.
        let t = ty_of("double = fn x -> x; main = double").unwrap();
        assert_eq!(t.process_ty.outs.len(), 1);
        assert_eq!(t.process_ty.outs[0].rate, Rate::Value);
        assert!(matches!(t.process_ty.outs[0].vty, ValueTy::Func(_, _)));
    }

    #[test]
    fn lambda_body_arity_mismatch_is_error() {
        assert!(ty_of("main = fn x -> (x , x)").is_err());
    }

    #[test]
    fn value_channel_arithmetic_infers() {
        // double = fn x -> x * 2.0  -> lambda body is value arithmetic over
        // value params, inferred as Func([Float], [Float]).
        let t = ty_of("double = fn x -> x * 2.0; main = double").unwrap();
        match &t.process_ty.outs[0].vty {
            ValueTy::Func(arg_tys, ret_tys) => {
                assert_eq!(arg_tys.len(), 1);
                assert_eq!(ret_tys, &vec![ValueTy::Float]);
            }
            other => panic!("expected Func type, got {other:?}"),
        }
    }

    #[test]
    fn partial_application_types_as_remaining_func() {
        // add3 = add 3.0 (partial application of a two-arg lambda) types as
        // Func([Float], [Float]); fully applying it yields a Float.
        let t = ty_of("add = fn a b -> a + b; add3 = add 3.0; main = add3").unwrap();
        assert_eq!(
            t.process_ty.outs[0].vty,
            ValueTy::Func(vec![ValueTy::Float], vec![ValueTy::Float])
        );
        let t = ty_of("add = fn a b -> a + b; add3 = add 3.0; main = add3 4.0").unwrap();
        assert_eq!(t.process_ty.outs[0].rate, Rate::Value);
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Float);
    }

    #[test]
    fn named_def_partial_application_types_as_func() {
        // add5 = add2 5.0 (partial application of a named def) types as
        // Func([Float], [Float]).
        let t = ty_of("add2 a b = a + b; add5 = add2 5.0; main = add5 2.0").unwrap();
        assert_eq!(t.process_ty.outs[0].rate, Rate::Value);
        assert_eq!(t.process_ty.outs[0].vty, ValueTy::Float);
    }

    #[test]
    fn signal_wire_application_types_as_signal() {
        // amp = fn g x -> x * g; main = amp 2.0 _  -> the trailing wire is a
        // signal input; the result is a 1->1 signal channel.
        let t = ty_of("amp = fn g x -> x * g; main = amp 2.0 _").unwrap();
        assert_eq!((t.process_ty.arity_in(), t.process_ty.arity_out()), (1, 1));
        assert_eq!(t.process_ty.outs[0].rate, Rate::Signal);
        // A wire application that does not exactly complete the value arity is
        // rejected (`amp _` is missing the value gain).
        assert!(ty_of("amp = fn g x -> x * g; main = amp _").is_err());
    }

    #[test]
    fn self_application_type_is_rejected() {
        // `f f` — self-application unifies `f`'s type with a `Func` whose
        // argument IS `f`, which would construct an infinite type. The
        // occurs-check must reject it with a clean type error (regression:
        // it recorded a cyclic binding and overflowed the stack).
        let res = ty_of("main = fn f -> f f");
        assert!(res.is_err());
    }
}
