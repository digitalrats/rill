//! Algorithm-W-style inference over scalar types, with bottom-up arity
//! synthesis and combinatorial arity checking.
//!
//! All binding groups (top-level, `where`, `let`) use mutual recursion:
//! every name in the group is visible to every body.

use std::collections::{HashMap, HashSet};

use super::ty::{
    ArrowTy, Block, Channel, DataInfo, InstanceInfo, Rate, Scalar, Scheme, Subst, TypeEnv,
    TypeVarId, TypeclassInfo, ValueTy,
};
use super::unify::{unify_scalar, unify_value};
use crate::ast::{sig_name, Def, Expr, Program};
use crate::builtin::{ParamType, SignatureSource};
use crate::error::{CompileError, Span};

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
        Expr::FieldProject { record, .. } => collect_static_calls(record, src, bound, nodes, out),
        Expr::FieldUpdate { record, value, .. } => {
            collect_static_calls(record, src, bound, nodes, out);
            collect_static_calls(value, src, bound, nodes, out);
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            collect_static_calls(scrutinee, src, bound, nodes, out);
            for (_, params, body) in arms {
                let mut inner = bound.clone();
                for p in params {
                    inner.insert(p.name.clone());
                }
                collect_static_calls(body, src, &inner, nodes, out);
            }
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

/// Validate every instance's method bodies at compile time, even when the
/// instance is never called. Each body must infer to a single value channel
/// (or a bare literal) with its parameter bound to a value of the instance's
/// bound type. The recursion guard applies here too, so self-inlining bodies
/// are rejected even when the instance is dead code.
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
        let info = ctx
            .env
            .instances
            .get(class.as_str())
            .and_then(|by_ty| by_ty.get(ty_name.as_str()))
            .cloned()
            .unwrap();
        let param_vty = ctx.env.vty_of_name(&ty_name);
        for (mname, (param, body)) in &info.methods {
            let key = (class.clone(), ty_name.clone(), mname.clone());
            if ctx.method_lifting.contains(&key) {
                return Err(CompileError::Type {
                    msg: format!("recursive typeclass method `{mname}` for type `{ty_name}`"),
                    span: body.span(),
                });
            }
            ctx.method_lifting.insert(key.clone());
            let saved = ctx.locals.clone();
            if let Some(p) = param {
                ctx.locals
                    .insert(p.clone(), ArrowTy::value_channel(param_vty.clone()));
            }
            let res = infer_method_value_vty(ctx, body, "body");
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
    for def in &program.defs {
        match def {
            Def::TypeAlias { name, target, .. } => {
                env.type_aliases.insert(name.clone(), target.clone());
            }
            Def::Newtype { name, target, .. } => {
                env.newtypes.insert(name.clone(), target.clone());
            }
            Def::Typeclass {
                name, var, methods, ..
            } => {
                env.typeclasses.insert(
                    name.clone(),
                    TypeclassInfo {
                        var: var.clone(),
                        arity: methods
                            .iter()
                            .map(|(_, sig)| TypeEnv::class_var_arity(var, sig))
                            .max()
                            .unwrap_or(0),
                        methods: methods.clone(),
                    },
                );
            }
            Def::Instance {
                class,
                ty,
                method_bodies,
                ..
            } => {
                let mut methods: HashMap<String, (Option<String>, Expr)> = HashMap::new();
                for (mname, param, body) in method_bodies {
                    let binding = param.clone().map(|p| p.name.clone());
                    methods.insert(mname.clone(), (binding, body.clone()));
                }
                env.instances.entry(class.clone()).or_default().insert(
                    ty.clone(),
                    InstanceInfo {
                        class: class.clone(),
                        ty: ty.clone(),
                        methods,
                    },
                );
            }
            _ => {}
        }
    }
    for def in &program.defs {
        match def {
            Def::Data { name, fields, .. } => {
                let fields_ty = fields
                    .iter()
                    .map(|(f, t)| (f.clone(), env.vty_of_name(&sig_name(t))))
                    .collect();
                env.data_types
                    .insert(name.clone(), DataInfo::Record(fields_ty));
            }
            Def::Sum { name, ctors, .. } => {
                let ctors_ty = ctors
                    .iter()
                    .map(|(c, ts)| {
                        (
                            c.clone(),
                            ts.iter().map(|t| env.vty_of_name(&sig_name(t))).collect(),
                        )
                    })
                    .collect();
                env.data_types.insert(name.clone(), DataInfo::Sum(ctors_ty));
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
    if def.params().is_empty() {
        ctx.locals.clear();
        return infer_expr(ctx, def.body());
    }
    let saved_subst = ctx.subst.clone();
    let saved_next = ctx.next;
    ctx.locals.clear();
    for p in def.params() {
        ctx.locals
            .insert(p.name.clone(), ArrowTy::uniform(0, 1, Scalar::Float));
    }
    match infer_expr(ctx, def.body()) {
        Ok(t) => Ok(t),
        Err(_) => {
            ctx.subst = saved_subst;
            ctx.next = saved_next;
            ctx.locals.clear();
            for p in def.params() {
                let v = ctx.next;
                ctx.next += 1;
                ctx.locals
                    .insert(p.name.clone(), ArrowTy::value_channel(ValueTy::Var(v)));
            }
            infer_expr(ctx, def.body())
        }
    }
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

/// Infer the diagram type of an expression, synthesizing concrete arities.
fn infer_expr(ctx: &mut Ctx<'_>, e: &Expr) -> Result<ArrowTy, CompileError> {
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
                ValueTy::Data(name, _) => match ctx.env.data_types.get(name.as_str()) {
                    Some(DataInfo::Record(fields)) => {
                        let fty = fields
                            .iter()
                            .find(|(f, _)| f == field)
                            .map(|(_, t)| t.clone());
                        match fty {
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
            // `_` in match-scrutinee position is the identity value wire: it is
            // a Value channel with a fresh value type, pinned to the arm-derived
            // sum type below by unification.
            let st = match scrutinee.as_ref() {
                Expr::Wire(_) => {
                    let v = ctx.next;
                    ctx.next += 1;
                    ArrowTy::value_channel(ValueTy::Var(v))
                }
                _ => infer_expr(ctx, scrutinee)?,
            };
            // The scrutinee must be a sum value.
            if st.arity_out() != 1 || st.outs[0].rate != Rate::Value {
                return Err(CompileError::Type {
                    msg: "match scrutinee must be a sum value".into(),
                    span: *span,
                });
            }
            let scrutinee_vty = st.outs[0].vty.clone();
            let scrutinee_sum = match &scrutinee_vty {
                ValueTy::Data(name, _) => Some(name.clone()),
                _ => None,
            };
            // Derive the sum type name: every arm's constructor must belong to
            // the same sum type. A concrete scrutinee type disambiguates ctor
            // names shared across sum types; otherwise an ambiguous or unknown
            // constructor is a deterministic error (no HashMap-order hazard).
            let mut candidates: Option<Vec<String>> = None;
            for (ctor, _, _) in arms {
                let mut per_ctor = sum_types_with_ctor(ctx, ctor);
                if let Some(sn) = &scrutinee_sum {
                    per_ctor.retain(|n| n == sn);
                }
                if per_ctor.is_empty() {
                    return Err(CompileError::Type {
                        msg: format!("unknown constructor `{ctor}`"),
                        span: *span,
                    });
                }
                candidates = Some(match candidates {
                    None => per_ctor,
                    Some(acc) => acc.into_iter().filter(|n| per_ctor.contains(n)).collect(),
                });
            }
            let sum_name = match candidates {
                Some(v) if v.len() == 1 => v[0].clone(),
                Some(_) => {
                    return Err(CompileError::Type {
                        msg: "match arms use constructors of ambiguous or different sum types"
                            .into(),
                        span: *span,
                    });
                }
                None => {
                    return Err(CompileError::Type {
                        msg: "match requires at least one arm".into(),
                        span: *span,
                    });
                }
            };
            // Pin the scrutinee's value type to the arm-derived sum type.
            unify_value(
                &scrutinee_vty,
                &ValueTy::Data(sum_name.clone(), vec![]),
                &mut ctx.subst,
                *span,
            )?;
            let mut result: Option<ArrowTy> = None;
            for (ctor, params, body) in arms {
                let payload = match sum_ctor_payload(ctx, &sum_name, ctor) {
                    Some(p) => p,
                    None => {
                        return Err(CompileError::Type {
                            msg: format!("unknown constructor `{ctor}` for `{sum_name}`"),
                            span: *span,
                        });
                    }
                };
                if params.len() > payload.len() {
                    return Err(CompileError::Type {
                        msg: format!("too many bindings for constructor `{ctor}`"),
                        span: body.span(),
                    });
                }
                let saved = ctx.locals.clone();
                for (idx, p) in params.iter().enumerate() {
                    let pty = payload.get(idx).cloned().unwrap_or(ValueTy::Float);
                    ctx.locals
                        .insert(p.name.clone(), ArrowTy::value_channel(pty));
                }
                let bt = infer_expr(ctx, body)?;
                ctx.locals = saved;
                // Each arm body must be a value expression (0→1 value channel).
                if bt.arity_in() != 0 || bt.arity_out() != 1 || bt.outs[0].rate != Rate::Value {
                    return Err(CompileError::Type {
                        msg: "match arm must be a value expression (0→1 value channel)".into(),
                        span: body.span(),
                    });
                }
                if let Some(ref acc) = result {
                    if acc.outs[0].vty != bt.outs[0].vty {
                        return Err(CompileError::Type {
                            msg: "match arms must produce the same value type".into(),
                            span: body.span(),
                        });
                    }
                } else {
                    result = Some(bt);
                }
            }
            Ok(result.unwrap_or(ArrowTy::value_channel(ValueTy::Float)))
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
            let mut arg_tys: Vec<ValueTy> = Vec::with_capacity(params.len());
            for p in params {
                let pty = ctx.fresh_vty();
                arg_tys.push(pty.clone());
                ctx.locals
                    .insert(p.name.clone(), ArrowTy::value_channel(pty));
            }
            let bt = infer_expr(ctx, body)?;
            ctx.locals = saved;
            if bt.arity_out() != 1 {
                return Err(CompileError::Type {
                    msg: "lambda body must produce one value".into(),
                    span: *span,
                });
            }
            let ret_ty = bt.outs[0].vty.clone();
            Ok(ArrowTy::value_channel(ValueTy::Func(arg_tys, vec![ret_ty])))
        }
    }
}

fn infer_ref(ctx: &mut Ctx<'_>, name: &str, span: Span) -> Result<ArrowTy, CompileError> {
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
    let ctor_sums = sum_types_with_ctor(ctx, name);
    if !ctor_sums.is_empty() {
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
    // needs an argument to select the instance.
    if let Some(class_name) = ctx.env.class_of_method(name) {
        return Err(CompileError::Type {
            msg: format!("method `{name}` of `{class_name}` requires an argument"),
            span,
        });
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

fn infer_apply(
    ctx: &mut Ctx<'_>,
    name: &str,
    args: &[Expr],
    span: Span,
) -> Result<ArrowTy, CompileError> {
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
                        for (f, e) in fields_expr {
                            let fty = fields
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
    // signature).
    if let Some(class_name) = ctx.env.class_of_method(name) {
        if args.len() != 1 {
            return Err(CompileError::Type {
                msg: format!(
                    "method `{name}` of `{class_name}` expects 1 argument, got {}",
                    args.len()
                ),
                span,
            });
        }
        let arg_vty = infer_method_value_vty(ctx, &args[0], "argument")?;
        let ty_name = match ctx.env.type_name_of_vty(&arg_vty) {
            Some(t) => t,
            None => {
                return Err(CompileError::Type {
                    msg: format!(
                        "cannot resolve method `{name}` of `{class_name}`: the argument type is not concrete"
                    ),
                    span: args[0].span(),
                });
            }
        };
        let (_, param, body) = match ctx.env.resolve_method(name, ty_name.as_str()) {
            Some(r) => r,
            None => {
                return Err(CompileError::Type {
                    msg: format!("no instance of `{class_name}` for type `{ty_name}`"),
                    span,
                });
            }
        };
        // Recursion guard: a method that inlines itself (directly or
        // transitively) is a compile error, not a stack overflow.
        let key = (class_name, ty_name.clone(), name.to_string());
        if ctx.method_lifting.contains(&key) {
            return Err(CompileError::Type {
                msg: format!("recursive typeclass method `{name}` for type `{ty_name}`"),
                span,
            });
        }
        ctx.method_lifting.insert(key.clone());
        // Bind the method parameter to the argument's value type and infer the
        // body; the resulting type is the call's value type.
        let saved = ctx.locals.clone();
        if let Some(p) = param {
            ctx.locals
                .insert(p.clone(), ArrowTy::value_channel(arg_vty.clone()));
        }
        let body_vty = infer_method_value_vty(ctx, &body, "body");
        ctx.locals = saved;
        ctx.method_lifting.remove(&key);
        let body_vty = body_vty?;
        return Ok(ArrowTy::value_channel(body_vty));
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
    // A first-class function value bound to a LOCAL (a lambda parameter, a
    // let/match binding): `f x` where `f` is a `Func`. The local's value type
    // is unified with a fresh `Func` signature of the applied arity — this both
    // type-checks the arguments AND binds an as-yet unresolved lambda parameter
    // to a `Func` (value variables record through `subst.value_map`). A local
    // already resolved to a concrete `Func` (a HOF parameter applied twice)
    // applies against that signature, so an arity inconsistency is rejected.
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
    fn match_arms_must_be_value_channels() {
        assert!(ty_of("data Shape = Circle Float; main = match _ of { Circle r => 1.0 }").is_err());
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
