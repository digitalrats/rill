//! Algorithm-W-style inference over scalar types, with bottom-up arity
//! synthesis and combinatorial arity checking.
//!
//! All binding groups (top-level, `where`, `let`) use mutual recursion:
//! every name in the group is visible to every body.

use std::collections::{HashMap, HashSet};

use super::ty::{
    concat_map_result_elem, ArrowTy, Block, DataInfo, Scheme, Subst, TypeEntry, TypeEnv, TypeVarId,
    ValueTy,
};
use super::unify::unify_value;
use crate::ast::{Def, Expr, MatchArm, Param, Pattern, Program};
use crate::error::{CompileError, Span};
use crate::mode::Mode;
use crate::reduce::pattern_vars;

/// The typed result of inference: the program's definitions plus the resolved
/// type of the output and the final substitution.
#[derive(Debug, Clone)]
pub struct TypedProgram {
    /// The program, desugared (foreign-fn combinator operands bound to signal
    /// wires) but otherwise unchanged; the caller's source is untouched.
    pub program: Program,
    /// Resolved type schemes of every top-level definition, keyed by name.
    /// The entry definition's scheme (`def_schemes[entry]`) replaces the old
    /// `process_ty`: lowering reads the entry's resolved type from here.
    pub def_schemes: HashMap<String, Scheme>,
    /// Names of closed top-level definitions (CAFs): zero λ-parameters and
    /// zero signal input channels. Referenced by name, shared across the graph.
    pub cafs: HashSet<String>,
    /// The compile-time type environment (aliases, newtypes, data types),
    /// built once here and shared with lowering.
    pub type_env: TypeEnv,
    /// Resolved λ-parameter value types of every def whose parameters live in
    /// its scheme, keyed by definition name: named lambda-literal definitions
    /// (`take = \n -> \xs -> …`, a curried λ-nest) and signature-less
    /// single-parameter defs (`first p = p.x`, a flat `Thunk([Point],[Double])`).
    /// Lowering types fragment-local parameter registers with these so a
    /// higher-order parameter is a `Func`, a record parameter its `Data` type,
    /// and field projection / function dispatch resolve at compile time.
    pub fn_param_tys: HashMap<String, Vec<ValueTy>>,
    /// Static RESULT value type of every shared-cell def (a named λ-parameter
    /// function or a self-recursive local), from the final substitution.
    /// Lowering types the def's shared cell with this so a recursive reference
    /// inside the body (`tail fibs`) resolves its static type (List) instead of
    /// the Double fallback.
    pub shared_cell_result_tys: HashMap<String, ValueTy>,
    /// Named tape declarations (`name = tape_loop <capacity>`) extracted from
    /// the AST before inference (see [`crate::lib::extract_resources`]). Lowering
    /// resolves a resource param's `Ref(name)` against these to assign the
    /// tape INDEX; a `Ref` absent from the map stays a name-based resource
    /// binding, which seating rejects as an undeclared resource.
    pub tape_decls: Vec<(String, usize)>,
    /// Span (`start`, `end`) of every `runST` / `runSTArray` application whose
    /// body is a real `ST s a` computation → the region's result type (`a` for
    /// `runST`, the frozen `Array i e` for `runSTArray`). Lowering emits the
    /// `RunST`/`RunSTArray` effect and returns this type; the lowered `ST`
    /// record type alone does not carry the payload (it appears only in the
    /// `unST` field's type).
    pub runst_payload_tys: HashMap<(usize, usize), ValueTy>,
    /// The evaluation mode this program was compiled under (the file pragma,
    /// or the API-supplied mode when the file carries none).
    pub mode: Mode,
    /// The dictionary the type checker SELECTED at every class-method call
    /// position, keyed by the call's source span: `(class, evidence)`.
    ///
    /// This is design §3.0's third consequence — inference proves the
    /// constraint and RECORDS the instance choice, so lowering inserts the
    /// dictionary the type checker chose instead of re-deriving it from
    /// signatures.
    ///
    /// The evidence is one of:
    ///
    /// - a **concrete type** (`Int`, `Pair Int Int`) — the call site solved the
    ///   class variable and that instance's dictionary is the argument;
    /// - **`ValueTy::Dict(g)`** — the call sits inside a class-constrained
    ///   body, so the class variable is ABSTRACT and the witness is the GIVEN
    ///   constraint's incoming dictionary for class `g` (the enclosing
    ///   `C g`). `g` differs from the method's own class only through a
    ///   superclass projection (`Ord a`'s dictionary carries `Eq a`'s), which
    ///   is exactly the slot lowering projects.
    ///
    /// A position with no entry has no dictionary argument (for example a
    /// primitive-kernel call selected outside a given context still records
    /// its concrete type; only positions inference never reached are absent).
    /// Prelude bodies are typed in a SEPARATE context whose spans index the
    /// prelude text, so their selections are deliberately NOT merged here — a
    /// span from that text would not name a position in the user's source.
    pub dict_selections: HashMap<Span, (String, ValueTy)>,
}

/// Inference context: fresh var supply, definition schemes, local bindings,
/// and a signature source for built-in resolution.
struct Ctx {
    next: TypeVarId,
    subst: Subst,
    defs: HashMap<String, Scheme>,
    /// Definition name → body expression, for every non-declaration def that
    /// has been registered. Used to resolve a func value's transitively
    /// referenced definition at application time (see [`func_target`]).
    def_bodies: HashMap<String, Expr>,
    locals: HashMap<String, ArrowTy>,
    /// The compile-time type environment (aliases, newtypes, data types).
    env: TypeEnv,
    /// Recursion guard for typeclass method inlining: resolved
    /// (class, type, method) calls currently on the expansion path.
    method_lifting: HashSet<(String, String, String)>,
    /// Head-argument bindings of a constraint instance (`instance (Monad m) =>
    /// Arrow (Kleisli m)`): head arg name → concrete type taken from the
    /// call-site concrete container's LEADING args (`Kleisli Maybe Double Double`
    /// → `m := Maybe`). The constraints discharge against these, and the
    /// instance body's own record construction unifies against the same
    /// concrete type via the expected-type threading.
    type_var_bindings: HashMap<String, ValueTy>,
    /// Names of SIGNATURE-LESS CURRIED definitions (`at a i = a ! i`, whose
    /// body is a λ-nest). Such a def is typed as a VALUE (its λ-chain), so its
    /// scheme carries the structured parameter shapes the body recovered
    /// (`Array i e` from the constrained `index`) — shapes the user never
    /// wrote. A call-site mismatch then dumps those recovered shapes as if
    /// they were the user's own source. The M10 diagnostic consults this set
    /// to point at the missing annotation instead.
    retry_inferred_defs: HashSet<String>,
    /// Names of definitions that carry an explicit `::` signature.
    ///
    /// A signature IS the call-site contract: `f x :: Double -> Double` rejects a
    /// `String` argument outright. A signature-LESS definition's parameter
    /// types are inferred from its body, where rill's numeric defaulting pins
    /// an otherwise-unconstrained operand to `Double` (`sum2 p = p.first +
    /// p.second` → `Pair Double Double -> Double`). That defaulted `Double` stands
    /// for a `Num`-polymorphic parameter, so the call-site argument check must
    /// not treat it as the concrete type the user wrote — see
    /// [`numeric_default_widens`].
    sig_defs: HashSet<String>,
    /// The evaluation mode (strict/complete). Complete mode's value-track
    /// retry re-registers a recursive def's scheme with value-rate channels
    /// (§: recursion is the lazy value track).
    mode: Mode,
    /// Span (`start`, `end`) of every `runST` / `runSTArray` application whose
    /// body is a real `ST s a` computation → the region's result type (`a` for
    /// `runST`, the frozen `Array i e` for `runSTArray`). Lowering has no
    /// unification substitution, so this is the only record of the result
    /// type: the `ST` record's own lowered type carries no argument (its `a`
    /// lives only inside the `unST` field type).
    runst_payload_tys: HashMap<(usize, usize), ValueTy>,
    /// How many NESTED definition groups (`let`/`where`) enclose the expression
    /// being inferred. A nested group's bodies see the enclosing λ-parameters,
    /// so `infer_def_body` keeps `ctx.locals` instead of clearing it.
    nested_group_depth: usize,
    /// The class constraints GIVEN to the body currently being inferred, in
    /// declaration order. Inside `mk :: Ix i => …` this holds one `Ix`/`i`
    /// witness, and a method call of `Ix` resolves against that GIVEN witness
    /// instead of demanding a concrete call-site type (design §3.0: the body is
    /// typed ONCE, under the abstract dictionary).
    ///
    /// Empty outside a constrained body. Saved and restored around every body
    /// inference, exactly like `locals`.
    given: Vec<GivenVar>,
    /// The dictionary the type checker selected at each method call position,
    /// keyed by the call's span (see [`TypedProgram::dict_selections`]).
    dict_selections: HashMap<Span, (String, ValueTy)>,
}

impl Ctx {
    /// Allocate a fresh, unbound type-variable id (shared by the scalar and
    /// value sides — both live in the same id space).
    fn fresh_id(&mut self) -> TypeVarId {
        let v = self.next;
        self.next += 1;
        v
    }

    fn fresh_vty(&mut self) -> ValueTy {
        ValueTy::Var(self.fresh_id())
    }

    /// Instantiate a scheme at a use site: every quantified variable becomes a
    /// fresh unbound variable, and the scheme's CLASS CONTEXT travels with it.
    ///
    /// The returned [`SchemeConstraint`]s are the constraints as they are in
    /// THIS instantiation: each carries the freshened variable from the same
    /// `vremap` the body type went through, so the constraint and the type
    /// still name one variable. That is what makes a constraint genuinely
    /// per-instantiation: without it, two uses of one polymorphic scheme would
    /// share a variable id and the second call site's solution would silently
    /// overwrite the first's dictionary.
    fn instantiate(&mut self, scheme: &Scheme) -> ArrowTy {
        self.instantiate_with_constraints(scheme).0
    }

    /// [`Self::instantiate`] plus the instantiated class context.
    fn instantiate_with_constraints(&mut self, scheme: &Scheme) -> (ArrowTy, SchemeContext) {
        let mut vremap: HashMap<TypeVarId, TypeVarId> = HashMap::new();
        for v in &scheme.vars {
            // A fresh id for the value side: a generalized value-type variable
            // becomes a new unbound var at every instantiation site (rank-N
            // value polymorphism).
            let vf = self.fresh_id();
            vremap.insert(*v, vf);
        }
        let block = |b: &Block| value_instantiate(b, &vremap);
        // A constraint's variable is quantified by the scheme (Task 2's
        // invariant), so it is in `vremap`. A constraint whose variable the
        // scheme does not quantify is already concrete and needs no dictionary;
        // it keeps its id so the pairing stays visible rather than being
        // silently dropped.
        let constraints: SchemeContext = scheme
            .constraints
            .iter()
            .map(|(class, var)| (class.clone(), vremap.get(var).copied().unwrap_or(*var)))
            .collect();
        (
            ArrowTy {
                vty: block(&scheme.ty.vty),
            },
            constraints,
        )
    }

    fn free_vars(&self, t: &ArrowTy) -> Vec<TypeVarId> {
        let mut acc = Vec::new();
        // Value channels generalize their free value-type variables, so a
        // polymorphic value function (`id2 = fn x -> x`) is instantiated fresh
        // at every call site.
        for v in value_free_vars(&t.vty, &self.subst) {
            if !acc.contains(&v) {
                acc.push(v);
            }
        }
        acc
    }
}

/// Standard HM let-generalization: the quantified variables of a `let`/`where`
/// definition are the body's free variables MINUS those free in the enclosing
/// environment (lambda/match parameters). Generalizing over environment-free
/// variables is unsound: `let u = k.unKleisli in u x` inside `\k -> …` would
/// re-instantiate `u`'s type at each use with FRESH `m a b`, severing the link
/// between the parameter's container (`k : Kleisli m a b`) and the result
/// (`u x : m b`) — so a result-directed method chain (`apply (compose (arr f)
/// (arr g)) 3.0`) never learns its monad. A top-level def (empty environment)
/// still generalizes fully and stays polymorphic at every call site.
fn free_vars_quantified(ctx: &Ctx, t: &ArrowTy) -> Vec<TypeVarId> {
    let all = ctx.free_vars(t);
    let mut env: Vec<TypeVarId> = Vec::new();
    for (_, at) in ctx.locals.iter() {
        for v in value_free_vars(&at.vty, &ctx.subst) {
            if !env.contains(&v) {
                env.push(v);
            }
        }
    }
    all.into_iter().filter(|v| !env.contains(v)).collect()
}

/// A class constraint GIVEN to the body currently being inferred (design §3.0:
/// `mk :: Ix i => …` types its body once, with `Ix i` GIVEN rather than solved).
///
/// The witness is the abstract class variable. A method call whose class
/// variable is witnessed by this constraint is typed against the incoming
/// dictionary instead of demanding a concrete instance.
#[derive(Debug, Clone)]
struct GivenVar {
    /// The class the constraint names (`Ix`, `Ord`, `Monad`).
    class: String,
    /// The class variable's name, as the definition's signature spells it.
    name: String,
    /// The variable's id in the signature-converted body parameter types.
    /// `None` for a CONSTRUCTOR class (`Functor f`, `Monad m`), whose variable
    /// the signature conversion spells as an application HEAD by name
    /// (`App("m", [a])`) rather than as a bare variable id.
    id: Option<TypeVarId>,
}

impl GivenVar {
    /// Whether `t` is this given witness — the evidence that a method call's
    /// class variable IS this constraint rather than an unrelated unsolved
    /// variable.
    ///
    /// An arity-0 class (`Ix i`) is witnessed by the variable occurring
    /// ANYWHERE in the type (`Pair i i` for `rangeSize`, bare `a` for `==`). A
    /// constructor class (`Functor f`) is witnessed by a type whose head is the
    /// class variable (`f a`), which the signature conversion spells with the
    /// variable's name.
    fn witness_of(&self, t: &ValueTy) -> bool {
        match self.id {
            // An arity-0 class variable (`Ix i`) is witnessed by the variable
            // occurring anywhere in the type.
            Some(id) => vty_mentions_tyvar(t, id),
            // A constructor-class variable (`Functor f`) is witnessed by a type
            // whose head is that class variable.
            None => matches!(
                t,
                ValueTy::App(n, _) | ValueTy::Data(n, _) | ValueTy::Newtype(n, _)
                    if *n == self.name
            ),
        }
    }

    /// The abstract type this given stands for, in the spelling the signature
    /// conversion gives the class variable: a bare variable id for an arity-0
    /// class, the named application head for a constructor class.
    fn abstract_ty(&self) -> ValueTy {
        match self.id {
            Some(id) => ValueTy::Var(id),
            None => ValueTy::App(self.name.clone(), Vec::new()),
        }
    }
}

/// The class context of a scheme INSTANTIATED at one use site: `(class, its
/// freshened type variable)`.
///
/// [`Ctx::instantiate_with_constraints`] produces this alongside the
/// instantiated type, so a caller learns which constraints the particular use
/// must discharge.
type SchemeContext = Vec<(String, TypeVarId)>;

/// The GIVEN constraint that witnesses a method call of class `class` against
/// `evidence`.
///
/// A class's constraint is witnessed by a given of that class OR of any class
/// that ENTAILS it (System FC: the superclass dictionary is a field of the
/// subclass one), so inside `Ord a =>` a call to `Eq`'s `(==)` resolves against
/// the `Ord` witness. The recorded evidence is the GIVEN class, which is the
/// dictionary that actually arrives — projecting the superclass slot is
/// lowering's job, not a second lookup here.
///
/// `None` when no given witnesses `evidence`: the call is then an ordinary
/// call-site dispatch (or an error, if no concrete type solved it either).
fn given_witness<'a>(
    given: &'a [GivenVar],
    env: &TypeEnv,
    class: &str,
    evidence: &ValueTy,
) -> Option<&'a GivenVar> {
    given
        .iter()
        .find(|g| env.entails(&g.class, class) && g.witness_of(evidence))
}

/// The GIVEN constraint (if any) that types a method call, decided by the same
/// selector the concrete dispatcher uses: the expected RESULT type for a
/// result-directed method, otherwise the class-var argument's static type.
///
/// This is the "resolve against the GIVEN constraint" half of design §3.0. It
/// returns a witness only when the call's selector IS one of the body's
/// constraints, so a concrete call inside a constrained body (`List`'s `length`
/// on a list, an `Ix Int` call on a literal bounds pair) still dispatches on its
/// own concrete type.
fn given_for_method_call(
    ctx: &mut Ctx,
    class_name: &str,
    class_var: &str,
    sig: &crate::ast::TypeExpr,
    args: &[Expr],
    expected: Option<&ValueTy>,
) -> Result<Option<GivenVar>, CompileError> {
    if ctx.given.is_empty() {
        return Ok(None);
    }
    let result_directed = matches!(
        sig,
        crate::ast::TypeExpr::TFunc(targs, _)
            if !targs.is_empty() && targs.iter().all(|a| !type_expr_mentions(a, class_var))
    );
    if result_directed {
        let Some(exp) = expected else {
            return Ok(None);
        };
        let exp = ctx.subst.resolve_value(exp);
        return Ok(given_witness(&ctx.given, &ctx.env, class_name, &exp).cloned());
    }
    let arity = ctx
        .env
        .typeclasses
        .get(class_name)
        .map(|c| c.arity)
        .unwrap_or(0);
    let selector_idx = if arity == 0 {
        class_var_selector_index(sig, class_var, args.len(), |i| {
            is_bare_nullary_method_ref(ctx, &args[i], class_name)
        })
    } else {
        ctx.env.class_var_arg_index(sig, class_var)
    };
    let Some(idx) = selector_idx.filter(|i| *i < args.len()) else {
        return Ok(None);
    };
    let at = infer_method_value_vty(ctx, &args[idx], "argument")?;
    let at = ctx.subst.resolve_value(&at);
    Ok(given_witness(&ctx.given, &ctx.env, class_name, &at).cloned())
}

/// Whether a call of the DUAL name `name` (a class method that is also a
/// class-constrained definition — `index` is both `Ix`'s offset method and the
/// array accessor `!`) routes to the METHOD because the body's GIVEN constraint
/// witnesses its class variable.
///
/// The test is the method's own class-var PATTERN (`Pair i i` for `Ix`), not
/// mere mention: inside `index :: Ix i => Array i e -> i -> e` the array's index
/// argument IS the given variable, but the accessor's first argument is an
/// `Array`, which the pattern cannot match — so that call still reaches the
/// definition, exactly as outside a constrained body.
fn given_routes_to_method(ctx: &Ctx, name: &str, selector_ty: &ValueTy) -> bool {
    if ctx.given.is_empty() {
        return false;
    }
    let Some(class_name) = ctx.env.class_of_method(name) else {
        return false;
    };
    let Some(info) = ctx.env.typeclasses.get(&class_name) else {
        return false;
    };
    let class_var = info.var.clone();
    let Some(sig) = info
        .methods
        .iter()
        .find(|(m, _)| m == name)
        .map(|(_, s)| s.clone())
    else {
        return false;
    };
    // The method's class-var parameter PATTERN with the given variable in the
    // class-var position (`Pair i i` for `Ix.index`). A method with no such
    // pattern (`mempty: m`) has nothing to match, so a dual name cannot occur.
    let Some(arg_idx) = crate::types::ty::class_var_param_pattern(&sig, &class_var) else {
        return false;
    };
    let inner = match &sig {
        crate::ast::TypeExpr::TConstrained(_, inner) | crate::ast::TypeExpr::TForall(_, inner) => {
            inner.as_ref()
        }
        other => other,
    };
    let arg_te = match inner {
        crate::ast::TypeExpr::TFunc(targs, _) => targs.get(arg_idx),
        _ => None,
    };
    let Some(arg_te) = arg_te else {
        return false;
    };
    for g in &ctx.given {
        if !ctx.env.entails(&g.class, &class_name) {
            continue;
        }
        // A constructor-class given (`Functor f`) carries no variable id; the
        // dual names in the prelude (`index`) are arity-0 only.
        let Some(id) = g.id else {
            continue;
        };
        let mut next = 0u32;
        let pattern = crate::types::infer::class_var_pattern_vty(
            &ctx.env,
            &class_var,
            id,
            arg_te,
            &mut || {
                next += 1;
                ValueTy::Var(next)
            },
        );
        let mut probe = Subst::default();
        if unify_value(&pattern, selector_ty, &mut probe, Span::new(0, 0)).is_err() {
            continue;
        }
        // An ABSTRACT match leaves the given variable unbound; a match that
        // SOLVED it to a concrete type is an ordinary concrete dispatch (the
        // body happens to call the method at one instantiation) and must not be
        // reported as a given route.
        if matches!(probe.resolve_value(&ValueTy::Var(id)), ValueTy::Var(_)) {
            return true;
        }
    }
    false
}

/// Type a class-method call under a GIVEN constraint: the method's type is its
/// class signature with the class variable kept ABSTRACT (the given's own
/// variable), never a concrete instance.
///
/// This is the `Ix i =>` judgement applied to one call: `inRange b ix` inside
/// `checkedOffset :: Ix i => …` has type `Pair i i -> i -> Bool` for the SAME
/// abstract `i` the enclosing body was annotated with, so the call composes with
/// the body's parameters. Nothing is selected: the dictionary is the caller's.
///
/// The class variable is spelled the way the definition's own signature
/// conversion spells it, so the call's type meets the body's types:
///
/// - arity-0 (`Ix i`): the bare variable id (`Pair i i` → `Pair Var(i) Var(i)`);
/// - constructor class (`Functor f`): the abstract application head, which the
///   signature conversion spells as the class variable's NAME (`f a` →
///   `App("f", [a])`), so keeping the name keeps the two in step.
fn infer_method_under_given(
    ctx: &mut Ctx,
    class_var: &str,
    given: &GivenVar,
    sig: &crate::ast::TypeExpr,
    args: &[Expr],
    span: Span,
) -> Result<ArrowTy, CompileError> {
    let cv_subst = ClassVarSubst::Ty(given.abstract_ty());
    let env = ctx.env.clone();
    let (param_tys, ret) =
        signature_param_and_result_tys_conv(&env, class_var, &cv_subst, sig, || ctx.fresh_vty());
    if args.len() > param_tys.len() {
        return Err(CompileError::Type {
            msg: format!(
                "method expects {} argument(s), got {}",
                param_tys.len(),
                args.len()
            ),
            span,
        });
    }
    for (i, a) in args.iter().enumerate() {
        if let Some(pt) = param_tys.get(i) {
            let at = infer_method_value_vty_expected(ctx, a, "argument", Some(pt.clone()))?;
            unify_value(pt, &at, &mut ctx.subst, a.span())?;
        }
    }
    let ret = ret.unwrap_or_else(|| ctx.fresh_vty());
    let rest: Vec<ValueTy> = param_tys[args.len().min(param_tys.len())..].to_vec();
    // A partial application keeps the remaining parameters as a function value
    // (Haskell 2010 §4.3.3); a full application yields the result.
    if rest.is_empty() {
        Ok(ArrowTy::value_channel(ret))
    } else {
        Ok(ArrowTy::value(nest_thunk(&rest, vec![ret])))
    }
}

/// The concrete type a dispatch selected, for [`record_selection`]. A
/// constructor-class dispatch (`ClassVarSubst::Ctor`) names the constructor
/// only — its head arguments come from the call site and are not part of the
/// selection — so it is recorded as the bare application.
fn selection_evidence(cv: &ClassVarSubst) -> ValueTy {
    match cv {
        ClassVarSubst::Ty(t) => t.clone(),
        ClassVarSubst::Ctor(c) => ValueTy::App(c.clone(), Vec::new()),
        ClassVarSubst::Var(id) => ValueTy::Var(*id),
    }
}

/// Record the dictionary the type checker selected at a method call position.
///
/// The evidence is a concrete [`ValueTy`] when a call site solved the class
/// variable, and `ValueTy::Dict(g)` when the call sits under a GIVEN constraint
/// `C g` (the abstract-dictionary case). Under a given constraint the class
/// variable IS the given variable, so the incoming dictionary is the only
/// witness that exists — there is no concrete type to record yet.
///
/// Recording is keyed by the call's span. Nothing is written for a position
/// with no witness: a call whose class variable neither the given context
/// witnesses nor a concrete type solves is a typing error before this point.
fn record_selection(ctx: &mut Ctx, span: Span, class: &str, evidence: &ValueTy) {
    ctx.dict_selections
        .insert(span, (class.to_string(), evidence.clone()));
}

///
/// The unary value model represents a multi-parameter function as a nest of
/// single-argument thunks (`\a -> \b -> …`), so a call site that applies `n`
/// arguments peels `n` layers. A non-`Thunk`, or a `Thunk` whose single result
/// is not itself a `Thunk`, flattens to itself.
fn flatten_thunk(vty: &ValueTy) -> (Vec<ValueTy>, Vec<ValueTy>) {
    let mut args: Vec<ValueTy> = Vec::new();
    let mut cur = vty.clone();
    loop {
        match cur {
            ValueTy::Thunk(caps, res) => {
                args.extend(caps);
                if res.len() == 1 {
                    if let ValueTy::Thunk(..) = &res[0] {
                        cur = res[0].clone();
                        continue;
                    }
                }
                return (args, res);
            }
            other => {
                if args.is_empty() {
                    return (Vec::new(), vec![other]);
                }
                return (args, vec![other]);
            }
        }
    }
}

/// The number of leading λs of a definition body.
///
/// The parser's currying pass clears a multi-parameter definition's parameter
/// list and wraps the body in nested lambdas (`take n xs = …` becomes
/// `take = \n -> \xs -> …`), so a definition's true λ-arity is its remaining
/// `params().len()` plus the body's leading λs.
///
/// Also consulted by lowering to align a recovered λ-parameter list with a
/// definition's λ-nest depth before specializing it per call site.
pub(crate) fn leading_lambda_count(body: &Expr) -> usize {
    match body {
        Expr::Lambda { body, .. } => 1 + leading_lambda_count(body),
        _ => 0,
    }
}

/// Wrap a flat argument list into a curried nest: `([a, b], [r])` →
/// `Thunk([a], [Thunk([b], [r])])`.
///
/// The unary value model represents a multi-parameter function as a nest of
/// single-argument thunks, so a reconstructed signature must be nested: a flat
/// `Thunk([a, b], [r])` would not unify with an inferred lambda type
/// (`\a -> \b -> …` infers to the nest).
fn nest_thunk(arg_tys: &[ValueTy], ret_tys: Vec<ValueTy>) -> ValueTy {
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

/// Consume `n` leading λ-captures from a definition's value type: for a
/// `Thunk(captures, result)` with `n` applied parameters, the remaining value
/// type is the result (or a `Thunk` over the leftover captures). A non-`Thunk`
/// (a plain value) is returned unchanged. Nested curried chains are flattened
/// first, so `applied_result(Thunk([a], [Thunk([b], [r])]), 2)` is `r`.
fn applied_result(vty: &ValueTy, n: usize) -> ValueTy {
    if n == 0 {
        return vty.clone();
    }
    let (args, res) = flatten_thunk(vty);
    if args.len() < n {
        return vty.clone();
    }
    let rest = args[n..].to_vec();
    if rest.is_empty() {
        res.first().cloned().unwrap_or(ValueTy::Double)
    } else {
        ValueTy::Thunk(rest, res.clone())
    }
}

/// Names of sum types that declare a constructor with the given name.
fn sum_types_with_ctor(ctx: &Ctx, ctor: &str) -> Vec<String> {
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
fn sum_ctor_payload(ctx: &Ctx, sum_name: &str, ctor: &str) -> Option<Vec<ValueTy>> {
    match ctx.env.data_types.get(sum_name) {
        Some(DataInfo::Sum(ctors)) => ctors
            .iter()
            .find(|(c, _)| c == ctor)
            .map(|(_, payload)| payload.iter().map(|(t, _)| t.clone()).collect()),
        _ => None,
    }
}

/// Convert a data-declaration field/payload type expression into a value type,
/// substituting type-variable names with placeholder positions (`data Box a` →
/// field `value: a` becomes `Var(1)`, mirroring the builtin `Maybe`/`Pair`
/// shapes). Concrete type names resolve through [`TypeEnv::vty_of_name`].
///
/// A free lowercase name that no declaration shadows is a TYPE VARIABLE by
/// Haskell's lexical rules (§3.1), and becomes a placeholder variable — never a
/// nominal type. A buffer CHANNEL constructor is the buffer value type itself
/// (`FixedBuffer Double` → `ValueTy::Buffer(Double)`). An undeclared UPPERCASE
/// name never reaches here — registration validation rejects it first.
///
/// Exposed for the category-prelude data registration (`Kleisli m a b` is
/// declared in the prelude and registered by [`TypeEnv::with_builtins`]).
pub(crate) fn data_field_vty(
    env: &TypeEnv,
    tyvars: &[String],
    te: &crate::ast::TypeExpr,
) -> ValueTy {
    let mut free: Vec<String> = Vec::new();
    data_field_vty_with_free(env, tyvars, te, &mut free)
}

/// [`data_field_vty`] with an explicit free-variable list.
///
/// A name that is neither a declared type nor one of the declaration's own type
/// parameters is a TYPE VARIABLE (Haskell 2010 §3.1), never a nominal type: the
/// nominal fallback that used to invent `Data(name, [])` here is what let an
/// unbound lowercase name pass as a phantom type. Free names are recorded in
/// `free` in first-appearance order so each keeps a stable placeholder id across
/// the fields of ONE declaration.
pub(crate) fn data_field_vty_with_free(
    env: &TypeEnv,
    tyvars: &[String],
    te: &crate::ast::TypeExpr,
    free: &mut Vec<String>,
) -> ValueTy {
    match te {
        crate::ast::TypeExpr::TName(n) => match tyvars.iter().position(|t| t == n) {
            Some(k) => ValueTy::Var((k + 1) as u32),
            None => match env.vty_of_name(n) {
                Some(t) => t,
                // An unbound name is a free type variable: give it the next
                // placeholder position after the declaration's own parameters.
                None => {
                    let idx = match free.iter().position(|f| f == n) {
                        Some(i) => i,
                        None => {
                            free.push(n.clone());
                            free.len() - 1
                        }
                    };
                    ValueTy::Var((tyvars.len() + 1 + idx) as u32)
                }
            },
        },
        crate::ast::TypeExpr::TApp(head, args) => {
            let vargs: Vec<ValueTy> = args
                .iter()
                .map(|a| data_field_vty_with_free(env, tyvars, a, free))
                .collect();
            // A buffer CHANNEL constructor is the buffer value type itself
            // (`FixedBuffer elem` → `ValueTy::Buffer(elem)`), not a second
            // `App("FixedBuffer", …)` shape.
            if let Some(buf) = env.channel_ctor_vty(head, &vargs) {
                return buf;
            }
            // A type PARAMETER applied as a constructor (`m b` in
            // `data K m a b`) becomes a type-constructor application whose head
            // is the parameter's var id. A concrete constructor stays `App`.
            if let Some(k) = tyvars.iter().position(|t| t == head) {
                ValueTy::TyConApp((k + 1) as u32, vargs)
            } else {
                ValueTy::App(head.clone(), vargs)
            }
        }
        crate::ast::TypeExpr::TFunc(args, ret) => ValueTy::Thunk(
            args.iter()
                .map(|a| data_field_vty_with_free(env, tyvars, a, free))
                .collect(),
            vec![data_field_vty_with_free(env, tyvars, ret, free)],
        ),
        crate::ast::TypeExpr::TForall(_, inner) => {
            data_field_vty_with_free(env, tyvars, inner, free)
        }
        crate::ast::TypeExpr::TConstrained(_, inner) => {
            data_field_vty_with_free(env, tyvars, inner, free)
        }
    }
}

/// The value type of a value of sum type `sum_name`: builtin sums are
/// parameterized (`Maybe a`, `Either a b`) and compare/construct as `App`,
/// while user sums are monomorphic `Data`. A fresh live value variable fills
/// each builtin type parameter (the shape's `Var(1)`/`Var(2)` placeholders are
/// NOT live unification vars — see [`TypeEnv::with_builtins`]).
fn fresh_sum_vty(ctx: &mut Ctx, sum_name: &str) -> ValueTy {
    match ctx.env.ctor_arity(sum_name) {
        Some(arity) => {
            let args = (0..arity).map(|_| ctx.fresh_vty()).collect();
            ValueTy::App(sum_name.into(), args)
        }
        None => ValueTy::Data(sum_name.into(), vec![]),
    }
}

/// Substitute a parameterized record's type params into a field type
/// (`data Kleisli m a b = { unKleisli: a -> m b }` → projecting `unKleisli` off
/// `Kleisli Maybe Double Double` yields `Double -> Maybe Double`). The env's field
/// types carry PLACEHOLDER ids (`Var(k)` for the k-th param, `TyConApp` heads in
/// the same id space); the record's concrete args fill them in positionally.
///
/// Exposed for lowering's record-ctor expected threading (`Kleisli (fn x ->
/// return (f x))` threads the substituted field type into the field value).
pub(crate) fn substitute_field_params(fty: &ValueTy, args: &[ValueTy]) -> ValueTy {
    fn sub(ty: &ValueTy, args: &[ValueTy]) -> ValueTy {
        match ty {
            ValueTy::Var(k) | ValueTy::TyConVar(k) => args
                .get(k.saturating_sub(1) as usize)
                .cloned()
                .unwrap_or_else(|| ty.clone()),
            ValueTy::TyConApp(f, xs) => {
                let head = args.get((*f).saturating_sub(1) as usize).cloned();
                let sub_args = xs.iter().map(|x| sub(x, args)).collect();
                match head {
                    Some(ValueTy::App(c, _)) => ValueTy::App(c, sub_args),
                    Some(ValueTy::Data(c, _)) => ValueTy::Data(c, sub_args),
                    Some(ValueTy::Newtype(c, _)) => ValueTy::Newtype(c, sub_args),
                    // An unresolved head (a fresh record type arg) carries the
                    // LIVE var id, not the placeholder id — so the substituted
                    // field type participates in live unification (`m := Maybe`)
                    // and lowering's expected threading can bind it.
                    Some(ValueTy::Var(v)) => ValueTy::TyConApp(v, sub_args),
                    Some(ValueTy::TyConVar(v)) => ValueTy::TyConApp(v, sub_args),
                    _ => ValueTy::TyConApp(*f, sub_args),
                }
            }
            ValueTy::Thunk(fa, fr) => ValueTy::Thunk(
                fa.iter().map(|x| sub(x, args)).collect(),
                fr.iter().map(|x| sub(x, args)).collect(),
            ),
            ValueTy::Data(n, a) => {
                ValueTy::Data(n.clone(), a.iter().map(|x| sub(x, args)).collect())
            }
            ValueTy::Newtype(n, a) => {
                ValueTy::Newtype(n.clone(), a.iter().map(|x| sub(x, args)).collect())
            }
            ValueTy::App(n, a) => ValueTy::App(n.clone(), a.iter().map(|x| sub(x, args)).collect()),
            // A parameterized buffer record field: the element is substituted
            // like any other type argument.
            ValueTy::Buffer(elem) => ValueTy::Buffer(Box::new(sub(elem, args))),
            t => t.clone(),
        }
    }
    sub(fty, args)
}

/// Infer an expression that must yield exactly one output and no inputs — a
/// constant or a per-block value. Signal-rate constants (literals) are coerced
/// to their value type; returns the resulting `ValueTy`.
fn infer_const_value(ctx: &mut Ctx, e: &Expr) -> Result<ValueTy, CompileError> {
    infer_const_value_expected(ctx, e, None)
}

/// [`infer_const_value`] with an optional expected value type threaded into `e`'s
/// own inference. A record field value (`unKleisli: \ x -> return (f x)`) is a
/// result-directed method container, so the declared field type must be visible
/// while it infers — see the record-constructor arm of `infer_apply_impl`.
fn infer_const_value_expected(
    ctx: &mut Ctx,
    e: &Expr,
    expected: Option<&ValueTy>,
) -> Result<ValueTy, CompileError> {
    let span = e.span();
    let t = infer_expr_expected(ctx, e, expected.cloned())?;
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
    let ch = &t.vty;
    Ok(ch.clone())
}

/// Whether `e` is a reference (possibly chained) to a definition whose body is
/// a constant literal — a numeric scalar the value track can capture by value.
fn is_const_value_ref(ctx: &Ctx, e: &Expr) -> bool {
    let mut cur = e;
    let mut seen = HashSet::new();
    while let Expr::Ref(name, _) = cur {
        if !seen.insert(name.clone()) {
            return false;
        }
        match ctx.def_bodies.get(name.as_str()) {
            Some(Expr::Int(_, _)) => return true,
            Some(Expr::Double(_, _)) => return true,
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
        Some(Def::Eval { params, body, .. }) if params.is_empty() => match body {
            Expr::Lambda { .. } => Some(name.to_string()),
            Expr::Apply { .. } => Some(name.to_string()),
            Expr::Ref(next, _) => fragment_target(defs, next, seen),
            _ => None,
        },
        Some(Def::Eval { params, .. }) if !params.is_empty() => Some(name.to_string()),
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
        Expr::ApplyExpr { callee, args, .. } => {
            collect_static_calls(callee, src, bound, nodes, out);
            for a in args {
                collect_static_calls(a, src, bound, nodes, out);
            }
        }
        Expr::Ref(_, _) | Expr::Int(_, _) | Expr::Double(_, _) | Expr::Str(_, _) => {}
        Expr::Neg(inner, _) => collect_static_calls(inner, src, bound, nodes, out),
        Expr::Arith { lhs, rhs, .. } => {
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
        Expr::Void(_) => {}
        Expr::Annot { expr, .. } => collect_static_calls(expr, src, bound, nodes, out),
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
    let (nodes, edges) = def_call_graph(defs);
    // Self-recursion is a self-loop; report it directly for a clear message.
    for d in defs {
        let name = d.name();
        if !nodes.contains(name) {
            continue;
        }
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
        edges: &[(String, String)],
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
fn class_var_pattern(ctx: &mut Ctx, sig: &crate::ast::TypeExpr, idx: usize) -> ValueTy {
    // A class context constrains the body; the container lives in the body's
    // argument list.
    let sig = match sig {
        crate::ast::TypeExpr::TConstrained(_, inner) => inner.as_ref(),
        other => other,
    };
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
/// a value channel ([`Rate::Value`]) or a bare Double/Int literal (value-
/// compatible in v1). Genuine signal computations (`sin 1.0`, arithmetic)
/// cannot flow through the value track, so they are rejected here — at
/// inference, with a clear message — rather than failing obscurely during
/// lowering.
fn infer_method_value_vty(ctx: &mut Ctx, e: &Expr, what: &str) -> Result<ValueTy, CompileError> {
    infer_method_value_vty_expected(ctx, e, what, None)
}

/// [`infer_method_value_vty`] with an expected value type (used for
/// result-directed typeclass dispatch inside method-call arguments).
fn infer_method_value_vty_expected(
    ctx: &mut Ctx,
    e: &Expr,
    _what: &str,
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
    match &t.vty {
        // A Buffer is an ordinary value (the block track is gone): a 0→1
        // Buffer expression is a valid value-track operand. Numeric literals
        // stay Int/Double (their `Buffer` typing is the signal-constant
        // compatibility), everything else resolves to `Buffer`.
        ValueTy::Buffer(_) => match e {
            Expr::Int(_, _) => Ok(ValueTy::Int),
            Expr::Double(_, _) => Ok(ValueTy::Double),
            _ => Ok(ctx.env.buffer_vty()),
        },
        vty => Ok(vty.clone()),
    }
}

/// Whether `e` is a bare reference to a nullary method of `class_name` (e.g./// `mempty` in `mappend xs mempty`) — such an argument resolves by the selector
/// argument's concrete type.
fn is_bare_nullary_method_ref(ctx: &Ctx, e: &Expr, class_name: &str) -> bool {
    match e {
        Expr::Ref(name, _) => ctx.env.is_nullary_method(class_name, name.as_str()),
        _ => false,
    }
}

/// The class-variable type a CLASS METHOD's first-class value dispatches on,
/// taken from the expected function type.
///
/// A method used as a value (`fold mappend "" xs`) carries no selector
/// argument, so the only evidence for the instance is the signature the
/// surrounding context demands. The expected `Thunk` is flattened and its
/// parameter and result types are tried in order; the first concrete candidate
/// naming an instance is the dispatch key. `mappend` under `b -> a -> b` with
/// `(a, b) = (String, String)` therefore selects `Monoid String`.
///
/// Returns BOTH the instance lookup key (the head constructor name) and the
/// candidate's full type. The full type matters: a parameterized instance head
/// (`instance Monoid List`) must substitute the class variable with `List a`,
/// not with the arity-0 `Data("List", [])` that the bare head name would give.
///
/// Returns `None` when no candidate is concrete — the caller then falls through
/// to the loud "requires an argument" diagnostic, never a silent default.
pub(crate) fn method_value_dispatch_env(
    env: &crate::types::ty::TypeEnv,
    expected: &ValueTy,
) -> Option<(String, ValueTy)> {
    let (args, rets) = match expected {
        ValueTy::Thunk(..) => flatten_thunk(expected),
        _ => return None,
    };
    for cand in args.iter().chain(rets.iter()) {
        if matches!(cand, ValueTy::Var(_) | ValueTy::TyConVar(_)) {
            continue;
        }
        if let Some(name) = env.type_name_of_vty(cand) {
            return Some((name, cand.clone()));
        }
    }
    None
}

/// Bind a constraint instance's head args from the LEADING args of the
/// call-site concrete container (`instance (Monad m) => Arrow (Kleisli m)`:
/// `Kleisli Maybe Double Double` + head `Kleisli m` → `m := Maybe`), recording
/// each binding in `ctx.type_var_bindings`, then discharge every constraint by
/// instance lookup of the bound type. A head arg that is still an unresolved
/// type variable at the call site is DEFERRED (recorded, not checked): the
/// concrete binding emerges when the surrounding expression's type is pinned
/// (a `match` on `m b`), and the constraint is then discharged by the body's
/// own resolution. Returns the instance's `(head_args, constraints)` (both
/// empty for a non-constraint instance).
#[allow(clippy::type_complexity)]
fn bind_constraint_instance(
    ctx: &mut Ctx,
    class_name: &str,
    ctor: &str,
    container_vty: &ValueTy,
    span: Span,
) -> Result<(Vec<String>, Vec<(String, String)>), CompileError> {
    let inst = ctx
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
        ValueTy::App(_, a) | ValueTy::Data(_, a) => a.clone(),
        _ => vec![],
    };
    for (k, hv) in head_args.iter().enumerate() {
        if let Some(ca) = concrete_args.get(k) {
            ctx.type_var_bindings.insert(hv.clone(), ca.clone());
        } else {
            return Err(CompileError::Type {
                msg: format!("head argument `{hv}` of `{ctor}` is not concrete at the call site"),
                span,
            });
        }
    }
    for (cclass, cv) in &constraints {
        let bound = ctx
            .type_var_bindings
            .get(cv)
            .cloned()
            .ok_or_else(|| CompileError::Type {
                msg: format!("constraint `{cclass} {cv}` has an unbound type variable"),
                span,
            })?;
        let tname = ctx.env.type_name_of_vty(&ctx.subst.resolve_value(&bound));
        if let Some(tname) = tname {
            if !ctx
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
        // An unresolved type variable defers the discharge to the point where
        // the head arg becomes concrete (the body's own method resolution).
    }
    Ok((head_args, constraints))
}

/// How a method signature's class variable is substituted while the signature
/// is converted to concrete value types.
#[derive(Debug, Clone)]
pub(crate) enum ClassVarSubst {
    /// A type-constructor NAME the caller resolved by the selector's head
    /// constructor (`List`, `Kleisli`, `Int`).
    Ctor(String),
    /// The class variable's fully resolved TYPE (`Int`, `Pair Int Int`) — an
    /// arity-0 dispatch recovered it by unification, or an instance head binds
    /// it directly.
    Ty(ValueTy),
    /// Keep the class variable LIVE as the given unification variable so a
    /// later `unify_value` can solve it from a call site's concrete argument
    /// (used by [`class_var_pattern_vty`]).
    Var(TypeVarId),
}

/// Convert one method-signature argument type into a unification PATTERN in
/// which the class variable is the live variable `class_var_var` and every
/// other type variable is fresh, so unifying the pattern against a call site's
/// concrete argument type solves the class variable (`Pair i i` vs
/// `Pair Int Int` → `i := Int`).
pub(crate) fn class_var_pattern_vty(
    env: &TypeEnv,
    class_var: &str,
    class_var_var: TypeVarId,
    te: &crate::ast::TypeExpr,
    mut fresh: impl FnMut() -> ValueTy,
) -> ValueTy {
    let mut vars: HashMap<String, ValueTy> = HashMap::new();
    conv(
        env,
        class_var,
        &ClassVarSubst::Var(class_var_var),
        &mut vars,
        &mut fresh,
        te,
    )
}

/// Whether a method signature's RESULT is the bare class variable itself
/// (`mempty: m`, `empty: a`) — the only shape for which a call site's expected
/// type IS the class variable's value. A signature that merely MENTIONS the
/// class variable in its result (`wrap: Double -> Pair a a`) does not qualify:
/// its expected type is an application of the class variable, not the variable
/// itself, so taking it as the solved value would dispatch on the wrong key.
/// `forall`/constraint wrappers are transparent (mirrors [`sig_result_directed`]).
fn sig_result_is_bare_class_var(sig: &crate::ast::TypeExpr, class_var: &str) -> bool {
    match sig {
        crate::ast::TypeExpr::TConstrained(_, inner) | crate::ast::TypeExpr::TForall(_, inner) => {
            sig_result_is_bare_class_var(inner, class_var)
        }
        crate::ast::TypeExpr::TName(n) => n == class_var,
        crate::ast::TypeExpr::TFunc(_, ret) => sig_result_is_bare_class_var(ret, class_var),
        _ => false,
    }
}

/// The argument an ARITY-0 method dispatch solves the class variable from.
///
/// Two criteria must agree, because the concrete type that solves the variable
/// must come from an argument whose signature type CONSTRAINS it:
///
/// 1. the argument's signature type MENTIONS the class variable — otherwise its
///    value says nothing about the instance (`pick: Bool -> Pair a a -> Double`
///    must select on argument 1, not on the `Bool` at 0), and
/// 2. the argument is not a bare nullary method reference — `mappend mempty xs`
///    must select on `xs`, even though `mappend: m -> m -> m` mentions the class
///    variable at both positions.
///
/// A signature with no class-var argument at all (`show: a`, `empty: a`) has no
/// mentioning index: the class variable IS the argument type, so the first
/// argument that is not a nullary method reference selects. `None` when only
/// nullary method references are available (the caller reports it).
pub(crate) fn class_var_selector_index(
    sig: &crate::ast::TypeExpr,
    class_var: &str,
    n_args: usize,
    mut is_bare_nullary_method_ref: impl FnMut(usize) -> bool,
) -> Option<usize> {
    let indices = crate::types::ty::class_var_param_indices(sig, class_var);
    if indices.is_empty() {
        return (0..n_args).find(|i| !is_bare_nullary_method_ref(*i));
    }
    indices
        .into_iter()
        .find(|i| *i < n_args && !is_bare_nullary_method_ref(*i))
}

/// The instance-registry key and class-variable substitution for a class-method
/// dispatch, given the method's resolved CONTAINER type `selector_ty` (the
/// expected result type for a result-directed method, the selector argument's
/// type otherwise).
///
/// For an ARITY-0 class (`C i`) the selector's head constructor cannot identify
/// the instance: `C Int` and `C (Pair Int Int)` share the head `Pair`. The class
/// variable is therefore recovered by UNIFYING the method's class-var parameter
/// pattern (`Pair i i` in `index: Pair i i -> ...`) against the selector's
/// concrete type (`Pair Int Int` -> `i := Int`) and rendered canonically
/// ([`crate::types::ty::value_type_key`]: `Int`, `Pair Int Int`) -- exactly the
/// spelling a fully concrete instance head registers under
/// ([`crate::render::render_type_key`]). A signature with no class-var
/// parameter (`mempty: m`) takes the class variable to BE the selector type.
///
/// Falls back to the selector's head-constructor name when the solved type has
/// no registered instance (a bare-constructor head such as `Monoid List`
/// answers a `List Double` selector) or when the class variable stays unsolved
/// (an open selector type) -- that is the pre-unification behaviour and keeps
/// `Eq`/`Ord`/`Monoid` dispatch unchanged. A constructor class (arity >= 1) is
/// always keyed by the head name; its partial head args are restored separately
/// ([`prepend_instance_head_args`]).
///
/// Requires a concrete selector type: `None` when neither the solved type nor
/// the head constructor is known.
pub(crate) fn method_dispatch_key(
    env: &TypeEnv,
    class_name: &str,
    class_var: &str,
    sig: &crate::ast::TypeExpr,
    selector_ty: &ValueTy,
    class_var_var: TypeVarId,
    fresh: impl FnMut() -> ValueTy,
) -> Option<(String, ClassVarSubst)> {
    let class_arity = env
        .typeclasses
        .get(class_name)
        .map(|c| c.arity)
        .unwrap_or(0);
    if class_arity == 0 {
        let solved = env
            .class_var_solved_ty(sig, class_var, selector_ty, class_var_var, fresh)
            .or_else(|| {
                // No parameter mentions the class variable, so the variable can
                // only be the method's RESULT — and only when the result IS the
                // bare class variable (`mempty: m`). A result that merely
                // MENTIONS it (`wrap: Double -> Pair a a`) says nothing about the
                // instance, so the head-name fallback below handles it.
                let in_params = crate::types::ty::class_var_param_pattern(sig, class_var).is_some();
                if in_params || !sig_result_is_bare_class_var(sig, class_var) {
                    None
                } else {
                    Some(selector_ty.clone())
                }
            });
        if let Some(solved) = solved {
            if let Some(key) = crate::types::ty::value_type_key(env, &solved) {
                let registered = env
                    .instances
                    .get(class_name)
                    .map(|by_ty| by_ty.contains_key(key.as_str()))
                    .unwrap_or(false);
                if registered {
                    return Some((key, ClassVarSubst::Ty(solved)));
                }
                // No instance under the full type: fall through to the head
                // constructor, which is how a bare or partial head is keyed.
            }
        }
    }
    let head = env.type_name_of_vty(selector_ty)?;
    Some((head.clone(), ClassVarSubst::Ctor(head)))
}

/// Whether a call to the class METHOD `name` can be dispatched by
/// `selector_ty` — the FIRST argument's static type.
///
/// This is the routing predicate for a DUAL name: a name that is BOTH a class
/// method and a class-constrained definition. `index` is the canonical case — it
/// is the `Ix` offset method (`index: Pair i i -> i -> Int`) and, separately,
/// the array accessor `!` (`index :: Ix i => Array i e -> i -> e`). The first
/// argument's type decides: a `Pair Int Int` bounds value solves the method's
/// class variable (an offset), an `Array …` value does not (the accessor).
///
/// It is DELIBERATELY stricter than [`method_dispatch_key`]: after solving the
/// class variable it requires a REGISTERED instance under the solved full type
/// and never falls back to the selector's head constructor name. That fallback
/// would "solve" `Ix` for an `Array` selector (head `Array`) and route the
/// accessor into the method path, reporting "no instance of `Ix` for `Array`"
/// instead of reaching the definition.
///
/// Inference and lowering both call this helper on their own recovered
/// first-argument type (`infer_expr` / the lowerer's sandbox probe), so the two
/// passes agree on which `index` a call means — the invariant the T4 review
/// established for class dispatch.
pub(crate) fn first_arg_solves_class_var(env: &TypeEnv, name: &str, selector_ty: &ValueTy) -> bool {
    fn value_has_var(ty: &ValueTy, var: TypeVarId) -> bool {
        match ty {
            ValueTy::Var(id) | ValueTy::TyConVar(id) => *id == var,
            ValueTy::Data(_, args) | ValueTy::Newtype(_, args) | ValueTy::App(_, args) => {
                args.iter().any(|t| value_has_var(t, var))
            }
            ValueTy::Thunk(args, rets) => {
                args.iter().any(|t| value_has_var(t, var))
                    || rets.iter().any(|t| value_has_var(t, var))
            }
            ValueTy::TyConApp(id, args) => *id == var || args.iter().any(|t| value_has_var(t, var)),
            // The element is part of the buffer type.
            ValueTy::Buffer(elem) => value_has_var(elem, var),
            _ => false,
        }
    }
    let Some(class_name) = env.class_of_method(name) else {
        return false;
    };
    let Some(info) = env.typeclasses.get(&class_name) else {
        return false;
    };
    let class_var = info.var.clone();
    let Some(sig) = info
        .methods
        .iter()
        .find(|(m, _)| m == name)
        .map(|(_, s)| s.clone())
    else {
        return false;
    };
    // A class-variable placeholder id outside any id the unification supply
    // issues; it never escapes this probe.
    const PROBE_VAR: TypeVarId = TypeVarId::MAX;
    let mut next = 0u32;
    let mut fresh = || {
        next += 1;
        ValueTy::Var(next)
    };
    let mut solved_for_debug: Option<ValueTy> = None;
    let key = if info.arity == 0 {
        env.class_var_solved_ty(&sig, &class_var, selector_ty, PROBE_VAR, &mut fresh)
            .and_then(|solved| {
                solved_for_debug = Some(solved.clone());
                crate::types::ty::value_type_key(env, &solved)
            })
    } else {
        // A constructor class (`Functor f`) dispatches by the container head.
        env.type_name_of_vty(selector_ty)
    };
    if let Some(solved) = solved_for_debug {
        debug_assert!(
            !value_has_var(&solved, PROBE_VAR),
            "probe class-var id leaked into method dispatch key"
        );
    }
    let Some(key) = key else {
        return false;
    };
    env.instances
        .get(&class_name)
        .map(|by_ty| by_ty.contains_key(key.as_str()))
        .unwrap_or(false)
}

/// Convert a typeclass method signature's curried argument types into concrete
/// parameter `ValueTy`s, substituting the class variable per `subst` (a
/// constructor name for constructor classes, the solved full type for an
/// arity-0 class). Type variables in the signature (`a`, `b`) map to fresh
/// unification vars, shared by name so `g : a -> b` and `xs : f a` agree on
/// `a`. Used to validate instance method bodies against the class contract.
fn signature_param_tys(
    ctx: &mut Ctx,
    class_var: &str,
    subst: &ClassVarSubst,
    sig: &crate::ast::TypeExpr,
) -> Vec<ValueTy> {
    let env = ctx.env.clone();
    signature_param_tys_conv(&env, class_var, subst, sig, || ctx.fresh_vty())
}

/// [`signature_param_tys`] usable from lowering (no `Ctx`): `fresh` supplies
/// fresh type variables. Exposed so the lowerer can thread expected types into
/// method-call arguments (`bind mx (fn x -> return x)`).
pub(crate) fn signature_param_tys_conv(
    env: &TypeEnv,
    class_var: &str,
    subst: &ClassVarSubst,
    sig: &crate::ast::TypeExpr,
    fresh: impl FnMut() -> ValueTy,
) -> Vec<ValueTy> {
    signature_param_and_result_tys_conv(env, class_var, subst, sig, fresh).0
}

/// [`signature_param_tys_conv`] plus the method's RESULT type, computed in the
/// SAME fresh-variable scope so a variable shared between a parameter and the
/// result (`compose: a b c -> a c d -> a b d`) stays linked. The result type is
/// used as the inlined body's expected type, so a default body that calls other
/// class methods (`both f g = compose (first f) (second g)`, `second k =
/// compose (compose (arr swap) (first k)) (arr swap)`) resolves their container
/// from the method's own result instead of failing with "expected type unknown".
/// `None` when the signature is not a function type (a nullary method).
pub(crate) fn signature_param_and_result_tys_conv(
    env: &TypeEnv,
    class_var: &str,
    subst: &ClassVarSubst,
    sig: &crate::ast::TypeExpr,
    mut fresh: impl FnMut() -> ValueTy,
) -> (Vec<ValueTy>, Option<ValueTy>) {
    let mut vars: HashMap<String, ValueTy> = HashMap::new();
    match sig {
        crate::ast::TypeExpr::TFunc(args, ret) if !args.is_empty() => {
            let params: Vec<ValueTy> = args
                .iter()
                .map(|a| conv(env, class_var, subst, &mut vars, &mut fresh, a))
                .collect();
            let result = conv(env, class_var, subst, &mut vars, &mut fresh, ret);
            (params, Some(result))
        }
        other => (
            vec![conv(env, class_var, subst, &mut vars, &mut fresh, other)],
            None,
        ),
    }
}

/// Convert a type expression to a value type, substituting the class variable
/// per `subst` and mapping each other type variable to a fresh unification
/// variable shared by name (`g : a -> b` and `xs : f a` agree on `a`).
///
/// Type names resolve through the registry ([`TypeEnv::type_entry`]): a
/// lowercase name is a type variable (one fresh variable per name), and every
/// other name is a TYPE. An undeclared uppercase name cannot reach this
/// function — [`validate_type_names`] rejects it when the environment is built
/// — so the fallback below is only the type-variable placeholder shape a data
/// field keeps for a free lowercase name.
fn conv(
    env: &TypeEnv,
    class_var: &str,
    class_var_subst: &ClassVarSubst,
    vars: &mut HashMap<String, ValueTy>,
    fresh: &mut impl FnMut() -> ValueTy,
    te: &crate::ast::TypeExpr,
) -> ValueTy {
    match te {
        crate::ast::TypeExpr::TName(n) if n == class_var => match class_var_subst {
            ClassVarSubst::Ctor(c) => resolve_type_name(env, c),
            ClassVarSubst::Ty(t) => t.clone(),
            ClassVarSubst::Var(id) => ValueTy::Var(*id),
        },
        crate::ast::TypeExpr::TName(n) => match env.type_entry(n) {
            Some(TypeEntry::Variable) | None => vars.entry(n.clone()).or_insert_with(fresh).clone(),
            Some(_) => resolve_type_name(env, n),
        },
        crate::ast::TypeExpr::TApp(head, args) if head == class_var => {
            let vargs: Vec<ValueTy> = args
                .iter()
                .map(|a| conv(env, class_var, class_var_subst, vars, fresh, a))
                .collect();
            match class_var_subst {
                ClassVarSubst::Ctor(ctor) => {
                    if env.ctor_arity(ctor).is_some() {
                        ValueTy::App(ctor.to_string(), vargs)
                    } else if env.data_arities.contains_key(ctor) {
                        ValueTy::Data(ctor.to_string(), vargs)
                    } else {
                        resolve_type_name(env, ctor)
                    }
                }
                // The class variable is a fully applied type used as a
                // constructor: splice its own arguments ahead of the new ones
                // (arity-0 classes never reach this).
                ClassVarSubst::Ty(t) => match t {
                    ValueTy::App(n, pre) => {
                        ValueTy::App(n.clone(), pre.iter().cloned().chain(vargs).collect())
                    }
                    ValueTy::Data(n, pre) => {
                        ValueTy::Data(n.clone(), pre.iter().cloned().chain(vargs).collect())
                    }
                    ValueTy::Newtype(n, pre) => {
                        ValueTy::Newtype(n.clone(), pre.iter().cloned().chain(vargs).collect())
                    }
                    other => other.clone(),
                },
                ClassVarSubst::Var(id) => ValueTy::TyConApp(*id, vargs),
            }
        }
        crate::ast::TypeExpr::TApp(head, args) => {
            // A DECLARED data type keeps its `Data` flavour, exactly as
            // `instantiate_builtin_sig_vty` builds a builtin signature's type:
            // `data_arities` is the set of parameterized declarations (`ST`,
            // `STArray`, `IO`, user `data`), while the builtin containers
            // (`List`, `Pair`, `Array`) stay `App`. The two must agree, or a
            // constrained definition's signature would produce `App("ST", …)`
            // where the ST machinery (and `runSTArray`'s payload check) expects
            // `Data("ST", …)`, and `unify_value` — which does not conflate the
            // flavours — would reject the call site.
            let vargs: Vec<ValueTy> = args
                .iter()
                .map(|a| conv(env, class_var, class_var_subst, vars, fresh, a))
                .collect();
            // A buffer CHANNEL constructor is the buffer value type — the one
            // representation, carrying the checked element.
            env.ctor_application(head, vargs)
        }
        crate::ast::TypeExpr::TFunc(args, ret) => ValueTy::Thunk(
            args.iter()
                .map(|a| conv(env, class_var, class_var_subst, vars, fresh, a))
                .collect(),
            vec![conv(env, class_var, class_var_subst, vars, fresh, ret)],
        ),
        crate::ast::TypeExpr::TForall(_, inner) => {
            conv(env, class_var, class_var_subst, vars, fresh, inner)
        }
        crate::ast::TypeExpr::TConstrained(_, inner) => {
            conv(env, class_var, class_var_subst, vars, fresh, inner)
        }
    }
}

/// The value type a registry-confirmed type name denotes. The name is already
/// known to be a type ([`TypeEnv::type_entry`]); a registry hit that cannot be
/// spelled as a value type degrades to its nominal form rather than panicking.
fn resolve_type_name(env: &TypeEnv, name: &str) -> ValueTy {
    env.vty_of_name(name)
        .unwrap_or_else(|| ValueTy::Data(name.to_string(), vec![]))
}

/// Prepend a partial instance head's bound args to each container-typed
/// signature param type. `instance (Monad m) => Arrow (Kleisli m)` resolves
/// `a b c` to `Data("Kleisli", [b, c])` (the head `m` is dropped by the
/// signature conversion in [`signature_param_tys_conv`]); this restores it to
/// `Data("Kleisli", [m, b, c])` with the concrete head binding from the call
/// site (`m := Maybe`), so a result-directed SECOND argument (`arr g` in
/// `compose (arr f) (arr g)`) resolves `return` by the concrete monad.
pub(crate) fn prepend_instance_head_args(
    env: &TypeEnv,
    class_name: &str,
    param_tys: &mut [ValueTy],
    ctor: &str,
    head_bindings: &HashMap<String, ValueTy>,
    mut fresh: impl FnMut() -> ValueTy,
) {
    let head_args = env
        .instances
        .get(class_name)
        .and_then(|by_ty| by_ty.get(ctor))
        .map(|i| i.head_args.clone())
        .unwrap_or_default();
    if head_args.is_empty() {
        return;
    }
    let head_values: Vec<ValueTy> = head_args
        .iter()
        .map(|hv| head_bindings.get(hv).cloned().unwrap_or_else(&mut fresh))
        .collect();
    for t in param_tys.iter_mut() {
        prepend_head_args_into_class_var(t, ctor, &head_values);
    }
}

/// Restore an instance's bound head args at every occurrence of the container
/// INSIDE `t` — not only at the top level.
///
/// The signature conversion drops the head (`m a` for `instance Monad (ST s)`
/// becomes `Data("ST", [a])`), and the class var can sit under a FUNCTION type
/// (`bind: m a -> (a -> m b) -> m b`: the second parameter's `m b` is inside a
/// `Thunk`). Restoring only the top level left the callback's return type one
/// argument short, so a body whose own computation was the full `ST s e`
/// failed to unify with it.
fn prepend_head_args_into_class_var(t: &mut ValueTy, ctor: &str, head_values: &[ValueTy]) {
    match t {
        ValueTy::Data(c, args) if c == ctor => {
            let mut full = head_values.to_vec();
            full.extend(args.clone());
            // The head values themselves may mention the container kind; they
            // are opaque to this repair (they come from the call site).
            *t = ValueTy::Data(c.clone(), full);
        }
        ValueTy::App(c, args) if c == ctor => {
            let mut full = head_values.to_vec();
            full.extend(args.clone());
            *t = ValueTy::App(c.clone(), full);
        }
        ValueTy::Thunk(args, rets) => {
            for a in args.iter_mut() {
                prepend_head_args_into_class_var(a, ctor, head_values);
            }
            for r in rets.iter_mut() {
                prepend_head_args_into_class_var(r, ctor, head_values);
            }
        }
        // A buffer's element is walked like an argument: the container can sit
        // under it in a higher-kinded signature.
        ValueTy::Buffer(elem) => prepend_head_args_into_class_var(elem, ctor, head_values),
        _ => {}
    }
}

/// Whether a signature carries a class context (`C v => T`), looking through an
/// explicit `forall` binder (`forall v. C v => T`).
///
/// Haskell 2010 spells the quantified form `forall v. C v => T`, so the context
/// can sit under the binder. Both definition sites (registration and the eager
/// inference deferral) accept either spelling.
pub(crate) fn is_constrained_sig(sig: &crate::ast::TypeExpr) -> bool {
    sig.is_constrained()
}

/// The class context of a definition's signature, when it carries one
/// (`mk :: Ix i => …`, or `forall i. Ix i => …`). The returned node is the
/// `TConstrained` itself — [`sig_constraints`] and [`sig_func_parts`] look
/// through the `forall` binder from there.
fn constrained_def_sig(def: &Def) -> Option<&crate::ast::TypeExpr> {
    fn constrained_node(te: &crate::ast::TypeExpr) -> Option<&crate::ast::TypeExpr> {
        match te {
            te @ crate::ast::TypeExpr::TConstrained(..) => Some(te),
            crate::ast::TypeExpr::TForall(_, inner) => constrained_node(inner),
            _ => None,
        }
    }
    def.sig().and_then(constrained_node)
}

/// The `(class, type variable)` constraints of a signature, looking through a
/// `forall` wrapper. Empty for an unconstrained signature.
fn sig_constraints(sig: &crate::ast::TypeExpr) -> &[(String, String)] {
    match sig {
        crate::ast::TypeExpr::TConstrained(cs, _) => cs,
        crate::ast::TypeExpr::TForall(_, inner) => sig_constraints(inner),
        _ => &[],
    }
}

/// Turn a signature's `(class, type-variable NAME)` context into a scheme's
/// `(class, type-variable ID)` constraints.
///
/// `vars` is the name→type map the signature conversion ([`def_sig_tys`])
/// produced for the scheme's value type, so a variable the type mentions keeps
/// the SAME id in the constraint — that is what lets a later dictionary
/// argument be paired with the parameter it witnesses. A recorded constraint's
/// variable therefore ALWAYS occurs in the scheme type.
///
/// Two contexts are not recorded:
///
/// - a context over a DECLARED type (`Ix Int => …`), which is already
///   discharged by that instance;
/// - a context variable the substitution already pinned to a concrete type,
///   which needs no dictionary parameter.
fn scheme_constraints(
    ctx: &mut Ctx,
    sig: &crate::ast::TypeExpr,
    vars: &HashMap<String, ValueTy>,
) -> Vec<(String, TypeVarId)> {
    let env = ctx.env.clone();
    let mut out: Vec<(String, TypeVarId)> = Vec::new();
    for (class, var) in sig_constraints(sig) {
        if is_declared_type_name(&env, var) {
            continue;
        }
        // A context variable the signature's arguments and result never
        // mention (`C i => Double`) has no type variable IN the scheme to pair
        // with, so there is nothing a dictionary parameter could witness. Such
        // a signature is AMBIGUOUS, which Haskell 2010 rejects outright; rill
        // has no ambiguity check today (an open defect tracked separately, and
        // one this change does not introduce — such a signature compiles at the
        // branch's base commit), so the constraint is dropped here rather than
        // paired with an invented id that no call site could ever solve.
        let Some(t) = vars.get(var) else {
            continue;
        };
        let id = match ctx.subst.resolve_value(t) {
            ValueTy::Var(v) | ValueTy::TyConVar(v) => v,
            // Solved to a concrete type: no dictionary parameter needed.
            _ => continue,
        };
        out.push((class.clone(), id));
    }
    out
}

/// The function-shaped body of a signature: `forall` binders and a class
/// context wrappers are transparent, so `C v => a -> b` and `forall v. a -> b`
/// both answer their `(argument types, result type)`. A signature that is not a
/// function type (a nullary definition) has no arguments and is its own result.
pub(crate) fn sig_func_parts(
    sig: &crate::ast::TypeExpr,
) -> (&[crate::ast::TypeExpr], &crate::ast::TypeExpr) {
    match sig {
        crate::ast::TypeExpr::TForall(_, inner) | crate::ast::TypeExpr::TConstrained(_, inner) => {
            sig_func_parts(inner)
        }
        crate::ast::TypeExpr::TFunc(args, ret) => (args, ret),
        other => (&[], other),
    }
}

/// A definition signature converted to value types: the parameter types, the
/// result type, and the name→variable map the call site fills while solving the
/// signature's type variables.
type DefSigTys = (Vec<ValueTy>, ValueTy, HashMap<String, ValueTy>);

/// Convert a definition signature (`mk :: Ix i => Pair i i -> Double -> Double`)
/// into its parameter and RESULT value types. Every signature type variable —
/// the class variables of the context included — becomes a fresh unification
/// variable shared by name, so a call site solves them by unifying its
/// arguments (`Pair Var(i) Var(i)` vs `Pair Int Int` → `i := Int`).
///
/// Returns the variable map too: it is where the constraint discharge reads the
/// solved class variable (`i → Int`).
fn def_sig_tys(ctx: &mut Ctx, sig: &crate::ast::TypeExpr) -> Result<DefSigTys, CompileError> {
    let env = ctx.env.clone();
    def_sig_tys_conv(&env, sig, || ctx.fresh_vty())
}

/// [`def_sig_tys`] without a `Ctx`: `fresh` supplies fresh variables.
fn def_sig_tys_conv(
    env: &TypeEnv,
    sig: &crate::ast::TypeExpr,
    mut fresh: impl FnMut() -> ValueTy,
) -> Result<DefSigTys, CompileError> {
    let (arg_tes, ret_te) = sig_func_parts(sig);
    for part in arg_tes.iter().chain(std::iter::once(ret_te)) {
        env.check_type_kind(part, Span::new(0, 0))?;
    }
    let mut vars: HashMap<String, ValueTy> = HashMap::new();
    // No class variable is special here: a context's class variables are
    // ordinary type variables until a call site solves them, so the empty
    // class-var name never matches a signature name.
    let subst = ClassVarSubst::Ctor(String::new());
    let params: Vec<ValueTy> = arg_tes
        .iter()
        .map(|a| conv(env, "", &subst, &mut vars, &mut fresh, a))
        .collect();
    let ret = conv(env, "", &subst, &mut vars, &mut fresh, ret_te);
    Ok((params, ret, vars))
}

/// Whether `name` is a DECLARED type rather than a signature type variable
/// (`Double`, `Pair`, a user `data` type).
///
/// A thin alias for [`TypeEnv::is_type_name`] — the registry is the ONE
/// predicate, so this cannot drift from the resolver
/// ([`TypeEnv::vty_of_name`]) again.
pub(crate) fn is_declared_type_name(env: &TypeEnv, name: &str) -> bool {
    env.is_type_name(name)
}

/// Discharge a constrained definition's class context against the types a call
/// site solved.
///
/// A class variable that is still UNRESOLVED defers: the call may sit inside
/// another constrained body whose own call site eventually pins it. A concrete
/// type with no registered instance is a COMPILE ERROR — there is no runtime
/// dictionary to fall back to, so the error must not be postponed.
pub(crate) fn discharge_def_constraints(
    env: &TypeEnv,
    def_name: &str,
    sig: &crate::ast::TypeExpr,
    bindings: &HashMap<String, ValueTy>,
    span: Span,
) -> Result<(), CompileError> {
    for (class, var) in sig_constraints(sig) {
        let Some(bound) = bindings.get(var) else {
            continue;
        };
        // Not concrete at this call site: defer to the enclosing context.
        if matches!(
            bound,
            ValueTy::Var(_) | ValueTy::TyConVar(_) | ValueTy::TyConApp(..)
        ) {
            continue;
        }
        // An arity-0 class (`Ix i`) keys its instances by the FULL applied type
        // (`Pair Int Int`); a constructor class (`Monad m`) by the constructor
        // name (`Maybe`). Mirrors `method_dispatch_key`.
        let arity = env.typeclasses.get(class).map(|c| c.arity).unwrap_or(0);
        let key = if arity == 0 {
            crate::types::ty::value_type_key(env, bound)
        } else {
            env.type_name_of_vty(bound)
        };
        let Some(key) = key else {
            continue;
        };
        // A class constraint is satisfiable by an instance of the class OR of
        // any class that ENTAILS it (System FC: the subclass dictionary
        // contains the superclass one). `Ord a` therefore satisfies an `Eq a`
        // constraint, `Fractional a` satisfies `Num a`, etc.
        let registered = env.entailed_classes(class.as_str()).iter().any(|c| {
            env.instances
                .get(c.as_str())
                .map(|m| m.contains_key(key.as_str()))
                .unwrap_or(false)
        });
        if !registered {
            return Err(CompileError::Type {
                msg: format!(
                    "no instance of `{class}` for type `{key}` (constraint of `{def_name}`)"
                ),
                span,
            });
        }
    }
    Ok(())
}

/// Bind the type variables of a signature pattern (`Pair i i`) to the concrete
/// call-site types they stand for (`Pair Int Int` → `i := Int`).
///
/// Lowering has no unification substitution, so this structural match is how it
/// recovers a constrained definition's class variable from the already-lowered
/// argument types. A variable bound by an earlier parameter keeps its first
/// binding (inference has already checked that the call is consistent).
pub(crate) fn bind_sig_type_vars(
    env: &TypeEnv,
    pat: &crate::ast::TypeExpr,
    actual: &ValueTy,
    out: &mut HashMap<String, ValueTy>,
) {
    match pat {
        crate::ast::TypeExpr::TName(n) => {
            if !is_declared_type_name(env, n) {
                out.entry(n.clone()).or_insert_with(|| actual.clone());
            }
        }
        crate::ast::TypeExpr::TApp(head, args) => {
            let actual_args: &[ValueTy] = match actual {
                ValueTy::App(_, a) | ValueTy::Data(_, a) | ValueTy::Newtype(_, a) => a,
                _ => &[],
            };
            if !is_declared_type_name(env, head) {
                // A type-constructor variable used as a head (`m a`): bind it
                // to the bare constructor (`m := Maybe`), the value-type form
                // the instance registry is keyed by.
                if let Some(c) = env.type_name_of_vty(actual) {
                    out.entry(head.clone())
                        .or_insert_with(|| ValueTy::App(c, vec![]));
                }
            }
            for (a, av) in args.iter().zip(actual_args.iter()) {
                bind_sig_type_vars(env, a, av, out);
            }
        }
        crate::ast::TypeExpr::TFunc(args, ret) => {
            if let ValueTy::Thunk(aa, rr) = actual {
                for (a, av) in args.iter().zip(aa.iter()) {
                    bind_sig_type_vars(env, a, av, out);
                }
                if let Some(r) = rr.first() {
                    bind_sig_type_vars(env, ret, r, out);
                }
            }
        }
        crate::ast::TypeExpr::TForall(_, inner) | crate::ast::TypeExpr::TConstrained(_, inner) => {
            bind_sig_type_vars(env, inner, actual, out);
        }
    }
}

/// Convert a signature type expression into a value type with the call-site
/// class-variable bindings substituted (`Pair i i` + `i := Int` → `App("Pair",
/// [Int, Int])`). Variables the call site did not solve become fresh
/// unification variables, shared by name through `vars`.
pub(crate) fn sig_ty_to_vty(
    env: &TypeEnv,
    te: &crate::ast::TypeExpr,
    bindings: &HashMap<String, ValueTy>,
    vars: &mut HashMap<String, ValueTy>,
    fresh: &mut dyn FnMut() -> ValueTy,
) -> Result<ValueTy, CompileError> {
    env.check_type_kind(te, Span::new(0, 0))?;
    Ok(sig_ty_to_vty_inner(env, te, bindings, vars, fresh))
}

fn sig_ty_to_vty_inner(
    env: &TypeEnv,
    te: &crate::ast::TypeExpr,
    bindings: &HashMap<String, ValueTy>,
    vars: &mut HashMap<String, ValueTy>,
    fresh: &mut dyn FnMut() -> ValueTy,
) -> ValueTy {
    match te {
        crate::ast::TypeExpr::TName(n) => match bindings.get(n) {
            Some(t) => t.clone(),
            None if is_declared_type_name(env, n) => resolve_type_name(env, n),
            None => vars.entry(n.clone()).or_insert_with(fresh).clone(),
        },
        crate::ast::TypeExpr::TApp(head, args) => {
            let vargs: Vec<ValueTy> = args
                .iter()
                .map(|a| sig_ty_to_vty_inner(env, a, bindings, vars, &mut *fresh))
                .collect();
            match bindings.get(head) {
                // A bound type-constructor variable is applied: splice its own
                // arguments ahead of the new ones (`m := Maybe`, `m a` →
                // `Maybe a`).
                Some(ValueTy::App(n, pre)) => {
                    ValueTy::App(n.clone(), pre.iter().cloned().chain(vargs).collect())
                }
                Some(ValueTy::Data(n, pre)) => {
                    ValueTy::Data(n.clone(), pre.iter().cloned().chain(vargs).collect())
                }
                Some(ValueTy::Newtype(n, pre)) => {
                    ValueTy::Newtype(n.clone(), pre.iter().cloned().chain(vargs).collect())
                }
                Some(other) => other.clone(),
                None => {
                    let head_ty = vars
                        .entry(head.clone())
                        .or_insert_with(|| {
                            if is_declared_type_name(env, head) {
                                resolve_type_name(env, head)
                            } else {
                                fresh()
                            }
                        })
                        .clone();
                    match head_ty {
                        ValueTy::Var(v) | ValueTy::TyConVar(v) => ValueTy::TyConApp(v, vargs),
                        ValueTy::App(n, pre) => {
                            ValueTy::App(n, pre.into_iter().chain(vargs).collect())
                        }
                        ValueTy::Data(n, pre) => {
                            ValueTy::Data(n, pre.into_iter().chain(vargs).collect())
                        }
                        // A declared constructor applied to MORE arguments than
                        // its head alone (`FixedBuffer Double` resolves to the
                        // buffer value type): build the application through the
                        // one converter so a channel constructor keeps its
                        // single buffer representation.
                        other => match vargs.len() {
                            0 => other,
                            _ => env.ctor_application(head, vargs),
                        },
                    }
                }
            }
        }
        crate::ast::TypeExpr::TFunc(args, ret) => ValueTy::Thunk(
            args.iter()
                .map(|a| sig_ty_to_vty_inner(env, a, bindings, vars, &mut *fresh))
                .collect(),
            vec![sig_ty_to_vty_inner(env, ret, bindings, vars, &mut *fresh)],
        ),
        crate::ast::TypeExpr::TForall(_, inner) | crate::ast::TypeExpr::TConstrained(_, inner) => {
            sig_ty_to_vty_inner(env, inner, bindings, vars, &mut *fresh)
        }
    }
}

/// Validate every instance's method bodies at compile time, even when the
/// instance is never called. Each body must infer to a single value channel
/// (or a bare literal) with its parameters bound to values of the class
/// signature's types (the class var substituted by the concrete constructor).
///
/// Every instance HEAD is KIND-checked here, prelude instances included
/// ([`TypeEnv::check_instance_head`]): the class variable's kind comes from the
/// class's arity, so `instance Functor Pair` (`* -> * -> *` bound to a `* -> *`
/// variable) and `instance Functor Int` (a type, not a constructor) are kind
/// errors. The recursion guard applies here too, so self-inlining bodies are
/// rejected even when the instance is dead code.
fn validate_instances(ctx: &mut Ctx) -> Result<(), CompileError> {
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
        // Kind check the HEAD — for EVERY instance, prelude ones included. The
        // trust flag below is about BODIES (a prelude body's field names can be
        // shadowed by a user `data` declaration), which does not apply to a
        // head: a head the compiler itself declares must be well kinded, and a
        // failure here is a real prelude defect, not a reason to skip.
        ctx.env
            .check_instance_head(&class, &info.head, Span::new(0, 0))?;
        // A prelude-DECLARED instance's BODY is trusted compiler machinery;
        // validating it here would type it against the USER's data environment,
        // where a legal shadowing declaration (`data Pair = { x, y }`) can make
        // a prelude body's field names unresolvable. Its body is still typed at
        // every call site (do-notation binds through `Monad IO`).
        //
        // The trust flag lives on the declaration, not on the `(class, type)`
        // key: a user `instance Monad IO` REPLACES the prelude entry with a
        // fresh `InstanceInfo` whose `from_prelude` is false, so it is
        // validated here like any other user instance.
        if info.from_prelude {
            continue;
        }
        // A constraint / partial-head instance (`instance (Monad m) => Arrow
        // (Kleisli m)`) cannot have its bodies inferred in isolation: the head
        // args are abstract until a call site binds them to concrete types
        // (`Kleisli Maybe Double Double` → `m := Maybe`), and a result-directed
        // method inside (`return (f x)`) needs that concrete type to resolve.
        // Bodies of such instances are inferred (and errors reported) at every
        // CALL SITE instead — the constraint discharge there is the real check.
        if !info.head_args.is_empty() || !info.constraints.is_empty() {
            continue;
        }
        // How the class variable is spelled in the signature conversion: an
        // arity-0 class binds it to the WHOLE instance type (`C (Pair Int Int)`
        // → `Pair Int Int`, an application), a constructor class to the head
        // constructor name (`Functor Maybe` → `Maybe`, so that `f a` becomes
        // `App("Maybe", [a])` rather than the environment's placeholder shape).
        let class_arity = ctx
            .env
            .typeclasses
            .get(&class)
            .map(|c| c.arity)
            .unwrap_or(0);
        let cv_subst = if class_arity == 0 {
            ClassVarSubst::Ty(info.class_var_ty(&ctx.env))
        } else {
            ClassVarSubst::Ctor(ty_name.clone())
        };
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
                (Some(cv), Some(sig)) => signature_param_tys(ctx, &cv, &cv_subst, &sig),
                _ => params
                    .iter()
                    .map(|_| resolve_type_name(&ctx.env, &ty_name))
                    .collect(),
            };
            // The method body's EXPECTED result type (from the signature's
            // return, class var → concrete constructor): result-directed method
            // calls inside (`pure x = return x`) resolve by it.
            let body_expected = match (class_var, method_sig) {
                (Some(cv), Some(crate::ast::TypeExpr::TFunc(_, ret))) => {
                    let env = ctx.env.clone();
                    Some(
                        signature_param_tys_conv(&env, &cv, &cv_subst, ret.as_ref(), || {
                            ctx.fresh_vty()
                        })
                        .first()
                        .cloned()
                        .unwrap_or(ValueTy::Double),
                    )
                }
                _ => None,
            };
            for (p, pt) in params.iter().zip(param_tys.iter()) {
                ctx.locals
                    .insert(p.clone(), ArrowTy::value_channel(pt.clone()));
            }
            let res = infer_method_value_vty_expected(ctx, body, "body", body_expected.clone());
            ctx.locals = saved;
            ctx.method_lifting.remove(&key);
            res?;
        }
    }
    Ok(())
}

/// Back-compat: infer with no built-ins and `main` as the entry name.
pub fn infer_program(program: &Program, mode: Mode) -> Result<TypedProgram, CompileError> {
    infer_program_with(program, mode)
}

/// Infer a program. Built-in signatures come from the FFI catalog in the type
/// environment (`foreign` declarations); no external signature source is
/// needed.
///
/// Top-level definitions are mutually recursive: all names are visible
/// to all bodies. `mode` is carried onto the returned [`TypedProgram`]; the
/// strict/complete distinction gates later pipeline stages (reduce, lower).
pub fn infer_program_with(program: &Program, mode: Mode) -> Result<TypedProgram, CompileError> {
    infer_program_with_entry(program, mode, "main")
}

/// The single Prelude-vs-user SHADOWING rule, applied identically in both
/// phases of the pipeline.
///
/// A Prelude definition supplies a name only while the program does not: a
/// user definition of the same name SHADOWS it, and the Prelude definition is
/// dropped entirely. The returned list is therefore the exact merged top-level
/// definition list both inference and lowering must consume — Prelude defs
/// first, then the user's.
///
/// Dropping rather than relying on "last one wins" is what makes the rule
/// explicit at both call sites. Before this rule existed, inference filtered
/// user defs OUT by Prelude name (so the name was typed with the Prelude's
/// scheme) while lowering's `name -> Def` map overwrote last-wins (so the
/// name RAN the user's body): a user `merge`/`seq` was silently typed with the
/// Prelude's arity and then failed to unify, or worse, ran a body of the wrong
/// shape.
///
/// Definitions that cannot own a value name (data/typeclass/instance/foreign
/// declarations) never shadow anything: only [`Def::Eval`] defs enter the
/// name space here, so a Prelude `data`/`instance` is always kept.
///
/// **Strict mode also drops recursive Prelude definitions.** Strict's contract
/// is an acyclic call graph, so it cannot execute one, and this compiler
/// rejects that at the name level: a Strict program that calls a recursive
/// Prelude helper must get a compile error, not an unbounded lowering
/// recursion. Keeping the definition out of the merged list makes every
/// reference to it a plain "unknown identifier", which is total (a call, a
/// bare reference, a first-class passing all fail the same way). Complete mode
/// keeps them — recursion is its normal evaluation strategy.
///
/// # User-visible consequence: recursive Prelude helpers are not exposed in
/// Strict mode
///
/// A `{-# MODE strict #-}` program does NOT see the Prelude's recursive
/// helpers — today `from`, and any recursive helper the Prelude gains later
/// (the planned `take`). Referencing one is a compile error:
///
/// ```text
/// type error: unknown identifier `from`
/// ```
///
/// This is deliberate, not an oversight. Exposing one to a Strict program makes
/// LOWERING recurse without bound and overflow the compiler's stack: Strict mode
/// has no runtime recursion to break the dispatch cycle, so the definition is
/// withheld rather than half-supported. The rule is total — a call, a bare
/// reference, and passing the name as a first-class value all report the same
/// unknown-identifier error.
///
/// `{-# MODE complete #-}` — the default for [`compile`](crate::compile) —
/// keeps recursive Prelude helpers and is the mode to use for them.
pub(crate) fn merge_prelude_defs(
    prelude_defs: &[crate::ast::Def],
    user_defs: &[crate::ast::Def],
    mode: Mode,
) -> Vec<crate::ast::Def> {
    let user_names: HashSet<&str> = user_defs
        .iter()
        .filter(|d| matches!(d, crate::ast::Def::Eval { .. }))
        .map(|d| d.name())
        .collect();
    let unavailable: HashSet<String> = if mode == Mode::Strict {
        cyclic_def_names(prelude_defs)
    } else {
        HashSet::new()
    };
    prelude_defs
        .iter()
        .filter(|d| !user_names.contains(d.name()) && !unavailable.contains(d.name()))
        .cloned()
        .collect()
}

/// Names of definitions that take part in a static call cycle, direct or
/// mutual (`from n = cons n (from (n + 1.0))` → `{from}`).
///
/// Shares the call graph CONSTRUCTION with [`check_recursion`] (see
/// [`def_call_graph`]); the cycle DETECTION below is its own implementation, not
/// [`check_recursion`]'s DFS. The two therefore agree on what a call edge is,
/// but not on how a cycle is found.
fn cyclic_def_names(defs: &[Def]) -> HashSet<String> {
    let (nodes, edges) = def_call_graph(defs);
    cyclic_in_call_graph(&nodes, &edges)
}

/// Build the static call graph over fragment-valued definitions: the names that
/// can own a runtime dispatch chain, and an edge `A → B` for every statically
/// named call from `A`'s body to `B`.
///
/// Only the CONSTRUCTION is shared, by [`check_recursion`] (which diagnoses a
/// cycle as a Strict-mode error) and [`cyclic_def_names`] (which uses reachability
/// as the Prelude availability test). Cycle DETECTION is duplicated: here
/// [`cyclic_in_call_graph`] (an explicit-stack reachability search feeding a
/// name set) and in [`check_recursion`] (a recursive DFS that reports the first
/// offending edge with its span). The two could in principle disagree about a
/// pathological graph, so a change to the notion of "recursive" must touch both.
fn def_call_graph(defs: &[Def]) -> (HashSet<String>, Vec<(String, String)>) {
    let nodes = fragment_valued_names(defs);
    let mut edges: Vec<(String, String)> = Vec::new();
    for d in defs {
        if !nodes.contains(d.name()) {
            continue;
        }
        collect_static_calls(d.body(), d.name(), &HashSet::new(), &nodes, &mut edges);
    }
    (nodes, edges)
}

/// The subset of `nodes` that can reach itself through `edges`.
fn cyclic_in_call_graph(nodes: &HashSet<String>, edges: &[(String, String)]) -> HashSet<String> {
    let mut cyclic = HashSet::new();
    for start in nodes {
        let mut visited = HashSet::new();
        let mut stack: Vec<String> = edges
            .iter()
            .filter(|(src, _)| src == start)
            .map(|(_, dst)| dst.clone())
            .collect();
        while let Some(cur) = stack.pop() {
            if cur == *start {
                cyclic.insert(start.clone());
                break;
            }
            if !visited.insert(cur.clone()) {
                continue;
            }
            stack.extend(
                edges
                    .iter()
                    .filter(|(src, _)| src == &cur)
                    .map(|(_, dst)| dst.clone()),
            );
        }
    }
    cyclic
}

/// Infer a program with an explicit entry name.
///
/// Inference is blind to the entry name: every top-level definition is typed
/// symmetrically and `def_schemes` carries all their schemes. The entry name
/// is accepted for call-compatibility (the compile pipeline threads it to
/// lowering, which reads the entry's scheme from `def_schemes`), but it does
/// not influence inference. All other behaviour is identical to
/// [`infer_program_with`].
pub fn infer_program_with_entry(
    program: &Program,
    mode: Mode,
    _entry: &str,
) -> Result<TypedProgram, CompileError> {
    // Build the type environment in two phases so declaration ORDER does not
    // matter. Phase 1 registers the pure name-mapping declarations (synonyms,
    // newtypes, typeclasses, instances); phase 2 resolves data-type
    // field/payload types against the COMPLETE alias/newtype environment. A
    // single-pass registration would wrongly reject
    // `data P = { x: Angles }; type Angles = Double; ...`.
    // The program is cloned so the prelude-combinator typing (below) can rewrite
    // it in place; inference, reduce and lower all consume the desugared form.
    let mut program = program.clone();
    let mut env = TypeEnv::with_builtins();
    // Typeclass/instance declarations are registered by `register_decls` (the
    // same path the category prelude uses).
    env.register_decls(&program.defs);
    // The USER's own definitions, captured BEFORE the Prelude merge below. This
    // is the list Strict mode's recursion check inspects (the Prelude is
    // trusted library code — a recursive prelude def such as `from` must not
    // make every Strict-mode program fail) and the shadowing set
    // [`merge_prelude_defs`] consults.
    let user_defs: Vec<crate::ast::Def> = program.defs.clone();
    // Prelude function definitions (the Buffer-arrow combinators:
    // `seq`/`par`/`split`/`merge`) are typed HERE, BEFORE the user's `data`
    // declarations register (a user `data Pair` would otherwise shadow the
    // builtin `Pair a b` the combinators' bodies use). They are pure functions,
    // so typing them against the builtin env is sound; they then flow into
    // `program.defs` so reduce/lower see them as ordinary top-level defs.
    //
    // The Prelude is typed as a WHOLE: one of its defs may call another
    // (`from` is recursive), and a user shadowing one name must not make a
    // sibling's body untypeable. Only the SURVIVING definitions
    // ([`merge_prelude_defs`]) are merged into the program and exposed as
    // schemes; the rest are typed for the Prelude's own sake and discarded.
    let prelude_defs: Vec<crate::ast::Def> =
        merge_prelude_defs(&env.prelude_defs, &user_defs, mode);
    let mut prelude_ctx: Option<Ctx> = None;
    if !env.prelude_defs.is_empty() {
        let mut pc = Ctx {
            next: 0,
            subst: Subst::default(),
            defs: HashMap::new(),
            def_bodies: HashMap::new(),
            locals: HashMap::new(),
            env: env.clone(),
            method_lifting: HashSet::new(),
            type_var_bindings: HashMap::new(),
            retry_inferred_defs: HashSet::new(),
            sig_defs: HashSet::new(),
            mode,
            runst_payload_tys: HashMap::new(),
            nested_group_depth: 0,
            given: Vec::new(),
            dict_selections: HashMap::new(),
        };
        infer_def_group(&mut pc, &env.prelude_defs)?;
        prelude_ctx = Some(pc);
        if !prelude_defs.is_empty() {
            let mut all_defs: Vec<crate::ast::Def> =
                Vec::with_capacity(program.defs.len() + prelude_defs.len());
            all_defs.extend(prelude_defs.iter().cloned());
            all_defs.extend(program.defs.iter().cloned());
            program.defs = all_defs;
        }
    }
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
                let mut free: Vec<String> = Vec::new();
                let fields_ty = fields
                    .iter()
                    .map(|(f, t, strict)| {
                        let ft = data_field_vty_with_free(&env, tyvars, t, &mut free);
                        (f.clone(), ft, *strict)
                    })
                    .collect();
                env.data_types
                    .insert(name.clone(), DataInfo::Record(fields_ty));
                if !tyvars.is_empty() {
                    env.data_arities.insert(name.clone(), tyvars.len());
                }
            }
            Def::Abstract { name, tyvars, .. } => {
                // Opaque types register exactly like records for NAME
                // resolution (so signatures may mention them), but carry no
                // constructors or fields — inference rejects constructing,
                // projecting, updating, and matching one.
                env.data_types.insert(name.clone(), DataInfo::Opaque);
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
                let mut free: Vec<String> = Vec::new();
                let ctors_ty = ctors
                    .iter()
                    .map(|(c, ts)| {
                        (
                            c.clone(),
                            ts.iter()
                                .map(|(t, strict)| {
                                    (
                                        data_field_vty_with_free(&env, tyvars, t, &mut free),
                                        *strict,
                                    )
                                })
                                .collect(),
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

    // Every type-level name a program writes must denote a type: `Integer`,
    // `Char`, `Double`, `Void` and any other undeclared uppercase name are
    // `unknown type name` (Haskell 2010 §3.1 — an uppercase identifier in a
    // type position names a declared type; a lowercase one is a type
    // variable). This runs against the COMPLETE environment, so declaration
    // order inside the program does not matter, and it covers every consumer
    // of a type name at once because each of them resolves through this same
    // environment.
    env.validate_type_names(&user_defs)?;

    // Reject recursive function definitions (v1's acyclic call contract): a
    // runtime dispatch chain must terminate, so the call stack is statically
    // bounded (see `check_recursion`). Complete mode lifts the ban: recursion
    // is supported via lazy thunks (bottom at runtime for a divergent def).
    //
    // USER defs only: the Prelude is trusted library code and may declare
    // recursive helpers (the lazy list producer `from`). Checking the merged
    // list would reject every Strict-mode program the moment such a helper
    // exists. A user definition that shadows a Prelude name is still a user
    // definition, so its recursion is still rejected.
    if mode == Mode::Strict {
        check_recursion(&user_defs)?;
    }

    // Derive `Eq`/`Ord` instances for every concrete data type (user + builtin)
    // and the scalar leaves, keeping any user-written instances intact.
    env.derive_eq_ord();

    let mut ctx = Ctx {
        next: 0,
        subst: Subst::default(),
        defs: HashMap::new(),
        def_bodies: HashMap::new(),
        locals: HashMap::new(),
        env,
        method_lifting: HashSet::new(),
        type_var_bindings: HashMap::new(),
        retry_inferred_defs: HashSet::new(),
        sig_defs: HashSet::new(),
        mode,
        runst_payload_tys: HashMap::new(),
        nested_group_depth: 0,
        given: Vec::new(),
        dict_selections: HashMap::new(),
    };

    // The prelude combinators were already typed above (against the builtin
    // env, before user `data` shadowed any builtin record). Seed their schemes
    // into the main context and exclude them from the per-program re-inference
    // so `seq`/`par`/`split`/`merge` resolve without re-typing (a user `data
    // Pair` would otherwise shadow the builtin `Pair` the bodies use).
    //
    // `prelude_defs` is already the SHADOWED list (see `merge_prelude_defs`): a
    // Prelude def whose name a user def owns is absent, so seeding it cannot
    // hand the user's name the Prelude's scheme.
    if let Some(pc) = prelude_ctx {
        for def in &prelude_defs {
            if let Some(scheme) = pc.defs.get(def.name()).cloned() {
                ctx.defs.insert(def.name().to_string(), scheme);
            }
            if let Some(b) = pc.def_bodies.get(def.name()).cloned() {
                ctx.def_bodies.insert(def.name().to_string(), b);
            }
        }
    }
    // Every USER definition is inferred, including one that shadows a Prelude
    // name: the shadowing rule already removed the Prelude def from the merged
    // list, and lowering builds its `name -> Def` map from that same list, so
    // inference and execution see one definition per name.
    infer_def_group(&mut ctx, &user_defs)?;

    // Validate instance method bodies even when the instance is never called:
    // each body must infer to a single value channel with its parameter bound
    // to a value of the instance's bound type. This catches garbage bodies
    // (signal expressions, unknown identifiers, recursive methods) at compile
    // time rather than leaving them to rot silently.
    validate_instances(&mut ctx)?;

    let cafs = program
        .defs
        .iter()
        .filter_map(|def| match def {
            Def::Eval { name, params, .. } if params.is_empty() => ctx
                .defs
                .get(name)
                .filter(|s| s.lam_count == 0)
                .map(|_| name.clone()),
            Def::Eval { .. } => None,
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
        if let Def::Eval { name, body, .. } = def {
            if def.params().is_empty() && matches!(body, Expr::Lambda { .. }) {
                let scheme = ctx.defs.get(name).cloned();
                if let Some(s) = scheme {
                    if let ValueTy::Thunk(..) = &s.ty.vty {
                        // A curried definition's signature is a NEST of unary
                        // thunks (`take = \n -> \xs -> …`); flatten to the flat
                        // λ-parameter list so the nested lambdas in lowering can
                        // consume their own leading slice.
                        let (arg_tys, _) = flatten_thunk(&s.ty.vty);
                        let resolved = arg_tys.iter().map(|t| ctx.subst.resolve_value(t)).collect();
                        fn_param_tys.insert(name.to_string(), resolved);
                    }
                }
            } else if def.params().len() == 1 {
                // A single-parameter def (`first p = p.x`, `swap p :: Pair …
                // = …`) keeps its parser parameter: the parser only curries
                // MULTI-parameter definitions into λ-nests. Its scheme is a
                // flat `Thunk([p],[r])` carrying the recovered/signature
                // parameter type for lowering's fragment-local parameter
                // registers.
                let scheme = ctx.defs.get(name).cloned();
                if let Some(s) = scheme {
                    if let ValueTy::Thunk(..) = &s.ty.vty {
                        let (arg_tys, _) = flatten_thunk(&s.ty.vty);
                        let mut resolved: Vec<ValueTy> =
                            arg_tys.iter().map(|t| ctx.subst.resolve_value(t)).collect();
                        // A body that itself returns a function (`f x = \y ->
                        // …`) flattens PAST the def's own parameter into the
                        // inner λ's; truncate to the def's formal parameter
                        // count so lowering sees one type per parameter (the
                        // signature's arity when it exceeds the written ones).
                        let arity = effective_def_params(def).0.len();
                        resolved.truncate(arity);
                        fn_param_tys.insert(name.to_string(), resolved);
                    }
                }
            }
        }
    }
    // Resolve the static RESULT value type of every shared-cell def (a named
    // λ-parameter function or a self-recursive local). Lowering types the def's
    // shared cell with this, so a recursive reference inside the body (`tail
    // fibs` — `fibs` still being compiled) resolves its static type (List)
    // instead of the Double fallback.
    let mut shared_cell_result_tys: HashMap<String, ValueTy> = HashMap::new();
    for def in &program.defs {
        // Shared-cell defs: named λ-parameter functions (Anchor) and local
        // definitions whose body is not a lambda literal (a self-recursive CAF
        // like `fibs = cons … fibs …`). Collecting every such def is harmless —
        // lowering only consults the map for shared-cell references.
        let is_shared = match def {
            Def::Eval { params, .. } if !params.is_empty() => true,
            Def::Eval { body, .. } => {
                // A self-recursive CAF (`fibs = cons … fibs …`) keeps its result
                // type; a CURRIED named function (`take = \n -> \xs -> …`) is a
                // function value, so its shared-cell type is its λ signature —
                // lowering needs it while the def's own anchor closure is still
                // compiling, so a recursive self-call dispatches through the nest
                // instead of falling back to `Double`.
                !matches!(body, Expr::Lambda { .. }) || leading_lambda_count(body) > 1
            }
            _ => false,
        };
        if is_shared {
            if let Some(s) = ctx.defs.get(def.name()).cloned() {
                let out = &s.ty.vty;
                let resolved = ctx.subst.resolve_value(out);
                // The def's scheme already carries its FULL function type
                // (`countup : Func([Double],[Double])`, `nth : <curried nest>`),
                // so the shared-cell capture type IS that signature. Wrapping it
                // in another λ layer would double the parameter list, and an
                // application through the captured value would then return the
                // leftover function type instead of the result.
                let capture_ty = resolved;
                shared_cell_result_tys.insert(def.name().to_string(), capture_ty);
            }
        }
    }

    Ok(TypedProgram {
        program: program.clone(),
        def_schemes: ctx.defs.clone(),
        cafs,
        type_env: ctx.env.clone(),
        fn_param_tys,
        shared_cell_result_tys,
        tape_decls: Vec::new(),
        runst_payload_tys: ctx.runst_payload_tys.clone(),
        mode,
        dict_selections: ctx.dict_selections.clone(),
    })
}

/// The definition's effective formal parameters and its uncurried body.
///
/// `parser::curry_defs_in` turns every definition with more than one parameter
/// into a nest of unary lambdas and clears `Def::Eval::params`, so
/// `def.params()` is empty for `f a b = …`. A `::` signature names the
/// definition's arity, and [`split_curried_params`] recovers the formal
/// parameters from the λ-chain — the same recovery constrained-definition
/// registration performs. An unannotated definition keeps its parser
/// parameters: its λ-chain is a function VALUE typed by the `Expr::Lambda` arm.
///
/// [`split_curried_params`]: crate::types::ty::split_curried_params
fn effective_def_params(def: &Def) -> (Vec<crate::ast::Param>, Expr) {
    let parser_params = def.params();
    if let Some(sig) = def.sig() {
        let arity = sig.arg_count();
        if arity > parser_params.len() {
            return crate::types::ty::split_curried_params(arity, parser_params, def.body());
        }
    }
    (parser_params.to_vec(), def.body().clone())
}

/// A definition body's inferred type, plus the signature argument types its
/// λ-parameters were bound to (empty when the definition is unannotated).
///
/// [`def_scheme`] reuses the EXACT types the body was checked against:
/// re-converting the signature would allocate fresh variables and lose the
/// sharing between a parameter and the result (`f x :: a -> a = x`).
struct DefBodyTy {
    /// The inferred body type.
    ty: ArrowTy,
    /// Signature argument types used for the λ-parameters, if any.
    sig_params: Vec<ValueTy>,
    /// Recovered λ-parameter types from the value-track retry, if any: empty
    /// when the first attempt typed the body (with signature types or the
    /// scalar default). [`def_scheme`] prefers these over the `Double`
    /// fallback so a signature-less def's scheme carries its structured
    /// parameters (`first p = p.x` → `p : Point`) instead of flattening them.
    lambda_param_tys: Vec<ValueTy>,
}

/// λ-parameters are bound as signal channels first. When that fails and the
/// definition has λ-parameters, retry with the parameters bound as VALUE
/// channels (fresh unresolved value types): a value function's λ-parameter
/// (`first p = p.x`) has no signal meaning, so its body only typechecks when
/// the parameter is a value. The failed attempt's substitution and fresh-var
/// counter are rolled back so the retry starts from a clean context. A def
/// with no λ-parameters that fails is genuinely broken and errors.
fn infer_def_body(ctx: &mut Ctx, def: &Def) -> Result<DefBodyTy, CompileError> {
    // A class-constrained definition (`mk p v :: Ix i => Pair i i -> …`) types
    // its body ONCE, under its GIVEN constraints (design §3.0). The class
    // variable stays ABSTRACT — it is not solved, it is assumed — so a method
    // call inside the body resolves against the incoming dictionary rather than
    // against a call-site type. That is the `Ix i =>` judgement: the body is
    // well-typed for EVERY `i`, and whether an instance exists is decided at
    // each call site (still a compile-time diagnostic).
    if let Some(sig) = constrained_def_sig(def) {
        return infer_constrained_def_body(ctx, def, sig);
    }
    // Effective parameters: the parser curries every multi-parameter definition
    // into a λ-nest and clears `Def::Eval::params`, so an annotated
    // `f a b :: …` keeps its formal parameters in the body's leading λs.
    let (params, body) = effective_def_params(def);
    let saved_locals = ctx.locals.clone();
    if params.is_empty() {
        // Capture the pre-attempt definition environment: a failed attempt may
        // have left nested placeholder schemes behind (an inlined instance body
        // such as `Monad (ST s)`'s `let p = … in let q = …` registers `p` in
        // `ctx.defs`, and the enclosing `let` only restores it on its success
        // path). Re-inferring on top of that leak reports a bogus
        // `duplicate definition` instead of the real error, so the retry starts
        // from the same defs the first attempt saw. Only the Complete-mode retry
        // re-infers, so Strict mode skips the map clones entirely.
        let attempt =
            (ctx.mode == Mode::Complete).then(|| (ctx.defs.clone(), ctx.def_bodies.clone()));
        let r = infer_expr(ctx, &body);
        if r.is_err() && ctx.mode == Mode::Complete {
            // Complete mode, self-recursive local (`fibs = cons … fibs …`):
            // the placeholder scheme defaults to a signal scalar, so the
            // self-reference fails value-demanding sites (`cons 1.0 fibs`).
            // Re-register the def as a value-rate channel (the lazy value
            // track is where recursion lives) and re-infer.
            let saved_subst = ctx.subst.clone();
            let saved_next = ctx.next;
            if let Some((defs, bodies)) = attempt {
                ctx.defs = defs;
                ctx.def_bodies = bodies;
            }
            rerate_value(ctx, def.name(), 0, leading_lambda_count(&body));
            let retry = infer_expr(ctx, &body);
            if retry.is_err() {
                ctx.subst = saved_subst;
                ctx.next = saved_next;
            }
            ctx.locals = saved_locals;
            return retry.map(|ty| DefBodyTy {
                ty,
                sig_params: Vec::new(),
                lambda_param_tys: Vec::new(),
            });
        }
        ctx.locals = saved_locals;
        return r.map(|ty| DefBodyTy {
            ty,
            sig_params: Vec::new(),
            lambda_param_tys: Vec::new(),
        });
    }
    let saved_subst = ctx.subst.clone();
    let saved_next = ctx.next;
    // Restore the defs/def_bodies too: a failed first attempt may leave stale
    // nested placeholder schemes in `ctx.defs`, so the retry's re-inference
    // would trip the `duplicate definition` guard. Note the empty-param path
    // restores `ctx.locals` by design — such defs capture the enclosing scope,
    // matching top-level semantics.
    let saved_defs = ctx.defs.clone();
    let saved_bodies = ctx.def_bodies.clone();
    // A nested group (`let`/`where`) keeps the enclosing λ-parameters in scope:
    // a `where` definition may reference the declaration's parameters (Haskell
    // 2010 §4.4.3.3). A top-level group starts from an empty local scope.
    if ctx.nested_group_depth == 0 {
        ctx.locals.clear();
    }
    // Signature-driven parameter typing: a definition annotated `:: Double ->
    // List a -> …` binds its λ-parameters to the signature's argument types
    // (shared type variables become fresh unification vars), instead of the
    // scalar `Double` default. The effective parameter list is the signature's
    // arity — `def.params()` is empty for a curried multi-parameter definition.
    let sig_param_tys: Option<Vec<ValueTy>> = match def.sig() {
        Some(sig) => {
            let tys = def_sig_tys(ctx, sig)?.0;
            (tys.len() == params.len()).then_some(tys)
        }
        None => None,
    };
    for (i, p) in params.iter().enumerate() {
        // λ-parameters take the signature's declared type when annotated. An
        // unannotated parameter starts as a FRESH TYPE VARIABLE, not a `Double`
        // default: Haskell 2010 §4.1.2 infers `f x = x` as `a -> a`, and
        // generalizes `a` at the definition. A `Double` default would make the
        // scheme monomorphic, so the definition could never be applied at any
        // other type and a call-site argument check would reject valid
        // programs. The body's own demands still bind the variable (`x + 1.0`
        // binds it to `Double`, `length x` to `List a`), so a scalar definition
        // ends up exactly as concrete as before.
        let ty = match sig_param_tys.as_ref().and_then(|tys| tys.get(i).cloned()) {
            Some(t) => t,
            None => ctx.fresh_vty(),
        };
        ctx.locals
            .insert(p.name.clone(), ArrowTy::uniform(0, 1, ty));
    }
    let (result, lambda_param_tys) = match infer_expr(ctx, &body) {
        Ok(t) => (
            Ok(t),
            // The parameter types the body was checked against, resolved
            // through the substitution: an unconstrained parameter stays a
            // structural `Var` (polymorphic), a constrained one is concrete.
            // `def_scheme` lifts these into the definition's scheme.
            resolved_param_tys(ctx, &params),
        ),
        Err(_) => {
            ctx.subst = saved_subst.clone();
            ctx.next = saved_next;
            ctx.defs = saved_defs.clone();
            ctx.def_bodies = saved_bodies.clone();
            ctx.locals.clear();
            for p in &params {
                let v = ctx.next;
                ctx.next += 1;
                ctx.locals
                    .insert(p.name.clone(), ArrowTy::value_channel(ValueTy::Var(v)));
            }
            // Complete-mode recursion: the value-track retry re-registers the
            // CURRENT def's placeholder scheme with VALUE-rate channels, so a
            // self-recursive reference (`countdown (n - 1)` inside `countdown`)
            // resolves to the value track. Recursion in the lazy model IS the
            // value track; the placeholder's signal default would otherwise
            // leak into the recursive call and fail any value-demanding site.
            if ctx.mode == Mode::Complete {
                // Re-register the CURRENT def as value-rate (self-recursion).
                rerate_value(ctx, def.name(), params.len(), leading_lambda_count(&body));
                // Also re-register the top-level defs this body REFERENCES —
                // mutual recursion (`even` calls `odd`) would otherwise leave
                // the partner on a signal placeholder and fail value-demanding
                // sites inside this body.
                let mut refs = HashSet::new();
                collect_ref_names(&body, &mut refs);
                for n in refs {
                    if let Some(s) = ctx.defs.get(n.as_str()) {
                        let lam_count = s.lam_count;
                        let body_lams = ctx
                            .def_bodies
                            .get(n.as_str())
                            .map(leading_lambda_count)
                            .unwrap_or(0);
                        rerate_value(ctx, n.as_str(), lam_count, body_lams);
                    }
                }
            }
            let retry = infer_expr(ctx, &body);
            // Carry the VALUE-track retry's resolved λ-parameter types in the
            // returned body type: the body's value demands (`tail xs` → xs :
            // List, `k - 1.0` → k : Double) resolve the fresh param vars here,
            // and the final scheme would otherwise discard them (λ-params are
            // stored as signal scalars). `def_scheme` lifts them into the
            // scheme, and lowering's `fn_param_tys` rebuild reads them from
            // there — no separate side-channel is needed.
            let lambda_param_tys = if retry.is_ok() {
                let mut param_tys = Vec::with_capacity(params.len());
                for p in &params {
                    let lt = ctx.locals.get(p.name.as_str()).cloned();
                    let vty = match lt {
                        Some(t) => t.vty.clone(),
                        _ => ValueTy::Double,
                    };
                    let resolved = ctx.subst.resolve_value(&vty);
                    // An unresolvable parameter (a Var never constrained by the
                    // body — a recursive call's arguments are not re-inferred)
                    // is a v1 scalar λ-parameter: Double.
                    param_tys.push(match resolved {
                        ValueTy::Var(_) => ValueTy::Double,
                        other => other,
                    });
                }
                param_tys
            } else {
                Vec::new()
            };
            (retry, lambda_param_tys)
        }
    };
    ctx.locals = saved_locals;
    result.map(|ty| DefBodyTy {
        ty,
        sig_params: sig_param_tys.unwrap_or_default(),
        lambda_param_tys,
    })
}

/// Type a class-constrained definition's body ONCE, under its GIVEN constraints
/// (design §3.0).
///
/// `mk p v :: Ix i => Pair i i -> Double -> Double` is the judgement
/// `Ix i => Pair i i -> Double -> Double`: the class variable `i` is ASSUMED, not
/// solved. The body is therefore typed with `Ix i` in scope, so `rangeSize p`
/// resolves against the incoming `Ix` DICTIONARY (an abstract witness) instead
/// of demanding a concrete index type — which is exactly the old bug this
/// replaces (typing `p` as the scalar `Double` default and failing).
///
/// The body's result is then checked against the signature's declared result
/// type, so an ill-typed constrained body (`mk p :: Ix i => Pair i i -> Double =
/// "nope"`) is rejected at LOAD rather than per call site.
///
/// GIVEN constraints are visible to nested λ-bodies and `where` definitions too
/// (`ctx.given` is saved and restored around the body, and λ bodies clear the
/// method-inlining guard but not the given context).
fn infer_constrained_def_body(
    ctx: &mut Ctx,
    def: &Def,
    sig: &crate::ast::TypeExpr,
) -> Result<DefBodyTy, CompileError> {
    let (params, body) = effective_def_params(def);
    // One signature conversion for BOTH the parameters and the result: the
    // class variable `i` and the element variables `e`/`s` must be the SAME ids
    // in the parameter types the body is typed against and in the result type
    // it is checked against.
    let (sig_params, sig_ret, vars) = def_sig_tys(ctx, sig)?;
    if sig_params.len() != params.len() {
        return Err(CompileError::Type {
            msg: format!(
                "signature of `{}` declares {} argument(s), but the definition binds {}",
                def.name(),
                sig_params.len(),
                params.len()
            ),
            span: def.body().span(),
        });
    }
    // The GIVEN context: every recorded class constraint, with the id the
    // signature conversion gave its variable. A constraint whose variable the
    // substitution already pinned CONCRETE (`Ix Int => …`) needs no dictionary
    // — it is discharged by that instance — and one the signature does not
    // mention at all is ambiguous; both are skipped, mirroring
    // [`scheme_constraints`].
    let mut given: Vec<GivenVar> = Vec::new();
    for (class, var) in sig_constraints(sig) {
        if is_declared_type_name(&ctx.env, var) {
            continue;
        }
        let arity = ctx
            .env
            .typeclasses
            .get(class.as_str())
            .map(|c| c.arity)
            .unwrap_or(0);
        // An arity-0 class variable is a plain type variable, so its id is the
        // one the body's types carry. A constructor-class variable (`f` in
        // `Functor f`) is an application HEAD, which the signature conversion
        // spells by NAME; the witness then matches on that head. An AMBIGUOUS
        // context (`C i => Double`, `i` in no argument or result) has no
        // variable to pair a dictionary with — the missing ambiguity check is an
        // open defect, and such a constraint is not a witness here either.
        let id = if arity == 0 {
            match vars.get(var).map(|t| ctx.subst.resolve_value(t)) {
                Some(ValueTy::Var(v)) | Some(ValueTy::TyConVar(v)) => Some(v),
                // Solved to a concrete type: already discharged by that
                // instance, so no dictionary parameter is needed.
                Some(_) => continue,
                // The signature never mentions the variable.
                None => continue,
            }
        } else {
            // A constructor-class variable is spelled as an application HEAD by
            // name, so it never appears as an entry in `vars` — its witness is
            // the head name itself.
            None
        };
        given.push(GivenVar {
            class: class.clone(),
            name: var.clone(),
            id,
        });
    }
    let saved_locals = ctx.locals.clone();
    let saved_given = ctx.given.clone();
    let saved_subst = ctx.subst.clone();
    let saved_next = ctx.next;
    if ctx.nested_group_depth == 0 {
        ctx.locals.clear();
    }
    ctx.given = given;
    for (p, ty) in params.iter().zip(sig_params.iter()) {
        ctx.locals
            .insert(p.name.clone(), ArrowTy::uniform(0, 1, ty.clone()));
    }
    let inferred = infer_expr_expected(ctx, &body, Some(sig_ret.clone()));
    let result = match inferred {
        Ok(t) => {
            // The body must produce the signature's declared result. A concrete
            // mismatch is rejected here; an abstract result (`e`, `ST s e`)
            // unifies its signature variables, which is the check that the
            // shape is the declared one.
            unify_value(&t.vty, &sig_ret, &mut ctx.subst, body.span()).map(|()| t)
        }
        Err(e) => Err(e),
    };
    let lambda_param_tys = if result.is_ok() {
        resolved_param_tys(ctx, &params)
    } else {
        Vec::new()
    };
    ctx.locals = saved_locals;
    ctx.given = saved_given;
    if result.is_err() {
        // Roll the substitution back so a rejected body leaves no bindings
        // behind (the same hygiene the unconstrained retry applies).
        ctx.subst = saved_subst;
        ctx.next = saved_next;
    }
    result.map(|ty| DefBodyTy {
        ty,
        sig_params,
        lambda_param_tys,
    })
}

/// Re-register a def's placeholder scheme with VALUE-rate channels (complete
/// mode): recursion in the lazy model IS the value track, and a signal-default
/// placeholder would leak into a recursive call and fail any value-demanding
/// site. `lam_count` is the number of λ-parameters.
fn rerate_value(ctx: &mut Ctx, name: &str, lam_count: usize, body_lams: usize) {
    let mut ins = Vec::with_capacity(lam_count);
    for _ in 0..lam_count {
        let v = ctx.next;
        ctx.next += 1;
        ins.push(ValueTy::Var(v));
    }
    let out = ctx.next;
    ctx.next += 1;
    let vty = if lam_count == 0 && body_lams == 0 {
        ValueTy::Var(out)
    } else if lam_count == 0 {
        let mut ty = ValueTy::Var(out);
        for _ in 0..body_lams {
            let v = ctx.next;
            ctx.next += 1;
            ty = ValueTy::Thunk(vec![ValueTy::Var(v)], vec![ty]);
        }
        ty
    } else {
        ValueTy::Thunk(ins, vec![ValueTy::Var(out)])
    };
    ctx.defs.insert(
        name.to_string(),
        Scheme {
            lam_count,
            vars: vec![],
            // A recursion placeholder is a fresh unconstrained signature, so it
            // carries no class context.
            constraints: Vec::new(),
            ty: ArrowTy { vty },
        },
    );
}

/// Collect every user-definition name referenced by `e` (a `Ref` or an applied
/// callee) into `out`.
fn collect_ref_names(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Ref(name, _) => {
            out.insert(name.to_string());
        }
        Expr::Apply { name, args, .. } => {
            out.insert(name.to_string());
            for a in args {
                collect_ref_names(a, out);
            }
        }
        Expr::ApplyExpr { callee, args, .. } => {
            collect_ref_names(callee, out);
            for a in args {
                collect_ref_names(a, out);
            }
        }
        Expr::Neg(inner, _) => collect_ref_names(inner, out),
        Expr::Arith { lhs, rhs, .. } => {
            collect_ref_names(lhs, out);
            collect_ref_names(rhs, out);
        }
        Expr::If {
            cond, then, els, ..
        } => {
            collect_ref_names(cond, out);
            collect_ref_names(then, out);
            collect_ref_names(els, out);
        }
        _ => {}
    }
}

/// Infer a group of mutually-recursive definitions (top-level, where, or let).
/// Two-phase: first register placeholder schemes for all names, then infer
/// each body with the full mutual environment.
fn infer_def_group(ctx: &mut Ctx, defs: &[Def]) -> Result<(), CompileError> {
    infer_def_group_scoped(ctx, defs, false)
}

/// Infer a group of definitions, saying whether the group is NESTED inside
/// another definition.
///
/// A nested group (`let`/`where`) captures the enclosing λ-parameters, so its
/// bodies are inferred with the enclosing `ctx.locals` still in scope (Haskell
/// 2010 §4.4.3.3: a `where` clause sees the declaration's formal parameters).
/// A top-level group starts from an empty local scope.
fn infer_def_group_scoped(ctx: &mut Ctx, defs: &[Def], nested: bool) -> Result<(), CompileError> {
    if defs.is_empty() {
        return Ok(());
    }
    let saved_depth = ctx.nested_group_depth;
    // A nested group's definitions shadow the enclosing ones, so their
    // signature status is scoped to the group: a `let`/`where` definition
    // without a `::` must not inherit the signature-ness of the top-level
    // definition it shadows (see [`Ctx::sig_defs`]).
    let saved_sig_defs = ctx.sig_defs.clone();
    if nested {
        ctx.nested_group_depth += 1;
    }
    let result = infer_def_group_inner(ctx, defs);
    ctx.nested_group_depth = saved_depth;
    ctx.sig_defs = saved_sig_defs;
    result
}

fn infer_def_group_inner(ctx: &mut Ctx, defs: &[Def]) -> Result<(), CompileError> {
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
        // An explicit `::` signature is the call-site contract: remember the
        // name so the argument check treats its parameter types as the concrete
        // types the user wrote (see [`Ctx::sig_defs`]).
        if def.sig().is_some() {
            ctx.sig_defs.insert(def.name().to_string());
        }
        let lam_count = def.params().len();
        let body_lams = leading_lambda_count(def.body());
        let mut ins = Vec::with_capacity(lam_count);
        for _ in 0..lam_count {
            ins.push(ctx.fresh_vty());
        }
        let out = ctx.fresh_vty();
        let vty = if lam_count == 0 && body_lams == 0 {
            out
        } else if lam_count == 0 {
            // Curried definition (`take n xs = …` → `take = \n -> \xs -> …`):
            // the parameter list is empty but the body is a nest of λs. The
            // placeholder must already be a function value, otherwise a
            // recursive self-call in the body cannot dispatch — `take` sees a
            // bare placeholder var and reports "expects 0 argument(s)".
            let mut ty = out;
            for _ in 0..body_lams {
                ty = ValueTy::Thunk(vec![ctx.fresh_vty()], vec![ty]);
            }
            ty
        } else {
            ValueTy::Thunk(ins, vec![out])
        };
        ctx.defs.insert(
            def.name().to_string(),
            Scheme {
                lam_count,
                vars: vec![],
                // Placeholder scheme: the class context (if any) is recorded
                // when the real scheme replaces this one in phase 2.
                constraints: Vec::new(),
                ty: ArrowTy { vty },
            },
        );
    }

    // Phase 2: infer bodies with placeholder visibility
    for def in defs {
        if def.is_decl() {
            continue;
        }
        if !def.where_defs().is_empty() {
            // A `where` block captures the declaration's λ-parameters, so its
            // group is nested (Haskell 2010 §4.4.3.3).
            infer_def_group_scoped(ctx, def.where_defs(), true)?;
        }
        let body_ty = infer_def_body(ctx, def)?;
        // M10 marker: a signature-less CURRIED definition (`at a i = a ! i`,
        // whose body is a λ-nest) recovers structured parameter types
        // (`Array i e`) from the constrained `index` inside the λ-body, but the
        // recovered shapes surface only at the call site. Remember the name so
        // a call-site mismatch can point at the missing annotation instead of
        // dumping the recovered shapes.
        if def.sig().is_none() && leading_lambda_count(def.body()) > 0 {
            ctx.retry_inferred_defs.insert(def.name().to_string());
        }
        let lam_count = effective_def_params(def).0.len();
        let (vty, constraints) = def_scheme(ctx, def, &body_ty, lam_count)?;
        let resolved = ctx.subst.apply(&ArrowTy { vty });
        let vars = free_vars_quantified(ctx, &resolved);
        ctx.defs.remove(def.name());
        ctx.defs.insert(
            def.name().to_string(),
            Scheme {
                lam_count,
                vars,
                constraints,
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
        let lam_count = effective_def_params(def).0.len();
        let (vty, constraints) = def_scheme(ctx, def, &body_ty, lam_count)?;
        let resolved = ctx.subst.apply(&ArrowTy { vty });
        let vars = free_vars_quantified(ctx, &resolved);
        ctx.defs.remove(def.name());
        ctx.defs.insert(
            def.name().to_string(),
            Scheme {
                lam_count,
                vars,
                constraints,
                ty: resolved,
            },
        );
    }

    Ok(())
}

/// Resolve a definition's parameter types through the substitution.
///
/// Called after a body is inferred: a parameter the body constrained
/// (`x + 1.0` → `Double`, `length x` → `List a`) resolves to its concrete type,
/// while a parameter the body never constrains (`f x = x`) stays a structural
/// type variable. Keeping it a variable is what makes the definition's scheme
/// genuinely polymorphic (Haskell 2010 §4.1.2), so `fn_param_tys` and the
/// call-site argument check can tell "accepts anything" apart from "accepts
/// `Double`".
fn resolved_param_tys(ctx: &Ctx, params: &[Param]) -> Vec<ValueTy> {
    params
        .iter()
        .map(|p| match ctx.locals.get(p.name.as_str()) {
            Some(t) => ctx.subst.resolve_value(&t.vty),
            None => ValueTy::Double,
        })
        .collect()
}

/// The definition's scheme: its generalized value type and the CLASS
/// CONSTRAINTS its signature requires.
///
/// A class-constrained definition takes its scheme from the SIGNATURE: its body
/// was deferred (it has no inferred type of its own), and the signature's
/// argument/result types are exactly the call-site contract. The signature's
/// arity — not `lam_count` — decides the shape, because a multi-parameter
/// definition is stored curried (`Def::Eval::params` empty, body a λ-chain).
/// An annotated definition reuses the argument types its body was checked
/// against (`body_ty.sig_params`), so the scheme and the lowered parameter
/// registers agree — a `Double` scheme for an `Int`-typed parameter is exactly
/// the divergence this avoids. An unannotated definition carries the parameter
/// types its body was checked against (`body_ty.lambda_param_tys`) — concrete
/// where the body constrained them (`first p = p.x` → `p : Point`), a type
/// variable where it did not (`f x = x` → `a -> a`) — instead of the scalar
/// `Double` default.
///
/// The constraint list comes from the signature's `(C1 a, C2 b) =>` context
/// (empty for an unconstrained definition). Each constraint's type variable is
/// resolved through the substitution, so it is the SAME id the scheme type
/// carries — a caller that later discharges the constraint (passing a
/// dictionary) can rely on the pairing. A constraint whose variable the
/// substitution solved to a concrete type is dropped: it needs no dictionary
/// because it is already determined.
fn def_scheme(
    ctx: &mut Ctx,
    def: &Def,
    body_ty: &DefBodyTy,
    lam_count: usize,
) -> Result<(ValueTy, Vec<(String, TypeVarId)>), CompileError> {
    if let Some(sig) = constrained_def_sig(def) {
        let (sig_params, sig_ret, vars) = def_sig_tys(ctx, sig)?;
        let vty = if sig_params.is_empty() {
            sig_ret
        } else {
            ValueTy::Thunk(sig_params, vec![sig_ret])
        };
        let constraints = scheme_constraints(ctx, sig, &vars);
        return Ok((vty, constraints));
    }
    let vty = if lam_count == 0 {
        body_ty.ty.vty.clone()
    } else if body_ty.sig_params.len() == lam_count {
        // Signature-driven scheme: the body was typed against these exact
        // argument types, so call sites and lowering agree.
        ValueTy::Thunk(body_ty.sig_params.clone(), vec![body_ty.ty.vty.clone()])
    } else if body_ty.lambda_param_tys.len() == lam_count {
        // Recovered parameter types: the types the body was actually checked
        // against (`x + 1.0` → `x : Double`, `p.x` → `p : Point`, an
        // unconstrained `x` → a type variable). Carrying them here — rather
        // than the scalar `Double` fallback — keeps an unannotated definition's
        // polymorphism and gives the call-site argument check the real
        // parameter types.
        ValueTy::Thunk(
            body_ty.lambda_param_tys.clone(),
            vec![body_ty.ty.vty.clone()],
        )
    } else {
        let param_tys: Vec<ValueTy> = (0..lam_count).map(|_| ValueTy::Double).collect();
        ValueTy::Thunk(param_tys, vec![body_ty.ty.vty.clone()])
    };
    Ok((vty, Vec::new()))
}

/// Resolve an arm/branch body's result value type: a value channel directly,
/// or a bare `Int`/`Double` literal (signal-rate in v1 but value-compatible in
/// value positions).
fn arm_result_vty(bt: ArrowTy, body: &Expr, span: Span) -> Result<ValueTy, CompileError> {
    if bt.arity_in() != 0 || bt.arity_out() != 1 {
        return Err(CompileError::Type {
            msg: "branch must be a value expression (0->1 value channel)".into(),
            span,
        });
    }
    match &bt.vty {
        ValueTy::Buffer(_) => match body {
            Expr::Int(_, _) => Ok(ValueTy::Int),
            Expr::Double(_, _) => Ok(ValueTy::Double),
            _ => Err(CompileError::Type {
                msg: "branch must be a value expression (0->1 value channel)".into(),
                span,
            }),
        },
        vty => Ok(vty.clone()),
    }
}

/// Whether a value type mentions a given type-variable id anywhere — a free
/// `Var(v)` / `TyConVar(v)` occurrence or a `TyConApp(v, ..)` head where
/// `v == id`. Used by `runST`'s rank-2 application (a body whose result type
/// mentions the fresh quantified phantom `s` escapes the region and is
/// rejected).
fn vty_mentions_tyvar(t: &ValueTy, id: TypeVarId) -> bool {
    match t {
        ValueTy::Var(v) | ValueTy::TyConVar(v) => *v == id,
        ValueTy::TyConApp(f, args) => *f == id || args.iter().any(|a| vty_mentions_tyvar(a, id)),
        ValueTy::Thunk(args, rets) => {
            args.iter().any(|a| vty_mentions_tyvar(a, id))
                || rets.iter().any(|r| vty_mentions_tyvar(r, id))
        }
        ValueTy::Data(_, a) | ValueTy::Newtype(_, a) | ValueTy::App(_, a) => {
            a.iter().any(|x| vty_mentions_tyvar(x, id))
        }
        // The element is part of the buffer type: a variable element is
        // mentioned by the buffer, so a region phantom / quantified element
        // cannot hide inside it.
        ValueTy::Buffer(elem) => vty_mentions_tyvar(elem, id),
        _ => false,
    }
}

/// Collect every free value-type variable id in `t` — a free `Var(v)` /
/// `TyConVar(v)` occurrence or a `TyConApp(v, ..)` head, plus `Func`
/// arguments/results and `Data`·`Newtype`·`App` arguments — resolved through
/// `subst.value_map` and deduplicated. Mirrors [`vty_mentions_tyvar`]'s
/// structure so the two can be cross-checked: a var is free exactly when it is
/// "mentioned" in the unresolved representative.
fn value_free_vars(t: &ValueTy, subst: &Subst) -> Vec<TypeVarId> {
    let mut acc = Vec::new();
    value_free_vars_into(t, subst, &mut acc);
    acc
}

fn value_free_vars_into(t: &ValueTy, subst: &Subst, acc: &mut Vec<TypeVarId>) {
    match t {
        ValueTy::Var(_) | ValueTy::TyConVar(_) => {
            // Follow the substitution chain; a var bound to a concrete type
            // contributes nothing (its free vars were resolved away).
            match subst.resolve_value(t) {
                ValueTy::Var(v) | ValueTy::TyConVar(v) => {
                    if !acc.contains(&v) {
                        acc.push(v);
                    }
                }
                other => value_free_vars_into(&other, subst, acc),
            }
        }
        ValueTy::TyConApp(f, args) => {
            if let ValueTy::TyConVar(rf) = subst.resolve_value(&ValueTy::TyConVar(*f)) {
                if !acc.contains(&rf) {
                    acc.push(rf);
                }
            }
            for a in args {
                value_free_vars_into(a, subst, acc);
            }
        }
        ValueTy::Thunk(args, rets) => {
            for a in args {
                value_free_vars_into(a, subst, acc);
            }
            for r in rets {
                value_free_vars_into(r, subst, acc);
            }
        }
        ValueTy::Data(_, a) | ValueTy::Newtype(_, a) | ValueTy::App(_, a) => {
            for x in a {
                value_free_vars_into(x, subst, acc);
            }
        }
        // A buffer's element carries free variables too.
        ValueTy::Buffer(elem) => value_free_vars_into(elem, subst, acc),
        _ => {}
    }
}

/// Substitute quantified value-type variables with fresh ids, recursively over
/// `Func`/`Data`/`Newtype`/`App` arguments and `TyConApp` heads. Every
/// `Var(v)`/`TyConVar(v)`/`TyConApp(v, ..)` whose id is in `fresh` is replaced
/// by its fresh counterpart; unquantified vars pass through unchanged.
fn value_instantiate(t: &ValueTy, fresh: &HashMap<TypeVarId, TypeVarId>) -> ValueTy {
    match t {
        ValueTy::Var(v) => fresh
            .get(v)
            .map(|f| ValueTy::Var(*f))
            .unwrap_or_else(|| t.clone()),
        ValueTy::TyConVar(v) => fresh
            .get(v)
            .map(|f| ValueTy::TyConVar(*f))
            .unwrap_or_else(|| t.clone()),
        ValueTy::TyConApp(f, args) => {
            let args = args.iter().map(|a| value_instantiate(a, fresh)).collect();
            let f = fresh.get(f).copied().unwrap_or(*f);
            ValueTy::TyConApp(f, args)
        }
        ValueTy::Thunk(args, rets) => ValueTy::Thunk(
            args.iter().map(|a| value_instantiate(a, fresh)).collect(),
            rets.iter().map(|r| value_instantiate(r, fresh)).collect(),
        ),
        ValueTy::Data(name, a) | ValueTy::Newtype(name, a) | ValueTy::App(name, a) => {
            let a = a.iter().map(|x| value_instantiate(x, fresh)).collect();
            match t {
                ValueTy::Data(..) => ValueTy::Data(name.clone(), a),
                ValueTy::Newtype(..) => ValueTy::Newtype(name.clone(), a),
                _ => ValueTy::App(name.clone(), a),
            }
        }
        // A quantified element variable must be FRESHENED with the buffer, or
        // two instantiations would share the element's id.
        ValueTy::Buffer(elem) => ValueTy::Buffer(Box::new(value_instantiate(elem, fresh))),
        _ => t.clone(),
    }
}

/// Infer one arm's guards (each must be a Bool value) and unify all its bodies
/// into the running result type. The arm's pattern vars must already be bound
/// in `ctx.locals`.
fn infer_guarded_arm_body(
    ctx: &mut Ctx,
    arm: &MatchArm,
    result: &mut Option<ValueTy>,
) -> Result<(), CompileError> {
    for (g, body) in &arm.guards {
        if !matches!(g, Expr::Bool(true, _)) {
            let gt = infer_expr(ctx, g)?;
            if gt.arity_out() != 1 || !matches!(ctx.subst.resolve_value(&gt.vty), ValueTy::Bool) {
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
/// alternative is not the bare `-> body` form (guard `true`).
fn arm_is_guarded(arm: &MatchArm) -> bool {
    arm.guards.len() > 1 || !matches!(arm.guards.first(), Some((Expr::Bool(true, _), _)))
}

/// Whether an arm's guard alternatives END in an unconditional fallback — a
/// literal `True` guard or the Prelude constant `otherwise`.
///
/// Such an arm covers its pattern completely: at runtime the preceding guards
/// either match or fall through into the final `True` alternative. Haskell 2010
/// gives `otherwise` no special status in the totality rule (§4.4.3.1): the
/// compiler recognises the fallback because the Prelude binds
/// `otherwise = True`. GHC's coverage checker does the same (its
/// always-true-guard test looks for the literal `True` and the name
/// `otherwise`), and so does this one — `otherwise` otherwise behaves as the
/// ordinary Bool value it is.
fn arm_ends_unconditionally(arm: &MatchArm) -> bool {
    matches!(
        arm.guards.last(),
        Some((flag, _)) if guard_is_always_true(flag)
    )
}

/// Whether a guard expression is unconditionally true by construction: the
/// literal `True`, or the Prelude constant `otherwise`.
fn guard_is_always_true(guard: &Expr) -> bool {
    match guard {
        Expr::Bool(true, _) => true,
        Expr::Ref(name, _) => name == "otherwise",
        _ => false,
    }
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
    ctx: &mut Ctx,
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
        Pattern::LitInt(_) | Pattern::LitDouble(_) | Pattern::LitBool(_) | Pattern::LitStr(_) => {
            Err(CompileError::Type {
                msg: "literal pattern requires a scalar scrutinee".into(),
                span: arm_span,
            })
        }
        // A bang pattern forces the matched value at runtime but does not change
        // its type: bind/check the inner pattern.
        Pattern::Bang(inner) => bind_pattern(ctx, inner, vty, fallback_sum, arm_span),
    }
}

/// Type-check every arm against the (already pinned) sum scrutinee: patterns,
/// guards (Bool), and unified bodies. The pattern is bound into `ctx.locals`
/// BEFORE the guards and bodies are inferred, because a guard references the
/// pattern's bound variables (`n | n > 0 => ...` needs `n`). Returns the
/// unified arm-body result value type.
fn check_match_arms(
    ctx: &mut Ctx,
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
    Ok(result.unwrap_or(ValueTy::Double))
}

/// Pin a literal pattern's type to the scrutinee type (so a `Wire` scrutinee
/// infers `Int` from a `0` arm) and bind variable patterns.
fn check_scalar_pattern(
    ctx: &mut Ctx,
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
        Pattern::LitDouble(_) => {
            unify_value(scrutinee_vty, &ValueTy::Double, &mut ctx.subst, arm_span)
        }
        Pattern::LitBool(_) => unify_value(scrutinee_vty, &ValueTy::Bool, &mut ctx.subst, arm_span),
        Pattern::LitStr(_) => {
            unify_value(scrutinee_vty, &ValueTy::String, &mut ctx.subst, arm_span)
        }
        Pattern::Ctor(_, _) => Err(CompileError::Type {
            msg: "constructor pattern requires a sum scrutinee".into(),
            span: arm_span,
        }),
        Pattern::Bang(inner) => check_scalar_pattern(ctx, inner, scrutinee_vty, arm_span),
    }
}

/// Scalar totality: a `Wild`/`Var` arm, or (for Bool) both literals.
fn check_exhaustive_scalar(
    ctx: &Ctx,
    arms: &[MatchArm],
    scrutinee_vty: &ValueTy,
    span: Span,
) -> Result<(), CompileError> {
    let is_bool = matches!(ctx.subst.resolve_value(scrutinee_vty), ValueTy::Bool);
    let mut has_wild = false;
    let mut has_true = false;
    let mut has_false = false;
    for a in arms {
        if arm_is_guarded(a) && !arm_ends_unconditionally(a) {
            continue;
        }
        match a.pattern.strip_bang() {
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
    ctx: &Ctx,
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
        if arm_is_guarded(a) && !arm_ends_unconditionally(a) {
            continue;
        }
        match a.pattern.strip_bang() {
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
/// Convert an EXPRESSION annotation type (`e :: T`, Haskell 2010 §3.12) into a
/// value type.
///
/// The conversion is exactly the one a definition signature gets
/// ([`def_sig_tys_conv`]): a declared type name resolves through the env, a
/// lowercase name is a fresh type VARIABLE shared by every occurrence of that
/// name in the annotation. Sharing the conversion is what makes `(e :: T)`
/// and `f :: T; f = e` equivalent, which is Haskell 2010's rule.
///
/// [`type_expr_to_vty`] is deliberately NOT used here: it maps an undeclared
/// name to a nominal `Data`, which is right for a builtin signature's concrete
/// types but wrong for an annotation — `e :: a` mentions a type variable, not
/// a type `a`.
fn annot_type_vty(ctx: &mut Ctx, te: &crate::ast::TypeExpr) -> ValueTy {
    let env = ctx.env.clone();
    let mut vars: HashMap<String, ValueTy> = HashMap::new();
    // No class variable is special in an expression annotation: a context's
    // variables are ordinary type variables until a call site solves them.
    let subst = ClassVarSubst::Ctor(String::new());
    let mut fresh = || ctx.fresh_vty();
    conv(&env, "", &subst, &mut vars, &mut fresh, te)
}

fn infer_expr(ctx: &mut Ctx, e: &Expr) -> Result<ArrowTy, CompileError> {
    infer_expr_expected(ctx, e, None)
}

/// Infer an expression with an optional **expected value type**. Used for
/// result-directed typeclass dispatch (`mempty`, `pure`, `return`): a method
/// whose signature has no class-var-applied argument is resolved by the type it
/// is expected to produce. `None` means "unknown" — such a method then errors
/// with "expected type unknown".
fn infer_expr_expected(
    ctx: &mut Ctx,
    e: &Expr,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    match e {
        Expr::Int(_, _) => Ok(ArrowTy::value(match expected {
            // A numeric literal in a Buffer-expected position is a signal
            // constant (`_ * 0.5` → `0.5 : Buffer`).
            Some(ValueTy::Buffer(_)) => ctx.env.buffer_vty(),
            _ => ValueTy::Int,
        })),
        Expr::Double(_, _) => Ok(ArrowTy::value(match expected {
            Some(ValueTy::Buffer(_)) => ctx.env.buffer_vty(),
            _ => ValueTy::Double,
        })),
        Expr::Void(_) => Ok(ArrowTy::value_channel(ValueTy::Void)),
        Expr::Ref(name, span) => infer_ref_expected(ctx, name, *span, expected),
        Expr::Neg(inner, span) => {
            let t = infer_expr(ctx, inner)?;
            check_all_numeric(ctx, &t, *span)?;
            Ok(t)
        }
        Expr::Annot { expr, ty, span } => {
            // `e :: T` (Haskell 2010 §3.12). The annotation pins the inner
            // expression's type and the whole node takes the declared type:
            // that is what makes `(1 :: Int) + 1.0` a `Fractional Int`-style
            // mismatch instead of silently retyping the literal. The type is
            // a compile-time construct — nothing about it survives to
            // lowering.
            let ann = annot_type_vty(ctx, ty);
            let inner = infer_expr_expected(ctx, expr, Some(ann.clone()))?;
            unify_value(&inner.vty, &ann, &mut ctx.subst, *span)?;
            Ok(ArrowTy::value(ctx.subst.resolve_value(&ann)))
        }
        Expr::Apply { name, args, span } => infer_apply_expected(ctx, name, args, *span, expected),
        Expr::ApplyExpr { callee, args, span } => {
            infer_apply_expr(ctx, callee, args, *span, expected)
        }
        Expr::Str(_, _) => Ok(ArrowTy::value_channel(ValueTy::String)),
        Expr::Arith { op, lhs, rhs, span } => infer_arith(ctx, *op, lhs, rhs, *span),
        Expr::Let {
            defs,
            body,
            span: _,
        } => {
            let saved_defs = ctx.defs.clone();
            // A `let` block captures the enclosing λ-parameters.
            infer_def_group_scoped(ctx, defs, true)?;
            let ty = infer_expr(ctx, body)?;
            ctx.defs = saved_defs;
            Ok(ty)
        }
        Expr::Record(_, _) => Ok(ArrowTy::uniform(0, 1, ValueTy::Double)),
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
            // An actor param is a control value: in a signal position it is a
            // signal constant (`Buffer`), elsewhere a numeric scalar. Leave it
            // unresolved so the surrounding arithmetic/combinator pins it.
            let s = ctx.fresh_vty();
            Ok(ArrowTy::uniform(0, 1, s))
        }
        Expr::FieldProject {
            record,
            field,
            span,
        } => {
            // A result-directed typeclass call in record position (`(arr f).f`)
            // selects its instance by the record type, which only the FIELD
            // name identifies (`unKleisli` → `Kleisli m a b`). When the field
            // names exactly one record type, thread it as the expected type so
            // the record's own inference resolves (an unresolved variable
            // record stays deferred as before).
            let rec_candidates: Vec<String> = ctx
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
            let rec_expected = if rec_candidates.len() == 1 {
                let rec_name = &rec_candidates[0];
                // A builtin constructor (`Pair a b`) lives in `ctor_kinds`, not
                // `data_arities`. Consult both so the record's type args are
                // FRESH vars, not the placeholder positions `Var(1)`/`Var(2)`
                // from [`DataInfo::Record`] — those placeholders share the id
                // space with live inference vars, so leaking them corrupts the
                // substitution (`p.second` typed as raw `Var(2)`).
                let arity = ctx
                    .env
                    .data_arities
                    .get(rec_name)
                    .copied()
                    .or_else(|| ctx.env.ctor_arity(rec_name))
                    .unwrap_or(0);
                let args = (0..arity).map(|_| ctx.fresh_vty()).collect::<Vec<_>>();
                // Builtin constructors are spelled `App`; user data types `Data`.
                let shape = if ctx.env.ctor_arity(rec_name).is_some() {
                    ValueTy::App(rec_name.clone(), args)
                } else {
                    ValueTy::Data(rec_name.clone(), args)
                };
                Some(shape)
            } else {
                None
            };
            let t = match &rec_expected {
                Some(e) => infer_expr_expected(ctx, record, Some(e.clone()))?,
                None => infer_expr(ctx, record)?,
            };
            // record must be a single Value channel of a known Data type
            if t.arity_out() != 1 {
                return Err(CompileError::Type {
                    msg: "field projection requires a data value".into(),
                    span: *span,
                });
            }
            match &t.vty {
                ValueTy::Data(name, args) => match ctx.env.data_types.get(name.as_str()) {
                    Some(DataInfo::Record(fields)) => {
                        let fty = fields
                            .iter()
                            .find(|(f, _, _)| f == field)
                            .map(|(_, t, _)| t.clone());
                        match fty {
                            // Parameterized user data (`data Box a = { value: a }`,
                            // `data Kleisli m a b = { unKleisli: a -> m b }`):
                            // substitute the record's concrete type args for the
                            // field type's placeholder positions.
                            Some(ft) if !args.is_empty() => {
                                Ok(ArrowTy::value_channel(substitute_field_params(&ft, args)))
                            }
                            Some(ft) => Ok(ArrowTy::value_channel(ft)),
                            None => Err(CompileError::Type {
                                msg: format!("no field `{field}` in `{name}`"),
                                span: *span,
                            }),
                        }
                    }
                    Some(DataInfo::Opaque) => Err(CompileError::Type {
                        msg: format!("opaque type `{name}` has no fields"),
                        span: *span,
                    }),
                    _ => Err(CompileError::Type {
                        msg: format!("`{name}` is not a record type"),
                        span: *span,
                    }),
                },
                ValueTy::App(name, args)
                    if ctx.env.ctor_arity(name.as_str()).is_some()
                        || matches!(
                            ctx.env.data_types.get(name.as_str()),
                            Some(DataInfo::Record(_))
                        ) =>
                {
                    // placeholder `Var(1)`/`Var(2)` positions, resolved against
                    // the concrete type args.
                    match ctx.env.data_types.get(name.as_str()) {
                        Some(DataInfo::Record(fields)) => {
                            match fields.iter().position(|(f, _, _)| f == field) {
                                Some(idx) => {
                                    let fty = match &fields[idx].1 {
                                        ValueTy::Var(k) => args
                                            .get(k.saturating_sub(1) as usize)
                                            .cloned()
                                            .unwrap_or(ValueTy::Double),
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
                    // Deferred record: the record type is not resolved here (a
                    // value function's λ-parameter). When the field uniquely
                    // identifies the record type (`unKleisli` → `Kleisli`, the
                    // `rec_expected` threaded above), the projection type IS
                    // known up to the record's fresh type args — return it so a
                    // let-bound projection types as a Func
                    // (`let u = k.unKleisli in u x`). A truly ambiguous/unknown
                    // record defers to a fresh var, resolved when the function
                    // is called with a concrete argument (`reduce` β-reduces).
                    let field_ty = match &rec_expected {
                        Some(ValueTy::Data(name, args)) | Some(ValueTy::App(name, args)) => {
                            match ctx.env.data_types.get(name.as_str()) {
                                Some(DataInfo::Record(fields)) => fields
                                    .iter()
                                    .find(|(f, _, _)| f == field)
                                    .map(|(_, ft, _)| {
                                        if args.is_empty() {
                                            ft.clone()
                                        } else {
                                            substitute_field_params(ft, args)
                                        }
                                    }),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    match field_ty {
                        Some(ft) => Ok(ArrowTy::value_channel(ft)),
                        None => {
                            let v = ctx.next;
                            ctx.next += 1;
                            Ok(ArrowTy::value_channel(ValueTy::Var(v)))
                        }
                    }
                }
                _ => Err(CompileError::Type {
                    msg: format!("field projection requires a record value (got {:?})", t.vty),
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
            if rt.arity_out() != 1 {
                return Err(CompileError::Type {
                    msg: "field update requires a data record".into(),
                    span: *span,
                });
            }
            match &rt.vty {
                ValueTy::Data(name, _) => {
                    if matches!(
                        ctx.env.data_types.get(name.as_str()),
                        Some(DataInfo::Opaque)
                    ) {
                        return Err(CompileError::Type {
                            msg: format!("opaque type `{name}` has no fields"),
                            span: *span,
                        });
                    }
                    let fty = ctx
                        .env
                        .data_types
                        .get(name.as_str())
                        .and_then(|info| match info {
                            DataInfo::Record(fields) => fields
                                .iter()
                                .find(|(f, _, _)| f == field)
                                .map(|(_, t, _)| t.clone()),
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
                                .position(|(f, _, _)| f == field)
                                .map(|idx| match &fields[idx].1 {
                                    ValueTy::Var(k) => args
                                        .get(k.saturating_sub(1) as usize)
                                        .cloned()
                                        .unwrap_or(ValueTy::Double),
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
            // Bare Int/Double literals are signal-rate channels in v1 but
            // value-compatible in value positions (mirroring `arm_result_vty`).
            // Derive the arm constructors' sum type BEFORE inferring the
            // scrutinee, so the scrutinee can be inferred with that sum as its
            // expected type. This pins a result-directed method chain's container
            // (`apply (compose (arr f) (arr g)) 3.0` matched by `Just v` → `m :=
            // Maybe`) before the chain's arguments infer, so the instance bodies'
            // `return`/`bind` resolve against the concrete monad.
            let pre_sum: Option<String> = if arms
                .iter()
                .any(|a| matches!(a.pattern.strip_bang(), Pattern::Ctor(_, _)))
            {
                let mut candidates: Option<Vec<String>> = None;
                let mut ok = true;
                for a in arms {
                    if let Pattern::Ctor(name, _) = a.pattern.strip_bang() {
                        let per_ctor = sum_types_with_ctor(ctx, name);
                        if per_ctor.is_empty() {
                            ok = false;
                            break;
                        }
                        candidates = Some(match candidates {
                            None => per_ctor,
                            Some(acc) => acc.into_iter().filter(|n| per_ctor.contains(n)).collect(),
                        });
                    }
                }
                match candidates {
                    Some(v) if ok && v.len() == 1 => Some(v[0].clone()),
                    _ => None,
                }
            } else {
                None
            };
            let pre_expected: Option<ValueTy> = pre_sum.as_ref().and_then(|n| {
                ctx.env.ctor_arity(n).map(|arity| {
                    let mut args: Vec<ValueTy> = Vec::new();
                    while args.len() < arity {
                        args.push(ctx.fresh_vty());
                    }
                    ValueTy::App(n.clone(), args)
                })
            });
            let st = match scrutinee.as_ref() {
                Expr::Int(_, _) => ArrowTy::value_channel(ValueTy::Int),
                Expr::Double(_, _) => ArrowTy::value_channel(ValueTy::Double),
                _ => infer_expr_expected(ctx, scrutinee, pre_expected)?,
            };
            // The scrutinee must be a single value channel (not a signal buffer).
            if st.arity_out() != 1 {
                return Err(CompileError::Type {
                    msg: "match scrutinee must be a value".into(),
                    span: *span,
                });
            }
            if matches!(st.vty, ValueTy::Buffer(_)) {
                return Err(CompileError::Type {
                    msg:
                        "match scrutinee cannot be a signal buffer; match requires a value channel"
                            .into(),
                    span: *span,
                });
            }
            let scrutinee_vty = st.vty.clone();
            // Sum or scalar? Resolve the scrutinee type enough to decide.
            // Builtin sums are parameterized (`Maybe a`, `Either a b`):
            // `Nothing` infers as `App("Maybe", [..])` and `head`/`lookup` as
            // `App("Maybe", [t])`, while user sums stay `Data(name, [])`. The
            // scrutinee's sum name is the `App` head (when it is a builtin Sum)
            // or the `Data` name.
            let resolved = ctx.subst.resolve_value(&scrutinee_vty);
            // An opaque value has no constructors or fields to inspect: it
            // cannot be pattern-matched (`data W; f w = case w of { … }`).
            if let ValueTy::Data(name, _) = &resolved {
                if matches!(
                    ctx.env.data_types.get(name.as_str()),
                    Some(DataInfo::Opaque)
                ) {
                    return Err(CompileError::Type {
                        msg: format!("opaque type `{name}` cannot be matched"),
                        span: *span,
                    });
                }
            }
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
                .any(|a| matches!(a.pattern.strip_bang(), Pattern::Ctor(_, _)))
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
                            if let Pattern::Ctor(name, _) = a.pattern.strip_bang() {
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
                Ok(ArrowTy::value_channel(result.unwrap_or(ValueTy::Double)))
            }
        }
        Expr::If {
            cond,
            then,
            els,
            span,
        } => {
            let ct = infer_expr(ctx, cond)?;
            if ct.arity_out() != 1 {
                return Err(CompileError::Type {
                    msg: "if condition must be a Bool value".into(),
                    span: cond.span(),
                });
            }
            if !matches!(ctx.subst.resolve_value(&ct.vty), ValueTy::Bool) {
                return Err(CompileError::Type {
                    msg: "if condition must be a Bool value".into(),
                    span: cond.span(),
                });
            }
            let tt = infer_expr_expected(ctx, then, expected.clone())?;
            let et = infer_expr_expected(ctx, els, expected)?;
            let tv = arm_result_vty(tt, then, then.span())?;
            let ev = arm_result_vty(et, els, els.span())?;
            unify_value(&tv, &ev, &mut ctx.subst, *span)?;
            Ok(ArrowTy::value_channel(tv))
        }
        Expr::Lambda { params, body, span } => {
            // Bind the parameters as value channels and infer the body; the
            // result type is the function type. v1 lambda parameters are
            // structural value types: the returned `Func` signature carries the
            // parameter variables (NOT hardcoded `Double`), so a call site can
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
            // method (`ap mf mx = bind mf (\ f -> bind mx (..))`) are not
            // misread as recursion.
            ctx.method_lifting.clear();
            let mut arg_tys: Vec<ValueTy> = Vec::with_capacity(params.len());
            // When the lambda's expected type is a `Func`, bind each parameter
            // to the corresponding ARGUMENT type (not a fresh Var). This lets a
            // caller thread a concrete element type through: `map (fn x -> x *
            // 0.5) (sine 440.0 0.5 0.0)` unifies `x : Buffer` BEFORE the body infers, so
            // the arithmetic sees a Buffer (not a Var that later conflicts).
            let expected_args = match &expected {
                Some(ValueTy::Thunk(es, _)) => Some(es.clone()),
                _ => None,
            };
            for (i, p) in params.iter().enumerate() {
                let pty = match &expected_args {
                    Some(es) => match es.get(i) {
                        Some(e) => e.clone(),
                        None => ctx.fresh_vty(),
                    },
                    _ => ctx.fresh_vty(),
                };
                arg_tys.push(pty.clone());
                ctx.locals
                    .insert(p.name.clone(), ArrowTy::value_channel(pty));
            }
            // The lambda's BODY is expected to produce the lambda's RESULT type, not
            // the whole `Func` type (which is what the caller expects).
            let body_expected = match expected {
                Some(ValueTy::Thunk(_, rets)) => rets.first().cloned(),
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
            let ret_ty = bt.vty.clone();
            Ok(ArrowTy::value_channel(ValueTy::Thunk(
                arg_tys,
                vec![ret_ty],
            )))
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
                .unwrap_or(ValueTy::Double);
            Ok(ArrowTy::value_channel(ValueTy::App(
                "List".into(),
                vec![elem_ty],
            )))
        }
        Expr::MapLit(entries, _) => {
            // Map literal with string keys: every entry's value must share ONE
            // type (mirroring the homogeneous List literal); that type is the
            // map's value type. A mixed-type literal is a compile error, and a
            // non-Double value type (a List, a record) is carried accurately.
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
                .unwrap_or(ValueTy::Double);
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

fn infer_ref(ctx: &mut Ctx, name: &str, span: Span) -> Result<ArrowTy, CompileError> {
    infer_ref_expected(ctx, name, span, None)
}

/// Resolve a `Ref` with an optional expected value type. A **result-directed**
/// typeclass method (`mempty`, `pure`, `return` — a method whose signature has
/// no class-var-applied argument) is resolved by the expected type's concrete
/// name; without an expected type it errors.
fn infer_ref_expected(
    ctx: &mut Ctx,
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
                Some(s) => sig_result_directed(s, &class_var),
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
                let sig = sig.as_ref().expect("result-directed sig");
                let env = ctx.env.clone();
                let cv_var = ctx.fresh_id();
                let Some((ty_name, cv_subst)) =
                    method_dispatch_key(&env, &class_name, &class_var, sig, &exp, cv_var, || {
                        ctx.fresh_vty()
                    })
                else {
                    return Err(CompileError::Type {
                        msg: format!(
                            "cannot resolve method `{name}` of `{class_name}`: the expected type is not concrete"
                        ),
                        span,
                    });
                };
                // Primitive instance: kernel-backed, no source body.
                record_selection(ctx, span, &class_name, &selection_evidence(&cv_subst));
                if ctx
                    .env
                    .instances
                    .get(class_name.as_str())
                    .and_then(|m| m.get(ty_name.as_str()))
                    .map(|i| i.primitive)
                    .unwrap_or(false)
                {
                    // A bare reference: there are no call-site arguments to
                    // unify against, so the method keeps its signature (the
                    // class variable is already solved by `cv_subst`).
                    let param_tys = signature_param_tys(ctx, &class_var, &cv_subst, sig);
                    let mut fresh_ids: Vec<ValueTy> = Vec::new();
                    for _ in 0..8 {
                        fresh_ids.push(ctx.fresh_vty());
                    }
                    let mut it = fresh_ids.into_iter();
                    let (_, ret) = signature_param_and_result_tys_conv(
                        &ctx.env,
                        &class_var,
                        &cv_subst,
                        sig,
                        || it.next().unwrap_or(ValueTy::Double),
                    );
                    let ret = ret.unwrap_or_else(|| ctx.fresh_vty());
                    return Ok(ArrowTy::value(nest_thunk(&param_tys, vec![ret])));
                }
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
                for (p, pt) in params
                    .iter()
                    .zip(signature_param_tys(ctx, &class_var, &cv_subst, sig))
                {
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
            // A NON-result-directed method used as a first-class VALUE
            // (`fold mappend "" xs`) — Haskell 2010 §4.3.3: a class method is
            // an ordinary variable. The instance is selected by the type the
            // surrounding context demands, so dispatch on the concrete types
            // the EXPECTED function signature carries. Without this the bare
            // reference was rejected outright, and `fold mappend` could never
            // work.
            //
            // Candidates come from the expected type's flattened parameter and
            // result types in order; the first one that names a registered
            // instance wins. `mappend :: m -> m -> m` under
            // `Thunk([String], [Thunk([String], [String])])` therefore selects
            // `Monoid String`.
            if let (Some(sig), Some(exp)) = (sig.as_ref(), expected.as_ref()) {
                if let Some((ty_name, cand_ty)) = method_value_dispatch_env(&ctx.env, exp) {
                    // Primitive instance used as a value: kernel-backed.
                    if ctx
                        .env
                        .instances
                        .get(class_name.as_str())
                        .and_then(|m| m.get(ty_name.as_str()))
                        .map(|i| i.primitive)
                        .unwrap_or(false)
                    {
                        let cv_subst0 = ClassVarSubst::Ty(cand_ty.clone());
                        let mut fresh_ids: Vec<ValueTy> = Vec::new();
                        for _ in 0..8 {
                            fresh_ids.push(ctx.fresh_vty());
                        }
                        let mut it = fresh_ids.into_iter();
                        let (param_tys, ret) = signature_param_and_result_tys_conv(
                            &ctx.env,
                            &class_var,
                            &cv_subst0,
                            sig,
                            || it.next().unwrap_or(ValueTy::Double),
                        );
                        let ret = ret.unwrap_or_else(|| ctx.fresh_vty());
                        return Ok(ArrowTy::value(nest_thunk(&param_tys, vec![ret])));
                    }
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
                            msg: format!(
                                "recursive typeclass method `{name}` for type `{ty_name}`"
                            ),
                            span,
                        });
                    }
                    ctx.method_lifting.insert(key.clone());
                    let cv_subst = ClassVarSubst::Ty(cand_ty);
                    let saved = ctx.locals.clone();
                    for (p, pt) in params
                        .iter()
                        .zip(signature_param_tys(ctx, &class_var, &cv_subst, sig))
                    {
                        ctx.locals.insert(p.clone(), ArrowTy::value_channel(pt));
                    }
                    let res = infer_expr(ctx, &body);
                    ctx.locals = saved;
                    ctx.method_lifting.remove(&key);
                    res?;
                    // The method's CONTRACT is its class signature with the
                    // class variable substituted by the dispatched instance —
                    // not the instance body's incidental type, which would make
                    // `fold mappend …` compare against the implementation's own
                    // parameter names.
                    let env = ctx.env.clone();
                    let (arg_tys, ret_ty) = signature_param_and_result_tys_conv(
                        &env,
                        &class_var,
                        &cv_subst,
                        sig,
                        || ctx.fresh_vty(),
                    );
                    let contract = match ret_ty {
                        Some(ret) => nest_thunk(&arg_tys, vec![ret]),
                        None => arg_tys.first().cloned().unwrap_or(ValueTy::Double),
                    };
                    return Ok(ArrowTy::value_channel(contract));
                }
            }
        }
    }
    // A reference to a compile-time NUMERIC constant (`c = 440.0`) in a
    // Buffer-expected position is a SIGNAL constant: a foreign signal slot
    // `c : Buffer`. Otherwise the constant stays a Double value.
    if matches!(expected, Some(ValueTy::Buffer(_)))
        && is_const_value_ref(ctx, &Expr::Ref(name.into(), span))
    {
        return Ok(ArrowTy::uniform(0, 1, ctx.env.buffer_vty()));
    }
    let t = infer_ref_inner(ctx, name, span)?;
    // Thread the expected type into a bare reference whose value type is
    // still an unresolved variable: a local (`k` in `k.unKleisli`) or a
    // definition used through a parameterized function has no type of its
    // own yet, so the surrounding expected type pins it. This is what lets a
    // result-directed/constructor-class method select its instance when its
    // container arrives through a function parameter (`apply (arr f) 3.0`
    // with `apply k x = k.unKleisli x`: `k : Kleisli m a b`, so `arr`
    // resolves over the concrete `Kleisli` head instead of an opaque var).
    //
    // The guard looks at the RAW local type, not the resolved one: the
    // multi-pass inference re-pins a lambda parameter on every pass (pass 2's
    // `k` is already bound to a concrete `Data` in the substitution from pass
    // 1), so skipping when `resolve_value` is concrete would let a let-bound
    // projection (`let u = k.unKleisli in u x`) re-freshen its type vars and
    // sever the parameter↔result link. `unify_value` resolves both sides, so
    // a re-pin is an arg-wise unify, not a clobber.
    if let Some(exp) = expected {
        if t.arity_in() == 0 && t.arity_out() == 1 && matches!(&t.vty, ValueTy::Var(_)) {
            unify_value(&t.vty, &exp, &mut ctx.subst, span)?;
        }
    }
    Ok(t)
}

/// The original `infer_ref` body: resolve a named reference (data type, sum
/// constructor, newtype, nullary constructor, method, builtin, def, local).
fn infer_ref_inner(ctx: &mut Ctx, name: &str, span: Span) -> Result<ArrowTy, CompileError> {
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
            DataInfo::Opaque => {
                return Err(CompileError::Type {
                    msg: format!("opaque type `{name}` cannot be used as a value"),
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
                vec![ValueTy::String, ValueTy::Double],
            )))
        }
        "empty_set" => {
            return Ok(ArrowTy::value_channel(ValueTy::App(
                "Set".into(),
                vec![ValueTy::Double],
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
            // A PRIMITIVE method is an ordinary curried value
            // (`(+) :: Num a => a -> a -> a`): a bare `(+)` or a partial
            // `(+) 1.0` is legal Haskell 2010 (§4.3.3). Only a method whose
            // instance needs a body to select still requires an argument.
            let sig = ctx.env.typeclasses.get(&class_name).and_then(|c| {
                c.methods
                    .iter()
                    .find(|(m, _)| m == name)
                    .map(|(_, s)| s.clone())
            });
            if let Some(sig) = sig {
                let class_var = ctx
                    .env
                    .typeclasses
                    .get(&class_name)
                    .map(|c| c.var.clone())
                    .unwrap_or_default();
                let mut fresh_ids: Vec<ValueTy> = Vec::new();
                for _ in 0..8 {
                    fresh_ids.push(ctx.fresh_vty());
                }
                let mut it = fresh_ids.into_iter();
                let (param_tys, ret) = signature_param_and_result_tys_conv(
                    &ctx.env,
                    &class_var,
                    &ClassVarSubst::Var(u32::MAX),
                    &sig,
                    || it.next().unwrap_or(ValueTy::Double),
                );
                let ret = ret.unwrap_or_else(|| ctx.fresh_vty());
                return Ok(ArrowTy::value(nest_thunk(&param_tys, vec![ret])));
            }
            return Err(CompileError::Type {
                msg: format!("method `{name}` of `{class_name}` requires an argument"),
                span,
            });
        }
    }
    if matches!(name, "+" | "-" | "*" | "/" | "%") {
        // Haskell 2010 Prelude: an arithmetic operator is an ordinary CURRIED
        // function value (`(+) :: a -> a -> a`), not a bare scalar. Typing it
        // as a 2-argument thunk is what lets `(+) 1.0 2.0`, `fold (+) 0 xs`
        // and `inc = (+) 1.0` work — the same shape the binary math builtins
        // (`min`/`max`) already use.
        let s = ctx.fresh_vty();
        let ty = nest_thunk(&[s.clone(), s.clone()], vec![s]);
        return Ok(ArrowTy::value(ty));
    }
    if matches!(
        name,
        "sin" | "cos" | "tan" | "sqrt" | "exp" | "ln" | "tanh" | "abs"
    ) {
        // A math builtin is an ordinary unary VALUE function, not a signal
        // arrow: `abs b : Buffer` over a buffer, `abs 1.0 : Double` over a
        // scalar. The parameter and result are the same type.
        let s = ctx.fresh_vty();
        return Ok(ArrowTy::value(ValueTy::Thunk(vec![s.clone()], vec![s])));
    }
    if matches!(name, "min" | "max" | "atan2") {
        // Binary math builtin: a curried value function of two arguments.
        let s = ctx.fresh_vty();
        let ty = nest_thunk(&[s.clone(), s.clone()], vec![s]);
        return Ok(ArrowTy::value(ty));
    }
    // Foreign (FFI) declaration: type through the language-side signature. A bare
    // ref is valid when every param is a signal channel (no scalar params yet).
    if let Some(fsig) = ctx.env.foreign_sigs.get(name).cloned() {
        if let Some(sig) = crate::types::ffi::ffi_sig_from_typeexpr(&ctx.env, name, &fsig) {
            if sig
                .params
                .iter()
                .all(|p| matches!(p, crate::types::ffi::FfiParam::Signal(_)))
                && sig.signal_outs > 0
            {
                // A bare FFI ref is a Buffer ARROW (`Buffer -> … -> Buffer`),
                // not a signal arrow: signals are Buffer values, so a bare
                // `integrator` is a first-class function value (HOF: `map
                // (fn x -> integrator x) (sine 440.0 0.5 0.0)`). Mirrors the partial
                // application typing in the FFI apply arm.
                let n_sig = sig.params.len();
                return Ok(ArrowTy::value_channel(ValueTy::Thunk(
                    vec![ctx.env.buffer_vty(); n_sig],
                    vec![ctx.env.buffer_vty(); sig.signal_outs],
                )));
            }
        }
    }
    // A NULLARY builtin signature (`stdout : () -> OutputStream String`):
    // a bare reference is a zero-argument call, returning the signature's
    // result. (`write` takes arguments, so it only appears as Apply.)
    if let Some(crate::ast::TypeExpr::TFunc(params, ret)) = ctx.env.builtin_sigs.get(name).cloned()
    {
        if params.is_empty() {
            let v = type_expr_to_vty(ctx, &ret)?;
            return Ok(ArrowTy::value_channel(v));
        }
    }
    if let Some(t) = ctx.locals.get(name) {
        return Ok(t.clone());
    }
    if let Some(scheme) = ctx.defs.get(name).cloned() {
        if scheme.lam_count > 0 {
            // A bare reference to a user definition with λ-parameters is a
            // first-class function value: `f = double` binds `f` to
            // `ValueTy::Thunk([], [])` (an unknown signature — a bare named
            // ref's arity is resolved when it is applied, see `infer_apply`).
            // Using it where a signal is required is rejected by the signal
            // combinators.
            return Ok(ArrowTy::value_channel(ValueTy::Thunk(vec![], vec![])));
        }
        return Ok(ctx.instantiate(&scheme));
    }
    // A type-scope binding: a constraint-instance head arg (`instance (Monad m)
    // => … → m := Maybe`). Referencing it as a VALUE resolves to the type.
    if let Some(vty) = ctx.type_var_bindings.get(name) {
        return Ok(ArrowTy::value_channel(vty.clone()));
    }
    Err(CompileError::Type {
        msg: format!("unknown identifier `{name}`"),
        span,
    })
}

/// Resolve the definition a func value dispatches to.
///
/// A func value binding (`f = double`) is a bare `Def::Eval` whose body is a bare
/// `Ref` to another definition; chains (`g = f`) follow the body refs until a
/// definition that is itself not a func value is reached (the referenced
/// definition, with λ-parameters). The structural `ValueTy::Thunk` no longer
/// carries the referenced name, so it is recovered from the AST bodies here.
/// (A later task replaces this with signature-based dispatch.)
fn func_target(ctx: &Ctx, name: &str) -> String {
    let mut cur = name.to_string();
    for _ in 0..=ctx.defs.len() {
        let is_func_value = ctx
            .defs
            .get(cur.as_str())
            .is_some_and(|s| matches!(s.ty.vty, ValueTy::Thunk(_, _)));
        match ctx.def_bodies.get(cur.as_str()) {
            // Follow the alias ONLY while the referenced name is itself a
            // definition. A body that names a BUILTIN (`plus = (+)`) has no
            // scheme to instantiate: stopping here types the call from the
            // alias's own scheme, which is the builtin operator's curried
            // signature.
            Some(Expr::Ref(next, _)) if is_func_value && ctx.defs.contains_key(next.as_str()) => {
                cur = next.clone();
                continue;
            }
            _ => return cur,
        }
    }
    cur
}

/// Convert a builtin-signature type expression to a concrete `ValueTy`.
/// `TName` resolves through the env; `TApp` builds `App(head, args)`; a `TFunc`
/// becomes `ValueTy::Thunk`. Used by the builtin-sig
/// application (their types are ordinary, unlike `runST`'s forall).
fn type_expr_to_vty(ctx: &Ctx, te: &crate::ast::TypeExpr) -> Result<ValueTy, CompileError> {
    // The SECOND conversion path (with [`sig_ty_to_vty`]) — and one the task's
    // "two converters, one rule" warning names explicitly. Guarded with the same
    // single rule, so neither can accept a `* -> *` in a `*` position.
    ctx.env.check_type_kind(te, Span::new(0, 0))?;
    type_expr_to_vty_inner(ctx, te)
}

/// [`type_expr_to_vty`] without the kind guard — the recursion body.
fn type_expr_to_vty_inner(ctx: &Ctx, te: &crate::ast::TypeExpr) -> Result<ValueTy, CompileError> {
    match te {
        crate::ast::TypeExpr::TName(n) => {
            // A phantom region token (`s` inside `runST (fn s -> …)`) resolves
            // through the caller's locals FIRST — `ST s (List Buffer)` in a
            // builtin signature refers to the enclosing region's `s`, not a
            // global type. Falls back to the env for ordinary type names.
            if let Some(t) = ctx.locals.get(n) {
                if matches!(t.vty, ValueTy::TyConVar(_)) {
                    return Ok(t.vty.clone());
                }
            }
            // A NAME that is not a declared type is a TYPE VARIABLE, not a
            // nominal `Data` type: `(+) : a -> a -> a` mentions `a` as the
            // class variable, and turning it into `Data("a")` makes it
            // un-unifiable with every concrete type. A name the registry does
            // not know is an unknown type (§3.1) — the builtin catalog is
            // compiler-owned and correct, so a miss here is a compiler bug.
            ctx.env
                .vty_of_name(n)
                .ok_or_else(|| crate::types::ty::unknown_type_name(n))
        }
        crate::ast::TypeExpr::TApp(head, args) => {
            let mut targs = Vec::with_capacity(args.len());
            for a in args {
                targs.push(type_expr_to_vty_inner(ctx, a)?);
            }
            // A user data type (`IO a`, `Kleisli m a b`) is a `Data` application
            // (mirroring how record construction types it), so `IO` unifies
            // consistently between builtin signatures and do-blocks. A buffer
            // CHANNEL constructor becomes the buffer value type, its element
            // checked like anywhere else.
            Ok(ctx.env.ctor_application(head, targs))
        }
        crate::ast::TypeExpr::TFunc(args, ret) => {
            let mut targs = Vec::with_capacity(args.len());
            for a in args {
                targs.push(type_expr_to_vty_inner(ctx, a)?);
            }
            Ok(ValueTy::Thunk(
                targs,
                vec![type_expr_to_vty_inner(ctx, ret)?],
            ))
        }
        crate::ast::TypeExpr::TForall(_, _) => Err(CompileError::Type {
            msg: "a forall-typed signature cannot be applied as a plain builtin".into(),
            span: Span::new(0, 0),
        }),
        // The class context is not enforced yet (constrained-def dispatch is a
        // later task); the type itself is the body's type.
        crate::ast::TypeExpr::TConstrained(_, inner) => type_expr_to_vty_inner(ctx, inner),
    }
}

/// Whether a name in a builtin signature is a type VARIABLE rather than a
/// declared type: a lowercase placeholder (`a`, `b`, `s`) that no declaration
/// resolves. The registry is the single answer — this is the type-variable arm
/// of [`TypeEnv::type_entry`], not a private name list.
fn is_builtin_sig_var(ctx: &Ctx, name: &str) -> bool {
    ctx.env.is_type_variable(name)
}

/// Collect every `forall`-bound name in a type expression, nested ones
/// included, in source order (duplicates are harmless — the binder map is
/// keyed by name).
fn collect_forall_binders(te: &crate::ast::TypeExpr, out: &mut Vec<String>) {
    use crate::ast::TypeExpr;
    match te {
        TypeExpr::TForall(bound, inner) => {
            out.extend(bound.iter().cloned());
            collect_forall_binders(inner, out);
        }
        TypeExpr::TApp(_, args) => {
            for a in args {
                collect_forall_binders(a, out);
            }
        }
        TypeExpr::TFunc(args, ret) => {
            for a in args {
                collect_forall_binders(a, out);
            }
            collect_forall_binders(ret, out);
        }
        TypeExpr::TName(_) => {}
        TypeExpr::TConstrained(_, inner) => collect_forall_binders(inner, out),
    }
}

/// Pre-bind every `forall` binder of a builtin signature into `vars`.
///
/// A `TForall` quantifies over the WHOLE signature, not just the sub-expression
/// it is written on, so the binder must exist before the parameter types are
/// instantiated. Without this hoist, a signature whose binders appear in result
/// position (`readSTRef :: STRef s a -> forall s. ST s a`) binds the result's
/// `s` to a var unrelated to the argument's: the region phantom is then severed
/// and a cell from one region can be written inside another region.
fn hoist_forall_binders(
    ctx: &mut Ctx,
    params: &[crate::ast::TypeExpr],
    ret: &crate::ast::TypeExpr,
    vars: &mut HashMap<String, ValueTy>,
) {
    let mut binders: Vec<String> = Vec::new();
    for p in params {
        collect_forall_binders(p, &mut binders);
    }
    collect_forall_binders(ret, &mut binders);
    for name in binders {
        vars.entry(name).or_insert_with(|| ctx.fresh_vty());
    }
}

/// Convert a builtin signature to a `ValueTy`, INSTANTIATING its type variables.
///
/// A `TForall` binder is stripped and each bound name — like each remaining
/// free type-variable name (`a`, `b`, …) — is replaced by one fresh inference
/// variable, consistently within the signature, so `a -> ST s (STRef s a)`
/// keeps `s`/`a` linked. The call site then unifies the instantiation with the
/// argument and result types; inside `runST (\s -> …)` the instantiated region
/// variable unifies with the enclosing region's `TyConVar`, which is exactly
/// what makes an escaping `STRef s a` a compile-time error.
///
/// Binders are hoisted over the whole call by [`hoist_forall_binders`] before
/// this runs, so a `TForall` in result position still shares its variable with
/// the parameters.
///
/// A name that resolves through the caller's locals to a region phantom is
/// left as that phantom (mirroring [`type_expr_to_vty`]); declared type names
/// and primitives resolve through the environment as usual.
fn instantiate_builtin_sig_vty(
    ctx: &mut Ctx,
    te: &crate::ast::TypeExpr,
    vars: &mut HashMap<String, ValueTy>,
) -> Result<ValueTy, CompileError> {
    // The THIRD path: `builtin_sigs` are assembled in Rust, so they never pass
    // the program-level walk. Guarded with the same single rule.
    ctx.env.check_type_kind(te, Span::new(0, 0))?;
    instantiate_builtin_sig_vty_inner(ctx, te, vars)
}

/// [`instantiate_builtin_sig_vty`] without the kind guard — the recursion body.
fn instantiate_builtin_sig_vty_inner(
    ctx: &mut Ctx,
    te: &crate::ast::TypeExpr,
    vars: &mut HashMap<String, ValueTy>,
) -> Result<ValueTy, CompileError> {
    use crate::ast::TypeExpr;
    match te {
        TypeExpr::TName(n) => {
            if let Some(t) = vars.get(n) {
                return Ok(t.clone());
            }
            if let Some(t) = ctx.locals.get(n) {
                if matches!(t.vty, ValueTy::TyConVar(_)) {
                    return Ok(t.vty.clone());
                }
            }
            if is_builtin_sig_var(ctx, n) {
                let v = ctx.fresh_vty();
                vars.insert(n.clone(), v.clone());
                return Ok(v);
            }
            ctx.env
                .vty_of_name(n)
                .ok_or_else(|| crate::types::ty::unknown_type_name(n))
        }
        TypeExpr::TApp(head, args) => {
            let mut targs = Vec::with_capacity(args.len());
            for a in args {
                targs.push(instantiate_builtin_sig_vty_inner(ctx, a, vars)?);
            }
            if ctx.env.data_arities.contains_key(head.as_str()) {
                Ok(ValueTy::Data(head.clone(), targs))
            } else {
                Ok(ValueTy::App(head.clone(), targs))
            }
        }
        TypeExpr::TFunc(args, ret) => {
            let mut targs = Vec::with_capacity(args.len());
            for a in args {
                targs.push(instantiate_builtin_sig_vty_inner(ctx, a, vars)?);
            }
            Ok(ValueTy::Thunk(
                targs,
                vec![instantiate_builtin_sig_vty_inner(ctx, ret, vars)?],
            ))
        }
        TypeExpr::TForall(bound, inner) => {
            // REUSE the hoisted binder (see `hoist_forall_binders`): rebinding
            // here would give the quantified variable a second, unrelated
            // inference variable and silently sever it from the occurrences
            // instantiated BEFORE this point — the region phantom would then
            // not link `STRef s a ->` to `ST s a`.
            for n in bound {
                vars.entry(n.clone()).or_insert_with(|| ctx.fresh_vty());
            }
            instantiate_builtin_sig_vty_inner(ctx, inner, vars)
        }
        // The class context is not enforced yet; the value type is the body's.
        TypeExpr::TConstrained(_, inner) => instantiate_builtin_sig_vty_inner(ctx, inner, vars),
    }
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
        crate::ast::TypeExpr::TForall(_, inner) => type_expr_mentions(inner, var),
        crate::ast::TypeExpr::TConstrained(_, inner) => type_expr_mentions(inner, var),
    }
}

/// Whether a method signature is result-directed: a `TFunc` whose arguments
/// never mention the class variable (`pure a`, `return x`) resolves by the
/// expected result type; a bare (non-function) signature (`mempty: m`) is also
/// result-directed. A `TForall` binder is stripped and the same test re-runs
/// on the inner type. NOTE: the binder-shadowing corner (`forall a. a -> b` in
/// `typeclass C a`) is deferred to R2 (free-vars) — the binder is stripped
/// without alpha-renaming, so a quantified variable that shadows the class var
/// is mis-read as the class var.
pub(crate) fn sig_result_directed(sig: &crate::ast::TypeExpr, class_var: &str) -> bool {
    match sig {
        crate::ast::TypeExpr::TForall(_, inner) => sig_result_directed(inner, class_var),
        crate::ast::TypeExpr::TConstrained(_, inner) => sig_result_directed(inner, class_var),
        crate::ast::TypeExpr::TFunc(args, _) => {
            args.iter().all(|a| !type_expr_mentions(a, class_var))
        }
        _ => true,
    }
}

/// Infer an `Apply`, threaded with an optional expected value type for
/// result-directed typeclass dispatch (`pure`, `return`, `mempty`).
fn infer_apply_expected(
    ctx: &mut Ctx,
    name: &str,
    args: &[Expr],
    span: Span,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    infer_apply_impl(ctx, name, args, span, expected)
}

/// Flatten a curried application chain rooted at a bare `Ref` name into
/// `(name, [args…])`. `f a b` parses as
/// `ApplyExpr(ApplyExpr(Ref f, [a]), [b])`; this returns `("f", [a, b])`.
/// Returns `None` when the chain root is not a bare reference, or a node in the
/// chain applies other than exactly one argument.
fn uncurry_name_chain(e: &Expr) -> Option<(String, Vec<&Expr>)> {
    match e {
        Expr::Ref(name, _) => Some((name.clone(), Vec::new())),
        Expr::ApplyExpr { callee, args, .. } if args.len() == 1 => {
            let (name, mut chain) = uncurry_name_chain(callee)?;
            chain.push(&args[0]);
            Some((name, chain))
        }
        _ => None,
    }
}

/// `(expr) a b` — apply a closure-valued expression. Infer the callee, unify it
/// against a fresh `Func` signature, infer and check args, return the result.
/// `expected` is the expression's expected result type; it is forwarded to a
/// name-based chain (a result-directed method chain keeps its expected type) and
/// threaded into the callee as a `Func` signature for the generic runtime path.
fn infer_apply_expr(
    ctx: &mut Ctx,
    callee: &Expr,
    args: &[Expr],
    span: Span,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    // Un-curry a chain rooted at a bare name (`ApplyExpr(ApplyExpr(Ref f, [a]), [b])`
    // from the parser's currying pass) back to the name-based form so the
    // name-dispatched arms (FFI builtins, collection ops, typeclass methods,
    // definitions) see the full argument list. A callee that is not a bare
    // reference (a lambda, a field projection, a nested application of another
    // expression) keeps the generic runtime-application path below.
    if let Some((name, chain_args)) = uncurry_name_chain(callee) {
        let mut all: Vec<&Expr> = chain_args;
        all.extend(args.iter());
        let owned: Vec<Expr> = all.into_iter().cloned().collect();
        return infer_apply_impl(ctx, &name, &owned, span, expected);
    }
    // Thread the call's expected result type into the callee as a `Func`
    // signature (`Thunk([fresh; arity], [expected])`) so a result-directed
    // method chain in callee position (`(compose …).unKleisli` in a
    // `Just`-matched scrutinee) resolves its instance from the concrete result
    // type before the callee is inferred — mirroring lowering.
    let callee_expected: Option<ValueTy> = expected.map(|exp| {
        let fresh_args: Vec<ValueTy> = (0..args.len()).map(|_| ctx.fresh_vty()).collect();
        ValueTy::Thunk(fresh_args, vec![exp])
    });
    let ct = match &callee_expected {
        Some(ce) => infer_expr_expected(ctx, callee, Some(ce.clone()))?,
        None => infer_expr(ctx, callee)?,
    };
    if ct.arity_out() != 1 {
        return Err(CompileError::Type {
            msg: "applied expression must be a value (function)".into(),
            span,
        });
    }
    let cty = ct.vty.clone();
    let (arg_tys, ret_tys) = match ctx.subst.resolve_value(&cty) {
        // A function value is a curried NEST of single-argument thunks
        // (`\a -> \b -> …`), so its total arity comes from flattening.
        ValueTy::Thunk(..) => flatten_thunk(&ctx.subst.resolve_value(&cty)),
        ValueTy::Var(_) => {
            let a = (0..args.len()).map(|_| ctx.fresh_vty()).collect::<Vec<_>>();
            let r = vec![ctx.fresh_vty()];
            unify_value(
                &cty,
                &ValueTy::Thunk(a.clone(), r.clone()),
                &mut ctx.subst,
                span,
            )?;
            (a, r)
        }
        _ => {
            return Err(CompileError::Type {
                msg: "applied expression must be a function value".into(),
                span,
            })
        }
    };
    if args.len() > arg_tys.len() {
        return Err(CompileError::Type {
            msg: format!(
                "expression expects {} argument(s), got {}",
                arg_tys.len(),
                args.len()
            ),
            span,
        });
    }
    for (a, pt) in args.iter().zip(arg_tys.iter()) {
        // A `Func` parameter typed `Buffer` accepts a signal computation
        // (`fp i1` where `fp : Buffer -> Buffer` and `i1` is the program's
        // Buffer input) as well as a Buffer value — mirroring the builtin-sig
        // Buffer-param acceptance in `infer_apply_impl`.
        if matches!(ctx.subst.resolve_value(pt), ValueTy::Buffer(_)) {
            let at = infer_expr(ctx, a)?;
            if at.arity_out() == 1 && matches!(at.vty, ValueTy::Buffer(_)) {
                unify_value(&at.vty, pt, &mut ctx.subst, a.span())?;
                continue;
            }
        }
        // Thread the callee's parameter type as the argument's expected type
        // (mirroring the user-def path): a result-directed/constructor-class
        // method inside the argument selects its instance by the container the
        // parameter demands.
        let vt = infer_method_value_vty_expected(ctx, a, "argument", Some(pt.clone()))?;
        unify_value(&vt, pt, &mut ctx.subst, a.span())?;
    }
    if args.len() < arg_tys.len() {
        // Partial application of a closure-valued expression: the remaining λs
        // stay as the value arguments of the leftover closure (kept nested, so
        // a further application unifies against the real lambda type).
        let remaining = &arg_tys[args.len()..];
        return Ok(ArrowTy::value_channel(nest_thunk(
            remaining,
            ret_tys.clone(),
        )));
    }
    Ok(ArrowTy::value_channel(
        ret_tys.first().cloned().unwrap_or(ValueTy::Double),
    ))
}

/// Unify an application's RESULT type with the caller's expected type before the
/// arguments are inferred. A result-directed method chain whose container is
/// fixed only by its result (`apply (compose (arr f) (arr g)) 3.0` matched by
/// `Just v` → `m := Maybe`) then binds its monad ahead of the argument
/// inference, so the inlined instance bodies' `return`/`bind` resolve by
/// instance. A non-concrete (`Var`) or function-shaped (`Thunk`) expectation is
/// left to the normal call-site unification.
fn pin_result_to_expected(
    ctx: &mut Ctx,
    ret: &ValueTy,
    expected: Option<&ValueTy>,
    span: Span,
) -> Result<(), CompileError> {
    if let Some(exp) = expected {
        let exp = ctx.subst.resolve_value(exp);
        if matches!(
            exp,
            ValueTy::Data(..) | ValueTy::App(..) | ValueTy::Newtype(..)
        ) {
            let ret = ctx.subst.resolve_value(ret);
            unify_value(&ret, &exp, &mut ctx.subst, span)?;
        }
    }
    Ok(())
}

/// Type a call to a class-constrained definition WITHOUT inlining its body.
///
/// The signature's parameter patterns are unified against the call's arguments,
/// which solves the class variables (`Pair i i` vs `Pair Int Int` → `i := Int`).
/// Each constraint is then discharged against the solved type: a concrete type
/// with no instance is a compile error, an unresolved variable defers. The
/// call's result is the signature's result type, so a caller learns the
/// concrete `Array i e`-style result the signature promises.
fn infer_constrained_apply(
    ctx: &mut Ctx,
    name: &str,
    cd: &crate::types::ty::ConstrainedDef,
    args: &[Expr],
    span: Span,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    let (param_tys, ret_ty, vars) = def_sig_tys(ctx, &cd.sig)?;
    if param_tys.len() != cd.params.len() {
        return Err(CompileError::Type {
            msg: format!(
                "signature of `{name}` declares {} argument(s), but the definition binds {}",
                param_tys.len(),
                cd.params.len()
            ),
            span,
        });
    }
    if args.len() > param_tys.len() {
        return Err(CompileError::Type {
            msg: format!(
                "`{name}` expects {} argument(s), got {}",
                param_tys.len(),
                args.len()
            ),
            span,
        });
    }
    // Pin the result to the caller's expected type BEFORE the arguments infer,
    // so a result-directed method inside an argument resolves by the container
    // this call must produce (mirrors the plain function path).
    pin_result_to_expected(ctx, &ret_ty, expected.as_ref(), span)?;
    for (i, (a, pt)) in args.iter().zip(param_tys.iter()).enumerate() {
        let vt = infer_method_value_vty_expected(ctx, a, "argument", Some(pt.clone()))?;
        // PARK M9: a bare named function reference (`f` in `ixmap b f a`) infers
        // as the EMPTY `Thunk([], [])` — a bare ref's arity resolves only when it
        // is applied — so a function-shaped constrained parameter (`i -> j`)
        // reports an opaque `Thunk … with Thunk …` mismatch. Name the parameter
        // and point at the missing annotation (or a lambda literal) instead of
        // dumping thunk shapes.
        let resolved_vt = ctx.subst.resolve_value(&vt);
        if matches!(&resolved_vt, ValueTy::Thunk(c, r) if c.is_empty() && r.is_empty())
            && matches!(pt, ValueTy::Thunk(..))
        {
            let pname = cd
                .params
                .get(i)
                .map(|p| p.name.as_str())
                .unwrap_or("<unnamed>");
            return Err(CompileError::Type {
                msg: format!(
                    "`{name}` expects a function value for parameter `{pname}`; a bare \
                     named function reference types as an unresolved thunk here — \
                     annotate the parameter's type (or wrap the function in a lambda \
                     literal)"
                ),
                span: a.span(),
            });
        }
        unify_value(&vt, pt, &mut ctx.subst, a.span())?;
    }
    // The class variables as the call site solved them.
    let bindings: HashMap<String, ValueTy> = vars
        .iter()
        .map(|(k, v)| (k.clone(), ctx.subst.resolve_value(v)))
        .collect();
    discharge_def_constraints(&ctx.env, name, &cd.sig, &bindings, span)?;
    let ret = ctx.subst.resolve_value(&ret_ty);
    if args.len() < param_tys.len() {
        // Partial application: the remaining parameters stay a function value.
        let remaining: Vec<ValueTy> = param_tys[args.len()..]
            .iter()
            .map(|t| ctx.subst.resolve_value(t))
            .collect();
        return Ok(ArrowTy::value_channel(nest_thunk(&remaining, vec![ret])));
    }
    Ok(ArrowTy::value_channel(ret))
}

/// Whether a call `name args…` must resolve to a DEFINITION rather than to a
/// class method of the same name.
///
/// A plain user definition shadows a class method outright (a user `eq`/`lt` is
/// a plain function). A class-CONSTRAINED definition that shares its name with
/// a method — `index` is both the `Ix` offset method and the array accessor `!`
/// — is a DUAL name: the FIRST argument's static type decides. An argument that
/// solves the method's class variable (`Pair Int Int` for `Ix`) routes to the
/// method, anything else (`Array i e`) to the definition. Lowering applies the
/// same predicate to the same recovered type, so the passes cannot disagree.
fn method_shadowed_by_def(ctx: &mut Ctx, name: &str, args: &[Expr]) -> bool {
    // Inference only knows about locals; fragment captures are a lowering-only
    // concept, so lowering checks them separately.
    if ctx.locals.contains_key(name) {
        return true;
    }
    let constrained = ctx.env.constrained_defs.contains_key(name);
    if !constrained && !ctx.defs.contains_key(name) {
        return false;
    }
    if constrained && ctx.env.class_of_method(name).is_some() {
        if let Some(first) = args.first() {
            return !probe_first_arg_solves_class_var(ctx, name, first);
        }
    }
    // Untypeable or absent first arg: fall back to the definition (matches
    // lowering's routing).
    true
}

/// Recover the first argument's static value type for the dual-name routing,
/// then DISCARD the speculative inference: the substitution and the fresh
/// variable supply are rolled back, so the real argument inference starts from
/// the same state and cannot inherit a bogus binding from the probe.
///
/// Inside a constrained body the given constraint decides FIRST: if the body's
/// own `Ix i` witness satisfies the method's class-var pattern, the call is the
/// METHOD. That is what keeps `checkedOffset :: Ix i => …`'s `index b ix`
/// (offset of an abstract index) distinct from the array accessor `index a ix`
/// in the same prelude body.
fn probe_first_arg_solves_class_var(ctx: &mut Ctx, name: &str, first: &Expr) -> bool {
    let saved_subst = ctx.subst.clone();
    let saved_next = ctx.next;
    let probed = infer_expr(ctx, first)
        .ok()
        .map(|t| ctx.subst.resolve_value(&t.vty));
    ctx.subst = saved_subst;
    ctx.next = saved_next;
    match probed {
        Some(ty) => {
            given_routes_to_method(ctx, name, &ty)
                || first_arg_solves_class_var(&ctx.env, name, &ty)
        }
        None => false,
    }
}

fn infer_apply_impl(
    ctx: &mut Ctx,
    name: &str,
    args: &[Expr],
    span: Span,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    // `runST` / `runSTArray` — built-in functions whose signatures
    // (`(forall s. ST s a) -> a` and
    // `(forall s. ST s (STArray s i e)) -> Array i e`) are registered in
    // [`TypeEnv::builtin_sigs`]. Each single argument is a lambda `fn s -> body`
    // where `s` is a PHANTOM type token: a fresh quantified type-constructor
    // variable. This is rank-N application — implemented specifically for these
    // two; a generic mechanism for any callee signature with a `TForall`-headed
    // argument would iterate the registered `TypeExpr`'s `TForall`-bound
    // parameters.
    //
    // The phantom is bound in `ctx.locals` as a VALUE channel whose vty is
    // `TyConVar(s_id)` (save/restore like the `Expr::Lambda` arm), so
    // `Ref("s")` resolves to the phantom AND the phantom shadows any outer
    // value binder named `s` (locals take precedence over defs in
    // `infer_ref_inner`). The lambda BODY is inferred with `s` visible. The
    // result `a` must not mention `s`: because `s` is quantified (a fresh var,
    // not free in the enclosing scope), a result that unified with the phantom
    // is rejected — both when `s_id` survives in the RESOLVED result type and
    // when cross-kind unification bound the phantom away into an outer
    // variable (an `s_id` entry in `subst.value_map`). The phantom is erased,
    // so the region types as `ArrowTy::value_channel(a)`.
    //
    // `runSTArray` differs in ONE place: its payload is BY DESIGN an
    // `STArray s i e`, so the payload's phantom is unified with the region's
    // (rather than rejected) and the region result becomes the frozen
    // `Array i e` — which must still not mention the phantom.
    let is_runst_array = name == "runSTArray";
    let is_runst = (name == "runST" || is_runst_array)
        && ctx.env.builtin_sigs.get(name).is_some_and(|sig| {
            matches!(
                sig,
                crate::ast::TypeExpr::TFunc(args, _)
                    if args
                        .first()
                        .is_some_and(|a| matches!(a, crate::ast::TypeExpr::TForall(_, _)))
            )
        });
    if is_runst {
        if args.len() != 1 {
            return Err(CompileError::Type {
                msg: format!("`{name}` expects 1 argument, got {}", args.len()),
                span,
            });
        }
        let Expr::Lambda {
            params,
            body,
            span: lspan,
        } = &args[0]
        else {
            return Err(CompileError::Type {
                msg: format!("{name} expects a parenthesized lambda `fn s -> body`"),
                span,
            });
        };
        if params.len() != 1 {
            return Err(CompileError::Type {
                msg: format!("{name} lambda must bind exactly one phantom type parameter"),
                span: *lspan,
            });
        }
        // A fresh quantified phantom id for `s`.
        let s_id = ctx.fresh_id();
        // A lambda body is a DEFERRED computation: clear the method-inlining
        // recursion guard so nested calls of the same method resolve here.
        let saved = ctx.locals.clone();
        let saved_lifting = ctx.method_lifting.clone();
        ctx.method_lifting.clear();
        ctx.locals.insert(
            params[0].name.clone(),
            ArrowTy::value_channel(ValueTy::TyConVar(s_id)),
        );
        // The body of a real `runST` is an `ST s a` computation, not the
        // region's result: infer it WITHOUT threading the call's expected type
        // (which is the payload type `a`, a different shape). A legacy body
        // that is not `ST`-headed is the erased region value itself; its
        // expected type is threaded below.
        let bt = infer_expr_expected(ctx, body, None);
        ctx.locals = saved;
        ctx.method_lifting = saved_lifting;
        let bt = bt?;
        if bt.arity_in() != 0 || bt.arity_out() != 1 {
            return Err(CompileError::Type {
                msg: format!("{name} body must produce one value"),
                span,
            });
        }
        // Real ST (`data ST s a = { unST: STState -> Pair STState a }`): the
        // body's type is `ST s a`, so its state parameter must be THIS region's
        // phantom and the region's result is the payload `a`. Unifying the
        // parameter with `TyConVar(s_id)` links every `STRef` the body created
        // (their `s` was instantiated fresh and unified with the same phantom
        // through the `monad` chain) to this region.
        let body_vty = bt.vty.clone();
        let resolved_body = ctx.subst.resolve_value(&body_vty);
        let (ret_vty, is_st) = match &resolved_body {
            ValueTy::Data(n, st_args) if n == "ST" && st_args.len() == 2 => {
                unify_value(&st_args[0], &ValueTy::TyConVar(s_id), &mut ctx.subst, span)?;
                (st_args[1].clone(), true)
            }
            _ => (body_vty, false),
        };
        // `runSTArray` freezes an `STArray`, so its body has no legacy erasure:
        // the region result must be an `STArray s i e` of THIS region. The
        // payload is the one type allowed to mention the phantom (it is what the
        // freeze consumes), and unifying its `s` with the region's keeps an
        // array allocated in a nested region from being frozen here.
        if is_runst_array {
            let payload = ctx.subst.resolve_value(&ret_vty);
            let (index, elem) = match (is_st, &payload) {
                (true, ValueTy::Data(n, args) | ValueTy::App(n, args))
                    if n == "STArray" && args.len() == 3 =>
                {
                    unify_value(&args[0], &ValueTy::TyConVar(s_id), &mut ctx.subst, span)?;
                    (args[1].clone(), args[2].clone())
                }
                _ => {
                    return Err(CompileError::Type {
                        msg: "runSTArray body must produce an `ST s (STArray s i e)` computation"
                            .into(),
                        span,
                    })
                }
            };
            // The frozen `Array i e` LEAVES the region: neither half may mention
            // the phantom (the same check-escaping rule as `runST`).
            let frozen = ctx
                .subst
                .resolve_value(&ValueTy::App("Array".into(), vec![index, elem]));
            if vty_mentions_tyvar(&frozen, s_id) || ctx.subst.value_map.contains_key(&s_id) {
                return Err(CompileError::Type {
                    msg: "ST region result escapes the phantom type s".into(),
                    span,
                });
            }
            ctx.runst_payload_tys
                .insert((span.start, span.end), frozen.clone());
            return Ok(ArrowTy::value_channel(frozen));
        }
        // Check-escaping: the region result must not mention the phantom. A
        // signal-rate channel carries no value type variables (its `vty` is a
        // placeholder), so only value-rate results can escape — `resolve_value`
        // follows any chain that unified an outer var with the phantom, and
        // `value_map.contains_key(s_id)` catches the case where a cross-kind
        // unification bound the phantom itself away into an outer variable
        // (the phantom leaked even though `s_id` no longer appears).
        let resolved = ctx.subst.resolve_value(&ret_vty);
        if vty_mentions_tyvar(&resolved, s_id) || ctx.subst.value_map.contains_key(&s_id) {
            return Err(CompileError::Type {
                msg: "ST region result escapes the phantom type s".into(),
                span,
            });
        }
        if is_st {
            // The region computes an `a`: the phantom and the state token are
            // erased from the static type, leaving a plain value channel.
            // Record the payload type for lowering, which has no substitution
            // of its own (`body_ty` alone is a bare `ST` head).
            ctx.runst_payload_tys
                .insert((span.start, span.end), resolved.clone());
            return Ok(ArrowTy::value_channel(resolved));
        }
        // Legacy erasure (a body that is not an `ST` computation, e.g. the
        // literal `runST (fn s -> 1.0)`): the phantom has no representation and
        // the region types as its body. Honor the call site's expected type.
        if let Some(exp) = &expected {
            let exp = ctx.subst.resolve_value(exp);
            if matches!(
                exp,
                ValueTy::Data(..) | ValueTy::App(..) | ValueTy::Newtype(..) | ValueTy::Thunk(..)
            ) {
                unify_value(&resolved, &exp, &mut ctx.subst, span)?;
            }
        }
        return Ok(bt);
    }
    // Builtin functions with an ordinary signature registered in
    // `TypeEnv::builtin_sigs`: `write : Maybe a -> OutputStream a -> IO ()`,
    // `stdout : OutputStream String`. A leading `forall` quantifies the whole
    // signature (the polymorphic io-streams operations), so it is stripped here
    // and its binders instantiated once for the call, sharing the element type
    // between arguments and result. This runs AFTER the `runST` special case
    // (its rank-2 signature is handled above) and BEFORE user
    // definitions/typeclass methods — so every `builtin_sigs` name (the STRef
    // and STArray effects included: `newSTRef`, `getBounds`, `getElems`, …) is a
    // PERMANENTLY RESERVED identifier that shadows a user definition of the same
    // name. Lowering reserves exactly the same set; the two must stay in step.
    let builtin_sig = ctx.env.builtin_sigs.get(name).cloned().map(|sig| {
        let mut binders = Vec::new();
        collect_forall_binders(&sig, &mut binders);
        let inner = match sig {
            crate::ast::TypeExpr::TForall(_, inner) => *inner,
            other => other,
        };
        (inner, binders)
    });
    if let Some((crate::ast::TypeExpr::TFunc(params, ret), binders)) = builtin_sig {
        if params.len() != args.len() {
            return Err(CompileError::Type {
                msg: format!(
                    "`{name}` expects {} argument(s), got {}",
                    params.len(),
                    args.len()
                ),
                span,
            });
        }
        // Instantiate the signature's type variables ONCE for the whole call,
        // so the arguments and the result share them (a `forall`-bound region
        // variable in the STRef signatures, plus the free element variable).
        // The binders are hoisted over the WHOLE signature first: those
        // signatures quantify `s` in result position, so binding it lazily
        // while instantiating the result would leave the already-instantiated
        // parameter's `s` unrelated — severing the region phantom.
        let mut sig_vars: HashMap<String, ValueTy> = HashMap::new();
        for n in binders {
            sig_vars.entry(n).or_insert_with(|| ctx.fresh_vty());
        }
        hoist_forall_binders(ctx, &params, &ret, &mut sig_vars);
        let param_tys: Vec<ValueTy> = params
            .iter()
            .map(|p| instantiate_builtin_sig_vty(ctx, p, &mut sig_vars))
            .collect::<Result<_, _>>()?;
        let ret_ty = instantiate_builtin_sig_vty(ctx, &ret, &mut sig_vars)?;
        for (pt, a) in param_tys.into_iter().zip(args.iter()) {
            // Thread the parameter type as the argument's expected type when it
            // is RESULT-DIRECTED for the argument's interior: an `IO` parameter
            // (`makeInputStream :: IO (Maybe a) -> …` handed
            // `return (Just 7.0)`) and a function parameter
            // (`makeOutputStream :: (Maybe a -> IO ()) -> …` handed
            // `\mb -> … return () …`). A result-directed method inside the
            // argument then selects its instance. Other parameters keep plain
            // inference (their expected type may be a signal channel).
            let resolved_pt = ctx.subst.resolve_value(&pt);
            let directed = matches!(&resolved_pt, ValueTy::Data(n, _) if n == "IO")
                || matches!(resolved_pt, ValueTy::Thunk(..));
            let at0 = if directed {
                infer_expr_expected(ctx, a, Some(pt.clone()))?
            } else {
                infer_expr(ctx, a)?
            };
            // A Buffer param (a foreign builtin's signal slot) accepts a signal
            // computation (a `Buffer -> … -> Buffer` arrow applied to the
            // program's buffers), a Buffer value, OR a numeric signal constant
            // (a literal or a reference to a compile-time constant — `c = 440.0`
            // used as `lowpass 1000.0 0.7 1.0`). Re-infer a numeric constant with
            // `Buffer` expected so it unifies as a buffer channel.
            let numeric_const = at0.arity_out() == 1
                && matches!(at0.vty, ValueTy::Double | ValueTy::Int)
                && (matches!(a, Expr::Int(_, _) | Expr::Double(_, _))
                    || is_const_value_ref(ctx, a));
            let at = if matches!(pt, ValueTy::Buffer(_)) && numeric_const {
                infer_expr_expected(ctx, a, Some(pt.clone()))?
            } else {
                at0
            };
            // A Buffer param (a foreign builtin's signal slot) accepts a signal
            // computation (a `Buffer -> … -> Buffer` arrow) OR a plain Buffer
            // value — either way its first
            // output is a Buffer. Non-Buffer params must be 0→1 values.
            let buffer_param = matches!(pt, ValueTy::Buffer(_));
            if !buffer_param && (at.arity_in() != 0 || at.arity_out() != 1) {
                return Err(CompileError::Type {
                    msg: format!(
                        "argument of `{name}` must be a value expression, got arity {}→{}",
                        at.arity_in(),
                        at.arity_out()
                    ),
                    span: a.span(),
                });
            }
            if at.arity_out() != 1 {
                return Err(CompileError::Type {
                    msg: format!(
                        "argument of `{name}` must produce exactly one channel, got {}",
                        at.arity_out()
                    ),
                    span: a.span(),
                });
            }
            unify_value(&at.vty, &pt, &mut ctx.subst, a.span())?;
        }
        // A builtin-sig result is a VALUE channel (`IO Buffer` is a
        // record value; `IO ()` a Void value).
        return Ok(ArrowTy::value_channel(ret_ty));
    }
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
    // Binary elementwise buffer ops (`atan2 y x`, `min a b`, `max a b`): each
    // operand may be a Buffer computation or a scalar constant (broadcast), so
    // type them as 2→1 over a fresh value type and let the lowering pin it.
    if matches!(name, "atan2" | "min" | "max") {
        if args.len() != 2 {
            return Err(CompileError::Type {
                msg: format!("`{name}` expects 2 arguments, got {}", args.len()),
                span,
            });
        }
        let s = ctx.fresh_vty();
        return Ok(ArrowTy::uniform(2, 1, s));
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
            ctx.env.buffer_vty(),
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
                // The single argument may be a record literal matched by field
                // name or — for a single-field record — the field VALUE
                // directly, newtype-style: `Box (fn x -> x * 2.0)` ≡
                // `Box { f: fn x -> x * 2.0 }`. Synthesize the field literal so
                // both shapes flow through the unification below unchanged.
                let fields_expr: Vec<(String, Expr)> = match &args[0] {
                    Expr::Record(f, _) => f.iter().map(|(n, e)| (n.clone(), e.clone())).collect(),
                    _ if fields.len() == 1 => vec![(fields[0].0.clone(), args[0].clone())],
                    _ => {
                        return Err(CompileError::Type {
                            msg: format!(
                                "record constructor `{name}` expects a record literal, e.g. `{name} {{ ... }}`"
                            ),
                            span,
                        });
                    }
                };
                // Every declared field exactly once, no duplicates.
                let mut seen: Vec<&String> = Vec::new();
                for (f, _) in &fields_expr {
                    if seen.contains(&f) {
                        return Err(CompileError::Type {
                            msg: format!("duplicate field `{f}` in `{name}` constructor"),
                            span,
                        });
                    }
                    seen.push(f);
                }
                for (fname, _, _) in &fields {
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
                // Track which placeholder a freshened field replaced
                // (`Var(k)` / `TyConVar(k)` / `TyConApp(k, ..)` head →
                // fresh var), so parameterized user data can resolve
                // each type parameter from the subst after the fields
                // unify. EVERY placeholder id is freshened — not just
                // bare `Var(k)` fields — because the raw ids live in the
                // SAME counter space as live inference vars: with a small
                // `ctx.next`, a field VALUE's own fresh vars (a lambda's
                // parameter `fn x -> Just x`) would otherwise collide
                // with the record's placeholder ids (`data Kleisli m a b`
                // uses ids 1..3), corrupting the parameter bindings.
                let mut placeholder_bindings: HashMap<u32, ValueTy> = HashMap::new();
                fn fresh_placeholder_id(
                    k: u32,
                    bindings: &mut HashMap<u32, ValueTy>,
                    ctx: &mut Ctx,
                ) -> u32 {
                    match bindings.get(&k) {
                        Some(ValueTy::Var(f)) | Some(ValueTy::TyConVar(f)) => *f,
                        Some(_) => unreachable!("placeholder binding is a fresh var"),
                        None => {
                            let f = ctx.next;
                            ctx.next += 1;
                            bindings.insert(k, ValueTy::Var(f));
                            f
                        }
                    }
                }
                fn freshen_placeholder(
                    fty: &ValueTy,
                    bindings: &mut HashMap<u32, ValueTy>,
                    ctx: &mut Ctx,
                ) -> ValueTy {
                    match fty {
                        ValueTy::Var(k) => ValueTy::Var(fresh_placeholder_id(*k, bindings, ctx)),
                        ValueTy::TyConVar(k) => {
                            ValueTy::TyConVar(fresh_placeholder_id(*k, bindings, ctx))
                        }
                        ValueTy::TyConApp(f, xs) => ValueTy::TyConApp(
                            fresh_placeholder_id(*f, bindings, ctx),
                            xs.iter()
                                .map(|x| freshen_placeholder(x, bindings, ctx))
                                .collect(),
                        ),
                        ValueTy::Thunk(fa, fr) => ValueTy::Thunk(
                            fa.iter()
                                .map(|x| freshen_placeholder(x, bindings, ctx))
                                .collect(),
                            fr.iter()
                                .map(|x| freshen_placeholder(x, bindings, ctx))
                                .collect(),
                        ),
                        ValueTy::Data(n, a) => ValueTy::Data(
                            n.clone(),
                            a.iter()
                                .map(|x| freshen_placeholder(x, bindings, ctx))
                                .collect(),
                        ),
                        ValueTy::Newtype(n, a) => ValueTy::Newtype(
                            n.clone(),
                            a.iter()
                                .map(|x| freshen_placeholder(x, bindings, ctx))
                                .collect(),
                        ),
                        ValueTy::App(n, a) => ValueTy::App(
                            n.clone(),
                            a.iter()
                                .map(|x| freshen_placeholder(x, bindings, ctx))
                                .collect(),
                        ),
                        t => t.clone(),
                    }
                }
                let field_tys: Vec<(String, ValueTy)> = fields
                    .iter()
                    .map(|(fname, fty, _)| {
                        (
                            fname.to_string(),
                            freshen_placeholder(fty, &mut placeholder_bindings, ctx),
                        )
                    })
                    .collect();
                // Seed the type-parameter placeholders from the record's own
                // expected type when the heads match: `Kleisli m a b` built
                // under an expected `Kleisli Maybe Double Double` pins `m :=
                // Maybe`, so a result-directed method inside a field value
                // (`arr f = Kleisli (\ x -> return (f x))` inlined under a
                // concrete container) resolves its instance. Each expected arg
                // is unified against the placeholder's fresh var so the field
                // types below resolve through the substitution.
                if let Some(exp) = &expected {
                    let eargs = match ctx.subst.resolve_value(exp) {
                        ValueTy::Data(n, a) if n == *name => Some(a.clone()),
                        ValueTy::App(n, a) if n == *name => Some(a.clone()),
                        _ => None,
                    };
                    if let Some(eargs) = eargs {
                        for (k, t) in eargs.into_iter().enumerate() {
                            let pid = (k + 1) as u32;
                            let f = fresh_placeholder_id(pid, &mut placeholder_bindings, ctx);
                            unify_value(&ValueTy::Var(f), &t, &mut ctx.subst, span)?;
                        }
                    }
                }
                for (f, e) in &fields_expr {
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
                    // Resolve the field type through the substitution so a
                    // container pinned by the expected type (or by an earlier
                    // field, `unKleisli: a -> m b` with `m := Maybe` from
                    // `unKleisli: \x -> Just x`) is visible to the field value
                    // as it infers.
                    let fty_resolved = ctx.subst.resolve_value(&fty);
                    let vt = infer_const_value_expected(ctx, e, Some(&fty_resolved))?;
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
                // type (m → Maybe, a → Double, b → Double).
                if let Some(arity) = ctx.env.data_arities.get(name) {
                    if *arity > 0 {
                        let arg_tys: Vec<ValueTy> = (0..*arity)
                            .map(|k| {
                                let pid = (k + 1) as u32;
                                match placeholder_bindings.get(&pid) {
                                    // A field mentioned this parameter, so its
                                    // placeholder was freshened to a LIVE var.
                                    // Return it as-is — even while still
                                    // unbound — so the constructor's type
                                    // argument stays LINKED to the field
                                    // values (`return r` must keep the
                                    // payload chained to `r`'s type).
                                    Some(fresh) => ctx.subst.resolve_value(fresh),
                                    // No field mentions it: the raw placeholder
                                    // id shares the live counter space, so
                                    // freshen an unbound one instead of
                                    // handing out a colliding id.
                                    None => match ctx.subst.resolve_value(&ValueTy::Var(pid)) {
                                        ValueTy::Var(_) => ctx.fresh_vty(),
                                        t => t,
                                    },
                                }
                            })
                            .collect();
                        return Ok(ArrowTy::value_channel(ValueTy::Data(name.into(), arg_tys)));
                    }
                }
                return Ok(ArrowTy::value_channel(ValueTy::Data(name.into(), vec![])));
            }
            DataInfo::Sum(_) => {
                // Applying the sum type name itself is not a constructor call.
                return Err(CompileError::Type {
                    msg: format!("`{name}` is a sum type; use one of its constructors"),
                    span,
                });
            }
            DataInfo::Opaque => {
                // An opaque type is nameable but has no constructor to apply.
                return Err(CompileError::Type {
                    msg: format!("opaque type `{name}` has no constructors"),
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
        let inner = resolve_type_name(&ctx.env, &inner_name);
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
        // (`Left 5.0` is `App("Either", [Double, ?])`). The match pin pads the
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
    if !method_shadowed_by_def(ctx, name, args) {
        if let Some(class_name) = ctx.env.class_of_method(name) {
            let class_info = ctx.env.typeclasses.get(&class_name).cloned().unwrap();
            let class_var = class_info.var.clone();
            let sig = class_info
                .methods
                .iter()
                .find(|(m, _)| m == name)
                .map(|(_, s)| s.clone());
            let sig = sig.as_ref().unwrap();
            // A GIVEN constraint is consulted FIRST (design §3.0): inside a
            // constrained body the class variable is ABSTRACT, so there is no
            // instance to select — the call is typed against the incoming
            // dictionary and its evidence recorded as `Dict(g)`.
            if let Some(given) =
                given_for_method_call(ctx, &class_name, &class_var, sig, args, expected.as_ref())?
            {
                let at = infer_method_under_given(ctx, &class_var, &given, sig, args, span)?;
                record_selection(ctx, span, &class_name, &ValueTy::Dict(given.class.clone()));
                return Ok(at);
            }
            // Argument count: a `TFunc` signature carries its argument list; a
            // bare class-var signature (`show: a`) takes exactly one selector
            // argument (the arity-0 instance binds it). A `TForall` binder
            // delegates to the inner type.
            let n_sig_args = sig.sig_arg_count();
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
                let env = ctx.env.clone();
                let cv_var = ctx.fresh_id();
                let Some((ty_name, cv_subst)) =
                    method_dispatch_key(&env, &class_name, &class_var, sig, &exp, cv_var, || {
                        ctx.fresh_vty()
                    })
                else {
                    return Err(CompileError::Type {
                        msg: format!(
                            "cannot resolve method `{name}` of `{class_name}`: the expected type is not concrete"
                        ),
                        span,
                    });
                };
                // Constraint instance: bind head args from the expected
                // container's leading args (`Kleisli Maybe Double Double` + head
                // `Kleisli m` → `m := Maybe`) and discharge the constraints.
                bind_constraint_instance(ctx, &class_name, &ty_name, &exp, span)?;
                // The type checker SELECTED an instance here: record the
                // dictionary lowering must pass (design §3.0 point 3).
                record_selection(ctx, span, &class_name, &selection_evidence(&cv_subst));
                // A PRIMITIVE instance (a compiler kernel: `Num Int`,
                // `Num Double`, a buffer arithmetic instance, a structural
                // Eq/Ord) has no source body. Its dictionary entry is the
                // kernel, selected by (class, type, method) at lowering, so
                // inference only needs the method's SIGNATURE, which the class
                // already declares.
                let is_primitive = ctx
                    .env
                    .instances
                    .get(class_name.as_str())
                    .and_then(|m| m.get(ty_name.as_str()))
                    .map(|i| i.primitive)
                    .unwrap_or(false);
                if is_primitive {
                    // A kernel-backed method: its type IS the class signature
                    // with the class variable solved to the dispatch type. The
                    // arguments are unified against the parameter types so a
                    // call like `(+) 1.0 2.0` solves `a := Double`.
                    let param_tys = signature_param_tys(ctx, &class_var, &cv_subst, sig);
                    for (i, a) in args.iter().enumerate() {
                        if let Some(pt) = param_tys.get(i) {
                            let at = infer_method_value_vty_expected(
                                ctx,
                                a,
                                "argument",
                                Some(pt.clone()),
                            )?;
                            unify_value(pt, &at, &mut ctx.subst, a.span())?;
                        }
                    }
                    let mut fresh_ids: Vec<ValueTy> = Vec::new();
                    for _ in 0..8 {
                        fresh_ids.push(ctx.fresh_vty());
                    }
                    let mut it = fresh_ids.into_iter();
                    let (_, ret) = signature_param_and_result_tys_conv(
                        &ctx.env,
                        &class_var,
                        &cv_subst,
                        sig,
                        || it.next().unwrap_or(ValueTy::Double),
                    );
                    let ret = ret.unwrap_or_else(|| ctx.fresh_vty());
                    return Ok(ArrowTy::value(nest_thunk(&param_tys, vec![ret])));
                }
                let (_, params, body) = match ctx.env.resolve_method(name, ty_name.as_str()) {
                    Some(r) => r,
                    None => match ctx.env.class_default(&class_name, name) {
                        Some((params, body)) => (class_name.clone(), params, body),
                        None => {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "no instance of `{class_name}` for type `{ty_name}` and no default for `{name}`"
                                ),
                                span,
                            });
                        }
                    },
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
                let param_tys = signature_param_tys(ctx, &class_var, &cv_subst, sig);
                // Infer the arguments BEFORE binding the method's params into
                // `ctx.locals`: a param that shares a name with a caller-local
                // (`return x` in `\x -> return (f x)` — the `arr` instance
                // body) must not capture the argument's free `x` (that made
                // `f x` apply the FRESH param var, and a later unify of that
                // var against the closure's result type tripped the occurs
                // check: `Var44 ~ Pair[Var44, Var44]`).
                let mut arg_vtys = Vec::with_capacity(args.len());
                for a in args.iter() {
                    arg_vtys.push(infer_method_value_vty(ctx, a, "argument")?);
                }
                for (p, pt) in params.iter().zip(param_tys) {
                    ctx.locals
                        .insert(p.clone(), ArrowTy::value_channel(pt.clone()));
                }
                // The single element argument (`pure x` / `return x`) unifies
                // against the signature's leading parameter type.
                for ((a, p), av) in args.iter().zip(params.iter()).zip(arg_vtys.iter()) {
                    let pt = ctx
                        .locals
                        .get(p)
                        .cloned()
                        .map(|t| t.vty.clone())
                        .unwrap_or(ValueTy::Double);
                    unify_value(av, &pt, &mut ctx.subst, a.span())?;
                }
                let body_vty =
                    infer_method_value_vty_expected(ctx, &body, "body", Some(exp.clone()));
                ctx.locals = saved;
                ctx.method_lifting.remove(&key);
                let body_vty = body_vty?;
                return Ok(ArrowTy::value_channel(body_vty));
            }
            if class_info.arity == 0 {
                // A PRIMITIVE method is curried like any function: fewer
                // arguments than the signature declares is PARTIAL APPLICATION
                // and yields the remaining function (Haskell 2010 §4.3.3), not
                // an error. The arity check applies only to methods whose
                // instance body must be inlined.
                let primitive = expected
                    .as_ref()
                    .and_then(|exp| method_value_dispatch_env(&ctx.env, exp))
                    .map(|(ty, _)| {
                        ctx.env
                            .instances
                            .get(class_name.as_str())
                            .and_then(|m| m.get(ty.as_str()))
                            .map(|i| i.primitive)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if args.len() < n_sig_args {
                    // Partial application: type the supplied arguments against
                    // the leading parameters and return the curried remainder.
                    let class_var = class_info.var.clone();
                    let mut fresh_ids: Vec<ValueTy> = Vec::new();
                    for _ in 0..8 {
                        fresh_ids.push(ctx.fresh_vty());
                    }
                    let mut it = fresh_ids.into_iter();
                    let (param_tys, ret) = signature_param_and_result_tys_conv(
                        &ctx.env,
                        &class_var,
                        &ClassVarSubst::Var(u32::MAX),
                        sig,
                        || it.next().unwrap_or(ValueTy::Double),
                    );
                    for (i, a) in args.iter().enumerate() {
                        if let Some(pt) = param_tys.get(i) {
                            let at = infer_method_value_vty_expected(
                                ctx,
                                a,
                                "argument",
                                Some(pt.clone()),
                            )?;
                            unify_value(pt, &at, &mut ctx.subst, a.span())?;
                        }
                    }
                    let ret_ty = ret.unwrap_or_else(|| ctx.fresh_vty());
                    let rest: Vec<ValueTy> = param_tys[args.len().min(param_tys.len())..].to_vec();
                    return Ok(ArrowTy::value(nest_thunk(&rest, vec![ret_ty])));
                }
                let _ = primitive;
                if args.len() != n_sig_args {
                    return Err(CompileError::Type {
                        msg: format!(
                            "method `{name}` of `{class_name}` expects {n_sig_args} argument(s), got {}",
                            args.len()
                        ),
                        span,
                    });
                }
                // Selector: the argument whose signature type mentions the class
                // variable AND which is not a bare nullary method ref — both
                // criteria must agree, because the same argument's concrete type
                // solves the class variable (`pick: Bool -> Pair a a -> Double`
                // selects on the `Pair`, `mappend mempty xs` on `xs`).
                let Some(selector_idx) =
                    class_var_selector_index(sig, &class_var, args.len(), |i| {
                        is_bare_nullary_method_ref(ctx, &args[i], &class_name)
                    })
                else {
                    return Err(CompileError::Type {
                        msg: format!(
                            "cannot resolve method `{name}` of `{class_name}`: no argument can select the instance (every class-var argument is a nullary method reference)"
                        ),
                        span,
                    });
                };
                let arg_vty = infer_method_value_vty(ctx, &args[selector_idx], "argument")?;
                let env = ctx.env.clone();
                let cv_var = ctx.fresh_id();
                let Some((ty_name, cv_subst)) = method_dispatch_key(
                    &env,
                    &class_name,
                    &class_var,
                    sig,
                    &arg_vty,
                    cv_var,
                    || ctx.fresh_vty(),
                ) else {
                    return Err(CompileError::Type {
                        msg: format!(
                            "cannot resolve method `{name}` of `{class_name}`: the argument type is not concrete"
                        ),
                        span: args[selector_idx].span(),
                    });
                };
                // The type checker SELECTED an instance here: record the
                // dictionary lowering must pass (design §3.0 point 3).
                record_selection(ctx, span, &class_name, &selection_evidence(&cv_subst));
                // Primitive instances (compiler kernels) have no source body:
                // the dictionary entry is the kernel, selected at lowering, so
                // inference needs only the method signature.
                let is_primitive = ctx
                    .env
                    .instances
                    .get(class_name.as_str())
                    .and_then(|m| m.get(ty_name.as_str()))
                    .map(|i| i.primitive)
                    .unwrap_or(false);
                if is_primitive {
                    let param_tys = signature_param_tys(ctx, &class_var, &cv_subst, sig);
                    for (i, a) in args.iter().enumerate() {
                        if let Some(pt) = param_tys.get(i) {
                            let at = infer_method_value_vty_expected(
                                ctx,
                                a,
                                "argument",
                                Some(pt.clone()),
                            )?;
                            unify_value(pt, &at, &mut ctx.subst, a.span())?;
                        }
                    }
                    let mut fresh_ids: Vec<ValueTy> = Vec::new();
                    for _ in 0..8 {
                        fresh_ids.push(ctx.fresh_vty());
                    }
                    let mut it = fresh_ids.into_iter();
                    let (_, ret) = signature_param_and_result_tys_conv(
                        &ctx.env,
                        &class_var,
                        &cv_subst,
                        sig,
                        || it.next().unwrap_or(ValueTy::Double),
                    );
                    let ret = ret.unwrap_or_else(|| ctx.fresh_vty());
                    return Ok(ArrowTy::value(nest_thunk(&param_tys, vec![ret])));
                }
                let (_, params, body) = match ctx.env.resolve_method(name, ty_name.as_str()) {
                    Some(r) => r,
                    None => match ctx.env.class_default(&class_name, name) {
                        Some((params, body)) => (class_name.clone(), params, body),
                        None => {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "no instance of `{class_name}` for type `{ty_name}` and no default for `{name}`"
                                ),
                                span,
                            });
                        }
                    },
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
                let param_tys = signature_param_tys(ctx, &class_var, &cv_subst, sig);
                // Infer every argument BEFORE binding the method's params into
                // `ctx.locals`: an argument is a caller expression, so a param
                // that shares a name with a caller-local must not capture it.
                let mut param_bind: Vec<ValueTy> = Vec::with_capacity(params.len());
                for (i, _p) in params.iter().enumerate() {
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
                        let expected = param_tys.get(i).cloned().unwrap_or(ValueTy::Double);
                        infer_method_value_vty_expected(ctx, &nbody, "argument", Some(expected))?
                    };
                    param_bind.push(pt);
                }
                for (p, pt) in params.iter().zip(param_bind) {
                    ctx.locals.insert(p.clone(), ArrowTy::value_channel(pt));
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
            // The container arg may itself be a result-directed method call
            // (`arr f` in `compose (arr f) (arr g)`) that needs an expected
            // type to resolve. When the enclosing expression expects a concrete
            // container (`Data("Kleisli", [..])` threaded from a field
            // projection / match), derive the container arg's expected type
            // from it: the instance head's args (`m`) from the expected's
            // leading args, the remaining class-var slots freshened.
            let class_info_for_container = ctx.env.typeclasses.get(&class_name).cloned();
            let container_expected: Option<ValueTy> = match &expected {
                Some(ValueTy::Data(c, args)) | Some(ValueTy::App(c, args)) => {
                    let inst = ctx.env.instances.get(&class_name).and_then(|m| m.get(c));
                    match inst {
                        Some(i) => {
                            let head_len = i.head_args.len();
                            let mut cargs: Vec<ValueTy> =
                                args.iter().take(head_len).cloned().collect();
                            let arity = class_info_for_container
                                .as_ref()
                                .map(|ci| ci.arity)
                                .unwrap_or(0);
                            while cargs.len() < head_len + arity {
                                cargs.push(ctx.fresh_vty());
                            }
                            Some(match expected {
                                Some(ValueTy::Data(..)) => ValueTy::Data(c.clone(), cargs),
                                _ => ValueTy::App(c.clone(), cargs),
                            })
                        }
                        None => None,
                    }
                }
                _ => None,
            };
            let container_vty = match container_expected {
                Some(ce) => infer_method_value_vty_expected(
                    ctx,
                    &args[container_idx],
                    "argument",
                    Some(ce),
                )?,
                None => infer_method_value_vty(ctx, &args[container_idx], "argument")?,
            };
            // Constraint instance: bind head args from the container's LEADING
            // args and discharge the constraints, then match the class pattern
            // against the REMAINING args.
            let container_head: Option<String> = match &container_vty {
                ValueTy::App(c, _) | ValueTy::Data(c, _) => Some(c.clone()),
                _ => None,
            };
            let (head_args, _) = match &container_head {
                Some(c) => bind_constraint_instance(ctx, &class_name, c, &container_vty, span)?,
                None => (vec![], vec![]),
            };
            let concrete_args = match &container_vty {
                ValueTy::App(_, a) | ValueTy::Data(_, a) => a.clone(),
                _ => vec![],
            };
            // Build the class-var pattern (`f a` → `App("f", [fresh])`) from the
            // container argument's signature type.
            let pattern = class_var_pattern(ctx, sig, container_idx);
            // A partial instance head (`Kleisli m`, head_args = ["m"]) means the
            // pattern matches the concrete container's args AFTER the bound
            // head args (`Kleisli Maybe Double Double` → match `[Double, Double]`).
            let match_target = if head_args.is_empty() {
                container_vty.clone()
            } else {
                match &container_vty {
                    ValueTy::App(c, _) => {
                        ValueTy::App(c.clone(), concrete_args[head_args.len()..].to_vec())
                    }
                    ValueTy::Data(c, _) => {
                        ValueTy::Data(c.clone(), concrete_args[head_args.len()..].to_vec())
                    }
                    _ => container_vty.clone(),
                }
            };
            let ctor = match ctx.env.match_ctor_pattern(
                &class_var,
                &pattern,
                &match_target,
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
                None => match ctx.env.class_default(&class_name, name) {
                    Some((params, body)) => (class_name.clone(), params, body),
                    None => {
                        return Err(CompileError::Type {
                            msg: format!(
                                "no instance of `{class_name}` for constructor `{ctor}` and no default for `{name}`"
                            ),
                            span,
                        });
                    }
                },
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
            // Phase 2: infer the remaining args with their signature param type
            // as expected (so `bind mx (fn x -> return x)` resolves `return` by
            // the monad), then bind all params.
            let (mut param_tys, result_ty) = signature_param_and_result_tys_conv(
                &ctx.env.clone(),
                &class_var,
                &ClassVarSubst::Ctor(ctor.clone()),
                sig,
                || ctx.fresh_vty(),
            );
            // A partial instance head (`Kleisli m` → head arg `m`): the
            // signature-converted container type drops the head (`a b c` →
            // `Data("Kleisli", [b, c])`). Prepend the head args bound at the
            // call site (`m := Maybe`) so the arg expected types carry the full
            // concrete container (`Data("Kleisli", [Maybe, b, c])`) — a
            // result-directed second arg (`arr g`) then resolves `return` by the
            // monad.
            prepend_instance_head_args(
                &ctx.env.clone(),
                &class_name,
                &mut param_tys,
                &ctor,
                &ctx.type_var_bindings.clone(),
                || ctx.fresh_vty(),
            );
            // The body's expected type is the method's own RESULT (with the
            // instance head args restored), so a default/instance body that
            // calls other class methods threads its concrete container into
            // them (`both f g = compose (first f) (second g)` → `compose` sees
            // `Kleisli m …`). Without this the body is inferred with no expected
            // and the nested `arr`/`first`/`second` calls fail with "expected
            // type unknown". The result shares fresh vars with `param_tys`.
            let body_expected = result_ty.map(|mut rt| {
                prepend_instance_head_args(
                    &ctx.env.clone(),
                    &class_name,
                    std::slice::from_mut(&mut rt),
                    &ctor,
                    &ctx.type_var_bindings.clone(),
                    || ctx.fresh_vty(),
                );
                rt
            });
            let mut arg_vtys = Vec::with_capacity(args.len());
            for (i, a) in args.iter().enumerate() {
                if i == container_idx {
                    // The container argument IS the method's container
                    // parameter: unify the two so the payload/head args inside
                    // it are shared with the remaining parameters. Without this
                    // the type variable a caller binds from the container (the
                    // element type in `x <- mx`) never meets the method's own
                    // `a`, so a `do`-bound name keeps an unconstrained type
                    // until its first use — enough to let a cell from one ST
                    // region be used inside another.
                    if let Some(exp) = param_tys.get(i) {
                        unify_value(&container_vty, exp, &mut ctx.subst, a.span())?;
                    }
                    arg_vtys.push(container_vty.clone());
                } else {
                    let exp = param_tys.get(i).cloned().unwrap_or(ValueTy::Double);
                    arg_vtys.push(infer_method_value_vty_expected(
                        ctx,
                        a,
                        "argument",
                        Some(exp),
                    )?);
                }
            }
            // Mirror of the lowering guard (lower.rs, constructor-class branch):
            // the key arms ONLY around the inlined BODY, not the arguments.
            // Arguments may legally inline other methods' defaults that
            // reference this method (the method-lifting composition — `both`'s
            // default reaches `compose` again through `second`'s default body
            // while the outer `compose`'s second argument is inferred); genuine
            // recursion (a body calling its own method name) is still caught.
            let key = (class_name.clone(), ctor.clone(), name.to_string());
            if ctx.method_lifting.contains(&key) {
                return Err(CompileError::Type {
                    msg: format!("recursive typeclass method `{name}` for type `{ctor}`"),
                    span,
                });
            }
            ctx.method_lifting.insert(key.clone());
            let saved = ctx.locals.clone();
            for (p, av) in params.iter().zip(arg_vtys.iter()) {
                ctx.locals
                    .insert(p.clone(), ArrowTy::value_channel(av.clone()));
            }
            let body_vty = infer_method_value_vty_expected(ctx, &body, "body", body_expected);
            ctx.locals = saved;
            ctx.method_lifting.remove(&key);
            let body_vty = body_vty?;
            return Ok(ArrowTy::value_channel(body_vty));
        }
    }
    // Foreign (FFI) declaration: validate arity + scalar params from the
    // language signature, then type as a signal arrow. Signal and scalar params
    // occupy positional arg slots in declaration order (`gain _ 0.5` → the wire
    // at args[0], the constant at args[1]); a variadic signal tail consumes all
    // remaining args.
    if let Some(fsig) = ctx.env.foreign_sigs.get(name).cloned() {
        if let Some(sig) = crate::types::ffi::ffi_sig_from_typeexpr(&ctx.env, name, &fsig) {
            let variadic_idx = sig
                .params
                .iter()
                .position(|p| matches!(p, crate::types::ffi::FfiParam::VariadicSignal(_)));
            // Required params = non-variadic params that must appear as call
            // args: Signal slots (positional wires) + scalars/resources +
            // non-all-default records. An all-default record is optional
            // (mirrors legacy `BuiltinSig::min_args`). A call may omit a
            // TRAILING run of required Signal slots — those are fed by the
            // combinator (the `(_, _) :> mixer` style emits
            // no signal call args) — but omitting a scalar/record is an arity
            // error.
            let required_params: Vec<crate::types::ffi::FfiParam> = sig
                .params
                .iter()
                .filter(|p| match p {
                    crate::types::ffi::FfiParam::VariadicSignal(_) => false,
                    crate::types::ffi::FfiParam::Record(rec_ty) => {
                        let schema = crate::types::ffi::ffi_record_schema(&ctx.env, rec_ty);
                        !schema.is_some_and(|s| s.fields.iter().all(|(_, _, d)| d.is_some()))
                    }
                    _ => true,
                })
                .cloned()
                .collect();
            let required = required_params.len();
            if args.len() < required {
                let deficit = required - args.len();
                let missing_are_signals = required_params
                    .iter()
                    .rev()
                    .take(deficit)
                    .all(|p| matches!(p, crate::types::ffi::FfiParam::Signal(_)));
                if !missing_are_signals {
                    return Err(CompileError::Type {
                        msg: format!(
                            "foreign `{name}` expects at least {required} arg(s), got {}",
                            args.len()
                        ),
                        span,
                    });
                }
            }
            let max_args = if sig
                .params
                .iter()
                .any(|p| matches!(p, crate::types::ffi::FfiParam::VariadicSignal(_)))
            {
                None
            } else {
                Some(sig.params.len())
            };
            if let Some(max) = max_args {
                if args.len() > max {
                    return Err(CompileError::Type {
                        msg: format!(
                            "foreign `{name}` expects at most {max} arg(s), got {}",
                            args.len()
                        ),
                        span,
                    });
                }
            }
            let mut signal_ins = 0usize;
            let mut pos = 0usize;
            // Trailing signal slots with no explicit call arg: either a
            // combinator-fed channel (the `(_, _) :> node`
            // style feeds signals via Merge, so the FFI Apply carries only the
            // scalar params) or a first-class partial application (`map (onepole
            // 200.0 0.7) buffers` — the scalars fold at build, the missing
            // Buffer remains). Count them; the arm's result distinguishes the
            // two at the end.
            let mut fed_signals = 0usize;
            for p in &sig.params {
                match p {
                    crate::types::ffi::FfiParam::Signal(_) => {
                        if pos >= args.len() {
                            // Combinator-fed signal / partial-application slot:
                            // no explicit wire call arg. The channel contributes
                            // to the arity; the remaining params consume no call
                            // args.
                            fed_signals += 1;
                            signal_ins += 1;
                            pos += 1;
                            continue;
                        }
                        // A foreign signal slot holds a Buffer expression (a
                        // 0-in generator like `sine 440.0`, another foreign
                        // call, or a buffer value); a VALUE-rate channel (a
                        // string, a record, a lambda) is a type error. A bare
                        // Double/Int literal IS a signal constant: infer it
                        // with `Buffer` expected, exactly as the buffer
                        // arithmetic path does (`lowering` broadcasts a scalar
                        // to a buffer constant).
                        let at = if matches!(&args[pos], Expr::Int(_, _) | Expr::Double(_, _)) {
                            infer_expr_expected(ctx, &args[pos], Some(ctx.env.buffer_vty()))?
                        } else {
                            infer_expr(ctx, &args[pos])?
                        };
                        if matches!(at.vty, ValueTy::Var(_)) {
                            // An unresolved λ-parameter used in a signal slot is
                            // a signal parameter: bind it to `Buffer`. (Curried
                            // definitions bind their λ-parameters as values, so a
                            // signal parameter arrives here as a fresh `Var`.)
                            unify_value(
                                &at.vty,
                                &ctx.env.buffer_vty(),
                                &mut ctx.subst,
                                args[pos].span(),
                            )?;
                        } else if !matches!(at.vty, ValueTy::Buffer(_)) {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "signal argument {pos} of `{name}` must be a signal \
                                     (found a value/constant)"
                                ),
                                span: args[pos].span(),
                            });
                        }
                        signal_ins += at.arity_in();
                        pos += 1;
                    }
                    crate::types::ffi::FfiParam::VariadicSignal(_) => {
                        // The variadic's explicit signal span stops before the
                        // trailing non-variadic params (each consumes one
                        // trailing call arg — mixer's trailing `Record` config).
                        let trailing = variadic_idx.map(|i| sig.params.len() - i - 1).unwrap_or(0);
                        let end = args.len().saturating_sub(trailing);
                        for arg in &args[pos..end] {
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
                        pos = end;
                    }
                    crate::types::ffi::FfiParam::Scalar => {
                        if pos >= args.len() {
                            break;
                        }
                        let at = infer_expr(ctx, &args[pos])?;
                        if at.arity_in() != 0 || at.arity_out() != 1 {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "param at position {pos} of `{name}` must be constant"
                                ),
                                span: args[pos].span(),
                            });
                        }
                        pos += 1;
                    }
                    crate::types::ffi::FfiParam::Record(rec_ty) => {
                        if pos >= args.len() {
                            break;
                        }
                        // A record param takes a record literal of the named
                        // data type; lowering flattens its schema fields into
                        // the folded param list.
                        match &args[pos] {
                            Expr::Record(..) => {
                                if !ctx.env.data_types.contains_key(rec_ty.as_str()) {
                                    return Err(CompileError::Type {
                                        msg: format!(
                                            "unknown record type `{rec_ty}` for foreign `{name}`"
                                        ),
                                        span: args[pos].span(),
                                    });
                                }
                            }
                            other => {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "argument {pos} of `{name}` must be a record literal"
                                    ),
                                    span: other.span(),
                                });
                            }
                        }
                        pos += 1;
                    }
                    crate::types::ffi::FfiParam::Resource(_) => {
                        if pos >= args.len() {
                            break;
                        }
                        // A resource param is a symbolic `Ref` to a declared
                        // tape name, or an inline `tape_loop <capacity>`
                        // constructor call. Lowering records the tape (name or
                        // constructor) and the build path resolves it to a
                        // shared cell by index.
                        match &args[pos] {
                            Expr::Ref(..) => {}
                            Expr::Apply { name, .. } if name == "tape_loop" => {}
                            other => {
                                return Err(CompileError::Type {
                                    msg: format!(
                                        "resource argument {pos} of `{name}` must be a symbolic reference or a `tape_loop` constructor"
                                    ),
                                    span: other.span(),
                                });
                            }
                        }
                        pos += 1;
                    }
                }
            }
            // A trailing run of omitted Signal slots (no explicit wire call
            // args) is either a combinator-fed channel (the legacy
            // `(_, _) :> node` style) or a first-class
            // partial application (`map (onepole 200.0 0.7) buffers` — the
            // scalars fold at build, the Buffer remains). After the
            // combinator-sugar pass appends wires for direct combinator
            // operands, only genuine partial applications reach here, so the
            // result is a `Func` value: `Buffer -> … -> Buffer`.
            if fed_signals > 0 {
                return Ok(ArrowTy::value_channel(ValueTy::Thunk(
                    vec![ctx.env.buffer_vty(); fed_signals],
                    vec![ctx.env.buffer_vty(); sig.signal_outs],
                )));
            }
            return Ok(ArrowTy::uniform(
                signal_ins,
                sig.signal_outs,
                ctx.env.buffer_vty(),
            ));
        }
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
        if local.arity_out() == 1 {
            let lty = &local.vty;
            let resolved = ctx.subst.resolve_value(lty);
            // A resolved function value is a curried NEST of single-argument
            // thunks (`\a -> \b -> …`), so its total arity comes from
            // flattening the chain. An unresolved parameter (a `Var`) is bound
            // to a fresh nest of the applied arity — nested, not flat, so it
            // unifies with the lambda actually passed for it.
            let (arg_tys, ret_tys, unresolved) = match &resolved {
                ValueTy::Thunk(..) => {
                    let (a, r) = flatten_thunk(&resolved);
                    (a, r, false)
                }
                _ => (
                    (0..args.len()).map(|_| ctx.fresh_vty()).collect::<Vec<_>>(),
                    vec![ctx.fresh_vty()],
                    true,
                ),
            };
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
            if unresolved {
                let func_ty = nest_thunk(&arg_tys, ret_tys.clone());
                unify_value(lty, &func_ty, &mut ctx.subst, span)?;
            }
            for (a, pty) in args.iter().zip(arg_tys.iter()) {
                let vt = infer_method_value_vty(ctx, a, "argument")?;
                unify_value(&vt, pty, &mut ctx.subst, a.span())?;
            }
            let ret_ty = ret_tys.first().cloned().unwrap_or(ValueTy::Double);
            return Ok(ArrowTy::value_channel(ret_ty));
        }
    }
    // Class-constrained definition (`mk :: Ix i => Pair i i -> …`): the scheme
    // came from the definition's signature, so its class variable is solved from
    // the call's arguments and its context discharged against them. A DUAL name
    // (a method that is also a constrained definition — `index` is both `Ix`'s
    // offset method and the array accessor) must first be routed: when the
    // body's GIVEN constraint witnesses the METHOD's class-var pattern, the call
    // is the method, not the definition.
    if !ctx.locals.contains_key(name) {
        if let Some(cd) = ctx.env.constrained_defs.get(name).cloned() {
            let routes_to_method = args.first().is_some_and(|first| {
                ctx.env.class_of_method(name).is_some()
                    && probe_first_arg_solves_class_var(ctx, name, first)
            });
            if !routes_to_method {
                return infer_constrained_apply(ctx, name, &cd, args, span, expected);
            }
        }
    }
    // User-defined function: λ-params are consumed, signal ports remain open
    if let Some(scheme) = ctx.defs.get(name).cloned() {
        // Func-value application: `f = double` binds `f` to `ValueTy::Thunk`,
        // and `f x` dispatches to the referenced definition by instantiating
        // its scheme (v1 resolves calls at compile time — `reduce` β-reduces
        // the body for lowering). The referenced definition's arity must match
        // the applied arguments.
        if let ValueTy::Thunk(_, _) = &scheme.ty.vty {
            let ref_name = func_target(ctx, name);
            let target = ctx.defs.get(ref_name.as_str()).cloned();
            // A closure-valued definition (a lambda literal, or an
            // application/ref chain that produces one) dispatches at
            // RUNTIME — `add2 = adder 2.0`, `main = add2 3.0`. Its call
            // type comes from the Func signature, and the args are value
            // expressions unified against the parameter types.
            let apply_by_signature = target
                .as_ref()
                .is_some_and(|ts| ts.lam_count == 0 && matches!(&ts.ty.vty, ValueTy::Thunk(_, _)));
            if apply_by_signature {
                // Instantiate the quantified Func signature so every call
                // site gets FRESH value-type variables (rank-N value
                // polymorphism). Without instantiation, a closure-valued
                // def with a free value var in its signature
                // (`id2 = fn x -> x` → `Func([Var(v)],[Var(v)])`) shares
                // that var across applications: the first call binds it,
                // and a second call at another type re-unifies against the
                // bound var and fails.
                let inst = ctx.instantiate(&scheme);
                let (arg_tys, ret_tys) = match &inst.vty {
                    ValueTy::Thunk(..) => flatten_thunk(&inst.vty),
                    _ => {
                        return Err(CompileError::Type {
                            msg: format!("`{name}` must be a function value"),
                            span,
                        })
                    }
                };
                // Partial application (currying): fewer args than the
                // signature produce a function over the remaining parameters.
                // `add3 = add 3.0` types as `Func([Double], [Double])`.
                if args.len() < arg_tys.len() {
                    for (a, pty) in args.iter().zip(arg_tys.iter()) {
                        let vt =
                            infer_method_value_vty_expected(ctx, a, "argument", Some(pty.clone()))?;
                        unify_value(&vt, pty, &mut ctx.subst, a.span())?;
                    }
                    let remaining: &[ValueTy] = &arg_tys[args.len()..];
                    return Ok(ArrowTy::value_channel(nest_thunk(
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
                // Pin the application's RESULT type to the caller's expected
                // type BEFORE the arguments infer: a chain whose container is
                // fixed only by its result (`apply (compose (arr f) (arr g))
                // 3.0` matched by `Just v` → `m := Maybe`) then binds the
                // monad ahead of the inlined instance bodies, so their
                // `return`/`bind` resolve by instance.
                pin_result_to_expected(
                    ctx,
                    &ret_tys.first().cloned().unwrap_or(ValueTy::Double),
                    expected.as_ref(),
                    span,
                )?;
                for (a, pty) in args.iter().zip(arg_tys.iter()) {
                    let vt =
                        infer_method_value_vty_expected(ctx, a, "argument", Some(pty.clone()))?;
                    // PARK M10: a signature-less CURRIED definition
                    // (`at a i = a ! i`) is typed as a VALUE (its λ-chain), so
                    // the structured parameter shapes its body recovered
                    // (`Array i e` from the constrained `index`) land straight
                    // in the scheme — shapes the user never wrote. A call-site
                    // mismatch thus dumps the recovered `App` shapes as if
                    // they were the user's own source. Point at the missing
                    // signature instead of dumping those shapes.
                    if let Err(e) = unify_value(&vt, pty, &mut ctx.subst, a.span()) {
                        if ctx.retry_inferred_defs.contains(name) {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "argument mismatch in a call to `{name}`: {e}. \
                                     `{name}` has no signature, so its parameter \
                                     types are inferred from its body and are \
                                     invisible here — annotate the parameter's type \
                                     to check the call against an explicit signature"
                                ),
                                span: a.span(),
                            });
                        }
                        return Err(e);
                    }
                }
                let ret_ty = ret_tys.first().cloned().unwrap_or(ValueTy::Double);
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
                // A named definition applied to its full argument list: the
                // arguments still must match the formal parameter types
                // (Haskell 2010 §4.1.3). `ty` is this call's instantiation, so a
                // polymorphic definition (a generalized `a -> a`) defers instead
                // of being pinned by this call.
                check_def_call_args(ctx, name, args, &ty.vty)?;
                return Ok(ArrowTy::value(applied_result(
                    &ty.vty,
                    ref_scheme.lam_count,
                )));
            }
        }
        if args.len() != scheme.lam_count {
            // Partial application of a named definition (`add5 = add2 5.0` where
            // `add2` takes two λ-params): the remaining λ-params become the
            // value arguments of a first-class function value. The applied args
            // must be value scalars, unified against Double (v1 λ-params are
            // Double-typed).
            if args.len() < scheme.lam_count {
                let remaining = scheme.lam_count - args.len();
                for a in args {
                    let vt = infer_const_value(ctx, a)?;
                    unify_value(&vt, &ValueTy::Double, &mut ctx.subst, a.span())?;
                }
                return Ok(ArrowTy::value_channel(nest_thunk(
                    &vec![ValueTy::Double; remaining],
                    vec![ValueTy::Double],
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
        // Check every actual argument against the definition's formal parameter
        // type (Haskell 2010 §4.1.3: application requires the argument type to
        // match the parameter type). The formal types are the scheme's leading
        // `Thunk` captures — exactly the source `TypedProgram::fn_param_tys` is
        // rebuilt from after inference, so the two cannot disagree.
        check_def_call_args(ctx, name, args, &ty.vty)?;
        return Ok(ArrowTy::value(applied_result(&ty.vty, scheme.lam_count)));
    }
    // Fallback: builtin reference (abs, sin, +, etc.) applied to arguments.
    // The unary core applies one argument at a time; the parser has already
    // curried multi-argument applications into nested `ApplyExpr`, so this
    // name-based arm receives at most one argument.
    let callee = infer_ref(ctx, name, span)?;
    if args.is_empty() {
        return Ok(callee);
    }
    let arg = &args[0];
    // A numeric literal argument to a Buffer-consuming function (`sin 440.0`)
    // is a SIGNAL constant: infer it with `Buffer` expected so the literal
    // unifies as a buffer value, not a value Double.
    let want_buffer = match ctx.subst.resolve_value(&callee.vty) {
        ValueTy::Thunk(caps, _) => caps
            .first()
            .is_some_and(|c| matches!(c, ValueTy::Buffer(_))),
        _ => false,
    };
    let at = if matches!(arg, Expr::Int(_, _) | Expr::Double(_, _)) && want_buffer {
        infer_expr_expected(ctx, arg, Some(ctx.env.buffer_vty()))?
    } else {
        infer_expr(ctx, arg)?
    };
    apply_one(ctx, &callee, &at, name, arg.span())
}

/// Check a named definition's actual arguments against its formal parameter
/// types.
///
/// Haskell 2010 §4.1.3 requires an application's argument type to match the
/// parameter type, so a `String` passed to a `Double` parameter is a type error —
/// builtin calls already enforce this, and a user definition must not be the one
/// hole. `fn_ty` is the call-site INSTANTIATION of the definition's scheme (fresh
/// variables per call, so polymorphic definitions stay polymorphic), and its
/// leading `Thunk` captures are exactly the formal parameters that
/// [`TypedProgram::fn_param_tys`] records.
///
/// A formal that resolves to a type VARIABLE is a generalized parameter
/// (`id x = x`): it accepts any argument, and unifying it here would pin the
/// definition to the first call site's type. Such a formal DEFERS — argument
/// inference is skipped entirely, leaving the previous behaviour intact.
///
/// Two rill value-track widenings stay accepted, because a buffer is an ordinary
/// value and integer literals are `fromInteger`-polymorphic in Haskell 2010:
/// a numeric formal takes a `Buffer` argument (and a `Buffer` formal takes any
/// value), and a `Double` formal takes a bare `Int` literal. Everything else goes
/// through [`unify_value`], whose failure is reported as an error naming the
/// definition and the argument position.
fn check_def_call_args(
    ctx: &mut Ctx,
    name: &str,
    args: &[Expr],
    fn_ty: &ValueTy,
) -> Result<(), CompileError> {
    let formals = match fn_ty {
        ValueTy::Thunk(..) => flatten_thunk(fn_ty).0,
        _ => return Ok(()),
    };
    // An explicit signature's parameter types are the types the user wrote, so
    // they are checked exactly. A signature-less definition's parameter types
    // are inferred from its body, where numeric defaulting pins an otherwise
    // unconstrained operand to `Double`; that `Double` stands for a
    // `Num`-polymorphic parameter and tolerates an `Int` argument.
    let defaulted = !ctx.sig_defs.contains(name);
    for (i, (a, formal)) in args.iter().zip(formals.iter()).enumerate() {
        let formal = ctx.subst.resolve_value(formal);
        if matches!(formal, ValueTy::Var(_) | ValueTy::TyConVar(_)) {
            // Polymorphic parameter: defer (see above).
            continue;
        }
        let actual = infer_argument_ty(ctx, a, &formal)?;
        // A BARE named function reference (`twice d 3.0`) types as the empty
        // `Thunk([], [])` until it is applied: its arity and signature resolve
        // only at ITS call site. There is no type to compare, so the argument
        // defers — rejecting it would refuse every higher-order call that passes
        // a named function, which is valid Haskell 2010.
        if is_unknown_thunk(&actual) {
            continue;
        }
        if def_arg_widens_to(&formal, &actual, a, defaulted) {
            continue;
        }
        if let Err(e) = unify_value(&actual, &formal, &mut ctx.subst, a.span()) {
            return Err(CompileError::Type {
                msg: format!(
                    "argument {} of `{name}` has type {}, but the parameter has type {}: {e}",
                    i + 1,
                    ctx.env.type_spelling(&actual),
                    ctx.env.type_spelling(&formal)
                ),
                span: a.span(),
            });
        }
    }
    Ok(())
}

/// Whether `t` is the empty `Thunk([], [])` that a BARE named definition
/// reference carries: its arity and parameter types are resolved when it is
/// applied, so it has no signature to check against a formal parameter.
fn is_unknown_thunk(t: &ValueTy) -> bool {
    matches!(t, ValueTy::Thunk(caps, res) if caps.is_empty() && res.is_empty())
}

/// Whether an actual argument is accepted by a formal parameter WITHOUT
/// unification — the value-track widenings documented on
/// [`check_def_call_args`].
fn def_arg_widens_to(formal: &ValueTy, actual: &ValueTy, arg: &Expr, defaulted: bool) -> bool {
    match (formal, actual) {
        // A buffer is an ordinary value: it flows into a numeric parameter (the
        // signal-constant / buffer-arithmetic path) and any value flows into a
        // `Buffer` parameter.
        (ValueTy::Buffer(_), _) => true,
        (ValueTy::Double | ValueTy::Int, ValueTy::Buffer(_)) => true,
        // `f 2` against `Double -> …`: an integer literal is `fromInteger`-
        // polymorphic (Haskell 2010 §6.4.1), so it inhabits `Double` too. A
        // `Double` literal against an `Int` parameter is NOT (there is no
        // `fromRational` narrowing), which is the mismatch this rejects.
        (ValueTy::Double, ValueTy::Int) if matches!(arg, Expr::Int(_, _)) => true,
        _ if defaulted => numeric_default_widens(formal, actual),
        _ => false,
    }
}

/// `Double` positions accept `Int` positions, structurally.
///
/// A body whose numeric operations default an unconstrained type variable
/// through the primitive `Num Double` instance (`sum2 p = p.first + p.second`
/// schemes as `Pair Double Double -> Double`) has no `Num`-polymorphic scheme yet
/// — that is the dictionary work this task's stage leads to. `Pair Int Int` is
/// nonetheless valid Haskell 2010 at that call site (`sum2 :: Num a => Pair a a
/// -> a`), so rejecting it would be the check being wrong, not the program.
///
/// The relation is DIRECTIONAL: a `Double` parameter tolerates an `Int`
/// argument, never the reverse. `f x :: Int -> Int = x` applied to `1.5`
/// therefore still fails, which is the mismatch the brief requires.
fn numeric_default_widens(formal: &ValueTy, actual: &ValueTy) -> bool {
    match (formal, actual) {
        (ValueTy::Double, ValueTy::Double | ValueTy::Int) => true,
        (ValueTy::Int, ValueTy::Int) => true,
        (ValueTy::Bool, ValueTy::Bool) | (ValueTy::String, ValueTy::String) => true,
        (ValueTy::Void, ValueTy::Void) => true,
        (ValueTy::Data(f, fa), ValueTy::Data(a, aa))
        | (ValueTy::Newtype(f, fa), ValueTy::Newtype(a, aa))
        | (ValueTy::App(f, fa), ValueTy::App(a, aa)) => {
            f == a
                && fa.len() == aa.len()
                && fa
                    .iter()
                    .zip(aa.iter())
                    .all(|(f, a)| numeric_default_widens(f, a))
        }
        (ValueTy::Thunk(fa, fr), ValueTy::Thunk(aa, ar)) => {
            fa.len() == aa.len()
                && fr.len() == ar.len()
                && fa
                    .iter()
                    .zip(aa.iter())
                    .chain(fr.iter().zip(ar.iter()))
                    .all(|(f, a)| numeric_default_widens(f, a))
        }
        _ => false,
    }
}

/// Infer an argument's value type for the user-definition argument check.
///
/// The formal type is threaded as the argument's EXPECTED type when the
/// argument cannot type itself — a result-directed method (`return x`, `pure
/// x`, `mempty`) needs a container — and when the formal is a FUNCTION type,
/// where the parameter and result types are what the check compares. Threading
/// it unconditionally would retype an integer literal in a `Double` parameter
/// position (`sum2 (Pair { first: 1, … })`) and reject a program that the
/// `Num`-polymorphic reading accepts, so plain inference comes first.
fn infer_argument_ty(ctx: &mut Ctx, e: &Expr, formal: &ValueTy) -> Result<ValueTy, CompileError> {
    if matches!(formal, ValueTy::Thunk(..)) {
        return infer_method_value_vty_expected(ctx, e, "argument", Some(formal.clone()));
    }
    let plain = infer_method_value_vty(ctx, e, "argument");
    match plain {
        Ok(t) => Ok(t),
        Err(plain_err) => {
            match infer_method_value_vty_expected(ctx, e, "argument", Some(formal.clone())) {
                Ok(t) => Ok(t),
                // Report the ORIGINAL failure: it is the one that names the real
                // problem in the argument, not a follow-on expected-type mismatch.
                Err(_) => Err(plain_err),
            }
        }
    }
}

/// Apply a single argument to a callee value type (unary application): `f : Thunk([a], [b])`,
/// `x : a` ⇒ `f x : b`. An unresolved callee `Var` is bound to a fresh unary
/// `Thunk([p], [r])`.
fn apply_one(
    ctx: &mut Ctx,
    callee: &ArrowTy,
    arg: &ArrowTy,
    name: &str,
    span: Span,
) -> Result<ArrowTy, CompileError> {
    match ctx.subst.resolve_value(&callee.vty) {
        ValueTy::Thunk(caps, res) => {
            let param = caps.first().cloned().unwrap_or(ValueTy::Double);
            unify_value(&arg.vty, &param, &mut ctx.subst, span)?;
            Ok(ArrowTy::value(
                res.first().cloned().unwrap_or(ValueTy::Double),
            ))
        }
        ValueTy::Var(_) => {
            let p = ctx.fresh_vty();
            let r = ctx.fresh_vty();
            unify_value(
                &callee.vty,
                &ValueTy::Thunk(vec![p.clone()], vec![r.clone()]),
                &mut ctx.subst,
                span,
            )?;
            unify_value(&arg.vty, &p, &mut ctx.subst, span)?;
            Ok(ArrowTy::value(r))
        }
        _ => Err(CompileError::Type {
            msg: format!("`{name}` is not a function value"),
            span,
        }),
    }
}

/// Infer a collection-operation call (`length`, `cons`, `head`, `tail`, `map`,
/// `fold`, `filter`, `list`, `insert`, `lookup`, `member`, `empty_map`,
/// `empty_set`): validates the argument arity and container types, returning
/// the result value type. Mirrors the lowerer's `value_builtin_ty` signatures
/// (Task 6.3).
fn infer_collection_call(
    ctx: &mut Ctx,
    name: &str,
    args: &[Expr],
    span: Span,
) -> Result<ValueTy, CompileError> {
    let arity_err = |expected: &str, got: usize| CompileError::Type {
        msg: format!("`{name}` expects {expected} argument(s), got {got}"),
        span,
    };
    let arg_vty = |ctx: &mut Ctx, i: usize| -> Result<ValueTy, CompileError> {
        match args.get(i) {
            Some(e) => infer_const_value(ctx, e),
            None => Err(arity_err(&(i + 1).to_string(), args.len())),
        }
    };
    // Enforce the `Ord` constraint on Map/Set keys (and Set elements): every
    // concrete key type must have a derived `Ord` instance (see
    // `derive_eq_ord`). `Func` gets no instance, so a function-typed key is a
    // compile error; an unresolved type var cannot select an instance either.
    let check_ord = |ctx: &mut Ctx, ty: &ValueTy, what: &str| -> Result<(), CompileError> {
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
    let expect_closure = |ctx: &mut Ctx, want: usize, what: &str| -> Result<(), CompileError> {
        let ft = arg_vty(ctx, 0)?;
        // A closure value is a curried NEST of single-argument thunks
        // (`\a -> \b -> …`), so its total arity comes from flattening the chain.
        let ok = match &ft {
            ValueTy::Thunk(..) => {
                let (arg_tys, _) = flatten_thunk(&ft);
                arg_tys.is_empty() || arg_tys.len() == want
            }
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            Err(CompileError::Type {
                msg: format!("`{name}` expects a {what} function"),
                span,
            })
        }
    };
    // The element type of a `List` value type; Double for a non-list.
    let list_elem = |t: &ValueTy| -> ValueTy {
        match t {
            ValueTy::App(n, inner) if n == "List" => {
                inner.first().cloned().unwrap_or(ValueTy::Double)
            }
            _ => ValueTy::Double,
        }
    };
    // `length`-style guards: a bulk list op's SOURCE must be a `List`. Without
    // this, `map f 1.0` / `fold f z 1.0` reached the interpreter, whose
    // `pooled_elems_of` returned `None` and the op answered an empty list (read
    // as `0` downstream) instead of being rejected.
    //
    // An unresolved type variable is CONSTRAINED to `List a` rather than
    // rejected, so a generic helper (`go xs = map f xs`) still types.
    let require_list = |ctx: &mut Ctx, idx: usize, what: &str| -> Result<ValueTy, CompileError> {
        let t = arg_vty(ctx, idx)?;
        match ctx.subst.resolve_value(&t) {
            ValueTy::App(n, inner) if n == "List" => {
                Ok(inner.first().cloned().unwrap_or(ValueTy::Double))
            }
            ValueTy::Var(_) => {
                let elem = ctx.fresh_vty();
                let list = ValueTy::App("List".into(), vec![elem.clone()]);
                unify_value(&list, &t, &mut ctx.subst, args[idx].span())?;
                Ok(elem)
            }
            other => Err(CompileError::Type {
                msg: format!("`{name}` expects a List as its {what}, got {other:?}"),
                span: args[idx].span(),
            }),
        }
    };
    let list_of = |t: &ValueTy| ValueTy::App("List".into(), vec![list_elem(t)]);
    match name {
        "length" => {
            if args.len() != 1 {
                return Err(arity_err("1", args.len()));
            }
            // `length :: [a] -> Int` (Prelude). A non-collection has no
            // length, so it must be a TYPE error rather than a silent `0`:
            // the old arm discarded the argument's type entirely, which is how
            // `length (mappend 1.0 2.0)` — a Double — answered 0.
            //
            // `Map`/`Set`/`Array` are rill's other sized containers and keep
            // their documented `length` (the interpreter counts their entries).
            //
            // An UNRESOLVED variable is left unconstrained: several containers
            // have a length, so pinning it to `List a` here would make a later
            // use that names another one (`length arr` before `arr` is known to
            // be an `Array`) a spurious unification failure.
            let at = arg_vty(ctx, 0)?;
            match ctx.subst.resolve_value(&at) {
                ValueTy::App(n, _) if n == "List" => {}
                ValueTy::App(n, _) if n == "Map" || n == "Set" || n == "Array" => {}
                ValueTy::Var(_) => {}
                other => {
                    return Err(CompileError::Type {
                        msg: format!("`length` expects a List, got {other:?}"),
                        span: args[0].span(),
                    })
                }
            }
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
            // Constrain the argument to a List so a generic function parameter
            // (`nth k xs = … head xs …`) unifies to List (a bare Var would
            // otherwise stay unconstrained and fail lowering's List check).
            let lt = arg_vty(ctx, 0)?;
            let fresh = ctx.fresh_vty();
            unify_value(
                &lt,
                &ValueTy::App("List".into(), vec![fresh]),
                &mut ctx.subst,
                args[0].span(),
            )?;
            Ok(ValueTy::App("Maybe".into(), vec![list_elem(&lt)]))
        }
        "tail" => {
            if args.len() != 1 {
                return Err(arity_err("1", args.len()));
            }
            let lt = arg_vty(ctx, 0)?;
            let fresh = ctx.fresh_vty();
            unify_value(
                &lt,
                &ValueTy::App("List".into(), vec![fresh]),
                &mut ctx.subst,
                args[0].span(),
            )?;
            Ok(list_of(&lt))
        }
        "map" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            // Infer the source list FIRST so its element type is known, then
            // infer the closure with the element threaded as its parameter
            // type (`map : (a -> b) -> List a -> List b`). Without this, a
            // lambda over a `List Buffer` (`fn x -> x * 0.5` over a Buffer)
            // types `x` as a bare Var and the body's Buffer ops conflict.
            // The source must be a List: a non-list is a type error, never an
            // empty result.
            let elem_ty = require_list(ctx, 1, "source")?;
            let fresh_ret = ctx.fresh_vty();
            let ft0 = infer_expr_expected(
                ctx,
                &args[0],
                Some(ValueTy::Thunk(vec![elem_ty], vec![fresh_ret])),
            )?;
            // The closure must take exactly one element argument. Flatten the
            // curried nest so a binary closure (`\a -> \b -> b`) is rejected and
            // the result element type is the chain's FINAL result, not the
            // leftover inner closure.
            let ft = ctx.subst.resolve_value(&ft0.vty);
            let (closure_args, closure_rets) = match &ft {
                ValueTy::Thunk(..) => flatten_thunk(&ft),
                _ => (Vec::new(), Vec::new()),
            };
            if !closure_args.is_empty() && closure_args.len() != 1 {
                return Err(CompileError::Type {
                    msg: "`map` expects a unary function".to_string(),
                    span: args[0].span(),
                });
            }
            let ret_ty = closure_rets.first().cloned().unwrap_or(ValueTy::Double);
            Ok(ValueTy::App("List".into(), vec![ret_ty]))
        }
        "fold" => {
            if args.len() != 3 {
                return Err(arity_err("3", args.len()));
            }
            // The accumulator type and the source element type are known
            // BEFORE the closure, so the closure can be typed against
            // `fold : (b -> a -> b) -> b -> List a -> b`. That expected
            // signature is what lets a method used as a value dispatch to its
            // instance (`fold mappend "" xs` → `Monoid String`) and what
            // rejects a mismatched closure (`fold (++) [] [1.0,2.0]`,
            // ill-typed in Haskell 2010, was silently producing no result).
            let zt = arg_vty(ctx, 1)?;
            let elem = require_list(ctx, 2, "source")?;
            let want = nest_thunk(&[zt.clone(), elem], vec![zt.clone()]);
            let got = infer_expr_expected(ctx, &args[0], Some(want.clone()))?;
            // The closure must be binary. Flatten the curried nest so a unary
            // or ternary closure is rejected here rather than at the runtime
            // call (which would pass the wrong number of operands).
            //
            // A bare METHOD reference resolves to the instance body's type
            // (body order), not the class signature's, so only an actual
            // `Thunk` chain is arity-checked — the value is what matters, and
            // the unification below pins it.
            let got_ty = ctx.subst.resolve_value(&got.vty);
            if let ValueTy::Thunk(..) = got_ty {
                let (c_args, _) = flatten_thunk(&got_ty);
                if !c_args.is_empty() && c_args.len() != 2 {
                    return Err(CompileError::Type {
                        msg: "`fold` expects a binary function".to_string(),
                        span: args[0].span(),
                    });
                }
            }
            // A named definition — and a method used as a value — resolves
            // through its own scheme/instance and does not consume the expected
            // type, so the conformance check has to be an explicit unification.
            // This is what rejects `fold (++) [] [1.0,2.0]` (ill-typed in
            // Haskell 2010; it used to produce no result, which `length`-
            // wrapping read as `0`).
            //
            // Both sides are FLATTENED first: a class-signature parameter is a
            // flat `Thunk([a, b], [r])` while a lambda literal is a curried nest
            // `Thunk([a], [Thunk([b], [r])])`, and the two denote the same
            // function. Comparing the raw shapes would reject the flat spelling
            // (`instance Foldable List where { foldr f z xs = fold f z xs }`).
            let (want_args, want_rets) = flatten_thunk(&want);
            let (got_args, got_rets) = flatten_thunk(&got_ty);
            if got_args.len() != want_args.len() {
                return Err(CompileError::Type {
                    msg: "`fold` expects a binary function".to_string(),
                    span: args[0].span(),
                });
            }
            for (g, w) in got_args.iter().zip(want_args.iter()) {
                unify_value(g, w, &mut ctx.subst, args[0].span())?;
            }
            for (g, w) in got_rets.iter().zip(want_rets.iter()) {
                unify_value(g, w, &mut ctx.subst, args[0].span())?;
            }
            Ok(zt)
        }
        "filter" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            expect_closure(ctx, 1, "unary")?;
            let elem = require_list(ctx, 1, "source")?;
            Ok(ValueTy::App("List".into(), vec![elem]))
        }
        "concat_map" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            expect_closure(ctx, 1, "unary")?;
            let elem = require_list(ctx, 1, "source")?;
            let ft0 = arg_vty(ctx, 0)?;
            let ft = ctx.subst.resolve_value(&ft0);
            // See `concat_map_result_elem`: `(a -> List b) -> List a -> List b`.
            let b = concat_map_result_elem(&ft, elem);
            Ok(ValueTy::App("List".into(), vec![b]))
        }
        "append_list" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            // `(++) :: [a] -> [a] -> [a]`. The first argument must be a list;
            // an UNRESOLVED type variable (the callee reached here through a
            // first-class reference, e.g. `fold (++) xs xss`) is constrained to
            // `List a` rather than rejected — that is what makes `++` usable as
            // an ordinary function value instead of only in infix position.
            let lt = arg_vty(ctx, 0)?;
            let elem = match ctx.subst.resolve_value(&lt) {
                ValueTy::App(n, inner) if n == "List" => {
                    inner.first().cloned().unwrap_or(ValueTy::Double)
                }
                ValueTy::Var(_) => {
                    let elem = ctx.fresh_vty();
                    let list = ValueTy::App("List".into(), vec![elem.clone()]);
                    unify_value(&list, &lt, &mut ctx.subst, args[0].span())?;
                    elem
                }
                // `(++) :: [a] -> [a] -> [a]`: a non-list FIRST operand is a
                // type error, not an empty result.
                other => {
                    return Err(CompileError::Type {
                        msg: format!(
                            "`++` expects a List as its first operand \
                             (`(++) :: [a] -> [a] -> [a]`), got {other:?}"
                        ),
                        span: args[0].span(),
                    })
                }
            };
            // The SECOND operand is constrained to the SAME list type. Without
            // this, `list ++ 1.0` reached the runtime and answered `None`
            // (which downstream code read as `0`) instead of being rejected,
            // and `fold (++) [] [1.0,2.0]` — ill-typed in Haskell 2010 —
            // silently produced no result.
            let rt = arg_vty(ctx, 1)?;
            let list_ty = ValueTy::App("List".into(), vec![elem.clone()]);
            unify_value(&list_ty, &rt, &mut ctx.subst, args[1].span())?;
            Ok(list_ty)
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
            Ok(ValueTy::App("List".into(), vec![ValueTy::Double]))
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
                    inner.get(1).cloned().unwrap_or(ValueTy::Double)
                }
                _ => ValueTy::Double,
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
                vec![ValueTy::String, ValueTy::Double],
            ))
        }
        "empty_set" => {
            if !args.is_empty() {
                return Err(arity_err("0", args.len()));
            }
            Ok(ValueTy::App("Set".into(), vec![ValueTy::Double]))
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
    Ok(ArrowTy::uniform(0, 1, ValueTy::Double))
}

fn infer_arith(
    ctx: &mut Ctx,
    op: crate::ast::ArithOp,
    lhs: &Expr,
    rhs: &Expr,
    span: Span,
) -> Result<ArrowTy, CompileError> {
    let a = infer_expr(ctx, lhs)?;
    let b = infer_expr(ctx, rhs)?;
    // Signal arithmetic: at least one operand is a `Buffer` (a wire, a
    // combinator output, a foreign call). A numeric literal / constant on the
    // OTHER side is then a SIGNAL constant — unify it with `Buffer` so
    // `_ * 0.5` types `0.5 : Buffer`, not a value `Double`.
    let a_buffer = matches!(a.vty, ValueTy::Buffer(_));
    let b_buffer = matches!(b.vty, ValueTy::Buffer(_));
    if a_buffer || b_buffer {
        // A numeric literal / constant on the non-buffer side is a SIGNAL
        // constant: `_ * 0.5` types `0.5 : Buffer`. Re-infer it with `Buffer`
        // as the expected channel type so the literal unifies as a buffer.
        let a = if !a_buffer && is_signal_const(ctx, &a, lhs) {
            infer_expr_expected(ctx, lhs, Some(ctx.env.buffer_vty()))?
        } else {
            a
        };
        let b = if !b_buffer && is_signal_const(ctx, &b, rhs) {
            infer_expr_expected(ctx, rhs, Some(ctx.env.buffer_vty()))?
        } else {
            b
        };
        return arith(ctx, &a, &b, span);
    }
    // Value-track arithmetic: a value operand (a lambda parameter, a value
    // function, a field projection, ...) combined with another value operand
    // or a bare Double/Int literal lowers on the value track and yields a Double
    // value channel — this is what makes lambda bodies like `fn x -> x * 2.0`
    // meaningful. A value channel mixed with a genuine signal computation
    // (`sin _`, a wire, a combinator) is still rejected.
    let a_value = !matches!(a.vty, ValueTy::Buffer(_));
    let b_value = !matches!(b.vty, ValueTy::Buffer(_));
    if a_value || b_value {
        // A value operand must be a numeric scalar (Double/Int, or an as-yet
        // unresolved Var such as a lambda parameter). Func values, records and
        // newtypes cannot take part in arithmetic (spec: no arithmetic on
        // closures); a bare Double/Int literal or a reference to a constant
        // definition (`k = 2.0`) is value-compatible — the latter is a common
        // lambda free variable, captured by value into the closure env.
        let scalar_vty =
            |v: &ValueTy| matches!(v, ValueTy::Double | ValueTy::Int | ValueTy::Var(_));
        let mut unify_operand = |t: &ArrowTy, e: &Expr| -> Result<(), CompileError> {
            if t.arity_in() == 0 && t.arity_out() == 1 {
                if scalar_vty(&t.vty) {
                    // A structural parameter variable in arithmetic is a
                    // numeric scalar: bind it to Double so the lambda's
                    // Func signature resolves concretely (`fn x -> x * 2`
                    // types `x` as Double, not an unconstrained Var).
                    if matches!(&t.vty, ValueTy::Var(_)) {
                        unify_value(&t.vty, &ValueTy::Double, &mut ctx.subst, e.span())?;
                    }
                    return Ok(());
                } else if matches!(e, Expr::Int(_, _) | Expr::Double(_, _))
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
        // Int-preserving value arithmetic: `+`/`-`/`*` over two `Int` operands
        // stays on the Int track, so an `Int`-typed slot (an `Ix` offset, an
        // array size) really holds a `Value::Int` at runtime. `/` is float
        // division and always yields a `Double`; `%` is unsupported. A free Var
        // operand was just bound to `Double` above, so mixed arithmetic and
        // lambda parameters keep the old Double behavior. Lowering and the
        // interpreter apply the same rule.
        let a_ty = ctx.subst.resolve_value(&a.vty);
        let b_ty = ctx.subst.resolve_value(&b.vty);
        let int_track = matches!(
            op,
            crate::ast::ArithOp::Add | crate::ast::ArithOp::Sub | crate::ast::ArithOp::Mul
        ) && matches!(a_ty, ValueTy::Int)
            && matches!(b_ty, ValueTy::Int);
        return Ok(ArrowTy::value_channel(if int_track {
            ValueTy::Int
        } else {
            ValueTy::Double
        }));
    }
    arith(ctx, &a, &b, span)
}

/// Whether `e` types as a SIGNAL constant: a 0→1 arrow whose single output is
/// a numeric scalar (Double/Int or an unresolved Var) and whose expression is a
/// numeric literal or a reference to a compile-time constant (`k = 2.0`). In
/// signal arithmetic such an operand unifies with `Buffer` (`_ * 0.5`).
fn is_signal_const(ctx: &Ctx, t: &ArrowTy, e: &Expr) -> bool {
    matches!(t.vty, ValueTy::Double | ValueTy::Int | ValueTy::Var(_))
        && (matches!(e, Expr::Int(_, _) | Expr::Double(_, _)) || is_const_value_ref(ctx, e))
}

fn arith(ctx: &mut Ctx, a: &ArrowTy, b: &ArrowTy, span: Span) -> Result<ArrowTy, CompileError> {
    reject_value_channels(a, span)?;
    reject_value_channels(b, span)?;
    unify_value(&a.vty, &b.vty, &mut ctx.subst, span)?;
    Ok(ArrowTy::value(a.vty.clone()))
}

fn check_all_numeric(_ctx: &mut Ctx, _t: &ArrowTy, _span: Span) -> Result<(), CompileError> {
    Ok(())
}

/// Reject value-rate output channels from signal combinators.
///
/// Value channels are the value-track's arena values (records, sums, func
/// values); composing one through `:`, `<:`, `:>`, `~`, `@`, or arithmetic has
/// no block representation — a bare func value (`f = double` used as a signal)
/// would otherwise typecheck as an Int block and panic in lowering. Value
/// programs are single expressions, so this never rejects a valid program.
/// Unresolved `Var`s pass (they unify with `Buffer` when the combinator
/// connects to a buffer channel).
fn reject_value_channels(t: &ArrowTy, span: Span) -> Result<(), CompileError> {
    if !matches!(t.vty, ValueTy::Buffer(_) | ValueTy::Var(_)) {
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
    use crate::ast::TypeExpr;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn ty_of(src: &str) -> Result<TypedProgram, CompileError> {
        infer_program(
            &parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap(),
            Mode::default(),
        )
    }

    #[test]
    fn instantiate_freshens_the_scheme_constraint_ids() {
        // A scheme's class context must travel WITH its instantiation: the
        // recorded variable has to be the FRESHENED one the instantiated type
        // carries, otherwise two uses of one polymorphic scheme would share a
        // variable id and the second call site's solution would silently
        // overwrite the first's dictionary.
        let scheme = Scheme {
            lam_count: 1,
            vars: vec![7],
            constraints: vec![("Ix".to_string(), 7)],
            ty: ArrowTy {
                vty: ValueTy::Thunk(vec![ValueTy::Var(7)], vec![ValueTy::Bool]),
            },
        };
        let mut ctx = Ctx {
            next: 0,
            subst: Subst::default(),
            defs: HashMap::new(),
            def_bodies: HashMap::new(),
            locals: HashMap::new(),
            env: TypeEnv::with_builtins(),
            method_lifting: HashSet::new(),
            type_var_bindings: HashMap::new(),
            retry_inferred_defs: HashSet::new(),
            sig_defs: HashSet::new(),
            mode: Mode::Strict,
            runst_payload_tys: HashMap::new(),
            nested_group_depth: 0,
            given: Vec::new(),
            dict_selections: HashMap::new(),
        };
        let (ty, constraints) = ctx.instantiate_with_constraints(&scheme);
        assert_eq!(constraints.len(), 1, "the context travels with the type");
        assert_eq!(constraints[0].0, "Ix");
        let fresh = constraints[0].1;
        assert_ne!(fresh, 7, "the constraint id is freshened, not the scheme's");
        assert!(
            vty_mentions_tyvar(&ty.vty, fresh),
            "the freshened constraint variable {:?} must occur in the instantiated \
             type {:?}",
            fresh,
            ty.vty
        );
        assert!(
            !vty_mentions_tyvar(&ty.vty, 7),
            "the scheme's own id must not survive instantiation: {:?}",
            ty.vty
        );
        // Two instantiations are independent.
        let (_, again) = ctx.instantiate_with_constraints(&scheme);
        assert_ne!(
            again[0].1, fresh,
            "each instantiation gets its own fresh constraint variable"
        );
    }

    #[test]
    fn only_a_bare_class_var_result_is_selected_by_expected_type() {
        // A `mempty: m`-shaped result IS the class variable, so a call site's
        // expected type is its value — the "result-directed" solve. A result
        // that merely MENTIONS the variable (`wrap: Double -> Pair a a`) is an
        // APPLICATION of it: taking the expected type as the solved value would
        // dispatch on the wrong key, so that shape must not qualify.
        let expected_shape =
            |ret: TypeExpr| TypeExpr::TFunc(vec![TypeExpr::TName("Double".into())], Box::new(ret));
        let pair_of_var = TypeExpr::TApp(
            "Pair".into(),
            vec![TypeExpr::TName("a".into()), TypeExpr::TName("a".into())],
        );
        assert!(
            sig_result_is_bare_class_var(&TypeExpr::TName("a".into()), "a"),
            "a bare `a` signature is the class variable itself"
        );
        assert!(
            sig_result_is_bare_class_var(&expected_shape(TypeExpr::TName("a".into())), "a"),
            "a function returning the bare class variable is result-directed too"
        );
        assert!(
            !sig_result_is_bare_class_var(&expected_shape(pair_of_var), "a"),
            "`Double -> Pair a a` only MENTIONS the class variable"
        );
        assert!(
            !sig_result_is_bare_class_var(
                &TypeExpr::TFunc(
                    vec![TypeExpr::TName("Double".into())],
                    Box::new(TypeExpr::TName("b".into())),
                ),
                "a"
            ),
            "a different type variable is not the class variable"
        );
        assert!(
            sig_result_is_bare_class_var(
                &TypeExpr::TConstrained(
                    vec![("Eq".into(), "a".into())],
                    Box::new(TypeExpr::TName("a".into())),
                ),
                "a"
            ),
            "a constraint wrapper is transparent"
        );
    }

    #[test]
    fn io_main_writes_io_void() {
        // The IO form (`main = do { … }`) is the program's `IO ()` action; the
        // runner applies it to the realized `World`.
        let t = ty_of("main = do { write Nothing stdout; return () }").unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Data("IO".into(), vec![ValueTy::Void])
        );
    }

    #[test]
    fn data_strictness_flows_into_data_info() {
        // The Haskell 2010 strictness flag parsed by the data declaration must
        // reach the type environment, because ctor lowering reads `DataInfo`
        // (never the AST) when deciding where to force.
        let t = ty_of(
            "data T = C !Double; data P = { x: !Double, y: Double }; main = P { x: 1.0, y: 2.0 }",
        )
        .unwrap();
        match t.type_env.data_types.get("T") {
            Some(DataInfo::Sum(ctors)) => {
                assert_eq!(ctors.len(), 1);
                assert_eq!(ctors[0].0, "C");
                assert_eq!(ctors[0].1, vec![(ValueTy::Double, true)]);
            }
            other => panic!("expected Sum for T, got {other:?}"),
        }
        match t.type_env.data_types.get("P") {
            Some(DataInfo::Record(fields)) => {
                assert_eq!(
                    fields,
                    &vec![
                        ("x".to_string(), ValueTy::Double, true),
                        ("y".to_string(), ValueTy::Double, false),
                    ]
                );
            }
            other => panic!("expected Record for P, got {other:?}"),
        }
    }

    #[test]
    fn gain_is_1_to_1() {
        // A buffer-valued signal expression over a literal generator.
        let t = ty_of(r"main = sine 440.0 0.5 0.0 * 0.5;").unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Buffer(Box::new(ValueTy::Double))
        );
    }

    #[test]
    fn integrator_is_1_to_1() {
        // A foreign 1→1 builtin applied to a buffer value.
        let t = ty_of("main = integrator (sine 440.0 0.5 0.0)").unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Buffer(Box::new(ValueTy::Double))
        );
    }

    #[test]
    fn user_def_alias_resolves() {
        let t = ty_of("main x = gain x where { gain y = y * 0.5; }").unwrap();
        assert!(matches!(t.def_schemes["main"].ty.vty, ValueTy::Thunk(ref c, _) if c.len() == 1));
    }

    #[test]
    fn function_application_ok() {
        let t = ty_of("main x = g x where { g y = y * 0.5; }").unwrap();
        assert!(matches!(t.def_schemes["main"].ty.vty, ValueTy::Thunk(ref c, _) if c.len() == 1));
    }

    #[test]
    fn where_mutual_visibility() {
        let t = ty_of("main x = a x where { a y = y * 0.5; b y = a y; }").unwrap();
        assert!(matches!(t.def_schemes["main"].ty.vty, ValueTy::Thunk(ref c, _) if c.len() == 1));
    }

    #[test]
    fn top_level_mutual_visibility() {
        let t = ty_of("a x = x * 0.5; main x = a x").unwrap();
        assert!(matches!(t.def_schemes["main"].ty.vty, ValueTy::Thunk(ref c, _) if c.len() == 1));
    }

    #[test]
    fn top_level_reverse_order() {
        let t = ty_of("main x = g x; g y = y * 0.5").unwrap();
        assert!(matches!(t.def_schemes["main"].ty.vty, ValueTy::Thunk(ref c, _) if c.len() == 1));
    }

    #[test]
    fn let_expression_scoping() {
        let t = ty_of("main = let g = 0.5 in g * 0.5").unwrap();
        assert_eq!(t.def_schemes["main"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn let_not_visible_outside() {
        assert!(ty_of("main x = let g = 0.5 in g * x; other = g").is_err());
    }

    fn ty_with(src: &str) -> Result<TypedProgram, CompileError> {
        infer_program_with(
            &parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap(),
            Mode::default(),
        )
    }

    #[test]
    fn builtin_call_is_1_to_1() {
        let t = ty_with("main = lowpass 1000.0 0.7 (sine 440.0 0.5 0.0)").unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Buffer(Box::new(ValueTy::Double))
        );
    }

    #[test]
    fn builtin_wrong_param_count_errors() {
        // Partial application is allowed (currying), but over-application past
        // the builtin's declared arity is a compile error.
        assert!(ty_with("main x = lowpass 1000.0 0.7 x 1.0").is_err());
    }

    #[test]
    fn builtin_ref_param_ok() {
        assert!(ty_with("lp f = lowpass f 0.7 (sine 440.0 0.5 0.0); main = lp 1000.0").is_ok());
    }

    #[test]
    fn sample_builtin_applies_in_value_position() {
        assert!(ty_with("main = onepole 200.0 0.5 (sine 440.0 0.5 0.0)").is_ok());
    }

    #[test]
    fn transitive_var_chain_resolves() {
        let t = ty_of("main x = g x where { f y = y * 0.5; g y = f 0.5; }").unwrap();
        assert!(matches!(t.def_schemes["main"].ty.vty, ValueTy::Thunk(ref c, _) if c.len() == 1));
    }

    #[test]
    fn scalar_param_broadcasts_to_buffer() {
        let t = ty_of("main = sine 440.0 0.5 0.0 * 0.5").unwrap();
        // A buffer-valued main is the program's single signal output.
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Buffer(Box::new(ValueTy::Double))
        );
    }

    #[test]
    fn annotated_scheme_parameter_types_come_from_the_signature() {
        // Direct scheme check (white-box): `Double`-based parameters in the
        // scheme would make the definition's advertised type disagree with the
        // body that was checked against the signature.
        fn scheme_args(src: &str) -> Vec<ValueTy> {
            let prog = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
            let mut ctx = Ctx {
                next: 0,
                subst: Subst::default(),
                defs: HashMap::new(),
                def_bodies: HashMap::new(),
                locals: HashMap::new(),
                env: TypeEnv::with_builtins(),
                method_lifting: HashSet::new(),
                type_var_bindings: HashMap::new(),
                retry_inferred_defs: HashSet::new(),
                sig_defs: HashSet::new(),
                mode: Mode::Strict,
                runst_payload_tys: HashMap::new(),
                nested_group_depth: 0,
                given: Vec::new(),
                dict_selections: HashMap::new(),
            };
            let def = &prog.defs[0];
            let body = infer_def_body(&mut ctx, def).unwrap();
            let lam_count = effective_def_params(def).0.len();
            match def_scheme(&mut ctx, def, &body, lam_count).unwrap().0 {
                ValueTy::Thunk(args, _) => args,
                other => panic!("expected a function scheme, got {other:?}"),
            }
        }
        // A single parameter: the scheme used to advertise `Double` while the
        // body was typed with `x : Int`.
        assert_eq!(
            scheme_args("inc x :: Int -> Int = x + 1;\nmain = 0.0;"),
            vec![ValueTy::Int]
        );
        // Multiple parameters: the parser curries them away, so the scheme used
        // to carry no signature types at all.
        assert_eq!(
            scheme_args("f a b :: Int -> Int -> Int = a + b;\nmain = 0.0;"),
            vec![ValueTy::Int, ValueTy::Int]
        );
    }

    #[test]
    fn forall_wrapped_constraint_context_defers_like_a_bare_one() {
        // Haskell 2010 spells the quantified form `forall i. C i => T`, which
        // puts the class context UNDER the binder. Both definition sites must
        // look through it: registration (so the body is typed under the given
        // constraint and specialized at call sites) and the eager inference
        // path that recognizes it.
        let sig = TypeExpr::TForall(
            vec!["i".into()],
            Box::new(TypeExpr::TConstrained(
                vec![("Ix".into(), "i".into())],
                Box::new(TypeExpr::TFunc(
                    vec![TypeExpr::TApp(
                        "Pair".into(),
                        vec![TypeExpr::TName("i".into()), TypeExpr::TName("i".into())],
                    )],
                    Box::new(TypeExpr::TName("Double".into())),
                )),
            )),
        );
        assert!(is_constrained_sig(&sig));
        assert_eq!(sig_constraints(&sig), [("Ix".to_string(), "i".to_string())]);
        // Registration recovers the formal parameters from the signature's
        // arity, exactly as for a bare `C i => T` signature.
        let def = Def::Eval {
            name: "mk".into(),
            params: vec![],
            body: Expr::Lambda {
                params: vec![crate::ast::Param {
                    name: "p".into(),
                    span: Span::new(0, 0),
                    bang: false,
                }],
                body: Box::new(Expr::Ref("rangeSize".into(), Span::new(0, 0))),
                span: Span::new(0, 0),
            },
            where_defs: vec![],
            sig: Some(sig),
            bang: false,
            span: Span::new(0, 0),
        };
        let mut env = TypeEnv::with_builtins();
        env.register_decls(std::slice::from_ref(&def));
        let cd = env
            .constrained_defs
            .get("mk")
            .expect("a forall-wrapped class context must register as constrained");
        assert_eq!(cd.params.len(), 1, "the λ-parameter is recovered");
    }

    #[test]
    fn int_float_mismatch_errors() {
        // A `Double` field does not unify with an `Int` field type.
        assert!(ty_of("data P = { f: Int }; main = P { f: 1.0 }").is_err());
    }

    #[test]
    fn actor_param_no_default_typechecks() {
        let t = ty_of(r"main = if ?gain > 0.5 then 1.0 else 2.0").unwrap();
        assert_eq!(t.def_schemes["main"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn actor_param_with_default_typechecks() {
        // A `?name=default` actor param in a Buffer position keeps the
        // program's single signal output.
        let t = ty_of(r"main = sine 440.0 ?gain=0.5 0.0").unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Buffer(Box::new(ValueTy::Double))
        );
    }

    #[test]
    fn closed_top_level_local_is_caf() {
        // osc = sine 440 0.5 0  (0 input channels, 0 λ-params) -> CAF
        let typed = ty_with("osc = sine 440 0.5 0; main = 0.5 * 0.5").unwrap();
        assert!(typed.cafs.contains("osc"));
    }

    #[test]
    fn open_block_is_not_caf() {
        // gain x = x * 0.5  (has a λ-param) -> macro, not CAF
        let typed = ty_with("gain x = x * 0.5; main = gain 0.5").unwrap();
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
        let t = ty_of("data Point = { x: Double, y: Double }; main = Point { x: 1.0, y: 2.0 }")
            .unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Data("Point".into(), vec![])
        );
    }

    #[test]
    fn field_project_produces_float_value() {
        let t = ty_of(
            "data Point = { x: Double, y: Double }; p = Point { x: 1.0, y: 2.0 }; main = p.x",
        )
        .unwrap();
        assert_eq!(t.def_schemes["main"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn sum_match_branches_on_constructor() {
        let t = ty_of(
            r"data Shape = Circle Double | Rect Double Double; main = case Circle 1.0 of { Circle r -> r; Rect w h -> w; }",
        )
        .unwrap();
        assert_eq!(t.def_schemes["main"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn field_update_preserves_record_type() {
        let t = ty_of(
            "data Point = { x: Double, y: Double }; p = Point { x: 1.0, y: 2.0 }; main = p.x := 3.0",
        )
        .unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Data("Point".into(), vec![])
        );
    }

    #[test]
    fn match_arms_must_be_value_expressions() {
        // A bare literal arm body is value-compatible (`Circle r -> 1.0`);
        // arms are value channels, so a bare literal is accepted.
        assert!(ty_of(
            r"data Shape = Circle Double; main = case Circle 1.0 of { Circle r -> 1.0 }"
        )
        .is_ok());
    }

    #[test]
    fn match_scrutinee_must_be_value() {
        assert!(ty_of(r"data Shape = Circle Double; main = case 3 of { Circle r -> r }").is_err());
    }

    #[test]
    fn match_arms_same_sum_type() {
        assert!(
            ty_of(r"data A = C Double; data B = C Int; main = case C 1.0 of { C r -> r }").is_err()
        );
    }

    #[test]
    fn match_too_many_arm_bindings_is_error() {
        assert!(ty_of(
            r"data Shape = Circle Double; main = case Circle 1.0 of { Circle r s -> r }"
        )
        .is_err());
    }

    #[test]
    fn guarded_match_arm_type_checks_and_is_non_exhaustive_alone() {
        // A guarded ctor arm (`Circle r | r > 0.0 => r`) type-checks its guard
        // as a Bool value, but a guarded arm does NOT cover its constructor for
        // exhaustiveness: without a wildcard the match must fail loudly, not
        // silently drop the guard.
        let res = ty_of(
            r"data Shape = Circle Double | Rect Double Double; main = case Circle 1.0 of { Circle r | r > 0.0 -> r; };",
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
            r"data Shape = Circle Double | Rect Double Double; main = case Circle 1.0 of { Circle r | r > 0.0 -> r; _ -> 0.0; };"
        )
        .is_ok());
        // A non-Bool guard is a type error.
        let res = ty_of(
            r"data Shape = Circle Double | Rect Double Double; main = case Circle 1.0 of { Circle r | r + 1.0 -> r; _ -> 0.0; };",
        );
        assert!(
            res.is_err(),
            "a non-Bool guard must be a compile error, not silently compiled"
        );
    }

    #[test]
    fn record_ctor_missing_field_is_error() {
        assert!(ty_of("data Point = { x: Double, y: Double }; main = Point { x: 1.0 }").is_err());
    }

    #[test]
    fn record_ctor_duplicate_field_is_error() {
        assert!(ty_of(
            "data Point = { x: Double, y: Double }; main = Point { x: 1.0, x: 2.0, y: 3.0 }"
        )
        .is_err());
    }

    #[test]
    fn if_infers_bool_condition_and_branch_types() {
        let t = ty_of("main = if 1.0 < 2.0 then 1.0 else 2.0").unwrap();
        assert_eq!(t.def_schemes["main"].ty.vty, ValueTy::Double);
        assert!(ty_of("main = if 1.0 then 1.0 else 2.0").is_err());
        assert!(ty_of("main = if 1.0 < 2.0 then 1 else 2").is_ok());
        let t2 = ty_of("main = if 1.0 < 2.0 then 1 else 2").unwrap();
        assert_eq!(t2.def_schemes["main"].ty.vty, ValueTy::Int);
    }

    #[test]
    fn scalar_match_infers_literal_patterns_and_totality() {
        let t = ty_of(r"main = case 0 of { 0 -> 1.0; _ -> 2.0; }").unwrap();
        assert_eq!(t.def_schemes["main"].ty.vty, ValueTy::Double);
        assert!(ty_of(r"main = case 0 of { 0 -> 1.0 }").is_err());
        let t2 = ty_of(r"main = case true of { true -> 1.0; false -> 2.0 }").unwrap();
        assert_eq!(t2.def_schemes["main"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn bare_int_literal_scrutinee_infers_as_value() {
        // `match 0 of {...}`: a bare Int literal is a signal-rate channel in v1
        // but value-compatible in value positions (mirroring arm_result_vty).
        let t = ty_of(r"main = case 0 of { 0 -> 1.0; _ -> 2.0; }").unwrap();
        assert_eq!(t.def_schemes["main"].ty.vty, ValueTy::Double);
        let t2 = ty_of(r"main = case 0.0 of { 0.0 -> 1; _ -> 2; }").unwrap();
        assert_eq!(t2.def_schemes["main"].ty.vty, ValueTy::Int);
    }

    #[test]
    fn nested_cross_sum_ctor_pattern_infers() {
        // `Just (Left x)`: the nested `Left` constructor belongs to `Either`,
        // NOT `Maybe`. The sum context must be re-derived from the payload type
        // at each pattern level.
        let t = ty_of(r"main = case Just (Left 2.0) of { Just (Left x) -> x; _ -> 0.0; }").unwrap();
        assert_eq!(t.def_schemes["main"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn nested_user_sum_ctor_pattern_infers() {
        // Same nesting for a USER sum: `Wrap (Foo x)` — `Foo` is looked up in
        // `Inner`, not `Outer`.
        let t = ty_of(
            r"data Inner = Foo Double; data Outer = Wrap Inner | Empty; main = case Wrap (Foo 2.0) of { Wrap (Foo x) -> x; _ -> 0.0; }",
        )
        .unwrap();
        assert_eq!(t.def_schemes["main"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn type_after_data_resolves() {
        // Regression: the synonym is declared AFTER the data type that uses it.
        // Registration must be order-independent (two-phase: aliases first).
        let t = ty_of("data P = { x: Angles }; type Angles = Double; main = P { x: 1.0 }").unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Data("P".into(), vec![])
        );
    }

    #[test]
    fn newtype_after_data_resolves() {
        // The newtype is declared AFTER the data type that uses it. The field
        // must resolve to `Newtype("Hz")` (not `Data("Hz")`), so constructing
        // it requires the explicit `Hz 440.0` wrapper.
        let t = ty_of("data P = { f: Hz }; newtype Hz = Double; main = P { f: Hz 440.0 }").unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Data("P".into(), vec![])
        );
        // Newtypes are distinct wrappers: a bare Double does not satisfy a Hz
        // field (no automatic wrapping) — this is what the ordering fix buys
        // (a pre-fix `Data("Hz")` field would accept nothing, not even `Hz 440.0`).
        assert!(ty_of("data P = { f: Hz }; newtype Hz = Double; main = P { f: 440.0 }").is_err());
    }

    #[test]
    fn chained_alias_resolves() {
        let t =
            ty_of("data P = { f: A }; type A = B; type B = Double; main = P { f: 1.0 }").unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Data("P".into(), vec![])
        );
    }

    #[test]
    fn recursive_data_type_is_compile_error() {
        // A self-referential type would materialise an unbounded arena subtree
        // and exhaust the fixed capacity (spec §9.1 strict acyclicity).
        assert!(
            ty_of("data List = Cons Double List | End Double; main = Cons 1.0 (End 2.0)").is_err()
        );
    }

    #[test]
    fn recursive_newtype_is_compile_error() {
        assert!(ty_of("newtype A = B; newtype B = A; main = A 1.0").is_err());
    }

    #[test]
    fn acyclic_data_type_still_compiles() {
        let t = ty_of("data Point = { x: Double, y: Double }; main = Point { x: 1.0, y: 2.0 }")
            .unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Data("Point".into(), vec![])
        );
    }

    #[test]
    fn lambda_def_infers_as_func() {
        // A bare Def::Eval whose body is a lambda literal infers to a Func value
        // channel via the normal def-body inference path.
        let t = ty_of(r"double = \x -> x; main = double").unwrap();
        assert!(matches!(t.def_schemes["main"].ty.vty, ValueTy::Thunk(_, _)));
    }

    #[test]
    fn lambda_body_value_tuple_is_pair() {
        // `(x , x)` with a value λ-param is a channel tuple → `Pair`, a single
        // 0→1 value channel, so the lambda body is well-typed (the tuple no
        // longer parses as two signal channels).
        let t = ty_of(r"main = \x -> (x , x)").unwrap();
        match &t.def_schemes["main"].ty.vty {
            ValueTy::Thunk(_, ret) => {
                assert_eq!(ret.len(), 1);
                match &ret[0] {
                    ValueTy::App(name, _) => {
                        assert_eq!(name.as_str(), "Pair");
                    }
                    other => panic!("expected Pair result, got {other:?}"),
                }
            }
            other => panic!("expected Func type, got {other:?}"),
        }
    }

    #[test]
    fn value_channel_arithmetic_infers() {
        // double = fn x -> x * 2.0  -> lambda body is value arithmetic over
        // value params, inferred as Func([Double], [Double]).
        let t = ty_of(r"double = \x -> x * 2.0; main = double").unwrap();
        match &t.def_schemes["main"].ty.vty {
            ValueTy::Thunk(arg_tys, ret_tys) => {
                assert_eq!(arg_tys.len(), 1);
                assert_eq!(ret_tys, &vec![ValueTy::Double]);
            }
            other => panic!("expected Func type, got {other:?}"),
        }
    }

    #[test]
    fn partial_application_types_as_remaining_func() {
        // add3 = add 3.0 (partial application of a two-arg lambda) types as
        // Func([Double], [Double]); fully applying it yields a Double.
        let t = ty_of(r"add = \a -> \b -> a + b; add3 = add 3.0; main = add3").unwrap();
        assert_eq!(
            t.def_schemes["main"].ty.vty,
            ValueTy::Thunk(vec![ValueTy::Double], vec![ValueTy::Double])
        );
        let t = ty_of(r"add = \a -> \b -> a + b; add3 = add 3.0; main = add3 4.0").unwrap();
        assert_eq!(t.def_schemes["main"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn named_def_partial_application_types_as_func() {
        // add5 = add2 5.0 (partial application of a named def) types as
        // Func([Double], [Double]).
        let t = ty_of("add2 a b = a + b; add5 = add2 5.0; main = add5 2.0").unwrap();
        assert_eq!(t.def_schemes["main"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn opaque_type_is_nameable_but_not_constructible() {
        // `data World;` declares a nameable type: a field/type-alias may
        // mention it — but nothing in source can CONSTRUCT one. `World {}` is
        // a compile-time error ("no constructors").
        let err = match ty_with("data World; main = World {}") {
            Ok(_) => panic!("constructing an opaque type must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string()
                .contains("opaque type `World` has no constructors"),
            "unexpected error: {err}"
        );
        assert!(
            ty_with("data World; data Box = { w: World }; main = 1.0").is_ok(),
            "an opaque type must be nameable in a type position"
        );
        assert!(
            ty_with("data World; type Alias = World; main = 1.0").is_ok(),
            "an opaque type must be usable through a type alias"
        );
    }

    #[test]
    fn opaque_type_has_no_projection_or_patterns() {
        // An opaque value has no fields to project and no constructors to
        // match against: both are compile-time errors. The record wrapper
        // pins the binder's type so the projection/match is not deferred.
        let err = match ty_with(
            "data World; data Box = { w: World }; name b = let v = b.w in v.name; main = 1.0",
        ) {
            Ok(_) => panic!("projecting a field of an opaque type must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string()
                .contains("opaque type `World` has no fields"),
            "unexpected error: {err}"
        );
        let err = match ty_with(
            "data W; data Box = { w: W }; first b = let v = b.w in case v of { W -> 1.0 }; main = 1.0",
        ) {
            Ok(_) => panic!("matching an opaque type's nonexistent constructor must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string()
                .contains("opaque type `W` cannot be matched"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn self_application_type_is_rejected() {
        // `f f` — self-application unifies `f`'s type with a `Func` whose
        // argument IS `f`, which would construct an infinite type. The
        // occurs-check must reject it with a clean type error (regression:
        // it recorded a cyclic binding and overflowed the stack).
        let res = ty_of(r"main = \f -> f f");
        assert!(res.is_err());
    }
}
