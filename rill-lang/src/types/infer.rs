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
    ///
    /// The key is the call's span **together with the class whose method is
    /// called**. A span alone is not unique: in `0 == (x + 0)` the outer `==`
    /// and the inner `+` are the same desugared operator token, so the two
    /// selections share a span. Keying on span alone let the inner insert
    /// overwrite the outer one, and lowering then fell back to its own coarse
    /// static type for the literal selector — a silent wrong `Bool`. Keying on
    /// both keeps every enclosing selection.
    pub dict_selections: HashMap<(Span, String), ValueTy>,
    /// The value type inference gave each LITERAL, keyed by its source span
    /// (`start`, `end`).
    ///
    /// A numeric literal is `fromInteger (n :: Integer)` / `fromRational (r ::
    /// Rational)` (Haskell 2010 §6.4.1), so the type it lowers at is the one its
    /// class variable finally took — by unification with the context, by
    /// instance selection, or by defaulting. Inference decides that ONCE, in its
    /// three stages; this map is that decision, so lowering converts to it
    /// instead of re-deriving a representation from the position.
    pub literal_tys: HashMap<(usize, usize), ValueTy>,
    /// The type each literal was DECIDED to have at its own span, recorded when
    /// unification settled its class variable (see `record_literal_instance`).
    /// Lowering reads it to convert the exact payload to the right type.
    pub literal_instances: HashMap<(usize, usize), ValueTy>,
    /// The RESOLVED value type of every expression, keyed by its span. Task 10's
    /// single channel from inference to lowering; see `Ctx::value_tys`.
    pub value_tys: HashMap<(usize, usize), ValueTy>,
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
    /// (`Array i e` from the constrained `(!)`) — shapes the user never
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
    /// True while inferring a body whose context is NOT a definition signature:
    /// an INSTANCE method body (its context is the instance head) or the
    /// compiler-owned PRELUDE. The §4.1.3 signature-context check does not apply
    /// to either — the prelude's own signatures are validated when it is loaded
    /// (`TypeEnv::register_prelude`), and an instance's bodies are typed under
    /// the instance's context.
    in_instance_body: bool,
    /// True while inferring the body of a definition that DECLARES a `::`
    /// signature. Only such a body has a context the §4.1.3 check can violate;
    /// an unsigned binding's context is INFERRED (GHC: `f (Just x) | x > 2.0 =
    /// …` compiles with `Ord a` inferred).
    in_signed_body: bool,
    /// The dictionary the type checker selected at each method call position,
    /// keyed by the call's span and the class whose method is called (see
    /// [`TypedProgram::dict_selections`]).
    dict_selections: HashMap<(Span, String), ValueTy>,
    /// The class constraints inference has not discharged yet — **stage 2's
    /// backlog**, spent when the enclosing binding group closes.
    ///
    /// Every numeric literal contributes one (`Num a` for `fromInteger`,
    /// `Fractional a` for `fromRational`: Haskell 2010 §6.4.1), and so does a
    /// method call whose class variable nothing has solved yet. Each entry names
    /// the class, the type variable its class variable resolved to, and the use
    /// site.
    ///
    /// Its two consumers are the stages this module separates explicitly:
    ///
    /// * **stage 2 (discharge)** — a variable unification has since made
    ///   CONCRETE must have an instance, and a missing one is reported as
    ///   `no instance of \`C\` for type \`T\`` (never as a unification failure:
    ///   unification is what made it concrete, so it succeeded);
    /// * **stage 3 (default)** — a variable STILL unresolved at a binding's
    ///   close, not fixed by the group's own types and carrying only standard
    ///   numeric classes, takes `default (Integer, Double)`.
    residual: Vec<Residual>,
    /// The type variable each numeric literal's `fromInteger`/`fromRational`
    /// produced, keyed by the literal's source span.
    ///
    /// A span is inferred many times in one program — a selector operand is
    /// typed by the dispatcher and again by the application rule, and a body is
    /// inferred twice per definition group — so the LAST write is the
    /// authoritative one: it is the variable the final substitution carries and
    /// the one stages 2 and 3 spend. Recording the VARIABLE (not a type) is what
    /// lets the three stages answer after the fact; [`literal_types`] is only
    /// consulted once they have run.
    literal_vars: HashMap<(usize, usize), TypeVarId>,
    /// The type each literal's class variable was DECIDED to have, keyed by the
    /// literal's source span. [`literal_vars`] names the variable; this names
    /// the answer once a stage or a unification settled it.
    literal_instances: HashMap<(usize, usize), ValueTy>,
    /// The RESOLVED value type of every expression, keyed by its span — Task
    /// 10's single channel from inference to lowering. Written by
    /// `infer_expr_expected`; read by nothing yet (adoptive phase).
    value_tys: HashMap<(usize, usize), ValueTy>,
    /// The program's ENTRY name (Haskell 2010 §5.1 gives it `IO t`).
    ///
    /// A Ctx field, not a hardcoded `"main"`, because `main` is an ordinary
    /// identifier everywhere else: [`compile_eval`] compiles an expression
    /// under the internal marker `__rill_eval` and documents that a `main`
    /// binding in its `defs` is an ordinary value. Keying the contract on the
    /// NAME would give that value an `IO t` type it never asked for.
    ///
    /// [`compile_eval`]: crate::compile_eval
    entry_name: String,
    /// Whether inference is in a definition group's FINAL pass.
    ///
    /// [`infer_def_group_inner`] types every body twice — first with placeholder
    /// schemes (to recover each definition's shape), then again with the real
    /// schemes (so signal port counts are right). Only the second pass's type
    /// variables are the ones the finished program carries, and only it can see
    /// the schemes of the definitions it calls. Literal typing is therefore
    /// recorded ONLY in that pass; see [`Ctx::literal_vars`].
    final_pass: bool,
}

/// One undischarged class constraint (see [`Ctx::residual`]).
#[derive(Debug, Clone)]
struct Residual {
    /// The class whose dictionary the constraint demands.
    class: String,
    /// The type variable its class variable resolved to.
    var: TypeVarId,
    /// The use site, for the diagnostic.
    span: Span,
    /// Whether the constraint was demanded by a NUMERIC LITERAL.
    ///
    /// A literal is not a dictionary USE. Haskell 2010 §6.4.1 gives it the
    /// meaning `fromInteger (n :: Integer)` / `fromRational (r :: Rational)`,
    /// and both payloads are exact and class-free: the digits the source wrote
    /// need no `Num`, `Fractional`, `Eq` or `Ord` — the ratio's numerator and
    /// denominator are `Integer` by `Ratio`'s own declaration. The class
    /// variable of such a constraint therefore decides only WHICH instance the
    /// CONVERSION targets, and lowering reads that from
    /// `TypedProgram::literal_tys` (the kernel payload) — never from the
    /// dictionary table. So the constraint must still be CHECKED (a literal
    /// unified with `List b` is `Fractional (List Integer)`: no instance, an
    /// error), but it must not create a table entry, or `main = 1.0` would
    /// claim four dictionaries that nothing reads.
    literal: bool,
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
            // Every class variable is witnessed by its ID occurring in the type,
            // whatever the class's arity. `m a` is `TyConApp(m, [a])` and `a` is
            // `Var(a)`, so one rule covers `Monad m`, `Functor f` and `Ix i` alike
            // (Haskell 2010 §4.1.2 does not distinguish a kind-`*` variable from a
            // kind-`* -> *` one).
            Some(id) => vty_mentions_tyvar(t, id),
            // A variable the signature never recorded has no id to match, so
            // fall back to the NAME the signature spells.
            None => matches!(
                t,
                ValueTy::Con(n, _) | ValueTy::Newtype(n, _) if *n == self.name
            ),
        }
    }

    /// The abstract type this given stands for, in the spelling the signature
    /// conversion gives the class variable: a bare variable id for an arity-0
    /// class, the named application head for a constructor class.
    fn abstract_ty(&self) -> ValueTy {
        match self.id {
            // The class variable as a TYPE VARIABLE. `ClassVarSubst::Ty` then
            // splices its own arguments, so `m a` becomes `TyConApp(m, [a])` and
            // unifies with `Maybe Double` by binding `m := Maybe`.
            Some(id) => ValueTy::Var(id),
            None => ValueTy::Con(self.name.clone(), Vec::new()),
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
    span: Span,
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
    let witness = given_witness(&ctx.given, &ctx.env, class_name, &at).cloned();
    if witness.is_none() {
        // No witness for this class. If the operand is ABSTRACT the body is
        // using a method its signature's context never granted — the §4.1.3
        // restriction, reported rather than silently dispatched (OD-54).
        require_context_witness(ctx, class_name, &at, span)?;
    }
    Ok(witness)
}

/// Whether a call of a name that is BOTH a class method and a class-constrained
/// definition routes to the METHOD because the body's GIVEN constraint witnesses
/// its class variable.
///
/// No PRELUDE name is dual any more: `!` is the array accessor and `index` the
/// pure `Ix` offset method, so each has one role and nothing has to guess. The
/// routing remains the general rule for any user definition that shadows a
/// method name with a constrained definition.
///
/// The test is the method's own class-var PATTERN (`Pair i i` for `Ix`), not
/// mere mention: a signature whose class variable also appears in a container
/// argument (`C i => Array i e -> i -> e`) must not have that argument's type
/// read as the class variable, or the pattern would match a shape the method
/// does not accept.
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
        // A constructor-class given (`Functor f`) carries no variable id, so it
        // can never witness an arity-0 class method's variable.
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
    let ret = ret.unwrap_or_else(|| ctx.fresh_vty());
    apply_args_curried(ctx, &param_tys, &ret, args, span, "method", ArgUnify::Exact)
}

/// How [`apply_args_curried`] unifies one argument against its parameter.
///
/// The distinction is a property of the CALL, not of the application rule.
/// Two call families read a numeric LITERAL in a `Buffer` parameter position as
/// that buffer's constant — the spelling lowering's `buffer_operand` emits:
/// an operator/method operand (`sine 440.0 0.5 0.0 * 0.5`, where `0.5` fills
/// the block) and a foreign builtin's signal slot (`arrayFromBuffer 1`, where
/// `1` is the block's constant). Both are Haskell 2010 §6.4.1
/// `fromInteger`-polymorphism: the literal takes the type its position demands.
///
/// Every OTHER application demands the argument inhabit the parameter exactly,
/// and `((\b -> b) 1.0) :: Buffer` is rejected as GHC rejects it — a
/// non-literal does NOT widen into a `Buffer` anywhere. Carrying the choice as
/// a parameter keeps the shared rule's arity/currying judgement in ONE place.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgUnify {
    /// The argument's type must be the parameter's type, exactly
    /// ([`unify_value`]).
    Exact,
    /// A numeric literal in a `Buffer` parameter position is that buffer's
    /// constant ([`literal_fills_buffer`]) — the operator/method path and the
    /// builtin signature catalog.
    BufferConstant,
}

/// THE application rule's ARITY judgement, split out so a caller can reject a
/// surplus argument before doing its own signature conversion — the order each
/// site's diagnostics had before the rule was shared.
fn check_application_arity(
    expected: usize,
    got: usize,
    span: Span,
    what: &str,
) -> Result<(), CompileError> {
    if got > expected {
        return Err(CompileError::Type {
            msg: format!("{what} expects {expected} argument(s), got {got}"),
            span,
        });
    }
    Ok(())
}

/// THE application rule: apply a function type — `params` and `ret`, already
/// converted from a signature — to a call's `args`.
///
/// Haskell 2010 §4.3.3: a function type is curried, so an application's first
/// judgement is its arity. Fewer arguments than `params` is a PARTIAL
/// application and yields the curried remainder (`a -> b -> r` applied to one
/// argument is `b -> r`); exactly `params.len()` is a FULL application and
/// yields `ret` — never the whole arrow; more arguments than parameters is a
/// compile error naming the arity.
///
/// This is the application half of every SIGNATURE-driven call site in this
/// module: the primitive-method paths, the source-instance dispatch, a method
/// or constructor used as a first-class value, the builtin signature catalog,
/// the runtime closure application, and a constrained definition's call.
/// `what` is the subject the arity diagnostic names (`"expression"`,
/// ``"`f`"``, ``"method `fromInteger` of `Num`"``, ``"constructor `Just`"``),
/// which is what keeps each site's existing wording byte-identical.
///
/// Callers keep their own signature conversion and substitution work: the
/// parameters and result arrive already converted, and only the application
/// half is shared.
///
/// Two families stay outside it, because their arity is not a signature's:
/// user definitions and closure values count λ-parameters (`check_def_call_args`
/// / `applied_result`), and the collection catalog carries its own
/// per-operation arities.
///
/// The result type is returned RESOLVED through the substitution, so a caller
/// that pinned the result to an expected type (`pin_result_to_expected`, or a
/// signature whose variables the arguments just solved) sees the concrete type
/// it will produce, not the variable that stood for it.
fn apply_args_curried(
    ctx: &mut Ctx,
    params: &[ValueTy],
    ret: &ValueTy,
    args: &[Expr],
    span: Span,
    what: &str,
    rule: ArgUnify,
) -> Result<ArrowTy, CompileError> {
    check_application_arity(params.len(), args.len(), span, what)?;
    // Every supplied argument is unified against its parameter, so the call
    // site — not the result — solves the signature's type variables.
    //
    // A parameter typed `Buffer` also accepts a signal computation
    // (`fp i1` where `fp : Buffer -> Buffer` and `i1` is the program's Buffer
    // input) as well as a Buffer value, so that one case is checked through the
    // expression's own type instead of being re-inferred at the parameter type.
    //
    // The bare inference is ATTEMPTED, never propagated: a result-directed
    // argument with no argument of its own to solve the class variable (`zero`,
    // `mempty` — `sine 440.0 0.5 0.0 + zero`) only types under the expected
    // type the parameter supplies, so a failed bare inference must fall through
    // to the expected-type path instead of aborting the application.
    for (a, pt) in args.iter().zip(params.iter()) {
        if matches!(ctx.subst.resolve_value(pt), ValueTy::Buffer(_)) {
            if let Ok(at) = infer_expr(ctx, a) {
                if at.arity_out() == 1 && matches!(at.vty, ValueTy::Buffer(_)) {
                    unify_value(&at.vty, pt, &mut ctx.subst, a.span())?;
                    continue;
                }
            }
        }
        let at = infer_method_value_vty_expected(ctx, a, "argument", Some(pt.clone()))?;
        // A LITERAL argument's class variable is its `fromInteger` /
        // `fromRational` target, and this unification just decided it. Record
        // the decision AT ITS OWN SPAN so lowering converts the exact payload to
        // the type the enclosing expression needed — not to a default it would
        // otherwise pick. Haskell 2010 folds the conversion into a constant at
        // compile time (`x :: Double; x = 1` is `D# 1.0##`); rill keeps the
        // exact payload and converts at run time, so the TARGET is the thing it
        // must remember, and the argument's parameter type is exactly that.
        if let Expr::IntegerLit(..) | Expr::FractionalLit(..) = a {
            record_literal_instance(ctx, a.span());
        }
        match rule {
            // An ordinary application: the argument must inhabit the parameter,
            // exactly. In particular a numeric literal does NOT widen into a
            // `Buffer` parameter here (`((\b -> b) 1.0) :: Buffer` is rejected,
            // as GHC rejects it). The argument's own type is the LEFT operand so
            // the diagnostic reads "cannot unify value type Double with Int" —
            // the mismatch's subject first, as every routed site reported it.
            ArgUnify::Exact => unify_value(&at, pt, &mut ctx.subst, a.span())?,
            // A `Buffer`-constant position: a numeric literal here is the
            // buffer's constant, exactly what lowering's `buffer_operand`
            // emits (`sine 440.0 0.5 0.0 * 0.5`, `arrayFromBuffer 1`). The
            // ARGUMENT stays the left operand, so the diagnostic keeps each
            // caller's `cannot unify value type Double with Int` ordering.
            ArgUnify::BufferConstant => {
                if literal_fills_buffer(ctx, pt, &at, a) {
                    continue;
                }
                unify_value(&at, pt, &mut ctx.subst, a.span())?;
            }
        }
    }
    // A partial application keeps the remaining parameters as a function value
    // (Haskell 2010 §4.3.3); a full application yields the result. Both are
    // resolved through the substitution, so the caller sees the concrete call
    // type rather than the variables the arguments just solved.
    let ret = ctx.subst.resolve_value(ret);
    let rest: Vec<ValueTy> = params[args.len()..]
        .iter()
        .map(|t| ctx.subst.resolve_value(t))
        .collect();
    if rest.is_empty() {
        Ok(ArrowTy::value_channel(ret))
    } else {
        Ok(ArrowTy::value(nest_thunk(&rest, vec![ret])))
    }
}

/// Whether a dispatch's evidence is only the selector's HEAD CONSTRUCTOR name —
/// the `method_dispatch_key` fallback — rather than a solved class variable.
///
/// The fallback exists so a structurally DERIVED instance (`Eq Array`, keyed by
/// the head `Array`) answers every `Array i e`. For a class whose instances are
/// keyed by the FULL type (`Ix`, keyed by `Pair Int Int`) the same fallback
/// names a type no instance exists for, and the class variable is the unsolved
/// thing the caller must still default.
fn cv_subst_is_head_only(cv: &ClassVarSubst, head: &str) -> bool {
    matches!(cv, ClassVarSubst::Ctor(c, _) if c == head)
}

/// The concrete type a dispatch selected, for [`record_selection`]. A
/// constructor-class dispatch (`ClassVarSubst::Ctor`) names the constructor
/// only — its head arguments come from the call site and are not part of the
/// selection — so it is recorded as the bare application.
fn selection_evidence(cv: &ClassVarSubst) -> ValueTy {
    match cv {
        ClassVarSubst::Ty(t) => t.clone(),
        // The applied form, so the evidence describes the whole type:
        // `Ctor("Kleisli", [Maybe])` is `Kleisli Maybe`, which is what a
        // constraint-parameterised instance is keyed against.
        ClassVarSubst::Ctor(c, lead) => ValueTy::Con(c.clone(), lead.clone()),
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
/// Recording is keyed by the call's span AND the class whose method is called.
/// The span alone is not unique — an inner and an outer operator desugar to the
/// same token span (`0 == (x + 0)`), so keying on the span alone silently
/// dropped one of the two selections. Nothing is written for a position with no
/// witness: a call whose class variable neither the given context witnesses nor
/// a concrete type solves is a typing error before this point.
/// Record the type a LITERAL's class variable finally has, read at its own
/// span.
///
/// A literal is `fromInteger`/`fromRational` applied to an exact payload, so its
/// "instance" is decided like any other class variable — by unification from
/// every direction. When the position has just decided it, the answer is stored
/// here so lowering converts the payload to THAT type instead of re-deriving a
/// default from a coarser static type.
fn record_literal_instance(ctx: &mut Ctx, span: Span) {
    let Some(&id) = ctx.literal_vars.get(&(span.start, span.end)) else {
        return;
    };
    let solved = ctx.subst.resolve_value(&ValueTy::Var(id));
    if !matches!(
        solved,
        ValueTy::Var(_) | ValueTy::TyConVar(_) | ValueTy::TyConApp(..)
    ) {
        ctx.literal_instances.insert((span.start, span.end), solved);
    }
}

/// Reject a class method used inside a signature whose CONTEXT does not supply
/// that class.
///
/// Haskell 2010 §4.1.3: a type signature's context restricts the body. Inside
/// `f :: a -> a -> Bool; f x y = x < y` there is no `Ord a` witness, so GHC
/// reports `No instance for Ord a` and tells the author to add the context.
/// rill resolved `(<)` against whatever instance the operand type named and
/// never compared the result with the declared context, so the context was
/// documentation rather than a restriction — which also hid every consequence
/// of the `Num`/`Ord` split (OD-52/OD-54).
///
/// A call is legal when either:
/// - the declared context (or a superclass of it) witnesses the class, or
/// - the operand's static type names a CONCRETE instance, so the call is a
///   call-site dispatch the body is entitled to make (`length` on a real list,
///   `(+)` on `Double` inside a `Num a =>` body, `Eq Int` under `Ord a`).
///
/// Only an ABSTRACT operand with no matching witness is rejected — exactly the
/// case GHC reports.
fn require_context_witness(
    ctx: &Ctx,
    class_name: &str,
    evidence: &ValueTy,
    span: Span,
) -> Result<(), CompileError> {
    // A concrete evidence type needs no witness: an instance exists for it.
    if !matches!(
        evidence,
        ValueTy::Var(_) | ValueTy::TyConVar(_) | ValueTy::TyConApp(..)
    ) {
        return Ok(());
    }
    // An OPEN variable is not evidence of anything: it is an operand whose type
    // a sibling or the signature has yet to fix (`0 == x` with `x :: Integer`
    // types the literal from `x`). Reporting here would reject programs GHC
    // accepts, so this check only judges what the signature has already made
    // abstract-but-known — the case where the variable IS one of the body's own
    // type variables.
    if !ctx
        .given
        .iter()
        .any(|g| matches!(evidence, ValueTy::Var(v) | ValueTy::TyConVar(v) | ValueTy::TyConApp(v, _) if g.id == Some(*v)))
        && !matches!(evidence, ValueTy::Var(v) | ValueTy::TyConVar(v) | ValueTy::TyConApp(v, _) if ctx.subst.rigid_name.contains_key(v))
    {
        return Ok(());
    }
    // The operand is abstract: the class must come from the declared context.
    // `given_witness` already looks through superclasses (`Ord a` witnesses
    // `Eq a`), so it is the one judgement to ask.
    if given_witness(&ctx.given, &ctx.env, class_name, evidence).is_some() {
        return Ok(());
    }
    // Inside an inlined INSTANCE body the class witness comes from the instance
    // head, not from a definition signature, so there is no declared context to
    // violate here.
    if ctx.in_instance_body {
        return Ok(());
    }
    // Only a definition that DECLARES a signature has a context to violate.
    // `f (Just x) | x > 2.0 = …` has no `::`, and GHC INFERS `Ord a` for it
    // (measured: that program compiles and runs), so an unsigned binding is
    // never this error.
    if !ctx.in_signed_body {
        return Ok(());
    }
    Err(CompileError::Type {
        msg: format!(
            "no witness for `{class_name}` in this signature's context: the body uses \
             `{class_name}` at the abstract type `{}`, which no declared constraint \
             supplies (add `{class_name}` to the context)",
            ctx.env.type_spelling(evidence)
        ),
        span,
    })
}

fn record_selection(ctx: &mut Ctx, span: Span, class: &str, evidence: &ValueTy) {
    // UNRESOLVED evidence records nothing. A DEFERRED dispatch (see
    // `infer_ref_expected`) legitimately runs before any stage has fixed the
    // class variable; writing the still-variable type here would later read as
    // a selection at an unknown type. The residual is what carries the demand
    // until a stage answers it.
    if matches!(
        evidence,
        ValueTy::Var(_) | ValueTy::TyConVar(_) | ValueTy::TyConApp(..)
    ) {
        return;
    }
    ctx.dict_selections
        .insert((span, class.to_string()), evidence.clone());
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
            info if info.constructor(ctor).is_some() => Some(tname.clone()),
            _ => None,
        })
        .collect()
}

/// The payload value types of `ctor` within the sum type `sum_name`.
fn sum_ctor_payload(ctx: &Ctx, sum_name: &str, ctor: &str) -> Option<Vec<ValueTy>> {
    ctx.env
        .data_types
        .get(sum_name)?
        .fields_of(ctor)
        .map(|fields| fields.iter().map(|f| f.ty.clone()).collect())
}

/// A sum constructor's CURRIED TYPE, the way Haskell 2010 §4.2.1 gives it:
/// `C :: t1 -> … -> tn -> T a1 … am`. Returned as the application rule's
/// `(params, ret)` pair.
///
/// - `params` are the constructor's PAYLOAD types, with the sum's type
///   parameters bound to those fresh immediately below;
/// - `ret` is the sum applied to FRESH type parameters (`Maybe a`, not the bare
///   `Maybe`), so a constructor used as a value keeps its result's type
///   parameter free.
///
/// A NULLARY constructor has empty `params` and is therefore a plain value
/// (`Nothing :: Maybe a`), by [`apply_args_curried`]'s own rule.
///
/// The sum's declared shapes carry PLACEHOLDER vars (`Var(1)`, `Var(2)` in the
/// builtin `Maybe`/`Either`, and the same ids for a declared `data Opt a`) that
/// are NOT live unification vars. Each is mapped to the positionally
/// corresponding fresh var of `ret`, which is what keeps the payload and the
/// result LINKED: `Cons 1.0 xs` is `t -> List t -> List t`, never two unrelated
/// types.
///
/// A placeholder BEYOND the sum's arity is a FREE type variable in a field of a
/// non-parameterized sum (`data T = C a`, where `a` is not bound by the
/// declaration — GHC rejects that declaration as a type error). Rill registers
/// it anyway, so it must not leak as a live unification variable: the
/// placeholder ids are small integers that ARE valid live ids, so leaving
/// `Var(1)` in place would make two independent uses (`x = C 1.0`, `y = C
/// "s"`) alias one variable and conflict. Each surplus placeholder gets a
/// FRESH var per instantiation instead, so the uses cannot alias.
fn sum_ctor_scheme(ctx: &mut Ctx, sum_name: &str, ctor: &str) -> (Vec<ValueTy>, ValueTy) {
    let payload = sum_ctor_payload(ctx, sum_name, ctor).unwrap_or_default();
    let arity = ctx.env.type_ctor_arity(sum_name).unwrap_or(0);
    let result_args: Vec<ValueTy> = (0..arity).map(|_| ctx.fresh_vty()).collect();
    // One substitution list shared by every payload entry, so a placeholder
    // that appears twice in one constructor still means ONE type. Positions past
    // the sum's own parameters are filled with fresh live vars (see above).
    let mut subst_args = result_args.clone();
    let needed = payload.iter().map(max_placeholder).max().unwrap_or(0) as usize;
    while subst_args.len() < needed {
        subst_args.push(ctx.fresh_vty());
    }
    let params: Vec<ValueTy> = payload
        .iter()
        .map(|p| substitute_field_params(p, &subst_args))
        .collect();
    (params, ValueTy::Con(sum_name.into(), result_args))
}

/// The largest placeholder index a declared field/payload type mentions (`1`
/// for `a`, `2` for `m b` in `data K m a b`), or `0` when it has none. Used to
/// size the substitution list for [`sum_ctor_scheme`].
fn max_placeholder(ty: &ValueTy) -> u32 {
    match ty {
        ValueTy::Var(k) | ValueTy::TyConVar(k) => *k,
        ValueTy::TyConApp(f, xs) => xs.iter().map(max_placeholder).fold(*f, |acc, x| acc.max(x)),
        ValueTy::Con(_, xs) | ValueTy::Newtype(_, xs) => {
            xs.iter().map(max_placeholder).max().unwrap_or(0)
        }
        ValueTy::Thunk(args, rets) => args
            .iter()
            .chain(rets.iter())
            .map(max_placeholder)
            .max()
            .unwrap_or(0),
        ValueTy::Buffer(elem) => max_placeholder(elem),
        _ => 0,
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
            // A synonym HEAD expands to its target (`type L = List` makes
            // `L Int` mean `List Int`), so both spellings denote one type.
            let (head, args) = env.expand_alias_head(head, args.clone());
            let vargs: Vec<ValueTy> = args
                .iter()
                .map(|a| data_field_vty_with_free(env, tyvars, a, free))
                .collect();
            // A buffer CHANNEL constructor is the buffer value type itself
            // (`FixedBuffer elem` → `ValueTy::Buffer(elem)`), not a second
            // `App("FixedBuffer", …)` shape.
            if let Some(buf) = env.channel_ctor_vty(&head, &vargs) {
                return buf;
            }
            // A type PARAMETER applied as a constructor (`m b` in
            // `data K m a b`) becomes a type-constructor application whose head
            // is the parameter's var id. A concrete constructor stays `App`.
            if let Some(k) = tyvars.iter().position(|t| *t == head) {
                ValueTy::TyConApp((k + 1) as u32, vargs)
            } else {
                ValueTy::Con(head.clone(), vargs)
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
                    Some(ValueTy::Con(c, _)) => ValueTy::Con(c, sub_args),
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
            ValueTy::Con(n, a) => ValueTy::Con(n.clone(), a.iter().map(|x| sub(x, args)).collect()),
            ValueTy::Newtype(n, a) => {
                ValueTy::Newtype(n.clone(), a.iter().map(|x| sub(x, args)).collect())
            }
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
            Some(Expr::IntegerLit(_, _)) => return true,
            Some(Expr::FractionalLit(_, _, _)) => return true,
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
        Some(Def::Eval { body, .. }) if leading_lambda_count(body) == 0 => match body {
            Expr::Apply { .. } => Some(name.to_string()),
            Expr::Ref(next, _) => fragment_target(defs, next, seen),
            _ => None,
        },
        Some(def @ Def::Eval { .. }) if def.arity() > 0 => Some(name.to_string()),
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
        // A kernel is a compiler primitive, not a call to a named definition, so
        // it contributes no edge to the static call graph — the same reason a
        // `builtin_sigs` call does not. Its arguments may still contain calls.
        Expr::Kernel { args, .. } => {
            for a in args {
                collect_static_calls(a, src, bound, nodes, out);
            }
        }
        Expr::Ref(_, _)
        | Expr::IntegerLit(_, _)
        | Expr::FractionalLit(_, _, _)
        | Expr::Str(_, _) => {}
        Expr::Neg(inner, _) => collect_static_calls(inner, src, bound, nodes, out),
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
        Expr::Void(_) => {}
        Expr::Annot { expr, .. } => collect_static_calls(expr, src, bound, nodes, out),
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
                    if let Some(g) = g {
                        collect_static_calls(g, src, &inner, nodes, out);
                    }
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
            ValueTy::Con(head, fresh)
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
            Expr::IntegerLit(_, _) => Ok(ValueTy::Int),
            Expr::FractionalLit(_, _, _) => Ok(ValueTy::Double),
            _ => Ok(ctx.env.buffer_vty()),
        },
        vty => Ok(vty.clone()),
    }
}

/// The literal carve-out both `Buffer`-constant positions share: a numeric
/// LITERAL in a `Buffer` parameter position is that buffer's constant, exactly
/// what lowering's `buffer_operand` emits. Haskell 2010 §6.4.1
/// `fromInteger`-polymorphism, read against the `Buffer` the position demands.
///
/// The rule is deliberately NARROW — only a LITERAL, and only when the parameter
/// is a `Buffer`. A non-literal never widens into a `Buffer`
/// (`((\b -> b) 1.0) :: Buffer` is rejected, as GHC rejects it), and a literal
/// never stands in for a different SCALAR type (`1 == 1.0` stays an error).
fn literal_fills_buffer(ctx: &Ctx, param: &ValueTy, actual: &ValueTy, arg: &Expr) -> bool {
    matches!(arg, Expr::IntegerLit(_, _) | Expr::FractionalLit(_, _, _))
        && matches!(ctx.subst.resolve_value(param), ValueTy::Buffer(_))
        && !matches!(ctx.subst.resolve_value(actual), ValueTy::Buffer(_))
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
        ValueTy::Con(_, a) => a.clone(),
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
    /// constructor (`List`, `Kleisli`, `Int`), together with the LEADING
    /// arguments a partial instance head binds.
    ///
    /// The arguments are what make the evidence describe the whole type:
    /// `instance (Monad m) => Arrow (Kleisli m)` is selected for
    /// `Kleisli Maybe b c`, and recording the bare name lost the `Maybe`, so the
    /// dictionary planner saw `Arrow Kleisli` — a pair no instance declares — and
    /// reported no gap for a constraint-parameterised instance it should have
    /// named. `Vec::new()` is the bare head, which is what a fully concrete
    /// instance keys by.
    Ctor(String, Vec<ValueTy>),
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
            }
            // No instance under the full type — either because the registry
            // keys it by the HEAD constructor (a structurally derived
            // `Eq Array` answers every `Array i e`) or because the type still
            // carries variables and therefore HAS no full key
            // (`Array Var(50) Var(51)`, which is what a literal-polymorphic
            // element type makes). Either way the LOOKUP falls back to the head,
            // and the substitution must still be the SOLVED TYPE: the method's
            // signature is then instantiated at the selector's own type, so
            // `(==) :: Array i e -> Array i e -> Bool` keeps the arguments
            // instead of collapsing to `Array` with none. That collapse is what
            // produced `cannot unify Con("Array", [Var(50), Var(51)]) with
            // Con("Array", [])`. Substituting the bare head instead of the
            // solved type would type the parameter as `Con("Array", [])` and
            // reject the real argument.
            if let Some(head) = env.type_name_of_vty(&solved) {
                if env
                    .instances
                    .get(class_name)
                    .map(|by_ty| by_ty.contains_key(head.as_str()))
                    .unwrap_or(false)
                {
                    return Some((head, ClassVarSubst::Ty(solved)));
                }
            }
        }
    }
    let head = env.type_name_of_vty(selector_ty)?;
    // The LEADING arguments the instance head binds: `instance Arrow (Kleisli
    // m)` applied to `Kleisli Maybe b c` binds `m := Maybe`. They travel with the
    // evidence so the dictionary planner can name the pair it cannot build.
    let lead: Vec<ValueTy> = match selector_ty {
        ValueTy::Con(_, args) | ValueTy::Newtype(_, args) => {
            let n = env
                .instances
                .get(class_name)
                .and_then(|m| m.get(head.as_str()))
                .map(|i| i.head_args.len())
                .unwrap_or(0);
            args.iter().take(n).cloned().collect()
        }
        _ => Vec::new(),
    };
    Some((head.clone(), ClassVarSubst::Ctor(head, lead)))
}

/// Whether a call to the class METHOD `name` can be dispatched by
/// `selector_ty` — the FIRST argument's static type.
///
/// This is the routing predicate for a name that is BOTH a class method and a
/// class-constrained definition. No PRELUDE name is dual any more — `!` is the
/// array accessor and `index` the pure `Ix` offset method — but the rule stays
/// for user definitions that shadow a method name: the first argument's type
/// decides, and a value whose type does not solve the method's class variable
/// routes to the definition.
///
/// It is DELIBERATELY stricter than [`method_dispatch_key`]: after solving the
/// class variable it requires a REGISTERED instance under the solved full type
/// and never falls back to the selector's head constructor name. That fallback
/// would "solve" a non-index class for a container selector and route a
/// definition call into the method path, reporting a misleading "no instance
/// of …" instead of reaching the definition.
///
/// Inference and lowering both call this helper on their own recovered
/// first-argument type (`infer_expr` / the lowerer's sandbox probe), so the two
/// passes agree on which name a call means — the invariant the T4 review
/// established for class dispatch.
pub(crate) fn first_arg_solves_class_var(env: &TypeEnv, name: &str, selector_ty: &ValueTy) -> bool {
    fn value_has_var(ty: &ValueTy, var: TypeVarId) -> bool {
        match ty {
            ValueTy::Var(id) | ValueTy::TyConVar(id) => *id == var,
            ValueTy::Con(_, args) | ValueTy::Newtype(_, args) => {
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
    // A quantifier or context quantifies the WHOLE signature, so it is stripped
    // here before the arrow is read. `TypeExpr::sig_arg_count` looks through the
    // same two wrappers, so the parameter list built here and the arity the
    // application rule reports can never disagree (`f: forall b. a -> b -> a` is
    // arity 2, not the single `a -> b -> a` thunk).
    let sig = match sig {
        crate::ast::TypeExpr::TForall(_, inner) => inner,
        crate::ast::TypeExpr::TConstrained(_, inner) => inner,
        other => other,
    };
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
            ClassVarSubst::Ctor(c, lead) => match resolve_type_name(env, c) {
                ValueTy::Con(n, _) | ValueTy::Newtype(n, _) if !lead.is_empty() => {
                    ValueTy::Con(n, lead.clone())
                }
                other => other,
            },
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
                ClassVarSubst::Ctor(ctor, lead) => {
                    // One representation: a constructor the registry knows gets
                    // the one `Con` shape. A name it does not know is not a
                    // constructor at all and resolves nominally. A partial head's
                    // leading arguments go FIRST, because the class variable
                    // stands for the partial application (`a := Kleisli m`).
                    if env.type_ctor_arity(ctor).is_some() {
                        ValueTy::Con(
                            ctor.to_string(),
                            lead.iter().cloned().chain(vargs).collect(),
                        )
                    } else {
                        resolve_type_name(env, ctor)
                    }
                }
                // The class variable is a fully applied type used as a
                // constructor: splice its own arguments ahead of the new ones
                // (arity-0 classes never reach this).
                ClassVarSubst::Ty(t) => match t {
                    ValueTy::Con(n, pre) => {
                        ValueTy::Con(n.clone(), pre.iter().cloned().chain(vargs).collect())
                    }
                    ValueTy::Newtype(n, pre) => {
                        ValueTy::Newtype(n.clone(), pre.iter().cloned().chain(vargs).collect())
                    }
                    // The class variable is a VARIABLE of higher kind (`m` in
                    // `Monad m`): the applied form is `TyConApp(m, args)`, NOT
                    // the bare variable. Returning `other.clone()` here dropped
                    // the arguments, so in a GIVEN context `m a` collapsed to `m`
                    // and unifying it with `m a` became the self-referential
                    // `Var(m) ~ TyConApp(m, [a])`.
                    ValueTy::Var(id) | ValueTy::TyConVar(id) => ValueTy::TyConApp(*id, vargs),
                    other => other.clone(),
                },
                ClassVarSubst::Var(id) => ValueTy::TyConApp(*id, vargs),
            }
        }
        crate::ast::TypeExpr::TApp(head, args) => {
            // A HIGHER-KINDED TYPE VARIABLE applied to arguments — `m a` in
            // `Monad m => m a`, `f b` in `Functor f` — is an APPLICATION OF A
            // VARIABLE, not a constructor application.
            //
            // `m` is a type variable of kind `* -> *` exactly as `a` is one of
            // kind `*` (Haskell 2010 §4.1.2 draws no distinction), so `m a` must
            // be `TyConApp(m, [a])`. Spelling it as `Con("m", [a])` made the name
            // `m` into a CONCRETE constructor, so `m Double` could never unify
            // with `Maybe Double`:
            //
            //   cannot unify value type Con("m", [Double]) with Con("Maybe", [Double])
            //
            // That is the whole reason a constrained definition could not be
            // instantiated at a concrete type. The same failure appears for a
            // user class (`Con("w", [Double])` vs `Con("Boxed", [Double])`), so
            // this is the general variable-application rule, not a `Monad`
            // special case.
            if matches!(env.type_entry(head), Some(TypeEntry::Variable) | None) && head != class_var
            {
                let id = match vars.get(head.as_str()) {
                    Some(ValueTy::Var(v)) | Some(ValueTy::TyConVar(v)) => *v,
                    Some(ValueTy::TyConApp(v, _)) => *v,
                    _ => {
                        let fresh_head = fresh();
                        let id = match fresh_head {
                            ValueTy::Var(v) => v,
                            _ => unreachable!("fresh() yields a variable"),
                        };
                        vars.insert(head.clone(), ValueTy::TyConVar(id));
                        id
                    }
                };
                let vargs: Vec<ValueTy> = args
                    .iter()
                    .map(|a| conv(env, class_var, class_var_subst, vars, fresh, a))
                    .collect();
                return ValueTy::TyConApp(id, vargs);
            }
            // One representation for every application: the converter never
            // chooses between "declared" and "builtin" shapes — whether a name
            // is an algebraic data type is a question about the constructor
            // ([`TypeEnv::is_declared_data`]), not a spelling of the type.
            // A synonym HEAD expands first (`type L = List` makes `L Int` mean
            // `List Int`), so the two spellings denote the SAME value type.
            let (head, args) = env.expand_alias_head(head, args.clone());
            let vargs: Vec<ValueTy> = args
                .iter()
                .map(|a| conv(env, class_var, class_var_subst, vars, fresh, a))
                .collect();
            // A buffer CHANNEL constructor is the buffer value type — the one
            // representation, carrying the checked element.
            env.ctor_application(&head, vargs)
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
        .unwrap_or_else(|| ValueTy::Con(name.to_string(), vec![]))
}

/// Prepend a partial instance head's bound args to each container-typed
/// signature param type. `instance (Monad m) => Arrow (Kleisli m)` resolves
/// `a b c` to `Con("Kleisli", [b, c])` (the head `m` is dropped by the
/// signature conversion in [`signature_param_tys_conv`]); this restores it to
/// `Con("Kleisli", [m, b, c])` with the concrete head binding from the call
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

/// Push an instance's own context onto the GIVEN stack, returning the stack to
/// restore afterwards.
///
/// Haskell 2010 §4.3.2: an instance BODY is typed in the instance's context, so
/// `instance (Monad m) => Arrow (Kleisli m)` may call `return` inside its bodies
/// and have `m b` resolve against that dictionary rather than demanding a
/// concrete container.
///
/// The witness id is the one the head binding gave the head variable — `m` may
/// still be a live variable (`Var(14)`) when the instance is used before its
/// container is concrete, and `m b` is then `TyConApp(14, [b])`, which mentions
/// it. A head variable bound to nothing is skipped: there is no id to witness.
///
/// The caller restores `ctx.given` to the returned value afterwards.
fn set_instance_given(ctx: &mut Ctx, class: &str, ctor: &str) -> Vec<GivenVar> {
    let saved = ctx.given.clone();
    for (cclass, cvar) in instance_constraints(&ctx.env.clone(), class, ctor) {
        let Some(bound) = ctx.type_var_bindings.get(&cvar).cloned() else {
            continue;
        };
        if let ValueTy::Var(v) | ValueTy::TyConVar(v) | ValueTy::TyConApp(v, _) =
            ctx.subst.resolve_value(&bound)
        {
            ctx.given.push(GivenVar {
                class: cclass,
                name: cvar,
                id: Some(v),
            });
        }
    }
    saved
}

/// The ONE place an instance's method body is inlined.
///
/// Every way of reaching a method — an applied call, a bare reference, a
/// result-directed method under an expected type, a constructor-class method
/// selected by its container — ends with the same act: type the INSTANCE BODY in
/// place of the call. That act has six parts, and each used to be copy-pasted at
/// its call site (five sites, drifting apart until one-arm-at-a-time fixes had
/// to be applied to each):
///
/// 1. bind the instance's HEAD arguments to the container that selected it
///    (`Kleisli Maybe …` → `m := Maybe`);
/// 2. take the recursion guard for `(class, type, method)` — it fires only on
///    BODY re-entry, never on an argument that legally inlines another method;
/// 3. bind the method's parameters with the head arguments restored into their
///    types (`m b` → `Maybe b` once the call site made `m` concrete);
/// 4. push the instance's OWN context onto the GIVEN stack (Haskell 2010
///    §4.3.2) so a result-directed method inside the body resolves against the
///    instance's dictionary;
/// 5. type the body against the caller's expected type;
/// 6. restore every scratch value, the guard included.
///
/// A caller supplies only what is genuinely its own: WHICH instance was
/// selected, the argument value types it already inferred, and the expected
/// type — see [`InlineCall`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParamBindings {
    Signature,
    Arguments,
}

struct InlineCall<'a> {
    /// The class declaring the method (`Monad`, `Arrow`).
    class_name: &'a str,
    /// The instance KEY that was selected (`Kleisli`, `Int`, `FixedBuffer`) —
    /// the key `resolve_method` is asked for, not the full applied type.
    ty_name: &'a str,
    /// The type that SELECTED the instance, used to bind head arguments
    /// (`Kleisli Maybe Double Double`). `None` for an arity-0 instance, which is
    /// selected by its type NAME and has no head arguments.
    container: Option<&'a ValueTy>,
    method: &'a str,
    class_var: &'a str,
    /// How the class variable is spelled in the instance's method types. A
    /// constructor class substitutes the HEAD (`Kleisli`); an arity-0 class
    /// whose variable is a whole type substitutes that TYPE (`Double`,
    /// `List ?a`), which is what a selector-driven dispatch already computed.
    cv_subst: ClassVarSubst,
    sig: &'a crate::ast::TypeExpr,
    /// The instance body, from `resolve_method` or the class default.
    body: &'a Expr,
    /// The body's formal parameter names, in order.
    params: &'a [String],
    /// The call-site value type of each supplied argument, typed against its
    /// parameter. Empty for a bare reference.
    arg_vtys: &'a [ValueTy],
    /// Where the body's parameter bindings come from.
    ///
    /// [`ParamBindings::Signature`] binds the signature's parameter types (with
    /// the instance's head arguments restored) and then unifies the argument
    /// types into them — the applied-call shape, where the parameter type is the
    /// authority.
    ///
    /// [`ParamBindings::Arguments`] binds the types the caller INFERRED for the
    /// arguments — the arity-0 shape, where a nullary argument (`mempty`)
    /// contributes a concrete `List ?a` the signature cannot name.
    param_bindings: ParamBindings,
    /// The expected type of the body.
    body_expected: Option<ValueTy>,
    span: Span,
}

/// Inline an instance body; see [`InlineCall`].
fn inline_instance_body(ctx: &mut Ctx, call: InlineCall<'_>) -> Result<ValueTy, CompileError> {
    let InlineCall {
        class_name,
        ty_name,
        container,
        method,
        class_var,
        cv_subst,
        sig,
        body,
        params,
        arg_vtys,
        param_bindings,
        body_expected,
        span,
    } = call;
    // 1. Head arguments from the container that selected this instance. An
    // arity-0 instance is selected by its type NAME and has no container, so
    // there is nothing to bind (and `bind_constraint_instance` would reject the
    // absent head).
    if let Some(container) = container {
        bind_constraint_instance(ctx, class_name, ty_name, container, span)?;
    }
    // 2. Recursion guard on BODY re-entry only.
    let key = (
        class_name.to_string(),
        ty_name.to_string(),
        method.to_string(),
    );
    if ctx.method_lifting.contains(&key) {
        return Err(CompileError::Type {
            msg: format!("recursive typeclass method `{method}` for type `{ty_name}`"),
            span,
        });
    }
    ctx.method_lifting.insert(key.clone());
    let saved_locals = ctx.locals.clone();
    // 3. Method parameters, with the instance's head arguments restored.
    let mut param_tys = signature_param_tys(ctx, class_var, &cv_subst, sig);
    prepend_instance_head_args(
        &ctx.env.clone(),
        class_name,
        &mut param_tys,
        ty_name,
        &ctx.type_var_bindings.clone(),
        || ctx.fresh_vty(),
    );
    for (i, p) in params.iter().enumerate() {
        let ty = match param_bindings {
            ParamBindings::Arguments => arg_vtys
                .get(i)
                .cloned()
                .or_else(|| param_tys.get(i).cloned())
                .unwrap_or_else(|| ctx.fresh_vty()),
            ParamBindings::Signature => {
                param_tys.get(i).cloned().unwrap_or_else(|| ctx.fresh_vty())
            }
        };
        ctx.locals.insert(p.clone(), ArrowTy::value_channel(ty));
    }
    // The ARGUMENT's own value type refines the parameter binding.
    //
    // NOT redundant with the caller's `apply_args_curried`, which was measured
    // (removing this loop breaks 8 tests, all `ST`/`IO`): that path unifies the
    // argument against the SIGNATURE's parameter while leaving it abstract, and
    // `infer_method_value_vty_expected` returns the argument's OWN type. For
    // `return ()` the signature says `a` and the argument says `Void`; the body's
    // result is `m a`, so without this unification `IO (Var)` is produced where
    // the program means `IO ()`.
    if param_bindings == ParamBindings::Signature {
        for (i, av) in arg_vtys.iter().enumerate() {
            if let Some(p) = params.get(i) {
                if let Some(bound) = ctx.locals.get(p).cloned() {
                    unify_value(av, &bound.vty, &mut ctx.subst, span)?;
                }
            }
        }
    }
    // 4. The instance's own context, as a GIVEN (Haskell 2010 §4.3.2).
    let saved_given = set_instance_given(ctx, class_name, ty_name);
    // 5. Type the body. This IS an instance body (its context comes from the
    // instance head, not a definition signature), so the §4.1.3 signature
    // check does not apply here.
    let saved_in_instance = ctx.in_instance_body;
    ctx.in_instance_body = true;
    let result = infer_method_value_vty_expected(ctx, body, "body", body_expected);
    ctx.in_instance_body = saved_in_instance;
    // 6. Restore.
    ctx.given = saved_given;
    ctx.locals = saved_locals;
    ctx.method_lifting.remove(&key);
    result
}

/// The `(class, variable)` context an instance DECLARES: the `Monad m` of
/// `instance (Monad m) => Arrow (Kleisli m)`.
///
/// Haskell 2010 §4.3.2: an instance's bodies are typed in the instance's own
/// context, so each declared constraint is a GIVEN inside them. Empty for an
/// ordinary monomorphic instance.
fn instance_constraints(env: &TypeEnv, class: &str, ctor: &str) -> Vec<(String, String)> {
    env.instances
        .get(class)
        .and_then(|by_ty| by_ty.get(ctor))
        .map(|i| i.constraints.clone())
        .unwrap_or_default()
}

/// Restore an instance's bound head args at every occurrence of the container
/// INSIDE `t` — not only at the top level.
///
/// The signature conversion drops the head (`m a` for `instance Monad (ST s)`
/// becomes `Con("ST", [a])`), and the class var can sit under a FUNCTION type
/// (`bind: m a -> (a -> m b) -> m b`: the second parameter's `m b` is inside a
/// `Thunk`). Restoring only the top level left the callback's return type one
/// argument short, so a body whose own computation was the full `ST s e`
/// failed to unify with it.
fn prepend_head_args_into_class_var(t: &mut ValueTy, ctor: &str, head_values: &[ValueTy]) {
    match t {
        ValueTy::Con(c, args) if c == ctor => {
            let mut full = head_values.to_vec();
            full.extend(args.clone());
            // The head values themselves may mention the container kind; they
            // are opaque to this repair (they come from the call site).
            *t = ValueTy::Con(c.clone(), full);
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
    let subst = ClassVarSubst::Ctor(String::new(), Vec::new());
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
                ValueTy::Con(_, a) | ValueTy::Newtype(_, a) => a,
                _ => &[],
            };
            if !is_declared_type_name(env, head) {
                // A type-constructor variable used as a head (`m a`): bind it
                // to the bare constructor (`m := Maybe`), the value-type form
                // the instance registry is keyed by.
                if let Some(c) = env.type_name_of_vty(actual) {
                    out.entry(head.clone())
                        .or_insert_with(|| ValueTy::Con(c, vec![]));
                }
            }
            // A synonym HEAD expands first, so the pattern's arguments line up
            // with the actual type's spine: with `type F = Pair Int`, the
            // pattern `F a` is `Pair Int a` and binds `a` to the SECOND
            // argument, not the first.
            let (_, args) = env.expand_alias_head(head, args.clone());
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
            // A synonym HEAD expands to its target (`type L = List` makes
            // `L Int` mean `List Int`), so both spellings denote one type.
            let (head, args) = env.expand_alias_head(head, args.clone());
            let vargs: Vec<ValueTy> = args
                .iter()
                .map(|a| sig_ty_to_vty_inner(env, a, bindings, vars, &mut *fresh))
                .collect();
            match bindings.get(&head) {
                // A bound type-constructor variable is applied: splice its own
                // arguments ahead of the new ones (`m := Maybe`, `m a` →
                // `Maybe a`).
                Some(ValueTy::Con(n, pre)) => {
                    ValueTy::Con(n.clone(), pre.iter().cloned().chain(vargs).collect())
                }
                Some(ValueTy::Newtype(n, pre)) => {
                    ValueTy::Newtype(n.clone(), pre.iter().cloned().chain(vargs).collect())
                }
                Some(other) => other.clone(),
                None => {
                    let head_ty = vars
                        .entry(head.clone())
                        .or_insert_with(|| {
                            if is_declared_type_name(env, &head) {
                                resolve_type_name(env, &head)
                            } else {
                                fresh()
                            }
                        })
                        .clone();
                    match head_ty {
                        ValueTy::Var(v) | ValueTy::TyConVar(v) => ValueTy::TyConApp(v, vargs),
                        ValueTy::Con(n, pre) => {
                            ValueTy::Con(n, pre.into_iter().chain(vargs).collect())
                        }
                        // A declared constructor applied to MORE arguments than
                        // its head alone (`FixedBuffer Double` resolves to the
                        // buffer value type): build the application through the
                        // one converter so a channel constructor keeps its
                        // single buffer representation.
                        other => match vargs.len() {
                            0 => other,
                            _ => env.ctor_application(&head, vargs),
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
            ClassVarSubst::Ctor(
                ty_name.clone(),
                match info.class_var_ty(&ctx.env) {
                    ValueTy::Con(_, a) | ValueTy::Newtype(_, a) => a,
                    _ => Vec::new(),
                },
            )
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
            // An INSTANCE body: its context comes from the instance head, not
            // from a definition signature, so the §4.1.3 check is not this
            // walk's to make.
            let saved_in_instance = ctx.in_instance_body;
            ctx.in_instance_body = true;
            let res = infer_method_value_vty_expected(ctx, body, "body", body_expected.clone());
            ctx.in_instance_body = saved_in_instance;
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
/// name RAN the user's body): a user def shadowing a Prelude value definition
/// was silently typed with the Prelude's arity and then failed to unify, or
/// worse, ran a body of the wrong shape.
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
        // A declaration variant (`g :: Num b => Int -> b;` on its own) carries
        // no expression body, so it has no calls and cannot be a node either.
        if d.is_decl() || !nodes.contains(d.name()) {
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
    entry: &str,
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
    // Prelude function definitions are typed HERE, BEFORE the user's `data`
    // declarations register (a user `data` would otherwise shadow a builtin
    // record the prelude bodies use). They are pure functions, so typing them
    // against the builtin env is sound; they then flow into `program.defs` so
    // reduce/lower see them as ordinary top-level defs.
    //
    // This list is EMPTY at present: the Faust-style Buffer-arrow combinators
    // (`seq`/`par`/`split`/`merge`) were removed — `seq` occupies a Haskell
    // 2010 Prelude name with an unrelated meaning, and the other three were
    // dead. The mechanism stays for the next prelude value definition.
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
            in_instance_body: false,
            in_signed_body: false,
            dict_selections: HashMap::new(),
            residual: Vec::new(),
            literal_vars: HashMap::new(),
            literal_instances: HashMap::new(),
            value_tys: HashMap::new(),
            entry_name: String::new(),
            final_pass: false,
        };
        // The PRELUDE is compiler-owned: its signatures were validated at
        // load (`register_prelude`), so the user-facing §4.1.3 context check
        // must not run over its bodies.
        pc.in_instance_body = true;
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
                let fields_ty: Vec<crate::types::ty::ConstructorField> = fields
                    .iter()
                    .map(|(f, t, strict)| crate::types::ty::ConstructorField {
                        name: Some(f.clone()),
                        ty: data_field_vty_with_free(&env, tyvars, t, &mut free),
                        strict: *strict,
                    })
                    .collect();
                env.data_types.insert(
                    name.clone(),
                    DataInfo::Data(vec![crate::types::ty::ConstructorInfo {
                        name: name.clone(),
                        fields: fields_ty,
                    }]),
                );
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
                let ctors_ty: Vec<crate::types::ty::ConstructorInfo> = ctors
                    .iter()
                    .map(|(c, ts)| crate::types::ty::ConstructorInfo {
                        name: c.clone(),
                        fields: ts
                            .iter()
                            .map(|(t, strict)| crate::types::ty::ConstructorField {
                                name: None,
                                ty: data_field_vty_with_free(&env, tyvars, t, &mut free),
                                strict: *strict,
                            })
                            .collect(),
                    })
                    .collect();
                env.data_types
                    .insert(name.clone(), DataInfo::Data(ctors_ty));
                // A parameterized SUM records its arity exactly like a record:
                // `data Opt a = …` has kind `* -> *`, which is what the kind
                // projection must answer (Haskell 2010 §4.1.1 gives a data
                // declaration its parameter count). The old exclusion existed
                // only because the two constructor-application shapes could not
                // unify; there is one `Con` shape now.
                if !tyvars.is_empty() {
                    env.data_arities.insert(name.clone(), tyvars.len());
                }
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

    // Every type-level name a program writes must denote a type: `Char`,
    // `Word8`, `Void` and any other undeclared uppercase name are
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
        in_instance_body: false,
        in_signed_body: false,
        dict_selections: HashMap::new(),
        residual: Vec::new(),
        literal_vars: HashMap::new(),
        literal_instances: HashMap::new(),
        value_tys: HashMap::new(),
        entry_name: String::new(),
        final_pass: false,
    };

    // Prelude value definitions were already typed above (against the builtin
    // env, before user `data` shadowed any builtin record). Seed their schemes
    // into the main context and exclude them from the per-program re-inference
    // so they resolve without re-typing (a user `data Pair` would otherwise
    // shadow the builtin `Pair` the bodies use). The list is empty today; see
    // the note above on the removed Buffer-arrow combinators.
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
    ctx.entry_name = entry.to_string();
    infer_def_group(&mut ctx, &user_defs)?;
    // The top-level group closed, so stage 2 and stage 3 ran over it. Anything
    // still residual belongs to the top level's own body inference (a nested
    // group ran its own pair), so spend it here too: a residual that outlives
    // every group would otherwise reach lowering unresolved.
    let top_names: HashSet<String> = user_defs
        .iter()
        .filter(|d| !d.is_decl())
        .map(|d| d.name().to_string())
        .collect();
    discharge_residuals(&mut ctx)?;
    default_residuals(&mut ctx, &top_names)?;

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
            Def::Eval { name, .. } => ctx
                .defs
                .get(name)
                .filter(|s| s.lam_count == 0)
                .map(|_| name.clone()),
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
        if let Def::Eval { name, .. } = def {
            // A definition with parameters (`f x = …`, which IS `f = \x -> …`
            // — Haskell 2010 §4.4.3.1) carries them in its scheme. Its scheme is
            // a NEST of unary thunks (`take = \n -> \xs -> …`); flatten to the
            // flat λ-parameter list so the nested lambdas in lowering can
            // consume their own leading slice, then truncate to the def's own
            // arity — a body that itself returns a function (`f x = \y -> …`)
            // flattens PAST the definition's parameters into the inner λ's.
            let arity = def_head_arity(def);
            if arity == 0 {
                continue;
            }
            let scheme = ctx.defs.get(name).cloned();
            if let Some(s) = scheme {
                if let ValueTy::Thunk(..) = &s.ty.vty {
                    let (arg_tys, _) = flatten_thunk(&s.ty.vty);
                    let mut resolved: Vec<ValueTy> =
                        arg_tys.iter().map(|t| ctx.subst.resolve_value(t)).collect();
                    resolved.truncate(arity);
                    fn_param_tys.insert(name.to_string(), resolved);
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
            // Every value definition is a shared-cell def: one with parameters
            // is a name bound to a function value (`f x = e` IS `f = \x -> e`),
            // and one without is a CAF whose body is evaluated through its cell.
            Def::Eval { .. } => true,
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
        literal_tys: literal_types(&ctx),
        literal_instances: ctx.literal_instances.clone(),
        value_tys: ctx.value_tys.clone(),
    })
}

/// The type each literal's `fromInteger`/`fromRational` finally produced, read
/// from the substitution after stages 2 and 3 have run.
///
/// A literal whose variable is STILL unresolved here is one no stage could
/// decide — a polymorphic position (`f :: Num a => a; f = 1`) whose dictionary
/// is the caller's. It records nothing, so lowering keeps the position's own
/// expected type, which is the caller's instantiation.
fn literal_types(ctx: &Ctx) -> HashMap<(usize, usize), ValueTy> {
    let mut out = HashMap::new();
    for (span, id) in &ctx.literal_vars {
        let ty = ctx.subst.resolve_value(&ValueTy::Var(*id));
        if !matches!(ty, ValueTy::Var(_) | ValueTy::TyConVar(_)) {
            out.insert(*span, ty);
        }
    }
    out
}

/// The definition's formal parameters and its uncurried body.
///
/// Haskell 2010 §4.4.3.1: `f x y = e` IS `f = \x -> \y -> e`, and
/// `Parser::parse_equations` performs exactly that rewrite, so a definition's
/// parameters and its body's leading λs are ONE thing and `Def::arity()` counts
/// them; this function returns them with the body's leading λs peeled off.
pub(crate) fn effective_def_params(def: &Def) -> (Vec<crate::ast::Param>, Expr) {
    crate::types::ty::split_curried_params(def_head_arity(def), &[], def.body())
}

/// The number of λ-parameters a definition's SCHEME advertises: the same ONE
/// arity as [`def_head_arity`].
///
/// There is no second notion. A scheme's `lam_count` and the definition's head
/// parameters describe one fact (Haskell 2010 §4.4.3.1), so a caller can read
/// either without the two disagreeing.
pub(crate) fn def_scheme_arity(def: &Def) -> usize {
    def_head_arity(def)
}

/// The number of λs in a definition's HEAD — the parameters it binds, as opposed
/// to a λ-nest the body RETURNS.
///
/// Arity is a property of the body (Haskell 2010 §4.4.3.1), so an unannotated
/// definition's head arity is its body's λ-nest depth. A `::` signature caps it:
/// `f :: Double -> (Double -> Double); f = \x -> \y -> …` binds ONE parameter
/// and returns a function.
pub(crate) fn def_head_arity(def: &Def) -> usize {
    match def.sig() {
        Some(sig) => sig.arg_count().min(def.arity()),
        None => def.arity(),
    }
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
    // Effective parameters: the parser writes every definition's parameters into
    // the body's leading λ-nest (`f x = e` IS `f = \x -> e`, Haskell 2010
    // §4.4.3.1), so no definition carries a second parameter list.
    let (params, body) = effective_def_params(def);
    let saved_locals = ctx.locals.clone();
    if params.is_empty() {
        // A binding's OWN declared type is the expected type of its right-hand
        // side. `x :: Double; x = 2` is well typed in Haskell 2010 because the
        // literal's representation comes from the signature (Report §6.4.1: it
        // is `fromInteger` at `Double`); inferring the body with NO expected
        // type made the literal an `Int` and the signature check a type error.
        // A binding's OWN declared type is the expected type of its right-hand
        // side. `x :: Double; x = 2` is well typed in Haskell 2010 because the
        // literal's representation comes from the signature (Report §6.4.1: it
        // is `fromInteger` at `Double`).
        //
        // A NULLARY signature publishes its bare result; a FUNCTION signature
        // publishes the WHOLE function type. `h :: Int -> Int; h = f 1`
        // declares that the right-hand side IS `Int -> Int`, and that
        // obligation is what solves a class variable inside the partial
        // application: with `f :: C a => a -> b -> a`, `h = f 1` gives
        // `a := Int, b := Int`, so the literal is `Int` and `C Int` is
        // discharged. GHC accepts exactly that source (measured) and reports
        // `Ambiguous type variable 'a0'` only when the annotation is absent.
        // This arm is reached only by a λ-LESS body: a body that BUILDS a λ
        // takes its parameter types from the signature by the param path below.
        let binding_ty: Option<ValueTy> = match def.sig() {
            Some(sig) if !sig_func_parts(sig).0.is_empty() => {
                let (params, ret, _) = def_sig_tys(ctx, sig)?;
                Some(ValueTy::Thunk(params, vec![ret]))
            }
            Some(sig) => Some(def_sig_tys(ctx, sig)?.1),
            _ => None,
        };
        // `main :: IO t` (Haskell 2010 §5.1) is a CHECK, not an expectation.
        //
        // Supplying it as the body's expected type made `main = 1.0` UNIFY with
        // `IO t` and compile — while GHC rejects exactly that source with
        // `No instance for 'Fractional (IO t0)'`, because the literal must
        // inhabit the entry's type and cannot. The distinction is the one
        // between "type the body, then require the result to be an action" and
        // "force the result to be an action while typing the body"; Haskell
        // does the former, so `lower.rs` and `lib.rs` do the check.
        //
        // Nothing is lost: a `do` block's `m` still comes from its own
        // contents (`main = do { write …; return () }` types through `write`),
        // which is the environment GHC actually uses for a block in a body with
        // no signature.
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
        let r = infer_expr_expected(ctx, &body, binding_ty.clone());
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
            let retry = infer_expr_expected(ctx, &body, binding_ty);
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
    // arity capped by the body's λ-nest depth (Haskell 2010 §4.4.3.1).
    //
    // The signature's RESULT is the body's expected type for the same reason a
    // nullary binding's is: `f :: Double -> Double; f p = 5` is well typed
    // because the literal's representation comes from the declared result
    // (Haskell 2010 §6.4.1 `fromInteger`), and inferring the body with no
    // expected type made it an `Int` and rejected the declaration.
    // The signature's variables are recorded with their PROVENANCE — this
    // definition plus the letter written — so the unifier can tell "the same
    // variable" from "two different variables of one signature" (Haskell 2010
    // §4.4.1.2). `saved_rigid` restores the map after this body, so a prelude
    // definition's letters never leak into a later definition's judgement.
    // Snapshot the OUTER provenance, then mark this definition's own letters on
    // top of it. The entries must survive into the value-track RETRY below
    // (`ctx.subst = saved_subst.clone()`), which is why `saved_subst` is taken
    // AFTER the signature conversion — taking it earlier gave the retry an empty
    // `rigid_name`, so `f :: a -> b -> c; f x y = x` was rejected on the first
    // attempt and then silently accepted by the retry.
    let saved_rigid = ctx.subst.rigid_name.clone();
    let sig_tys: Option<(Vec<ValueTy>, ValueTy)> = match def.sig() {
        Some(sig) => {
            let (param_tys, ret_ty, vars) = def_sig_tys(ctx, sig)?;
            for (letter, t) in vars.iter() {
                if let ValueTy::Var(v) | ValueTy::TyConVar(v) = t {
                    ctx.subst
                        .rigid_name
                        .insert(*v, (def.name().to_string(), letter.clone()));
                }
            }
            (param_tys.len() == params.len()).then_some((param_tys, ret_ty))
        }
        None => None,
    };
    let sig_param_tys: Option<&Vec<ValueTy>> = sig_tys.as_ref().map(|(p, _)| p);
    let sig_ret: Option<ValueTy> = sig_tys.as_ref().map(|(_, r)| r.clone());
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
    // Taken AFTER the signature conversion so the value-track retry restores a
    // substitution that still carries this definition's rigid provenance.
    let saved_subst = ctx.subst.clone();
    // A SIGNED definition's body is checked against its declared context (§4.1.3);
    // an unsigned binding's context is inferred, so it is exempt.
    let saved_signed = ctx.in_signed_body;
    ctx.in_signed_body = def.sig().is_some();
    let (result, lambda_param_tys) = match infer_expr_expected(ctx, &body, sig_ret.clone()) {
        Ok(t) => {
            // The body's result must BE the declared result (Haskell 2010
            // §4.4.1.2). This check CANNOT live in the `expected` type the body
            // is typed against: a mismatch there is swallowed by the retry
            // below (a failed first attempt is normal — the retry re-binds the
            // parameters as value channels), so `f :: a -> b -> c; f x y = x`
            // was rejected on the first attempt and then quietly accepted.
            //
            // Signature variables are RIGID while this body is checked, so
            // unifying the body's result with the declared one is exactly the
            // judgement GHC 9.10.3 makes: `c := a` is refused because `a` and
            // `c` are different declared types, while `a -> a -> a` and
            // `a -> b -> a` (body `x`) pass because the result IS the declared
            // variable.
            {
                if let Some(declared) = sig_ret.as_ref() {
                    if let Err(e) = unify_value(&t.vty, declared, &mut ctx.subst, body.span()) {
                        ctx.subst.rigid_name = saved_rigid.clone();
                        return Err(e);
                    }
                }
            }
            (
                Ok(t),
                // The parameter types the body was checked against, resolved
                // through the substitution: an unconstrained parameter stays a
                // structural `Var` (polymorphic), a constrained one is concrete.
                // `def_scheme` lifts these into the definition's scheme.
                resolved_param_tys(ctx, &params),
            )
        }
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
            let retry = infer_expr_expected(ctx, &body, sig_ret.clone());
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
    ctx.subst.rigid_name = saved_rigid;
    ctx.in_signed_body = saved_signed;
    result.map(|ty| DefBodyTy {
        ty,
        sig_params: sig_param_tys.cloned().unwrap_or_default(),
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
    // A constrained body is typed ONCE under its GIVEN constraints (design
    // §3.0), so its class variables are ASSUMED, never ambiguous: stage 3 must
    // not default them, or `indices a :: Ix i => …` would be specialized to
    // `[Integer]` inside its own body and rejected against its signature. Save
    // and restore the store around the body instead.
    let saved_residual = std::mem::take(&mut ctx.residual);
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
        // A class variable is a TYPE VARIABLE whatever the class's arity:
        // `m` in `Monad m => m a` has kind `* -> *`, `a` in `Eq a` has kind `*`,
        // and Haskell 2010 §4.1.2 draws no distinction between them (`Maybe`,
        // `W m`, `Kleisli m` are all things `m` can BE). `conv` records the
        // higher-kinded one as `TyConApp(m, args)` / `TyConVar(m)`, so its id is
        // available for both arities. Using `None` here made `abstract_ty()`
        // return `Con("m", [])` — the NAME used as a concrete constructor — and
        // `m Double` could never unify with `Maybe Double`.
        let _ = arity;
        let id = match vars.get(var).map(|t| ctx.subst.resolve_value(t)) {
            Some(ValueTy::Var(v)) | Some(ValueTy::TyConVar(v)) => Some(v),
            Some(ValueTy::TyConApp(v, _)) => Some(v),
            // Solved to a concrete type: already discharged by that instance, so
            // no dictionary parameter is needed.
            Some(_) => continue,
            // The signature never mentions the variable.
            None => continue,
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
    // A class-CONSTRAINED definition body: `given` above carries its declared
    // context, and the §4.1.3 check uses it — `f :: Num a => a -> Bool;
    // f x = x < x` must be REJECTED because `Num` does not witness `Ord`.
    // A constrained definition is SIGNED by construction (`is_constrained_sig`
    // requires a `=>`), so the check applies.
    let saved_signed = ctx.in_signed_body;
    ctx.in_signed_body = true;
    let inferred = infer_expr_expected(ctx, &body, Some(sig_ret.clone()));
    ctx.in_signed_body = saved_signed;
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
    ctx.residual = saved_residual;
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
        // A kernel contributes no NAME (there is no callee to resolve), but an
        // argument may reference a top-level def whose value-rate
        // re-registration this walk feeds (`infer_def_body`'s complete-mode
        // retry). Stopping at the node left a mutual-recursion partner on its
        // signal-rate placeholder, so a value-demanding site inside the
        // kernel argument failed.
        Expr::Kernel { args, .. } => {
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
        // The placeholder's shape comes from the BODY: a definition binds a name
        // to an expression (Haskell 2010 §4.4.3.1), so its λ-count is how many
        // λs the body's head builds. A nest must already be a function value,
        // otherwise a recursive self-call in the body cannot dispatch — `take`
        // would see a bare placeholder var and report "expects 0 argument(s)".
        let body_lams = leading_lambda_count(def.body());
        let out = ctx.fresh_vty();
        let mut vty = out;
        for _ in 0..body_lams {
            vty = ValueTy::Thunk(vec![ctx.fresh_vty()], vec![vty]);
        }
        ctx.defs.insert(
            def.name().to_string(),
            Scheme {
                lam_count: body_lams,
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
        // (`Array i e`) from the constrained `(!)` inside the λ-body, but the
        // recovered shapes surface only at the call site. Remember the name so
        // a call-site mismatch can point at the missing annotation instead of
        // dumping the recovered shapes.
        if def.sig().is_none() && leading_lambda_count(def.body()) > 0 {
            ctx.retry_inferred_defs.insert(def.name().to_string());
        }
        let lam_count = def_scheme_arity(def);
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

    // Second pass: re-infer with actual schemes for correct signal port counts.
    // This is the pass whose variables the finished program carries, so literal
    // typing is recorded only here.
    let saved_final = ctx.final_pass;
    let group_names: HashSet<String> = defs
        .iter()
        .filter(|d| !d.is_decl())
        .map(|d| d.name().to_string())
        .collect();
    // The placeholder pass's residuals name ITS variables, which the finished
    // program does not carry; drop them so only the final pass's constraints are
    // discharged and defaulted.
    ctx.residual.clear();
    ctx.final_pass = true;
    for def in defs {
        if def.is_decl() {
            continue;
        }
        let body_ty = infer_def_body(ctx, def)?;
        let lam_count = def_scheme_arity(def);
        let (vty, constraints) = def_scheme(ctx, def, &body_ty, lam_count)?;
        let resolved = ctx.subst.apply(&ArrowTy { vty });
        let vars = quantified_vars(ctx, def, &resolved);
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

    // ---- Stage 2 and stage 3, at this binding group's close ----------------
    //
    // Everything above is stage 1: unification wrote the equations. What is left
    // in `ctx.residual` is the set of class constraints those equations did NOT
    // settle, and this is the one place that spends them — in the two separated
    // steps Haskell's inference has:
    //
    // * stage 2 ([`discharge_residuals`]) — a variable unification has since
    //   made CONCRETE must have an instance, and a missing one is
    //   `no instance of \`C\` for type \`T\``;
    // * stage 3 ([`default_residuals`]) — a variable still unresolved and not
    //   fixed by the group's own types takes `default (Integer, Double)`.
    ctx.final_pass = saved_final;
    discharge_residuals(ctx)?;
    // Stage 3 defaults the variables stage 2 could not spend, and hands back
    // the constraints whose variable had already become a STRUCTURE. Those are
    // stage 2's business again — a structure is not ambiguous, and its
    // instance must exist. Re-running stage 2 here is what rejects a mixed-type
    // list literal: `[1.0, [1.0]]` solves the first literal's `a` to `List b`,
    // stage 3 defaults the inner `b` to `Integer`, and `Num (List Integer)` has
    // no instance. Discharging after defaulting (not only before it) is the
    // order the two stages are separated to make possible.
    default_residuals(ctx, &group_names)?;
    discharge_residuals(ctx)?;
    // Stage 3 solved variables the schemes above were built from, so re-resolve
    // them: a scheme that kept a pre-defaulting variable would leave `1 + 2`'s
    // type a `Var` in `def_schemes`, which is what call sites and lowering read.
    refresh_group_schemes(ctx, defs);

    Ok(())
}

/// The type variables a definition's scheme QUANTIFIES.
///
/// The starting point is [`free_vars_quantified`] (the body's free variables
/// minus those free in the enclosing environment), restricted by the
/// **monomorphism restriction** (Haskell 2010 §4.5.5): a SIMPLE PATTERN
/// BINDING — one with no parameter list (`v = …`, `v = \x -> …`) — whose
/// inferred type carries a CLASS CONSTRAINT is not generalized.
///
/// The consequence is the one §4.5.5 is for: the constrained variable stays
/// ONE variable shared by every binding in the group (`main = double` sees the
/// SAME variable rather than a fresh instantiation), so stage 3's defaulting
/// applies to a type the whole group agrees on. `instantiate` freshens only
/// quantified variables, which is what makes the sharing automatic.
///
/// A FUNCTION binding (`add2 a b = a + b`) has a parameter list and is
/// generalized normally, so it stays `Num a => a -> a -> a` and applies at
/// whatever type its call sites demand.
fn quantified_vars(ctx: &Ctx, def: &Def, resolved: &ArrowTy) -> Vec<TypeVarId> {
    let mut vars = free_vars_quantified(ctx, resolved);
    // §4.5.5 applies ONLY to a binding with no explicit signature. The Report
    // states the restriction for a group whose bindings have none: an
    // annotation is the programmer SAYING what the type is, so generalising it
    // needs no permission. GHC 9.10.3 measured:
    //
    //   bare = 1                  → Integer          (restriction applies)
    //   h :: Num b => b; h = 1    → forall b. Num b => b
    //   h :: Double; h = 1        → Double
    //
    // Without this guard `h` stayed a shared monomorphic variable and stage 3
    // defaulted it to `Integer`, so `(h :: Integer, h :: Double)` — legal in
    // one GHC program — was a rejection here.
    if def.sig().is_none() && !def.wrote_params() {
        // §4.5.5 applies to a SIMPLE PATTERN BINDING — `v = …`, and also
        // `v = \x -> …`, because a λ on the right-hand side does not turn the
        // binding into a function binding. That is the Report's distinction
        // between `pat` and `funlhs`, and it is why `def.wrote_params()` — not
        // the arity — is what this guard reads: after Task 10a every definition
        // with parameters carries them as the body's λ-nest, so `f x = e` and
        // `f = \x -> e` have the SAME arity and only the spelling tells them
        // apart. Measured on GHC 9.10.3 (compiled module, two incompatible
        // call-site types): `double = \x -> x * 2.0` is monomorphic (error)
        // while `double x = x * 2.0` generalises (legal).
        // The definition's own constrained variables: every residual whose class
        // variable its type mentions.
        //
        // Both sides must be read at the SAME stage. `own` is resolved through
        // the substitution, so a residual's `r.var` has to be resolved too —
        // comparing a raw id against resolved ids missed every variable an
        // earlier unification had already chained, and the guard silently did
        // nothing (measured: `double = \x -> x * 2.0` kept a quantified `Var(7)`
        // and generalised, which §4.5.5 forbids).
        let own = value_free_vars(&resolved.vty, &ctx.subst);
        let constrained: HashSet<TypeVarId> = ctx
            .residual
            .iter()
            .filter_map(|r| match ctx.subst.resolve_value(&ValueTy::Var(r.var)) {
                ValueTy::Var(v) | ValueTy::TyConVar(v) => Some(v),
                _ => None,
            })
            .filter(|v| own.contains(v))
            .collect();
        vars.retain(|v| !constrained.contains(v));
    }
    vars
}

/// Re-resolve every scheme in `defs` through the substitution after stages 2
/// and 3.
///
/// The second inference pass builds each scheme from the body type it just
/// inferred, which is BEFORE defaulting runs (defaulting needs the whole group
/// inferred to know which variables are ambiguous). Defaulting then binds a
/// variable the scheme carries, so the stored type must be resolved again or a
/// caller sees `a` where the answer is `Integer`.
fn refresh_group_schemes(ctx: &mut Ctx, defs: &[Def]) {
    for def in defs {
        if def.is_decl() {
            continue;
        }
        let Some(scheme) = ctx.defs.get(def.name()).cloned() else {
            continue;
        };
        let resolved = ctx.subst.apply(&scheme.ty);
        // This pass exists to re-resolve the type after defaulting, and it must
        // not be a BACK DOOR around the monomorphism restriction. A simple
        // pattern binding — no signature, no written parameter list — keeps the
        // quantification §4.5.5 decided for it, so its variable is NOT
        // re-generalised here; only a binding the restriction does not cover
        // takes a freshly computed (possibly larger) quantified set.
        //
        // Without this, `double = \x -> x * 2.0` was re-generalised right after
        // `quantified_vars` had correctly left it monomorphic, and the program
        // used at two incompatible types compiled instead of being rejected
        // (GHC 9.10.3 rejects it).
        let vars = if def.sig().is_none() && !def.wrote_params() {
            scheme.vars
        } else {
            free_vars_quantified(ctx, &resolved)
        };
        ctx.defs.insert(
            def.name().to_string(),
            Scheme {
                vars,
                ty: resolved,
                ..scheme
            },
        );
    }
}

/// **Stage 2 — discharge.** For every residual class constraint whose class
/// variable unification has made CONCRETE, find the instance.
///
/// This is deliberately its own stage with its own error. Unification already
/// SUCCEEDED by the time a variable is concrete — `x :: Int; x == 1.0` unified
/// the literal's variable with `Int`, and that unification is WHERE `Int` came
/// from — so reporting the missing `Fractional Int` as a unification failure
/// would name the wrong stage and send the reader to the wrong equation. GHC's
/// own words are `No instance for 'Fractional Int'`.
///
/// Constraints whose variable is still unresolved stay in the store for stage
/// 3. Constraints the body's GIVEN context witnesses are left as dictionaries:
/// the witness IS the instance, and there is no concrete type to look up.
fn discharge_residuals(ctx: &mut Ctx) -> Result<(), CompileError> {
    let given_ids: Vec<TypeVarId> = ctx.given.iter().filter_map(|g| g.id).collect();
    let mut kept: Vec<Residual> = Vec::new();
    for r in std::mem::take(&mut ctx.residual) {
        let solved = ctx.subst.resolve_value(&ValueTy::Var(r.var));
        match solved {
            ValueTy::Var(id) => {
                // Still unresolved: stage 3's business, unless the body's GIVEN
                // context witnesses it (the incoming dictionary IS the instance).
                if !given_ids.contains(&id) {
                    kept.push(Residual { var: id, ..r });
                }
            }
            // A dictionary is compiler-side evidence, not a type an instance is
            // declared for: nothing to look up.
            ValueTy::Dict(_) => {}
            concrete => {
                // A structure whose COMPONENTS are still variables (`Pair a b`
                // before its fields default) is not a concrete type yet: stage 3
                // may still fix it, so a missing instance here is not the final
                // answer. Deferring is what lets `Ix (Pair Integer Integer)`
                // become reachable after defaulting.
                if contains_unresolved(&concrete, &ctx.subst) {
                    kept.push(r);
                    continue;
                }
                if !ctx.env.instance_exists(&r.class, &concrete) {
                    return Err(CompileError::Type {
                        msg: format!(
                            "no instance of `{}` for type `{}`",
                            r.class,
                            ctx.env.type_spelling(&concrete)
                        ),
                        span: r.span,
                    });
                }
                // Unification already fixed this variable; record the instance
                // it demands so lowering converges on the SAME answer instead of
                // re-deriving one from its own coarser static type.
                //
                // A LITERAL's constraint is exempt: its dictionary is never
                // consulted (lowering converts the exact payload through
                // `literal_tys`), so writing it here would put a slot in the
                // table that no code path reads — and `main = 1.0` would
                // advertise `Eq`/`Ord`/`Num`/`Fractional Double` for a program
                // whose only class variable is the conversion target.
                if !r.literal {
                    ctx.dict_selections
                        .insert((r.span, r.class.clone()), concrete);
                }
            }
        }
    }
    ctx.residual = kept;
    Ok(())
}

/// **Stage 3 — default.** For every variable still unresolved at this binding
/// group's close, apply Haskell 2010 §4.3.4's defaulting rule with the list
/// `default (Integer, Double)`.
///
/// The rule is the Report's, in order:
///
/// 1. the variable must be AMBIGUOUS — absent from the types this group's
///    bindings actually have. A variable a signature or an annotation fixed is
///    not ambiguous, so it is never defaulted; that half of the boundary is
///    enforced by [`group_type_vars`], which collects exactly the variables the
///    group's own types mention.
/// 2. every constraint on it must be a STANDARD class, and at least one must be
///    numeric (`Num`/`Fractional`). A variable constrained only by a user class
///    has no default and is reported `Ambiguous type variable`, exactly as GHC
///    reports it.
/// 3. the candidates `Integer` then `Double` are tried in order, and the FIRST
///    that satisfies EVERY constraint wins. `Integer` has no `Fractional`
///    instance, so a fractional literal selects `Double`; `1 + 2` selects
///    `Integer`, because `Integer` comes first and does satisfy `Num`.
///
/// A variable no candidate satisfies is genuinely ambiguous: it can neither be
/// defaulted nor discharged, so it is reported.
fn default_residuals(ctx: &mut Ctx, group: &HashSet<String>) -> Result<bool, CompileError> {
    if ctx.residual.is_empty() {
        return Ok(false);
    }
    // Rule 1's input: the variables the group's own types mention.
    let fixed = group_type_vars(ctx, group);
    let mut defaulted_any = false;
    // Group the residuals by variable, preserving first-seen order so a
    // diagnostic names the first use site.
    let mut order: Vec<TypeVarId> = Vec::new();
    let mut by_var: HashMap<TypeVarId, Vec<(String, Span, bool)>> = HashMap::new();
    // Constraints whose variable unification already solved to a STRUCTURE
    // (`[1.0, [1.0]]` binds the first literal's `a` to `List b`). They are not
    // this stage's business — there is no variable left to default — but they
    // are NOT discharged either: their structure may still contain a variable
    // that the loop below defaults, and that is what turns `List b` into the
    // `List Integer` stage 2 must then reject. Dropping them here (as this
    // stage used to) swallowed the constraint entirely and let a mixed-type
    // list literal compile.
    let mut carried: Vec<Residual> = Vec::new();
    for r in std::mem::take(&mut ctx.residual) {
        // Re-resolve: an earlier default in this same pass may have solved it.
        match ctx.subst.resolve_value(&ValueTy::Var(r.var)) {
            ValueTy::Var(id) => {
                if !by_var.contains_key(&id) {
                    order.push(id);
                }
                by_var
                    .entry(id)
                    .or_default()
                    .push((r.class, r.span, r.literal));
            }
            // A dictionary witness needed no instance lookup (see stage 2).
            ValueTy::Dict(_) => {}
            // A structure: hand it back for the caller's re-discharge.
            _ => carried.push(Residual { var: r.var, ..r }),
        }
    }
    ctx.residual = carried;
    for id in order {
        let classes = by_var.remove(&id).unwrap_or_default();

        let first_span = classes
            .first()
            .map(|(_, s, _)| *s)
            .unwrap_or(Span::new(0, 0));
        // Rule 1: a variable the group's own types mention is not ambiguous.
        if fixed.contains(&id) {
            continue;
        }
        // Rule 2: defaulting is the Report's rule for an AMBIGUOUS variable
        // constrained by the STANDARD classes (§4.3.4's `P` is the Prelude's
        // class set). A USER class is not in that set, so a default candidate —
        // however well it happens to fit — is not a default. The constraint
        // need not be `Num`/`Fractional`: `f x y = x <= y` applied at two
        // literals is `Ord a`, and GHC 9.10.3 ANSWERS `True` for it by
        // defaulting `a` to `Integer`, because the candidates are numeric TYPES
        // and only the constraint SET has to be standard.
        let all_standard = classes.iter().all(|(c, _, _)| {
            matches!(
                c.as_str(),
                "Num"
                    | "Fractional"
                    | "Eq"
                    | "Ord"
                    | "Show"
                    | "Enum"
                    | "Real"
                    | "Integral"
                    // `Ix` is a Prelude class (Data.Ix), so a variable
                    // constrained by it is still a STANDARD class variable and
                    // takes the default list: `at a i = a ! i` applied at two
                    // literals is `Ix Integer`, which GHC 9.10.3 accepts.
                    | "Ix"
            )
        });
        // Rule 2' applies WITHIN rule 2: the standard-class check above gates
        // it, so a candidate satisfying a user class is still not a default.
        // This gate must come first — running the candidate search before the
        // filter let a user class with an `Integer` instance be "defaulted"
        // silently, which the Report does not allow and GHC does not do:
        //
        //   mk :: ProbeIx i => (i, i) -> Double -> Double
        //   main = mk (0, 9) 0.0     -- + instance ProbeIx Integer
        //     → GHC: Ambiguous type variable 'i0'   (OD-50)
        //
        // When the classes ARE standard, a candidate that satisfies them all is
        // exactly "collect the constraints, then choose a type that has them
        // all": `Monoid`+`Fractional` both hold at `Double`, so
        // `mappend mempty 3.5` defaults instead of being refused.
        if !all_standard {
            return Err(CompileError::Type {
                msg: format!(
                    "Ambiguous type variable: `{}` is constrained by `{}` but no default applies",
                    ctx.env.type_spelling(&ValueTy::Var(id)),
                    classes
                        .iter()
                        .map(|(c, _, _)| c.as_str())
                        .collect::<Vec<_>>()
                        .join("`, `")
                ),
                span: first_span,
            });
        }
        // Rule 3: `default (Integer, Double)`, first candidate satisfying all.
        let chosen =
            ["Integer", "Double"]
                .into_iter()
                .find(|cand| match ctx.env.vty_of_name(cand) {
                    Some(ty) => classes
                        .iter()
                        .all(|(c, _, _)| ctx.env.instance_exists(c, &ty)),
                    None => false,
                });
        let Some(cand) = chosen else {
            return Err(CompileError::Type {
                msg: format!(
                    "Ambiguous type variable: `{}` has no instance among the default candidates                      (Integer, Double)",
                    ctx.env.type_spelling(&ValueTy::Var(id))
                ),
                span: first_span,
            });
        };
        let ty = ctx
            .env
            .vty_of_name(cand)
            .expect("the chosen candidate is a registered type");
        unify_value(&ValueTy::Var(id), &ty, &mut ctx.subst, first_span)?;
        defaulted_any = true;
        // Lowering reads the answer from the recorded dictionaries, so record
        // the defaulted instance at EVERY use site of this variable. A site with
        // no entry would make lowering re-derive the instance from its own
        // coarser static type — the second judgement this stage exists to avoid.
        //
        // A variable that several classes share (`Ix` and its `Ord`/`Eq`
        // superclasses are separate classes over one `i`) is recorded once per
        // class, and the INSTANCE must exist for each: `Ix Integer` implies
        // `Ord Integer` here because the derived/registered sets agree.
        // Same exemption as stage 2: a literal's conversion target is a kernel
        // payload (`literal_tys`), not a table entry. Writing one would make
        // `main = 1.0` advertise four dictionaries nothing reads. A variable
        // with at least one METHOD constraint keeps its entry — that is a real
        // use — and it is recorded under each class it must satisfy.
        for (class, span, literal) in &classes {
            if *literal {
                continue;
            }
            ctx.dict_selections
                .insert((*span, class.clone()), ty.clone());
        }
    }
    Ok(defaulted_any)
}

/// Whether `t` still mentions a type variable anywhere inside it.
fn contains_unresolved(t: &ValueTy, subst: &Subst) -> bool {
    let t = subst.resolve_value(t);
    match &t {
        ValueTy::Var(_) | ValueTy::TyConVar(_) | ValueTy::TyConApp(..) => true,
        ValueTy::Con(_, args) | ValueTy::Newtype(_, args) => {
            args.iter().any(|a| contains_unresolved(a, subst))
        }
        ValueTy::Thunk(a, r) => {
            a.iter().any(|x| contains_unresolved(x, subst))
                || r.iter().any(|x| contains_unresolved(x, subst))
        }
        ValueTy::Buffer(e) => contains_unresolved(e, subst),
        _ => false,
    }
}

/// Every type variable the current binding group's own types MENTION — the
/// parameter types and inferred result types of its definitions.
///
/// Stage 3 defaults only a variable ABSENT from this set (§4.3.4's `tv`/`tvs`
/// conditions in their simplest form): a variable here is part of a binding's
/// meaning, so it is the signature's or the body's to fix, never an ambiguity to
/// resolve by picking a type.
fn group_type_vars(ctx: &Ctx, group: &HashSet<String>) -> HashSet<TypeVarId> {
    let mut out = HashSet::new();
    for (name, scheme) in ctx.defs.iter() {
        // A binder this group is generalizing is not yet part of the enclosing
        // environment: the group IS what is being typed, so its own inferred
        // type cannot make its variables unambiguous. Counting it would mean
        // `main = 1 + 2` could never be defaulted — its own scheme would be the
        // very thing that "fixes" it.
        //
        // The exception is the MONOMORPHISM RESTRICTION (§4.5.5): a SIMPLE
        // PATTERN BINDING (`v = …`, no parameter list) that was NOT
        // generalized — its type still carries an unquantified variable — has
        // its variables discharged at its own close. A FUNCTION binding
        // (`f x = …`) is generalized normally and is part of the environment.
        if group.contains(name) && scheme.lam_count == 0 {
            continue;
        }
        for v in value_free_vars(&scheme.ty.vty, &ctx.subst) {
            out.insert(v);
        }
    }
    for at in ctx.locals.values() {
        for v in value_free_vars(&at.vty, &ctx.subst) {
            out.insert(v);
        }
    }
    out
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
            Expr::IntegerLit(_, _) => Ok(ValueTy::Int),
            Expr::FractionalLit(_, _, _) => Ok(ValueTy::Double),
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
        ValueTy::Con(_, a) | ValueTy::Newtype(_, a) => a.iter().any(|x| vty_mentions_tyvar(x, id)),
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
        ValueTy::Con(_, a) | ValueTy::Newtype(_, a) => {
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
        ValueTy::Con(name, a) | ValueTy::Newtype(name, a) => {
            let a = a.iter().map(|x| value_instantiate(x, fresh)).collect();
            match t {
                ValueTy::Con(..) => ValueTy::Con(name.clone(), a),
                ValueTy::Newtype(..) => ValueTy::Newtype(name.clone(), a),
                _ => ValueTy::Con(name.clone(), a),
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
///
/// `expected` is the whole `match`'s expected type, threaded into each arm body
/// exactly as `if` threads it into its branches: an arm body that IS a literal
/// takes its representation from there (`f :: Integer -> Integer; f p = case p
/// of { 2 -> 5; _ -> p }`), instead of defaulting to `Int` and contradicting the
/// declaration.
fn infer_guarded_arm_body(
    ctx: &mut Ctx,
    arm: &MatchArm,
    result: &mut Option<ValueTy>,
    expected: Option<ValueTy>,
) -> Result<(), CompileError> {
    for (g, body) in &arm.guards {
        if let Some(g) = g {
            let gt = infer_expr(ctx, g)?;
            if gt.arity_out() != 1
                || !matches!(ctx.subst.resolve_value(&gt.vty), t if t == ValueTy::bool_ty())
            {
                return Err(CompileError::Type {
                    msg: "match guard must be a Bool value".into(),
                    span: g.span(),
                });
            }
        }
        let bt = infer_expr_expected(ctx, body, expected.clone())?;
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
/// alternative carries a guard expression (the bare `-> body` form has none).
fn arm_is_guarded(arm: &MatchArm) -> bool {
    arm.guards.len() > 1 || !matches!(arm.guards.first(), Some((None, _)))
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
    match arm.guards.last() {
        // A written alternative with NO guard is the bare `pattern -> body`
        // form, which always matches.
        Some((None, _)) => true,
        Some((Some(flag), _)) => guard_is_always_true(flag),
        None => false,
    }
}

/// Whether a guard expression is unconditionally true by construction: the
/// literal `True`, or the Prelude constant `otherwise`.
fn guard_is_always_true(guard: &Expr) -> bool {
    match guard {
        // The Report spells the constructor `True`; the lowercase spelling is
        // an alias the parser resolves to the same reference.
        Expr::Ref(name, _) => name == "True" || name == "otherwise",
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
                // ANY declared type can be pattern-matched, including a
                // product: its single constructor is a constructor like any
                // other (`Void`'s field, `Pair`'s `first`/`second`). Requiring
                // more than one constructor made a product's constructor
                // unknown to the matcher.
                ValueTy::Con(n, _)
                    if ctx
                        .env
                        .data_types
                        .get(n.as_str())
                        .is_some_and(|i| !i.constructors().is_empty()) =>
                {
                    n.clone()
                }
                _ => fallback_sum.to_string(),
            };
            let is_builtin = ctx.env.type_ctor_arity(&sum_name).is_some();
            let scrutinee_args: Vec<ValueTy> = match &resolved {
                ValueTy::Con(_, a) => a.clone(),
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
        Pattern::IntegerLit(_) | Pattern::FractionalLit(..) | Pattern::LitStr(_) => {
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
    expected: Option<ValueTy>,
) -> Result<ValueTy, CompileError> {
    let mut result: Option<ValueTy> = None;
    for arm in arms {
        let saved = ctx.locals.clone();
        bind_pattern(ctx, &arm.pattern, scrutinee_vty, sum_name, arm.span)?;
        infer_guarded_arm_body(ctx, arm, &mut result, expected.clone())?;
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
        Pattern::IntegerLit(_) => {
            // An integer PATTERN literal is the same `fromInteger` leaf its
            // scrutinee has: `f :: Double -> Double; f 2 = …` is well typed in
            // Haskell 2010 (§3.17/§6.4.1), exactly like `f 2` as an argument.
            // Pinning every integer pattern to `Int` rejected that AND made the
            // lowered constant (`lower_pattern_test`) a cross-kind comparison
            // against an `Integer`/`Double` scrutinee.
            let resolved = ctx.subst.resolve_value(scrutinee_vty);
            if matches!(resolved, ValueTy::Int | ValueTy::Integer | ValueTy::Double) {
                Ok(())
            } else {
                unify_value(scrutinee_vty, &ValueTy::Int, &mut ctx.subst, arm_span)
            }
        }
        Pattern::FractionalLit(..) => {
            unify_value(scrutinee_vty, &ValueTy::Double, &mut ctx.subst, arm_span)
        }
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

/// Scalar totality: the match needs a `Wild`/`Var` arm.
///
/// A boolean match is NOT a scalar match: `Bool` is an ordinary sum type, so a
/// `case b of { False -> …; True -> … }` is checked against the sum's
/// constructor set by [`check_exhaustive_sum`], exactly like `Maybe`.
fn check_exhaustive_scalar(
    _ctx: &Ctx,
    arms: &[MatchArm],
    _scrutinee_vty: &ValueTy,
    span: Span,
) -> Result<(), CompileError> {
    let has_wild = arms.iter().any(|a| {
        (!arm_is_guarded(a) || arm_ends_unconditionally(a))
            && matches!(a.pattern.strip_bang(), Pattern::Wild | Pattern::Var(_))
    });
    if has_wild {
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
        Some(info) => info.constructors().iter().map(|c| c.name.clone()).collect(),
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
    let subst = ClassVarSubst::Ctor(String::new(), Vec::new());
    let mut fresh = || ctx.fresh_vty();
    conv(&env, "", &subst, &mut vars, &mut fresh, te)
}

fn infer_expr(ctx: &mut Ctx, e: &Expr) -> Result<ArrowTy, CompileError> {
    infer_expr_expected(ctx, e, None)
}

/// Infer a NUMERIC LITERAL — **stage 1** of the three stages, and the whole of
/// it.
///
/// Haskell 2010 §6.4.1 makes the spelling of a literal an APPLICATION:
///
/// ```text
/// 42    means  fromInteger  (42  :: Integer)  at  Num a        => a
/// 3.14  means  fromRational (a   :: Rational) at  Fractional a => a
/// ```
///
/// so a literal has NO type of its own. What it has is the RESULT VARIABLE of
/// that application, plus the class constraint its method belongs to. This
/// function therefore does exactly three things and decides nothing:
///
/// 1. allocate the result variable;
/// 2. unify it with the expected type, when the position supplies one — the
///    strongest form of the unification evidence the Report's rule reads;
/// 3. record the constraint (`Num a` / `Fractional a`) in the residual store,
///    where stage 2 discharges it against an instance and stage 3 defaults it.
///
/// No type name is read and no representation is chosen here, which is what
/// distinguishes this from the per-position heuristics it replaces (an integer
/// literal used to be typed `Int`, or `Double`, or `Integer` depending on which
/// site was looking at it).
fn infer_literal(
    ctx: &mut Ctx,
    class: &str,
    e: &Expr,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    let span = e.span();
    let v = ctx.fresh_vty();
    let id = match &v {
        ValueTy::Var(id) => *id,
        _ => unreachable!("a fresh literal type is a variable"),
    };
    // The FIRST inference of a span is the authoritative one: a definition group
    // types every body TWICE (a second pass re-infers with the real schemes so
    // signal port counts are right), and that second pass makes FRESH variables
    // for the same literals. Keeping the later variable would let the second
    // pass's `Num a` default to `Integer` even though the first pass had already
    // unified it with `Int` through the annotation beside it — `1 + (2 :: Int)`
    // lowered as mixed-kind arithmetic. Each pass is the same judgement about the
    // same source text, so the first answer is the one the substitution carries.
    //
    // The variable is recorded BEFORE the unification below because a position
    // that fixes the type immediately (`(2 :: Int)`) leaves no unresolved
    // variable to remember afterwards — and that is precisely the case lowering
    // must honour.
    if ctx.final_pass {
        // WITHIN the final pass the LAST inference of a span wins: the
        // dispatcher types an operand once to probe its type and the
        // application rule then types it again against the method's parameter,
        // and the second inference is the one whose variable the finished
        // program carries.
        ctx.literal_vars.insert((span.start, span.end), id);
    }
    if let Some(exp) = expected {
        unify_value(&v, &exp, &mut ctx.subst, span)?;
    }
    // The residual names the literal's class variable, so a later unification of
    // that variable is visible to stages 2 and 3.
    ctx.residual.push(Residual {
        class: class.to_string(),
        var: id,
        span,
        literal: true,
    });
    Ok(ArrowTy::value(ctx.subst.resolve_value(&v)))
}

/// Infer an expression with an optional **expected value type**. Used for
/// result-directed typeclass dispatch (`mempty`, `pure`, `return`): a method
/// whose signature has no class-var-applied argument is resolved by the type it
/// is expected to produce. `None` means "unknown" — such a method then errors
/// with "expected type unknown".
/// Infer `e`, recording the RESOLVED result at `e`'s span in
/// [`Ctx::value_tys`].
///
/// This is Task 10's one channel: lowering has no unification substitution, so
/// today it asks inference for an answer through seven parallel maps
/// (`literal_tys`, `literal_instances`, `runst_payload_tys`, `fn_param_tys`,
/// `dict_selections`, `shared_cell_result_tys`, `def_schemes`). Every defect the
/// Task 7 work hit — `h + 2.5`, `Ix` across a `runSTArray` lambda, `!` inside a
/// fragment — is one of those maps missing. Recording the type of EVERY
/// expression, keyed by its span, gives lowering the same answer everywhere.
///
/// The recording is ADOPTIVE while the maps still exist: it writes only, and
/// nothing reads it yet. Each later step points one consumer at it and deletes
/// the map it replaces.
fn infer_expr_expected(
    ctx: &mut Ctx,
    e: &Expr,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    let span = e.span();
    let ty = infer_expr_expected_inner(ctx, e, expected)?;
    // The RESOLVED type: a variable the substitution has solved reports its
    // solution, which is what a consumer needs. A genuinely polymorphic
    // position stays a variable, and the consumer falls back to its own
    // knowledge (a call site's instantiation).
    let resolved = ctx.subst.resolve_value(&ty.vty);
    if ty.arity_in() == 0 && ty.arity_out() == 1 {
        ctx.value_tys.insert((span.start, span.end), resolved);
    }
    Ok(ty)
}

fn infer_expr_expected_inner(
    ctx: &mut Ctx,
    e: &Expr,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    match e {
        Expr::IntegerLit(_, _) => infer_literal(ctx, "Num", e, expected),
        Expr::FractionalLit(_, _, _) => infer_literal(ctx, "Fractional", e, expected),
        Expr::Void(_) => Ok(ArrowTy::value_channel(ValueTy::Void)),
        Expr::Ref(name, span) => infer_ref_expected(ctx, name, *span, expected),
        // Unary minus keeps the operand's type: `-3` is an `Int`, `-3.0` a
        // `Double`, `-3 :: Integer` an `Integer`. Haskell 2010 spells it
        // `negate x`, whose `Num` dictionary is a later stage's slot
        // projection; the operand unification is what makes a mixed
        // `-(1) + 1.0` a type error. The expected type is threaded through, so
        // an `Integer`-expected literal under the minus types as an `Integer`
        // instead of defaulting to `Int` and failing the annotation's
        // unification.
        Expr::Neg(inner, _span) => infer_expr_expected(ctx, inner, expected),
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
        // A Prelude-internal kernel, bound at parse time. Its typing rule lives
        // in [`infer_kernel`], shared with the name-based fallback, so this node
        // and a hand-built application of the same kernel cannot disagree.
        Expr::Kernel { kernel, args, span } => infer_kernel(ctx, *kernel, args, *span, expected),
        Expr::Str(_, _) => Ok(ArrowTy::value_channel(ValueTy::String)),
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
                .filter_map(|(n, info)| info.field_named(field).map(|_| n.clone()))
                .collect();
            let rec_expected = if rec_candidates.len() == 1 {
                let rec_name = &rec_candidates[0];
                // A builtin constructor (`Pair a b`) lives in `ctor_kinds`, not
                // `data_arities`; `type_ctor_arity` is the ONE answer, so the
                // record's type args are FRESH vars, not the placeholder
                // positions `Var(1)`/`Var(2)` from [`DataInfo::Record`] — those
                // placeholders share the id space with live inference vars, so
                // leaking them corrupts the substitution (`p.second` typed as
                // raw `Var(2)`).
                let arity = ctx.env.type_ctor_arity(rec_name).unwrap_or(0);
                let args = (0..arity).map(|_| ctx.fresh_vty()).collect::<Vec<_>>();
                Some(ValueTy::Con(rec_name.clone(), args))
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
                ValueTy::Con(name, args) => match ctx.env.data_types.get(name.as_str()) {
                    // An OPAQUE type is a handle: it has no fields, and saying so
                    // is a different answer from "no field `x` here".
                    Some(DataInfo::Opaque) => Err(CompileError::Type {
                        msg: format!("opaque type `{name}` has no fields"),
                        span: *span,
                    }),
                    Some(info) => {
                        let fty = info.field_named(field).map(|(_, f)| f.ty.clone());
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
                    None => Err(CompileError::Type {
                        msg: format!("`{name}` is not a record type"),
                        span: *span,
                    }),
                },
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
                        Some(ValueTy::Con(name, args)) => ctx
                            .env
                            .data_types
                            .get(name.as_str())
                            .and_then(|info| info.field_named(field))
                            .map(|(_, ft)| {
                                if args.is_empty() {
                                    ft.ty.clone()
                                } else {
                                    substitute_field_params(&ft.ty, args)
                                }
                            }),
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
                ValueTy::Con(name, args) => {
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
                        .and_then(|info| info.field_named(field))
                        .map(|(_, f)| substitute_field_params(&f.ty, args));
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
            if arms.is_empty() {
                return Err(CompileError::Type {
                    msg: "match requires at least one arm".into(),
                    span: *span,
                });
            }
            // `_` in match-scrutinee position is the identity value wire: it is
            // a Value channel with a fresh value type, pinned to the arm-derived
            // sum type (or a literal arm's scalar type) below by unification.
            // Bare Int/Fractional literals are signal-rate channels in v1 but
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
                ctx.env.type_ctor_arity(n).map(|arity| {
                    let mut args: Vec<ValueTy> = Vec::new();
                    while args.len() < arity {
                        args.push(ctx.fresh_vty());
                    }
                    ValueTy::Con(n.clone(), args)
                })
            });
            let st = match scrutinee.as_ref() {
                Expr::IntegerLit(_, _) => ArrowTy::value_channel(ValueTy::Int),
                Expr::FractionalLit(_, _, _) => ArrowTy::value_channel(ValueTy::Double),
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
            if let ValueTy::Con(name, _) = &resolved {
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
            // A DECLARED type pins the scrutinee to itself; a builtin
            // non-declared constructor (`List`, `Map`) does not, so the arm
            // constructors decide. That is a question about the constructor
            // (`is_declared_data`), not about the type's shape.
            let scrutinee_sum = match &resolved {
                ValueTy::Con(name, _) if ctx.env.is_declared_data(name) => Some(name.clone()),
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
                let pin_ty = match ctx.env.type_ctor_arity(&sum_name) {
                    Some(arity) => {
                        let mut args: Vec<ValueTy> = match &scrutinee_vty {
                            ValueTy::Con(_, a) => a.clone(),
                            _ => vec![],
                        };
                        while args.len() < arity {
                            args.push(ctx.fresh_vty());
                        }
                        ValueTy::Con(sum_name.clone(), args)
                    }
                    None => ValueTy::Con(sum_name.clone(), vec![]),
                };
                unify_value(&scrutinee_vty, &pin_ty, &mut ctx.subst, *span)?;
                let result =
                    check_match_arms(ctx, &sum_name, &pin_ty, arms, *span, expected.clone())?;
                Ok(ArrowTy::value_channel(result))
            } else {
                // Scalar (or unknown) match: literal/Var/Wild patterns only.
                // Pin literal patterns to the scrutinee type; require totality.
                let mut result: Option<ValueTy> = None;
                for a in arms {
                    let saved = ctx.locals.clone();
                    check_scalar_pattern(ctx, &a.pattern, &scrutinee_vty, a.span)?;
                    infer_guarded_arm_body(ctx, a, &mut result, expected.clone())?;
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
            if ctx.subst.resolve_value(&ct.vty) != ValueTy::bool_ty() {
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
            Ok(ArrowTy::value_channel(ValueTy::Con(
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
            Ok(ArrowTy::value_channel(ValueTy::Con(
                "Map".into(),
                vec![ValueTy::String, val_ty],
            )))
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
                // The expected type may still be a VARIABLE that a sibling
                // argument will constrain (`mappend mempty 3.5`: `mempty`'s
                // `m` is the same `a` the literal makes `Fractional`). Haskell
                // 2010 COLLECTS that constraint and chooses at the group's
                // close; demanding a concrete type here is choosing too early.
                //
                // So an unresolved selector is not an error: the class
                // variable is unified WITH it (`ClassVarSubst::Var`) and a
                // residual records the demand. Stage 2 will find the instance
                // once the sibling (or the group's default) has fixed the type,
                // and a type no stage fixes is still reported — at the right
                // stage, by `default_residuals`.
                let resolved_exp = ctx.subst.resolve_value(&exp);
                let deferred = matches!(
                    resolved_exp,
                    ValueTy::Var(_) | ValueTy::TyConVar(_) | ValueTy::TyConApp(..)
                );
                let (ty_name, cv_subst) = if deferred {
                    // Keep the class variable live as the expected type itself,
                    // so every later unification of that variable is the
                    // method's class variable.
                    if let ValueTy::Var(id) = resolved_exp {
                        ctx.residual.push(Residual {
                            class: class_name.clone(),
                            var: id,
                            span,
                            literal: false,
                        });
                    }
                    // A deferred call is a POLYMORPHIC value: the method keeps
                    // its signature's shape, instantiated with the class
                    // variable left as the live sibling variable. There is no
                    // instance body to inline yet — stage 2 picks the instance
                    // once the variable is solved, and lowering asks for the
                    // SAME answer through `dict_selections`.
                    let subst = match &resolved_exp {
                        ValueTy::Var(id) => ClassVarSubst::Var(*id),
                        _ => ClassVarSubst::Ty(resolved_exp.clone()),
                    };
                    // `mempty: m` is a NULLARY method: its VALUE is the class
                    // variable itself. `signature_param_and_result_tys_conv`
                    // reports a nullary signature's result in its FIRST slot
                    // (there is no arrow to split), so the shape is read from
                    // `sig` here rather than guessed from the arity — routing a
                    // bare `m` through the curried application would wrap it in
                    // a `Thunk`, which no class variable can unify with.
                    let env = ctx.env.clone();
                    // `signature_param_and_result_tys_conv` reports `None` for
                    // the result EXACTLY when the signature is not a function
                    // type — a nullary method. That is the reliable signal: a
                    // `sig_arg_count()` on the wrapped spelling can disagree.
                    let mut it = (0..8).map(|_| ctx.fresh_vty());
                    let (first, ret) =
                        signature_param_and_result_tys_conv(&env, &class_var, &subst, sig, || {
                            it.next().unwrap_or(ValueTy::Double)
                        });
                    let nullary = ret.is_none();
                    if nullary {
                        let value_ty = ret
                            .or_else(|| first.first().cloned())
                            .unwrap_or_else(|| resolved_exp.clone());
                        // Unify with the sibling's variable so the collected
                        // constraints land on ONE type: `mempty`'s `m` and the
                        // literal's `a` must be the same variable, which is what
                        // lets `3.5`'s `Fractional` reach this residual.
                        unify_value(&value_ty, &resolved_exp, &mut ctx.subst, span)?;
                        return Ok(ArrowTy::value_channel(resolved_exp));
                    }
                    let param_tys = first;
                    let ret = ret.unwrap_or_else(|| resolved_exp.clone());
                    let what = format!("method `{name}` of `{class_name}`");
                    return apply_args_curried(
                        ctx,
                        &param_tys,
                        &ret,
                        &[],
                        span,
                        &what,
                        ArgUnify::Exact,
                    );
                } else {
                    match method_dispatch_key(
                        &env,
                        &class_name,
                        &class_var,
                        sig,
                        &exp,
                        cv_var,
                        || ctx.fresh_vty(),
                    ) {
                        Some(pair) => pair,
                        None => {
                            return Err(CompileError::Type {
                                msg: format!(
                                    "cannot resolve method `{name}` of `{class_name}`: the expected type is not concrete"
                                ),
                                span,
                            });
                        }
                    }
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
                    let what = format!("method `{name}` of `{class_name}`");
                    return apply_args_curried(
                        ctx,
                        &param_tys,
                        &ret,
                        &[],
                        span,
                        &what,
                        ArgUnify::Exact,
                    );
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
                // The inline itself is the shared act (see
                // `inline_instance_body`): it binds the instance head arguments
                // from the expected container, takes the recursion guard,
                // restores the head arguments into the parameter types, pushes
                // the instance context as a GIVEN, and types the body against
                // the caller expectation. A bare, result-directed name has no
                // call-site arguments, so `arg_vtys` is empty and the parameter
                // types come from the signature.
                let body_vty = inline_instance_body(
                    ctx,
                    InlineCall {
                        class_name: &class_name,
                        ty_name: &ty_name,
                        container: Some(&exp),
                        method: name,
                        class_var: &class_var,
                        cv_subst: ClassVarSubst::Ty(ValueTy::Con(ty_name.clone(), Vec::new())),
                        sig,
                        body: &body,
                        params: &params,
                        arg_vtys: &[],
                        param_bindings: ParamBindings::Signature,
                        body_expected: Some(exp.clone()),
                        span,
                    },
                )?;
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
                        let what = format!("method `{name}` of `{class_name}`");
                        return apply_args_curried(
                            ctx,
                            &param_tys,
                            &ret,
                            &[],
                            span,
                            &what,
                            ArgUnify::Exact,
                        );
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
                    // The body is TYPED (not returned): that is what makes the
                    // dispatch legal and records the nested selections. Its own
                    // type is discarded because the method CONTRACT is the class
                    // signature below — `fold mappend …` must compare against
                    // `mappend : m -> m -> m`, not against an implementation's
                    // incidental parameter shapes.
                    inline_instance_body(
                        ctx,
                        InlineCall {
                            class_name: &class_name,
                            ty_name: &ty_name,
                            container: Some(exp),
                            method: name,
                            class_var: &class_var,
                            cv_subst: ClassVarSubst::Ty(cand_ty.clone()),
                            sig,
                            body: &body,
                            params: &params,
                            arg_vtys: &[],
                            param_bindings: ParamBindings::Arguments,
                            body_expected: None,
                            span,
                        },
                    )?;
                    // The method's CONTRACT is its class signature with the
                    // class variable substituted by the dispatched instance —
                    // not the instance body's incidental type, which would make
                    // `fold mappend …` compare against the implementation's own
                    // parameter names.
                    let cv_subst = ClassVarSubst::Ty(cand_ty);
                    let env = ctx.env.clone();
                    let (arg_tys, ret_ty) = signature_param_and_result_tys_conv(
                        &env,
                        &class_var,
                        &cv_subst,
                        sig,
                        || ctx.fresh_vty(),
                    );
                    // The method's contract is its curried signature: a
                    // nullary method's single signature type stands alone, a
                    // function-shaped one is the unapplied arrow.
                    if let Some(ret) = ret_ty {
                        let what = format!("method `{name}` of `{class_name}`");
                        return apply_args_curried(
                            ctx,
                            &arg_tys,
                            &ret,
                            &[],
                            span,
                            &what,
                            ArgUnify::Exact,
                        );
                    }
                    return Ok(ArrowTy::value_channel(
                        arg_tys.first().cloned().unwrap_or(ValueTy::Double),
                    ));
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
            // A PRODUCT's type name doubles as its constructor, so it is a
            // value. A SUM's name is not a constructor: the program must apply
            // one of them.
            DataInfo::Data(ctors) if ctors.len() == 1 => {
                return Ok(ArrowTy::value_channel(ValueTy::Con(name.into(), vec![])))
            }
            DataInfo::Data(_) => {
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
            return Ok(ArrowTy::value_channel(ValueTy::Con(
                "List".into(),
                vec![ctx.fresh_vty()],
            )));
        }
        "empty_map" => {
            return Ok(ArrowTy::value_channel(ValueTy::Con(
                "Map".into(),
                vec![ValueTy::String, ValueTy::Double],
            )))
        }
        "empty_set" => {
            return Ok(ArrowTy::value_channel(ValueTy::Con(
                "Set".into(),
                vec![ValueTy::Double],
            )))
        }
        _ => {}
    }
    let ctor_sums = sum_types_with_ctor(ctx, name);
    if !ctor_sums.is_empty() {
        // A constructor is an ordinary curried VALUE (Haskell 2010 §4.2.1):
        // `Just :: a -> Maybe a`, `Nothing :: Maybe a`. Its type is built from
        // its sum, so an UNAPPLIED reference is the whole arrow (or, for a
        // nullary constructor, the plain sum value) — exactly what the shared
        // rule returns for zero arguments.
        let sum_name = if ctor_sums.len() == 1 {
            ctor_sums[0].clone()
        } else {
            // If the ctor name is shared, the ambiguity is reported rather than
            // resolved by map order.
            return Err(CompileError::Type {
                msg: format!("ambiguous constructor `{name}` (declared in multiple sum types)"),
                span,
            });
        };
        let (params, ret) = sum_ctor_scheme(ctx, &sum_name, name);
        let what = format!("constructor `{name}`");
        return apply_args_curried(ctx, &params, &ret, &[], span, &what, ArgUnify::Exact);
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
                // A LIVE class variable with a recorded demand, not the old
                // `u32::MAX` SENTINEL: the sentinel produced
                // `Thunk([Var(4294967295)], [ret])`, a type nothing could unify
                // with. The variable must survive to the stage-2/3 solver.
                let cv_var = ctx.fresh_id();
                ctx.residual.push(Residual {
                    class: class_name.clone(),
                    var: cv_var,
                    span,
                    literal: false,
                });
                let mut fresh_ids: Vec<ValueTy> = Vec::new();
                for _ in 0..8 {
                    fresh_ids.push(ctx.fresh_vty());
                }
                let mut it = fresh_ids.into_iter();
                let (param_tys, ret) = signature_param_and_result_tys_conv(
                    &ctx.env,
                    &class_var,
                    &ClassVarSubst::Var(cv_var),
                    &sig,
                    || it.next().unwrap_or(ValueTy::Double),
                );
                let ret = ret.unwrap_or_else(|| ctx.fresh_vty());
                let what = format!("method `{name}` of `{class_name}`");
                return apply_args_curried(
                    ctx,
                    &param_tys,
                    &ret,
                    &[],
                    span,
                    &what,
                    ArgUnify::Exact,
                );
            }
            return Err(CompileError::Type {
                msg: format!("method `{name}` of `{class_name}` requires an argument"),
                span,
            });
        }
    }
    if matches!(name, "sin" | "cos" | "tan" | "sqrt" | "exp" | "ln" | "tanh") {
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
        // A bare reference instantiates the definition's scheme, giving the
        // reference that definition's FUNCTION type — `at1` is
        // `Array i e -> e`, so a bare `at1` is a function value of that type,
        // not an unknown signature. Substituting an empty `Thunk([], [])` here
        // severed the ONE link between a definition and its use: the scheme's
        // constraints were never pushed, so a reference carried no class demand
        // and every call site through it had to GUESS the dictionary. Spec
        // §3.3 states the rule this restores — a constraint is a PARAMETER, and
        // a reference is where the parameter is passed.
        let (t, constraints) = ctx.instantiate_with_constraints(&scheme);
        for (class, var) in constraints {
            ctx.residual.push(Residual {
                class,
                var,
                span,
                literal: false,
            });
        }
        return Ok(t);
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
            // A synonym HEAD expands to its target (`type L = List` makes
            // `L Int` mean `List Int`), so both spellings denote one type.
            let (head, args) = ctx.env.expand_alias_head(head, args.clone());
            let mut targs = Vec::with_capacity(args.len());
            for a in args {
                targs.push(type_expr_to_vty_inner(ctx, &a)?);
            }
            // A user data type (`IO a`, `Kleisli m a b`) is a `Data` application
            // (mirroring how record construction types it), so `IO` unifies
            // consistently between builtin signatures and do-blocks. A buffer
            // CHANNEL constructor becomes the buffer value type, its element
            // checked like anywhere else.
            Ok(ctx.env.ctor_application(&head, targs))
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
            // A synonym HEAD expands to its target (`type L = List` makes
            // `L Int` mean `List Int`), so both spellings denote one type.
            let (head, args) = ctx.env.expand_alias_head(head, args.clone());
            let mut targs = Vec::with_capacity(args.len());
            for a in args {
                targs.push(instantiate_builtin_sig_vty_inner(ctx, &a, vars)?);
            }
            // One representation: a declared type and a builtin constructor
            // both build the one `Con` application.
            Ok(ValueTy::Con(head, targs))
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
    let ret = ret_tys.first().cloned().unwrap_or(ValueTy::Double);
    apply_args_curried(
        ctx,
        &arg_tys,
        &ret,
        args,
        span,
        "expression",
        ArgUnify::Exact,
    )
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
        // Any CONCRETE expectation pins the result — not only a `Con`/`Newtype`.
        // Enumerating two variants left the scalar leaf types out, and they are
        // not `Con`s: `Int`, `Double`, `Integer`, `String` and `()` are their own
        // `ValueTy` variants. So `x :: Int; x = mk3 0.0` with
        // `mk3 :: ProbeIx i => Double -> i` unified the class variable with
        // NOTHING, its `ProbeIx` residual stayed open, and the program was
        // rejected as ambiguous — while GHC solves `i := Int` from the position.
        //
        // The predicate is the same one lowering uses to decide whether a type
        // can name an instance (`is_concrete_ty`): an unresolved variable or a
        // function type pins nothing.
        if is_concrete_vty(&exp) {
            let ret = ctx.subst.resolve_value(ret);
            unify_value(&ret, &exp, &mut ctx.subst, span)?;
        }
    }
    Ok(())
}

/// Whether a value type carries no unresolved variable — the SAME judgement as
/// lowering's `is_concrete_ty`, stated once for inference.
///
/// A function type is not concrete HERE even when its parts are: an expectation
/// of `a -> b` must not force a call's result to be a function, because the call
/// may still be a partial application whose remainder the signature describes.
fn is_concrete_vty(t: &ValueTy) -> bool {
    match t {
        ValueTy::Var(_) | ValueTy::TyConVar(_) | ValueTy::TyConApp(..) | ValueTy::Thunk(..) => {
            false
        }
        ValueTy::Dict(_) => false,
        ValueTy::Con(_, args) | ValueTy::Newtype(_, args) => args.iter().all(is_concrete_vty),
        ValueTy::Buffer(elem) => is_concrete_vty(elem),
        _ => true,
    }
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
    sig: &crate::ast::TypeExpr,
    args: &[Expr],
    span: Span,
    expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    let (param_tys, ret_ty, vars) = def_sig_tys(ctx, sig)?;
    // No arity comparison with the definition here: that judgement is made ONCE
    // at the declaration (`infer_constrained_def_body`, which HAS the body to
    // count λ-parameters from), and a declaration whose body cannot be typed is
    // rejected before any call site exists. Repeating it here would need the
    // definition back, which is precisely what this path no longer takes.
    // Pin the result to the caller's expected type before the arguments infer,
    // so a result-directed method inside an argument resolves by the container
    // this call must produce (mirrors the plain function path).
    //
    // Arity comes FIRST: a surplus argument is the call's own arity error and
    // must be reported before any argument's typing, exactly as before the
    // application rule was shared.
    let what = format!("`{name}`");
    check_application_arity(param_tys.len(), args.len(), span, &what)?;
    pin_result_to_expected(ctx, &ret_ty, expected.as_ref(), span)?;

    // PARK M9: a bare named function reference (`f` in `ixmap b f a`) infers as
    // the EMPTY `Thunk([], [])` — a bare ref's arity resolves only when it is
    // applied — so a function-shaped constrained parameter (`i -> j`) reports an
    // opaque `Thunk … with Thunk …` mismatch. Name the parameter and point at the
    // missing annotation (or a lambda literal) instead of dumping thunk shapes.
    //
    // This is a call-site diagnostic, not part of the application rule, so it
    // stays here. It recognizes statically exactly the references that type as
    // the empty `Thunk([], [])`: a definition with λ-parameters (which
    // `infer_ref_inner` spells as the unapplied placeholder) or one whose scheme
    // is already the empty thunk, and which no local shadows.
    for (i, (a, pt)) in args.iter().zip(param_tys.iter()).enumerate() {
        if !matches!(pt, ValueTy::Thunk(..)) {
            continue;
        }
        let Expr::Ref(rname, _) = a else { continue };
        if ctx.locals.contains_key(rname) {
            continue;
        }
        let unresolved_thunk = ctx.defs.get(rname).is_some_and(|s| {
            s.lam_count > 0
                || matches!(&s.ty.vty, ValueTy::Thunk(c, r) if c.is_empty() && r.is_empty())
        });
        if !unresolved_thunk {
            continue;
        }
        // The parameter NAME is diagnostic only, so it is recovered from the
        // body rather than kept in a registry: a definition's parameters ARE its
        // body's leading λ-nest (Haskell 2010 §4.4.3.1), and the signature's
        // arity says how many λs belong to the head.
        let pname = ctx
            .def_bodies
            .get(name)
            .and_then(|body| {
                crate::types::ty::split_curried_params(sig.arg_count(), &[], body)
                    .0
                    .get(i)
                    .map(|p| p.name.clone())
            })
            .unwrap_or_else(|| "<unnamed>".to_string());
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
    let ret = ctx.subst.resolve_value(&ret_ty);
    let at = apply_args_curried(ctx, &param_tys, &ret, args, span, &what, ArgUnify::Exact)?;
    let bindings: HashMap<String, ValueTy> = vars
        .iter()
        .map(|(k, v)| (k.clone(), ctx.subst.resolve_value(v)))
        .collect();
    discharge_def_constraints(&ctx.env, name, sig, &bindings, span)?;
    // A class variable the call site did NOT solve is a constraint of THIS use
    // site, carried as a residual for the same stages that answer a method
    // call's class variable (Haskell 2010 §4.3.4). Without this the demand was
    // dropped, `i` kept only the literals' `Num`, and defaulting silently chose
    // `Integer` — accepting `mk (0, 9) 0.0`, which GHC rejects as ambiguous
    // even when a `ProbeIx Integer` instance exists (OD-50).
    //
    // A LITERAL's variable is exempt for the same reason stage 2 exempts one:
    // its conversion target is a kernel payload, not a dictionary.
    let arity = |class: &str| ctx.env.typeclasses.get(class).map(|c| c.arity).unwrap_or(0);
    for (class, var) in sig_constraints(sig) {
        let Some(bound) = vars.get(var) else {
            continue;
        };
        let ValueTy::Var(id) = ctx.subst.resolve_value(bound) else {
            continue;
        };
        if ctx.env.class_of_method(name).is_some() && arity(class) > 0 {
            // A constructor class's variable is an application HEAD
            // (`TyConVar`), not a `Var`; those never reach a residual.
            continue;
        }
        ctx.residual.push(Residual {
            class: class.clone(),
            var: id,
            span,
            literal: false,
        });
    }
    Ok(at)
}

/// Whether a call `name args…` must resolve to a DEFINITION rather than to a
/// class method of the same name.
///
/// A plain user definition shadows a class method outright (a user `eq`/`lt` is
/// a plain function). A class-CONSTRAINED definition that shares its name with
/// a method is DUAL: the FIRST argument's static type decides — an argument that
/// solves the method's class variable (`Pair Int Int` for `Ix`) routes to the
/// method, anything else to the definition. Lowering applies the same predicate
/// to the same recovered type, so the passes cannot disagree.
///
/// The PRELUDE no longer contains such a name: `!` is the array accessor and
/// `index` the pure `Ix` method, one role each.
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
/// own witness satisfies the method's class-var pattern, the call is the
/// METHOD. That is what keeps `checkedOffset :: Ix i => …`'s `index b ix`
/// (the offset of an abstract index) on the METHOD path rather than being read
/// as any same-named definition in scope.
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

/// Type an application of one of the PRELUDE's internal kernels
/// ([`crate::ast::Kernel`]).
///
/// This is the ONE typing rule for the kernels, reached from the
/// [`Expr::Kernel`](crate::ast::Expr::Kernel) node by inference and — for a
/// hand-built AST that still spells one as an application — by the name-based
/// fallback in [`infer_apply_impl`]. Both routes agree because both call here,
/// which is what the two copies of this rule previously could not guarantee.
///
/// None of the three is SHADOWABLE, and none needs to be: a kernel is not a
/// name in the language any more (see [`crate::ast::Expr::Kernel`] for why a
/// reserved spelling was not enough). The `Ratio` instance bodies are inlined
/// into their call sites, so a program's own `(%)` or `gcd` must never be able
/// to answer `a / b` or the Prelude's `gcd` — binding the operation at the node
/// makes that structural rather than a lookup order a later pass can undo.
fn infer_kernel(
    ctx: &mut Ctx,
    kernel: crate::ast::Kernel,
    args: &[Expr],
    span: Span,
    _expected: Option<ValueTy>,
) -> Result<ArrowTy, CompileError> {
    let name = kernel.spelling();
    match kernel {
        // `__fault msg` — the Prelude's internal bottom. Its result type is a
        // FRESH variable: it is bottom, so the call site's expectation decides
        // it, exactly like `error`'s.
        crate::ast::Kernel::Fault => {
            if args.len() != 1 {
                return Err(CompileError::Type {
                    msg: format!("`{name}` expects 1 argument, got {}", args.len()),
                    span,
                });
            }
            let msg = infer_const_value(ctx, &args[0])?;
            if ctx.subst.resolve_value(&msg) != ValueTy::String {
                return Err(CompileError::Type {
                    msg: format!("`{name}` expects a String message, got {msg:?}"),
                    span: args[0].span(),
                });
            }
            let ret = ctx.fresh_vty();
            Ok(ArrowTy::value_channel(ret))
        }
        // `__ratio_percent n d` and `__gcd a b` — both `Integer -> Integer -> …`
        // (there is no `Integral` class to generalise over: design A6 / OD-24).
        crate::ast::Kernel::RatioPercent | crate::ast::Kernel::Gcd => {
            if args.len() != 2 {
                return Err(CompileError::Type {
                    msg: format!("`{name}` expects 2 argument(s), got {}", args.len()),
                    span,
                });
            }
            for (i, a) in args.iter().enumerate() {
                let t = infer_method_value_vty(ctx, a, "argument")?;
                if let Err(e) = unify_value(&t, &ValueTy::Integer, &mut ctx.subst, a.span()) {
                    return Err(CompileError::Type {
                        msg: format!(
                            "`{name}` expects Integer argument {}, got {t:?}: {e} (no \
                             `Integral` class exists, so this kernel is Integer-only — OD-24)",
                            i + 1
                        ),
                        span: a.span(),
                    });
                }
            }
            let ret = if kernel == crate::ast::Kernel::RatioPercent {
                ValueTy::Con("Ratio".into(), vec![ValueTy::Integer])
            } else {
                ValueTy::Integer
            };
            Ok(ArrowTy::value_channel(ret))
        }
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
            ValueTy::Con(n, st_args) if n == "ST" && st_args.len() == 2 => {
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
                (true, ValueTy::Con(n, args)) if n == "STArray" && args.len() == 3 => {
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
                .resolve_value(&ValueTy::Con("Array".into(), vec![index, elem]));
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
                ValueTy::Con(..) | ValueTy::Newtype(..) | ValueTy::Thunk(..)
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
        // A builtin signature is an ordinary curried function type, so its
        // arity is the shared rule's judgement: `write (Just 1.0)` is a partial
        // application (GHC accepts it) and yields `OutputStream a -> IO ()`.
        // Every argument is inferred AT its declared parameter type, which is
        // what teaches a result-directed method inside the argument (`show_int
        // (fromInteger 2)`) its instance: `fromInteger : Int -> a` has nothing
        // in its own arguments to solve `a`.
        //
        // A `Buffer` parameter is a buffer-CONSTANT position (a foreign
        // builtin's signal slot): a numeric literal there is the block's
        // constant, Haskell 2010 §6.4.1 `fromInteger`-polymorphism read against
        // the `Buffer` the slot demands (`arrayFromBuffer 1`, `lowpass 1000.0
        // 0.7 1.0`). That is the same rule the operator/method path applies, and
        // it is why the catalog passes `BufferConstant` rather than `Exact`.
        // It does NOT widen a non-literal: `((\b -> b) 1.0) :: Buffer` stays
        // rejected at whichever site applies it.
        let what = format!("`{name}`");
        return apply_args_curried(
            ctx,
            &param_tys,
            &ret_ty,
            args,
            span,
            &what,
            ArgUnify::BufferConstant,
        );
    }
    if name == "not" {
        if args.len() != 1 {
            return Err(CompileError::Type {
                msg: format!("`not` expects 1 argument, got {}", args.len()),
                span,
            });
        }
        let arg_vty = infer_method_value_vty(ctx, &args[0], "argument")?;
        if arg_vty != ValueTy::bool_ty() {
            return Err(CompileError::Type {
                msg: format!("`not` expects a Bool argument, got {arg_vty:?}"),
                span: args[0].span(),
            });
        }
        return Ok(ArrowTy::value_channel(ValueTy::bool_ty()));
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
    // A reserved kernel spelling that reached THIS name-based path (only the
    // PRELUDE's own `numerator`/`denominator`-style bodies and a program text
    // parsed before the node existed can): type it exactly like the node form.
    // The node ([`Expr::Kernel`]) is the REAL carrier — this is the fallback,
    // kept so a hand-built AST in a test types the same way a parsed one does.
    if let Some(kernel) = crate::ast::Kernel::from_reserved_spelling(name) {
        return infer_kernel(ctx, kernel, args, span, expected);
    }
    // `error :: String -> a` (Haskell 2010 Prelude §9.1). Its result type is a
    // FRESH variable: it is bottom, so the call site's expectation decides it
    // and a use in `Ratio a`'s field position types just as one at `Double`
    // does. A non-`String` message is rejected here, before lowering. A USER
    // definition of the name shadows it (a Prelude name is a binding, not a
    // keyword), the same precedence every other builtin name follows.
    if name == "error" && !ctx.locals.contains_key(name) && !ctx.defs.contains_key(name) {
        if args.len() != 1 {
            return Err(CompileError::Type {
                msg: format!("`error` expects 1 argument, got {}", args.len()),
                span,
            });
        }
        let msg = infer_const_value(ctx, &args[0])?;
        if ctx.subst.resolve_value(&msg) != ValueTy::String {
            return Err(CompileError::Type {
                msg: format!("`error` expects a String message, got {msg:?}"),
                span: args[0].span(),
            });
        }
        let ret = ctx.fresh_vty();
        return Ok(ArrowTy::value_channel(ret));
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
            DataInfo::Data(fields) => {
                // A product constructor's fields, in declaration order. The
                // record literal path below matches them by NAME; a positional
                // call path takes them by POSITION.
                let fields: Vec<(String, ValueTy, bool)> = fields
                    .first()
                    .map(|c| {
                        c.fields
                            .iter()
                            .map(|f| (f.name.clone().unwrap_or_default(), f.ty.clone(), f.strict))
                            .collect()
                    })
                    .unwrap_or_default();
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
                // A parameterized record — a builtin shape (`Pair a b`) or a
                // declared `data Kleisli m a b` alike — carries PLACEHOLDER
                // type parameters (`Var(1)`, `Var(2)`) that are NOT live
                // unification vars. Freshen each placeholder to a fresh
                // live var BEFORE unifying the fields: unifying the raw
                // ids would bind them in the live subst, colliding with
                // subsequently freshened ids (`Pair { first: 1.0,
                // second: 2 }` failed on the second field).
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
                        ValueTy::Con(n, a) => ValueTy::Con(
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
                        ValueTy::Con(n, a) if n == *name => Some(a.clone()),
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
                // A PARAMETERIZED record (a builtin shape `Pair a b`, a
                // declared `data Kleisli m a b`, `data ST s a`): resolve each
                // type parameter from the substitution built by unifying the
                // fields. `Var(k+1)` and the `TyConApp` head share one id
                // space, so resolving `Var(k+1)` yields the concrete type
                // (m → Maybe, a → Double, b → Double).
                match ctx.env.type_ctor_arity(name) {
                    Some(arity) if arity > 0 => {
                        let arg_tys: Vec<ValueTy> = (0..arity)
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
                        return Ok(ArrowTy::value_channel(ValueTy::Con(name.into(), arg_tys)));
                    }
                    // A MONOMORPHIC record: it carries no type arguments. The
                    // freshly resolved field types are not its type args.
                    _ => {
                        return Ok(ArrowTy::value_channel(ValueTy::Con(name.into(), vec![])));
                    }
                }
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
        // The newtype's declared TARGET type is the single argument's expected
        // type — the same rule a formal parameter follows, and a DECLARATION
        // rather than a guess. Without it `Hz 9223372036854775808` inferred the
        // literal on the `Int` track and then failed to unify against the
        // declared `Double` (an error where GHC accepts the program, since a
        // literal is `fromInteger`-polymorphic). A target that names no
        // concrete type supplies no expectation.
        let inner = resolve_type_name(&ctx.env, &inner_name);
        let vt = infer_const_value_expected(ctx, &args[0], Some(&inner))?;
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
        // A constructor IS an ordinary curried function (Haskell 2010 §4.2.1):
        // `Just :: a -> Maybe a`, so the ARITY judgement belongs to the shared
        // application rule. Fewer arguments than the payload is a PARTIAL
        // application and yields the curried remainder (`g = Just`, `map Just`);
        // exactly the payload is the sum value; more is an error. The scheme's
        // payload placeholders are instantiated together with the result's type
        // parameters, so the argument binds the sum's parameter (`Just 5.0` is
        // `Maybe Double`, `Left 5.0` is `Either Double ?` — the match pin pads
        // the scrutinee to `arity`, so the arities must agree).
        let (params, ret) = sum_ctor_scheme(ctx, &sum_name, name);
        let what = format!("constructor `{name}`");
        return apply_args_curried(ctx, &params, &ret, args, span, &what, ArgUnify::Exact);
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
            if let Some(given) = given_for_method_call(
                ctx,
                &class_name,
                &class_var,
                sig,
                args,
                expected.as_ref(),
                span,
            )? {
                let at = infer_method_under_given(ctx, &class_var, &given, sig, args, span)?;
                record_selection(ctx, span, &class_name, &ValueTy::Dict(given.class.clone()));
                return Ok(at);
            }
            // No GIVEN witness. If the body is under a declared context at all,
            // the call must still be one the context permits: an ABSTRACT
            // operand with no witness for its class is exactly what Haskell 2010
            // §4.1.3 forbids (`f :: a -> a -> Bool; f x y = x < y` → "add (Ord
            // a) to the context"). A concrete operand needs no witness — that is
            // an ordinary call-site dispatch (`length` on a real list, `(+)` on
            // `Double` inside a `Num a =>` body).
            //
            // The check is here rather than in `given_for_method_call` because
            // that function returns early when `ctx.given` is EMPTY, which is
            // precisely the `f :: a -> a -> Bool` case.
            {
                let arity = ctx
                    .env
                    .typeclasses
                    .get(class_name.as_str())
                    .map(|c| c.arity)
                    .unwrap_or(0);
                let selector = if arity == 0 {
                    class_var_selector_index(sig, &class_var, args.len(), |i| {
                        is_bare_nullary_method_ref(ctx, &args[i], &class_name)
                    })
                } else {
                    ctx.env.class_var_arg_index(sig, &class_var)
                };
                if let Some(i) = selector.filter(|i| *i < args.len()) {
                    // Only when the class variable actually OCCURS in that
                    // parameter pattern: `fromInteger : Integer -> a` names `a`
                    // in its RESULT, so its `Integer` operand says nothing about
                    // the class and must not be read as evidence.
                    let mentions =
                        crate::types::ty::class_var_param_indices(sig, &class_var).contains(&i);
                    if mentions {
                        // A PROBE, rolled back exactly as `try_dispatch` does:
                        // typing the operand again must not leave a second
                        // residual behind (that is what reported `Ambiguous type
                        // variable: a is constrained by Arrow`).
                        let subst_mark = ctx.subst.clone();
                        let next_mark = ctx.next;
                        let residual_mark = ctx.residual.len();
                        let at = infer_method_value_vty(ctx, &args[i], "argument")?;
                        let at = ctx.subst.resolve_value(&at);
                        ctx.subst = subst_mark;
                        ctx.next = next_mark;
                        ctx.residual.truncate(residual_mark);
                        require_context_witness(ctx, &class_name, &at, span)?;
                    }
                } else if args.is_empty() && expected.is_none() {
                    // A NULLARY method (`mempty`, `fromInteger` with no operand
                    // to inspect) has no selector argument: its evidence is the
                    // RESULT, which only the caller's expectation can fix.
                    // With neither, there is nothing to check here.
                }
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
                //
                // With NO expectation the call is DEFERRED, not rejected: the
                // class variable stays LIVE and the demand is recorded, so a
                // stage-2/3 solve (or a later sibling) can settle it. This is
                // the same rule the bare-reference path applies — one mechanism,
                // not two — and it is what lets `arr` inside a larger expression
                // wait for the container its context supplies.
                let Some(exp) = expected else {
                    let cv_var = ctx.fresh_id();
                    ctx.residual.push(Residual {
                        class: class_name.clone(),
                        var: cv_var,
                        span,
                        literal: false,
                    });
                    let subst = ClassVarSubst::Var(cv_var);
                    let env = ctx.env.clone();
                    let mut it = (0..8).map(|_| ctx.fresh_vty());
                    let (param_tys, ret) =
                        signature_param_and_result_tys_conv(&env, &class_var, &subst, sig, || {
                            it.next().unwrap_or(ValueTy::Double)
                        });
                    // A NULLARY method's value IS the container, with no arrow to
                    // apply — `signature_param_and_result_tys_conv` reports `None`
                    // for its result exactly then.
                    if ret.is_none() {
                        let value_ty = param_tys
                            .first()
                            .cloned()
                            .unwrap_or_else(|| ctx.fresh_vty());
                        return Ok(ArrowTy::value_channel(value_ty));
                    }
                    let ret_ty = ret.unwrap_or_else(|| ctx.fresh_vty());
                    let what = format!("method `{name}` of `{class_name}`");
                    return apply_args_curried(
                        ctx,
                        &param_tys,
                        &ret_ty,
                        args,
                        span,
                        &what,
                        ArgUnify::Exact,
                    );
                };
                let env = ctx.env.clone();
                // An expectation that is still a VARIABLE is not a failure: it
                // is the ordinary mid-inference state (Haskell 2010 §4.3.4).
                // `fromInteger 2 + 0.5` has ONE `a` shared by `(+)`, `Num a`
                // and `Fractional a`; GHC measures Core
                // `(+ @Double $fNumDouble (fromInteger @Double $fNumDouble
                // (IS 2#)) (D# 0.5##))` — the instance is chosen by DEFAULTING
                // at the group's close, not by a concrete type coming down.
                // Defer exactly like the `None` arm above: keep the class
                // variable live as the sibling's variable and let the residual
                // carry the demand to stages 2–3.
                let resolved_exp2 = ctx.subst.resolve_value(&exp);
                let exp_is_open = matches!(
                    resolved_exp2,
                    ValueTy::Var(_) | ValueTy::TyConVar(_) | ValueTy::TyConApp(..)
                );
                let cv_var = ctx.fresh_id();
                if exp_is_open {
                    if let ValueTy::Var(id) = resolved_exp2 {
                        ctx.residual.push(Residual {
                            class: class_name.clone(),
                            var: id,
                            span,
                            literal: false,
                        });
                    }
                    let subst = match &resolved_exp2 {
                        ValueTy::Var(id) => ClassVarSubst::Var(*id),
                        _ => ClassVarSubst::Ty(resolved_exp2.clone()),
                    };
                    let env2 = ctx.env.clone();
                    let mut it = (0..8).map(|_| ctx.fresh_vty());
                    let (param_tys, ret) =
                        signature_param_and_result_tys_conv(&env2, &class_var, &subst, sig, || {
                            it.next().unwrap_or(ValueTy::Double)
                        });
                    if ret.is_none() {
                        let value_ty = param_tys
                            .first()
                            .cloned()
                            .unwrap_or_else(|| ctx.fresh_vty());
                        let what = format!("method `{name}` of `{class_name}`");
                        return apply_args_curried(
                            ctx,
                            &param_tys,
                            &value_ty,
                            args,
                            span,
                            &what,
                            ArgUnify::Exact,
                        );
                    }
                    let ret_ty = ret.unwrap_or_else(|| ctx.fresh_vty());
                    let what = format!("method `{name}` of `{class_name}`");
                    return apply_args_curried(
                        ctx,
                        &param_tys,
                        &ret_ty,
                        args,
                        span,
                        &what,
                        ArgUnify::Exact,
                    );
                }
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
                    // call like `(+) 1.0 2.0` solves `a := Double`, and the
                    // arity/currying judgement is the shared one (Haskell 2010
                    // §4.3.3) — so `fromInteger 1 2` is the arity error GHC
                    // reports ("No instance for `Num (t0 -> t1)`") instead of
                    // silently typechecking as `Integer`.
                    let mut fresh_ids: Vec<ValueTy> = Vec::new();
                    for _ in 0..8 {
                        fresh_ids.push(ctx.fresh_vty());
                    }
                    let mut it = fresh_ids.into_iter();
                    let (sig_param_tys, ret) = signature_param_and_result_tys_conv(
                        &ctx.env,
                        &class_var,
                        &cv_subst,
                        sig,
                        || it.next().unwrap_or(ValueTy::Double),
                    );
                    let ret_ty = ret.unwrap_or_else(|| ctx.fresh_vty());
                    let what = format!("method `{name}` of `{class_name}`");
                    return apply_args_curried(
                        ctx,
                        &sig_param_tys,
                        &ret_ty,
                        args,
                        span,
                        &what,
                        ArgUnify::Exact,
                    );
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
                // Each argument is typed AGAINST its parameter type. Typing it
                // bare and unifying afterwards does NOT bind the class variable:
                // the bare pass instantiates a definition's scheme at a FRESH
                // variable, so the later unify compares two unrelated variables
                // and the original one stays unresolved (`h + 2.5` left `h` at
                // `Var` while `2.5` was `Double` — two answers for `(+)`'s one
                // `a`). Threading the parameter in makes them the same variable
                // from the start. The arguments are typed BEFORE the params are
                // bound: an argument is a caller expression, so a param sharing a
                // name with a caller-local must not capture it (`return x` inside
                // `\x -> return (f x)`).
                let param_tys = signature_param_tys(ctx, &class_var, &cv_subst, sig);
                let mut arg_vtys: Vec<ValueTy> = Vec::with_capacity(args.len());
                for (i, a) in args.iter().enumerate() {
                    match param_tys.get(i).cloned() {
                        Some(e) => arg_vtys.push(infer_method_value_vty_expected(
                            ctx,
                            a,
                            "argument",
                            Some(e),
                        )?),
                        None => arg_vtys.push(infer_method_value_vty(ctx, a, "argument")?),
                    }
                }
                // The value type each argument ACTUALLY has may be narrower than
                // its declared parameter (`return ()`): the body needs the
                // argument's type, not the parameter's, so unify the two through
                // the instance's own parameter binding before inlining.
                for (i, (a, av)) in args.iter().zip(arg_vtys.iter()).enumerate() {
                    if let Some(pt) = param_tys.get(i) {
                        unify_value(av, pt, &mut ctx.subst, a.span())?;
                    }
                }
                let body_vty = inline_instance_body(
                    ctx,
                    InlineCall {
                        class_name: &class_name,
                        ty_name: &ty_name,
                        container: Some(&exp),
                        method: name,
                        class_var: &class_var,
                        cv_subst: ClassVarSubst::Ty(ValueTy::Con(ty_name.clone(), Vec::new())),
                        sig,
                        body: &body,
                        params: &params,
                        arg_vtys: &arg_vtys,
                        param_bindings: ParamBindings::Signature,
                        body_expected: Some(exp.clone()),
                        span,
                    },
                )?;
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
                    // A live variable with a recorded demand (see the note in
                    // `infer_ref_inner`): the sentinel was a type no context
                    // could take.
                    let cv_var = ctx.fresh_id();
                    ctx.residual.push(Residual {
                        class: class_name.clone(),
                        var: cv_var,
                        span,
                        literal: false,
                    });
                    let mut fresh_ids: Vec<ValueTy> = Vec::new();
                    for _ in 0..8 {
                        fresh_ids.push(ctx.fresh_vty());
                    }
                    let mut it = fresh_ids.into_iter();
                    let (param_tys, ret) = signature_param_and_result_tys_conv(
                        &ctx.env,
                        &class_var,
                        &ClassVarSubst::Var(cv_var),
                        sig,
                        || it.next().unwrap_or(ValueTy::Double),
                    );
                    let ret_ty = ret.unwrap_or_else(|| ctx.fresh_vty());
                    // The remainder's RESULT carries the class variable: when
                    // the caller supplies an expectation (`h :: Int -> Int`),
                    // unifying with it solves `a` right here — GHC accepts such
                    // a partial application for exactly that reason, and leaving
                    // the variable free made the residual ambiguous
                    // (`C a`, `Num a`) and REJECTED a program GHC takes.
                    let ret_expected = expected.clone().map(|e| match e {
                        ValueTy::Thunk(_, rets) if rets.len() == 1 => rets[0].clone(),
                        other => other,
                    });
                    if let Some(exp) = ret_expected {
                        let expected_ret = ret_ty.clone();
                        let _ = unify_value(&expected_ret, &exp, &mut ctx.subst, span);
                    }
                    let what = format!("method `{name}` of `{class_name}`");
                    return apply_args_curried(
                        ctx,
                        &param_tys,
                        &ret_ty,
                        args,
                        span,
                        &what,
                        ArgUnify::Exact,
                    );
                }
                let _ = primitive;
                if args.len() > n_sig_args {
                    // Over-application is the shared arity judgement. A FULL
                    // application falls through to selector resolution, which
                    // selects the instance from the argument types — that is
                    // dispatch work, not application, and it unifies its own
                    // arguments against the signature.
                    let what = format!("method `{name}` of `{class_name}`");
                    check_application_arity(n_sig_args, args.len(), span, &what)?;
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
                // Every operand shares the class variable (`(+) :: a -> a ->
                // a`, `(<) :: a -> a -> Bool`), so the instance is solved by
                // ANY concrete operand — not only by the signature's selector
                // position. `?gain > 0.5` has an unresolved actor parameter in
                // the selector slot and a `Double` literal beside it; Haskell
                // unifies the two and selects `Ord Double`.
                let env = ctx.env.clone();
                // A NON-selector operand's own typing failure is not this
                // dispatch's to report: the argument is typed again against the
                // method's parameter type below, where the diagnostic names the
                // method. The SELECTOR's is (`show (sin 1.0)` must report the
                // `sin` error, not "the argument type is not concrete").
                let try_dispatch =
                    |ctx: &mut Ctx,
                     arg: &Expr|
                     -> Result<Option<(String, ClassVarSubst)>, CompileError> {
                        let cv_var = ctx.fresh_id();
                        // A PROBE, not a decision: it answers "does this
                        // operand's type name an instance?" and nothing more.
                        // The OPERATIVE typing is `apply_args_curried`, against
                        // the method's parameter type.
                        //
                        // Leaving the probe's effects behind makes the operand
                        // be typed TWICE, and each typing instantiates `h :: Num
                        // b => b` at a FRESH variable. Only one of the two is
                        // under the method's `a`, so the other never receives a
                        // constraint: it stays ambiguous, stage 3 defaults it,
                        // and it then OVERWRITES the correct selection — measured
                        // `h + 2.5` recorded `Num@h = Double` from stage 2 and
                        // `Num@h = Integer` from the default for the same span.
                        // Rolling the probe back makes the later, real typing the
                        // only one that writes.
                        let subst_mark = ctx.subst.clone();
                        let next_mark = ctx.next;
                        let residual_mark = ctx.residual.len();
                        let vty = infer_method_value_vty(ctx, arg, "argument")?;
                        ctx.subst = subst_mark;
                        ctx.next = next_mark;
                        ctx.residual.truncate(residual_mark);
                        // Resolve through the substitution: the operand may
                        // already be bound (`a + b` where `a` was pinned by a
                        // previous argument or by defaulting), and an unresolved
                        // `Var` in the selector position names no instance.
                        let vty = ctx.subst.resolve_value(&vty);
                        Ok(method_dispatch_key(
                            &env,
                            &class_name,
                            &class_var,
                            sig,
                            &vty,
                            cv_var,
                            || ctx.fresh_vty(),
                        ))
                    };
                // An integer LITERAL is `fromInteger`-polymorphic (Haskell 2010
                // §6.4.1): its own static type is the DEFAULT (`Int`), not
                // evidence for what the shared class variable is. Every operand
                // of `(==) :: a -> a -> Bool` is the same `a`, so in
                // `isZero x = 0 == x` with `x : Integer` the literal takes
                // `Integer` from `x`. Selecting the instance from the literal
                // itself selected `Eq Int`, and the two operands then disagreed
                // — the comparison silently became a cross-kind one.
                //
                // The literal is a FALLBACK, not an exclusion: with no other
                // operand naming an instance (`f x = x == 0`, `x` unconstrained)
                // the literal's default is still the only evidence there is.
                let selector_is_literal = matches!(args[selector_idx], Expr::IntegerLit(_, _));
                // The selector's OWN static type, measured with the probe's
                // effects rolled back. It is the "container" an arity-0 instance
                // may still have: `instance Mono (List a)` is a NULLARY class
                // variable whose head is `List a`, so `a` must be bound even
                // though the class has no applied variable.
                let selector_ty = {
                    let subst_mark = ctx.subst.clone();
                    let next_mark = ctx.next;
                    let residual_mark = ctx.residual.len();
                    let t = infer_method_value_vty(ctx, &args[selector_idx], "argument")?;
                    ctx.subst = subst_mark;
                    ctx.next = next_mark;
                    ctx.residual.truncate(residual_mark);
                    ctx.subst.resolve_value(&t)
                };
                let selector_dispatch = try_dispatch(ctx, &args[selector_idx])?;
                // The literal's dispatch is a FALLBACK, kept for the case where
                // no other operand names an instance.
                let mut dispatch = if selector_is_literal {
                    None
                } else {
                    selector_dispatch.clone()
                };
                if dispatch.is_none() {
                    for (i, a) in args.iter().enumerate() {
                        if i == selector_idx {
                            continue;
                        }
                        if let Some(d) = try_dispatch(ctx, a).unwrap_or(None) {
                            dispatch = Some(d);
                            break;
                        }
                    }
                }
                if dispatch.is_none() && selector_is_literal {
                    dispatch = selector_dispatch;
                }
                // No instance yet: DEFER, do not fail.
                //
                // This is the boundary the three stages exist for. When the
                // operands' class variable is still an unresolved type variable
                // there is no instance to select and no unified type to
                // contradict — the variable is simply AMBIGUOUS, and the Report
                // resolves an ambiguous numeric variable by defaulting it at the
                // enclosing binding's close (§4.3.4). Failing here is what made
                // `1 == 1.0` and `f x = x == 0` unreachable: the instance search
                // was asked a question only defaulting could answer.
                //
                // The deferral records the constraint in the residual store — so
                // stage 2 can report a genuinely missing instance and stage 3 can
                // default an ambiguous variable — and gives the call the
                // signature's own type with the class variable left ABSTRACT.
                // That abstract form is also the GIVEN witness's type, which is
                // why one deferral serves the concrete-unknown and the
                // constrained-body cases alike.
                // A dispatch key can name a HEAD with no registered instance:
                // `method_dispatch_key` falls back to the selector's
                // constructor name (`Pair`), which is right for a structurally
                // derived instance (`Eq Array`) and WRONG for a class whose
                // instances are keyed by the full type (`Ix (Pair Int Int)`).
                // Such a key is not an instance, it is an unsolved class
                // variable, so it defers like one — reporting "no instance of
                // `Ix` for type `Pair`" for `range (0, 1)` would name a type the
                // program never wrote.
                let dispatch = dispatch.filter(|(ty_name, cv_subst)| {
                    let registered = ctx
                        .env
                        .instances
                        .get(class_name.as_str())
                        .map(|m| m.contains_key(ty_name.as_str()))
                        .unwrap_or(false);
                    if registered {
                        return true;
                    }
                    // Unregistered, but the class variable IS concrete: that is
                    // a genuine stage-2 failure and stays with the resolver,
                    // which reports it.
                    !matches!(selection_evidence(cv_subst), ValueTy::Con(_, _))
                        || !cv_subst_is_head_only(cv_subst, ty_name)
                });
                let Some((ty_name, cv_subst)) = dispatch else {
                    let cv_subst = ClassVarSubst::Var(ctx.fresh_id());
                    let abstract_var = match cv_subst {
                        ClassVarSubst::Var(v) => ValueTy::Var(v),
                        _ => unreachable!("a fresh abstract class variable"),
                    };
                    let env_clone = ctx.env.clone();
                    let (param_tys, ret) = signature_param_and_result_tys_conv(
                        &env_clone,
                        &class_var,
                        &cv_subst,
                        sig,
                        || ctx.fresh_vty(),
                    );
                    let ret = ret.unwrap_or_else(|| ctx.fresh_vty());
                    let what = format!("method `{name}` of `{class_name}`");
                    // Unifying every operand with the (abstract) class variable
                    // is what makes a MIXED pair a stage-1 error here instead of
                    // a silent cross-kind comparison.
                    let at = apply_args_curried(
                        ctx,
                        &param_tys,
                        &ret,
                        args,
                        span,
                        &what,
                        ArgUnify::Exact,
                    )?;
                    // Record the ambiguity for stages 2 and 3. The CLASS variable
                    // is what these stages resolve — `apply_args_curried` unified
                    // it with the operands, so its resolved id IS the shared
                    // operand variable. Nothing else is recorded: a parameter's
                    // OTHER free variables are not constrained by this class
                    // (`Array i e` under `Ix` constrains `i`, never `e`), and
                    // recording them would demand an instance for a type the
                    // class says nothing about.
                    // The call's RESULT is `a` (for `(+)`, `a -> a -> a`), so an
                    // expectation on the whole expression is a constraint on the
                    // operands' shared variable. Unifying it here is what makes
                    // `show_double (h + 2.5)` decide `Double`: GHC 9.10.3 measures
                    // that program as `3.5`, with `h :: Num b => b` instantiated
                    // at `Double` — the same variable the literal constrains.
                    // Without this the variable stayed unresolved, lowering had
                    // no instance to select, and it refused `h`.
                    if let Some(exp) = expected.as_ref() {
                        unify_value(&at.vty, exp, &mut ctx.subst, span)?;
                    }
                    if let ValueTy::Var(id) = ctx.subst.resolve_value(&abstract_var) {
                        ctx.residual.push(Residual {
                            class: class_name.clone(),
                            var: id,
                            // A METHOD call's constraint, not a literal's.
                            literal: false,
                            span,
                        });
                    }
                    return Ok(at);
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
                    let mut fresh_ids: Vec<ValueTy> = Vec::new();
                    for _ in 0..8 {
                        fresh_ids.push(ctx.fresh_vty());
                    }
                    let mut it = fresh_ids.into_iter();
                    let (param_tys, ret) = signature_param_and_result_tys_conv(
                        &ctx.env,
                        &class_var,
                        &cv_subst,
                        sig,
                        || it.next().unwrap_or(ValueTy::Double),
                    );
                    // A FULL application yields the signature's RESULT; fewer
                    // arguments than parameters is a partial application and
                    // keeps the curried remainder (Haskell 2010 §4.3.3). This is
                    // what makes `1.0 + 2.0 : Double` and `(+) 1.0 : Double ->
                    // Double`, and it is the type the enclosing expression
                    // unifies against (`f (n + 1.0)` checks `Double`, not the
                    // whole remaining signature).
                    let ret = ret.unwrap_or_else(|| ctx.fresh_vty());
                    let what = format!("method `{name}` of `{class_name}`");
                    let at = apply_args_curried(
                        ctx,
                        &param_tys,
                        &ret,
                        args,
                        span,
                        &what,
                        ArgUnify::BufferConstant,
                    )?;
                    // The call's RESULT is its class variable (`(+), (*), (==)`
                    // all return `a`), so an expectation on the whole expression
                    // constrains that SAME variable — and therefore every
                    // operand. Unifying here is what makes
                    // `show_double (h + 2.5)` decide `Double`: GHC 9.10.3
                    // measures it as `3.5` with `h :: Num b => b` instantiated at
                    // `Double`. Without it `h` stayed a fresh variable while
                    // `2.5` became `Double` — two answers for one `a` — and
                    // lowering had no instance to select.
                    if let Some(exp) = expected.as_ref() {
                        unify_value(&at.vty, exp, &mut ctx.subst, span)?;
                    }
                    return Ok(at);
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
                let param_tys = signature_param_tys(ctx, &class_var, &cv_subst, sig);
                // Infer every argument BEFORE the method's params are bound: an
                // argument is a caller expression, so a param that shares a name
                // with a caller-local must not capture it.
                let mut param_bind: Vec<ValueTy> = Vec::with_capacity(params.len());
                for (i, _p) in params.iter().enumerate() {
                    let pt = if i == selector_idx
                        || !is_bare_nullary_method_ref(ctx, &args[i], &class_name)
                    {
                        infer_method_value_vty(ctx, &args[i], "argument")?
                    } else {
                        // Nullary arg (`mempty`): inline ITS instance body with
                        // the signature's param type as expected so it unifies
                        // (`list` -> `List ?a`).
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
                // The method's DECLARED RESULT is the body's expected type:
                // `index : Pair a a -> a -> Int` makes `fromInteger (...)` in the
                // body's tail a `Num Int` application instead of a result-directed
                // call with no expected type.
                let body_expected = {
                    let env_clone = ctx.env.clone();
                    let (_p_tys, ret) = signature_param_and_result_tys_conv(
                        &env_clone,
                        &class_var,
                        &cv_subst,
                        sig,
                        || ctx.fresh_vty(),
                    );
                    ret
                };
                let body_vty = inline_instance_body(
                    ctx,
                    InlineCall {
                        class_name: &class_name,
                        ty_name: &ty_name,
                        // An arity-0 instance is keyed by the FULL selector
                        // type, but its head may still be APPLIED
                        // (`instance Mono (List a)`: nullary class variable, head
                        // `List a`, `head_args = ["a"]`). Passing the selector
                        // type lets the shared inline bind those arguments — a
                        // `None` here lost them.
                        container: Some(&selector_ty),
                        method: name,
                        class_var: &class_var,
                        cv_subst,
                        sig,
                        body: &body,
                        params: &params,
                        arg_vtys: &param_bind,
                        param_bindings: ParamBindings::Arguments,
                        body_expected,
                        span,
                    },
                )?;
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
            if args.len() > n_sig_args {
                // Over-application is the shared arity judgement: `fmap f m x`
                // applies a value that is not a function.
                let what = format!("method `{name}` of `{class_name}`");
                check_application_arity(n_sig_args, args.len(), span, &what)?;
            }
            if args.len() < n_sig_args {
                // A constructor-class method is an ordinary curried function
                // value like any other (Haskell 2010 §4.3.3): `fmap (+ 1.0)`
                // supplies the function and leaves the container, so the call
                // is a PARTIAL application and its type is the curried
                // remainder — GHC types `g = fmap (+1)` as
                // `Functor f => f Integer -> f Integer` and `g (Just 1)` is
                // `Just 2`. The instance is NOT selected here, because the
                // argument that would select it (the class-var-applied
                // container) is exactly the missing one; making the class
                // variable a live unification variable leaves the call site
                // that eventually supplies the container to solve it. This is
                // the same shape the bare method reference produces
                // (`infer_ref_expected`), and the same shared rule decides the
                // arity, so a partial application and a bare reference can
                // never disagree about the remainder.
                let cv_var = ctx.fresh_id();
                ctx.residual.push(Residual {
                    class: class_name.clone(),
                    var: cv_var,
                    span,
                    literal: false,
                });
                let mut fresh_ids: Vec<ValueTy> = Vec::new();
                for _ in 0..8 {
                    fresh_ids.push(ctx.fresh_vty());
                }
                let mut it = fresh_ids.into_iter();
                let (param_tys, ret) = signature_param_and_result_tys_conv(
                    &ctx.env,
                    &class_var,
                    &ClassVarSubst::Var(cv_var),
                    sig,
                    || it.next().unwrap_or(ValueTy::Double),
                );
                let ret_ty = ret.unwrap_or_else(|| ctx.fresh_vty());
                let what = format!("method `{name}` of `{class_name}`");
                return apply_args_curried(
                    ctx,
                    &param_tys,
                    &ret_ty,
                    args,
                    span,
                    &what,
                    ArgUnify::Exact,
                );
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
                Some(ValueTy::Con(c, args)) => {
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
                                Some(ValueTy::Con(..)) => ValueTy::Con(c.clone(), cargs),
                                _ => ValueTy::Con(c.clone(), cargs),
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
                ValueTy::Con(c, _) => Some(c.clone()),
                _ => None,
            };
            let (head_args, _) = match &container_head {
                Some(c) => bind_constraint_instance(ctx, &class_name, c, &container_vty, span)?,
                None => (vec![], vec![]),
            };
            let concrete_args = match &container_vty {
                ValueTy::Con(_, a) => a.clone(),
                _ => vec![],
            };
            // Build the class-var pattern (`f a` → `Con("f", [fresh])`) from
            // the container argument's signature type.
            let pattern = class_var_pattern(ctx, sig, container_idx);
            // A partial instance head (`Kleisli m`, head_args = ["m"]) means the
            // pattern matches the concrete container's args AFTER the bound
            // head args (`Kleisli Maybe Double Double` → match `[Double, Double]`).
            let match_target = if head_args.is_empty() {
                container_vty.clone()
            } else {
                match &container_vty {
                    ValueTy::Con(c, _) => {
                        ValueTy::Con(c.clone(), concrete_args[head_args.len()..].to_vec())
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
                &ClassVarSubst::Ctor(ctor.clone(), Vec::new()),
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
            // Mirror of the lowering guard (lower.rs, constructor-class branch):
            // the key arms ONLY around the inlined BODY, not the arguments.
            // Arguments may legally inline other methods' defaults that
            // reference this method (the method-lifting composition — `both`'s
            // default reaches `compose` again through `second`'s default body
            // while the outer `compose`'s second argument is inferred); genuine
            // recursion (a body calling its own method name) is still caught.
            // The inline is the SAME act as every other dispatch site — see
            // `inline_instance_body`, which now owns the guard the old comment
            // here described. What stays branch-specific is the container/param
            // unification ABOVE: an arity>=1 class has a container that binds
            // the payload variable (`a` in `m a`).
            let body_vty = inline_instance_body(
                ctx,
                InlineCall {
                    class_name: &class_name,
                    ty_name: &ctor,
                    container: Some(&container_vty),
                    method: name,
                    class_var: &class_var,
                    cv_subst: ClassVarSubst::Ctor(ctor.clone(), Vec::new()),
                    sig,
                    body: &body,
                    params: &params,
                    arg_vtys: &arg_vtys,
                    param_bindings: ParamBindings::Arguments,
                    body_expected,
                    span,
                },
            )?;
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
                        let at = if matches!(
                            &args[pos],
                            Expr::IntegerLit(_, _) | Expr::FractionalLit(_, _, _)
                        ) {
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
            if unresolved {
                // An UNRESOLVED parameter has no signature to count, so the
                // call defines the arity: bind the local to a fresh nest of the
                // applied arity and unify the arguments against it.
                let func_ty = nest_thunk(&arg_tys, ret_tys.clone());
                unify_value(lty, &func_ty, &mut ctx.subst, span)?;
                for (a, pty) in args.iter().zip(arg_tys.iter()) {
                    let vt = infer_method_value_vty(ctx, a, "argument")?;
                    unify_value(&vt, pty, &mut ctx.subst, a.span())?;
                }
                let ret_ty = ret_tys.first().cloned().unwrap_or(ValueTy::Double);
                return Ok(ArrowTy::value_channel(ret_ty));
            }
            // A RESOLVED function value has a real curried type: `arg_tys` are
            // its parameters and `ret_tys` its result, so the shared rule
            // decides the call (`f 1.0` of a two-parameter local is the curried
            // remainder).
            let ret = ret_tys.first().cloned().unwrap_or(ValueTy::Double);
            let what = format!("`{name}`");
            return apply_args_curried(ctx, &arg_tys, &ret, args, span, &what, ArgUnify::Exact);
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
        if let Some(sig) = ctx.env.constrained_defs.get(name).cloned() {
            let routes_to_method = args.first().is_some_and(|first| {
                ctx.env.class_of_method(name).is_some()
                    && probe_first_arg_solves_class_var(ctx, name, first)
            });
            if !routes_to_method {
                return infer_constrained_apply(ctx, name, &sig, args, span, expected);
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
                    let what = format!("`{name}`");
                    check_application_arity(arg_tys.len(), args.len(), span, &what)?;
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
                    // A generalised parameter (`g = \y -> f y`, `g y = f y`
                    // schemes as `a -> a`) is a FRESH variable of this
                    // instantiation. An integer literal must DEFER against it:
                    // the literal's type IS its expected type (Haskell 2010
                    // §6.4.1 `fromInteger`), so binding the fresh variable to
                    // the `Int` a context-free literal defaults to decides the
                    // literal before the enclosing expectation is known, and
                    // `show_double (g 9223372036854775808)` then fails to
                    // unify instead of taking `Double`. Deferring leaves the
                    // variable free for the call's own result to receive the
                    // expectation from.
                    let bare_var = matches!(ctx.subst.resolve_value(pty), ValueTy::Var(_));
                    if bare_var && a.is_int_literal() {
                        continue;
                    }
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
                // The referenced definition's type IS a curried function type
                // (`ref_scheme` is a `Thunk`), so a call with fewer arguments is
                // a PARTIAL application and yields the curried remainder
                // (Haskell 2010 §4.3.3 — `f 1.0` of a two-λ target is
                // GHC-valid). The arity judgement is the shared one; the
                // argument CHECKING stays here, because a signature-less
                // definition's formal types are recovered from its body and get
                // this site's own diagnostic (PARK M10).
                let what = format!("`{name}` references `{ref_name}`, which");
                check_application_arity(ref_scheme.lam_count, args.len(), span, &what)?;
                let ty = ctx.instantiate(&ref_scheme);
                // A named definition applied to its full argument list: the
                // arguments still must match the formal parameter types
                // (Haskell 2010 §4.1.3). `ty` is this call's instantiation, so a
                // polymorphic definition (a generalized `a -> a`) defers instead
                // of being pinned by this call.
                check_def_call_args(ctx, name, args, &ty.vty)?;
                return Ok(ArrowTy::value(applied_result(&ty.vty, args.len())));
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
    let at = if matches!(arg, Expr::IntegerLit(_, _) | Expr::FractionalLit(_, _, _)) && want_buffer
    {
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
        if matches!(formal, ValueTy::TyConVar(_)) {
            // A type-CONSTRUCTOR variable parameter (`m`, `f`) has no value
            // shape to check against: defer.
            continue;
        }
        if matches!(formal, ValueTy::Var(_)) {
            // A generalised parameter (`id x = x` schemes as `a -> a`). The
            // variable is FRESH for this instantiation, so unifying it records
            // the call's argument type and nothing about the definition. That
            // link is what makes the RESULT type mean the argument type: with
            // the unification skipped, `g y = f y` schemed as `a -> b`, so a
            // call site could not relate its expected type to the parameter.
            //
            // An integer literal DEFERS: its type IS its expected type
            // (Haskell 2010 §6.4.1, `fromInteger`), so pinning the fresh
            // variable to the `Int` a context-free literal defaults to would
            // decide the literal before the enclosing expectation is known
            // (`show_double (f 9223372036854775808)` must type the literal at
            // `Double`).
            //
            // The link is made ONLY from an argument whose type is still an
            // unresolved variable — the `g y = f y` shape, where `y` is a bare
            // parameter reference with no type of its own. Any other argument
            // comes with a type already settled by its OWN inference: a
            // computed `2 + 3` or `if True then 1 else 2` defaults its
            // unconstrained `Num` operand to `Int`, and an annotated `d ::
            // Double` is `Double`. Committing that settled type to the fresh
            // formal would decide the argument before the enclosing
            // expectation (`show_double`) could reach it, turning the valid
            // `show_double (f (2 + 3))` into a type error.
            //
            // A numeric LITERAL is included: its own type IS a fresh variable
            // (Haskell 2010 §6.4.1 `fromInteger`), so unifying it with the
            // formal is exactly the Report's rule — and it is what keeps
            // `f x y = x <= y` applied at `Integer` from leaving the class
            // variable abstract. (The old exclusion existed only because a
            // literal used to be pinned to `Int` here, which would have decided
            // it before the enclosing expectation could.)
            {
                let actual = infer_method_value_vty(ctx, a, "argument")?;
                if !is_unknown_thunk(&actual)
                    && matches!(ctx.subst.resolve_value(&actual), ValueTy::Var(_))
                {
                    unify_value(&actual, &formal, &mut ctx.subst, a.span())?;
                }
            }
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
        if def_arg_widens_to(&formal, &actual, defaulted) {
            continue;
        }
        if let Err(e) = unify_value(&actual, &formal, &mut ctx.subst, a.span()) {
            // PARK M10 — now a contract of Task 10a, not a stopgap.
            //
            // A signature-less definition's parameter types are the shapes its
            // BODY recovered (`at = \a -> \i -> a ! i` gives `a : Array i e`
            // from the constrained `(!)`), so they are invisible at the call
            // site: the caller sees `Array a b` where it wrote nothing. GHC
            // 9.10.3 reports the same program as a plain unification mismatch
            // (`Couldn't match expected type: Array i0 a0 with actual type:
            // (a1, b0)`) and offers no fix, but rill knows WHY the parameter
            // has that shape — it is inferred, not declared — so the actionable
            // advice is to annotate. Naming the definition and pointing at the
            // annotation is what makes the recovered shape legible instead of
            // dumping it as if the user had written it.
            if ctx.retry_inferred_defs.contains(name) {
                return Err(CompileError::Type {
                    msg: format!(
                        "argument {} of `{name}` does not match: {e}. \
                         `{name}` has no signature, so its parameter types are \
                         inferred from its body and are invisible here — \
                         annotate the parameter's type to check the call against \
                         an explicit signature",
                        i + 1
                    ),
                    span: a.span(),
                });
            }
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
fn def_arg_widens_to(formal: &ValueTy, actual: &ValueTy, defaulted: bool) -> bool {
    match (formal, actual) {
        // A buffer is an ordinary value: it flows into a numeric parameter (the
        // signal-constant / buffer-arithmetic path) and any value flows into a
        // `Buffer` parameter.
        (ValueTy::Buffer(_), _) => true,
        (ValueTy::Double | ValueTy::Int, ValueTy::Buffer(_)) => true,
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
        (ValueTy::String, ValueTy::String) => true,
        (ValueTy::Void, ValueTy::Void) => true,
        (ValueTy::Con(f, fa), ValueTy::Con(a, aa))
        | (ValueTy::Newtype(f, fa), ValueTy::Newtype(a, aa)) => {
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
    // A RESULT-DIRECTED method in argument position (`arr f`, `pure x`,
    // `return x`) can only be typed WITH an expected type: its container comes
    // from there and from nowhere else. Probing it bare first is guaranteed to
    // fail, and reporting that probe's error was the reason a working call was
    // rejected — `ap (arr f) 3.0` measured
    //
    //     name=arr nargs=1 expected=None                    ← probe, always fails
    //     name=arr nargs=1 expected=Some(Kleisli m a b)     ← the real typing
    //
    // and the probe's failure won. The expected-type attempt comes FIRST here;
    // the bare one is the fallback, for an argument that types fine on its own.
    match infer_method_value_vty_expected(ctx, e, "argument", Some(formal.clone())) {
        Ok(t) => Ok(t),
        Err(expected_err) => match infer_method_value_vty(ctx, e, "argument") {
            Ok(t) => Ok(t),
            // Report the ORIGINAL failure: it is the one that names the real
            // problem in the argument, not a follow-on expected-type mismatch.
            Err(_) => Err(expected_err),
        },
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
            ValueTy::Con(n, inner) if n == "List" => {
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
            ValueTy::Con(n, inner) if n == "List" => {
                Ok(inner.first().cloned().unwrap_or(ValueTy::Double))
            }
            ValueTy::Var(_) => {
                let elem = ctx.fresh_vty();
                let list = ValueTy::Con("List".into(), vec![elem.clone()]);
                unify_value(&list, &t, &mut ctx.subst, args[idx].span())?;
                Ok(elem)
            }
            other => Err(CompileError::Type {
                msg: format!("`{name}` expects a List as its {what}, got {other:?}"),
                span: args[idx].span(),
            }),
        }
    };
    let list_of = |t: &ValueTy| ValueTy::Con("List".into(), vec![list_elem(t)]);
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
                ValueTy::Con(n, _) if n == "List" => {}
                ValueTy::Con(n, _) if n == "Map" || n == "Set" || n == "Array" => {}
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
            Ok(ValueTy::Con("List".into(), vec![elem]))
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
                &ValueTy::Con("List".into(), vec![fresh]),
                &mut ctx.subst,
                args[0].span(),
            )?;
            Ok(ValueTy::Con("Maybe".into(), vec![list_elem(&lt)]))
        }
        "tail" => {
            if args.len() != 1 {
                return Err(arity_err("1", args.len()));
            }
            let lt = arg_vty(ctx, 0)?;
            let fresh = ctx.fresh_vty();
            unify_value(
                &lt,
                &ValueTy::Con("List".into(), vec![fresh]),
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
            Ok(ValueTy::Con("List".into(), vec![ret_ty]))
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
            Ok(ValueTy::Con("List".into(), vec![elem]))
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
            Ok(ValueTy::Con("List".into(), vec![b]))
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
                ValueTy::Con(n, inner) if n == "List" => {
                    inner.first().cloned().unwrap_or(ValueTy::Double)
                }
                ValueTy::Var(_) => {
                    let elem = ctx.fresh_vty();
                    let list = ValueTy::Con("List".into(), vec![elem.clone()]);
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
            let list_ty = ValueTy::Con("List".into(), vec![elem.clone()]);
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
            Ok(ValueTy::Con("List".into(), vec![ValueTy::Double]))
        }
        "insert" => match args.len() {
            3 => {
                let kt = arg_vty(ctx, 0)?;
                let vt = arg_vty(ctx, 1)?;
                let _ = arg_vty(ctx, 2)?;
                // The `Ord` check runs LAST: the map operand may be what
                // solves the key's type when the key is a literal
                // (`insert 1 2 m` with `m : Map Int Double`), and the literal's
                // variable is unresolved until then.
                check_ord(ctx, &kt, "key")?;
                Ok(ValueTy::Con("Map".into(), vec![kt, vt]))
            }
            2 => {
                let kt = arg_vty(ctx, 0)?;
                let st = arg_vty(ctx, 1)?;
                // `insert x s`: the SET solves the element type, so a literal
                // element takes the set's own type (`insert 5 (empty_set)`
                // with `empty_set : Set Double` is `Set Double`, as in GHC)
                // instead of staying an unconstrained variable that `check_ord`
                // then rejects.
                let elem = match &st {
                    ValueTy::Con(n, inner) if n == "Set" => {
                        inner.first().cloned().unwrap_or(ValueTy::Double)
                    }
                    ValueTy::Con(n, inner) if n == "List" => {
                        inner.first().cloned().unwrap_or(ValueTy::Double)
                    }
                    _ => kt.clone(),
                };
                unify_value(&kt, &elem, &mut ctx.subst, args[0].span())?;
                check_ord(ctx, &elem, "element")?;
                Ok(ValueTy::Con("Set".into(), vec![elem]))
            }
            _ => Err(arity_err("2 or 3", args.len())),
        },
        "lookup" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            // The CONTAINER is typed first: its element type is what a literal
            // key takes (`lookup 1 m` with `m : Map Int v`), and checking the
            // key's `Ord` before that unification sees an unresolved variable
            // and rejects a program GHC accepts.
            let mt = arg_vty(ctx, 1)?;
            let kt = arg_vty(ctx, 0)?;
            let (key_ty, v) = match &mt {
                ValueTy::Con(n, inner) if n == "Map" => (
                    inner.first().cloned().unwrap_or(ValueTy::Int),
                    inner.get(1).cloned().unwrap_or(ValueTy::Double),
                ),
                ValueTy::Con(n, inner) if n == "Set" => (
                    inner.first().cloned().unwrap_or(ValueTy::Int),
                    ValueTy::bool_ty(),
                ),
                _ => (kt.clone(), ValueTy::Double),
            };
            unify_value(&kt, &key_ty, &mut ctx.subst, args[0].span())?;
            check_ord(ctx, &key_ty, "key")?;
            Ok(ValueTy::Con("Maybe".into(), vec![v]))
        }
        "member" => {
            if args.len() != 2 {
                return Err(arity_err("2", args.len()));
            }
            // Same order as `lookup`: the container solves the literal.
            let ct = arg_vty(ctx, 1)?;
            let kt = arg_vty(ctx, 0)?;
            let elem = match &ct {
                ValueTy::Con(n, inner) if n == "Set" || n == "Map" => {
                    inner.first().cloned().unwrap_or(ValueTy::Int)
                }
                ValueTy::Con(n, inner) if n == "List" => {
                    inner.first().cloned().unwrap_or(ValueTy::Double)
                }
                _ => kt.clone(),
            };
            unify_value(&kt, &elem, &mut ctx.subst, args[0].span())?;
            check_ord(ctx, &elem, "element")?;
            Ok(ValueTy::bool_ty())
        }
        "empty_map" => {
            if !args.is_empty() {
                return Err(arity_err("0", args.len()));
            }
            Ok(ValueTy::Con(
                "Map".into(),
                vec![ValueTy::String, ValueTy::Double],
            ))
        }
        "empty_set" => {
            if !args.is_empty() {
                return Err(arity_err("0", args.len()));
            }
            Ok(ValueTy::Con("Set".into(), vec![ValueTy::Double]))
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
                vty: ValueTy::Thunk(vec![ValueTy::Var(7)], vec![ValueTy::bool_ty()]),
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
            in_instance_body: false,
            in_signed_body: false,
            dict_selections: HashMap::new(),
            residual: Vec::new(),
            literal_vars: HashMap::new(),
            literal_instances: HashMap::new(),
            value_tys: HashMap::new(),
            entry_name: String::new(),
            final_pass: false,
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
        let t = ty_of("foo = do { write Nothing stdout; return () }").unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
            ValueTy::Con("IO".into(), vec![ValueTy::Void])
        );
    }

    #[test]
    fn data_strictness_flows_into_data_info() {
        // The Haskell 2010 strictness flag parsed by the data declaration must
        // reach the type environment, because ctor lowering reads `DataInfo`
        // (never the AST) when deciding where to force.
        let t = ty_of(
            "data T = C !Double; data P = { x: !Double, y: Double }; foo = P { x: 1.0, y: 2.0 }",
        )
        .unwrap();
        // Both declarations are the ONE data shape; only one has named fields.
        match t.type_env.data_types.get("T") {
            Some(DataInfo::Data(ctors)) => {
                assert_eq!(ctors.len(), 1);
                assert_eq!(ctors[0].name, "C");
                assert_eq!(ctors[0].fields.len(), 1);
                assert_eq!(ctors[0].fields[0].ty, ValueTy::Double);
                assert!(ctors[0].fields[0].strict);
                assert!(ctors[0].fields[0].name.is_none());
            }
            other => panic!("expected one constructor for T, got {other:?}"),
        }
        match t.type_env.data_types.get("P") {
            Some(DataInfo::Data(ctors)) => {
                assert_eq!(ctors.len(), 1);
                let f = &ctors[0].fields;
                assert_eq!(f.len(), 2);
                assert_eq!(f[0].name.as_deref(), Some("x"));
                assert_eq!(f[0].ty, ValueTy::Double);
                assert!(f[0].strict);
                assert_eq!(f[1].name.as_deref(), Some("y"));
                assert!(!f[1].strict);
            }
            other => panic!("expected one constructor for P, got {other:?}"),
        }
    }

    #[test]
    fn gain_is_1_to_1() {
        // A buffer-valued signal expression over a literal generator.
        let t = ty_of(r"foo = sine 440.0 0.5 0.0 * 0.5;").unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
            ValueTy::Buffer(Box::new(ValueTy::Double))
        );
    }

    #[test]
    fn integrator_is_1_to_1() {
        // A foreign 1→1 builtin applied to a buffer value.
        let t = ty_of("foo = integrator (sine 440.0 0.5 0.0)").unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
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
        let t = ty_of("foo = let g = 0.5 in g * 0.5").unwrap();
        assert_eq!(t.def_schemes["foo"].ty.vty, ValueTy::Double);
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
        let t = ty_with("foo = lowpass 1000.0 0.7 (sine 440.0 0.5 0.0)").unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
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
        assert!(ty_with("lp f = lowpass f 0.7 (sine 440.0 0.5 0.0); foo = lp 1000.0").is_ok());
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
        let t = ty_of("foo = sine 440.0 0.5 0.0 * 0.5").unwrap();
        // A buffer-valued main is the program's single signal output.
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
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
                in_instance_body: false,
                in_signed_body: false,
                dict_selections: HashMap::new(),
                residual: Vec::new(),
                literal_vars: HashMap::new(),
                literal_instances: HashMap::new(),
                value_tys: HashMap::new(),
                entry_name: String::new(),
                final_pass: false,
            };
            let def = &prog.defs[0];
            let body = infer_def_body(&mut ctx, def).unwrap();
            let lam_count = def_scheme_arity(def);
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
            wrote_params: false,
            span: Span::new(0, 0),
        };
        let mut env = TypeEnv::with_builtins();
        env.register_decls(std::slice::from_ref(&def));
        let sig = env
            .constrained_defs
            .get("mk")
            .expect("a forall-wrapped class context must register as constrained");
        // The registry holds the SIGNATURE; the λ-parameter comes from the
        // definition's own body on demand. Both must agree on the arity.
        assert!(matches!(
            sig,
            TypeExpr::TForall(_, _) | TypeExpr::TConstrained(_, _)
        ));
        let (params, _) = effective_def_params(&def);
        assert_eq!(params.len(), 1, "the λ-parameter is recovered");
    }

    #[test]
    fn int_float_mismatch_errors() {
        // A `Double` field does not unify with an `Int` field type.
        assert!(ty_of("data P = { f: Int }; foo = P { f: 1.0 }").is_err());
    }

    #[test]
    fn actor_param_no_default_typechecks() {
        let t = ty_of(r"foo = if ?gain > 0.5 then 1.0 else 2.0").unwrap();
        assert_eq!(t.def_schemes["foo"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn actor_param_with_default_typechecks() {
        // A `?name=default` actor param in a Buffer position keeps the
        // program's single signal output.
        let t = ty_of(r"foo = sine 440.0 ?gain=0.5 0.0").unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
            ValueTy::Buffer(Box::new(ValueTy::Double))
        );
    }

    #[test]
    fn closed_top_level_local_is_caf() {
        // osc = sine 440 0.5 0  (0 input channels, 0 λ-params) -> CAF
        let typed = ty_with("osc = sine 440 0.5 0; foo = 0.5 * 0.5").unwrap();
        assert!(typed.cafs.contains("osc"));
    }

    #[test]
    fn open_block_is_not_caf() {
        // gain x = x * 0.5  (has a λ-param) -> macro, not CAF
        let typed = ty_with("gain x = x * 0.5; foo = gain 0.5").unwrap();
        assert!(!typed.cafs.contains("gain"));
    }

    #[test]
    fn function_with_params_is_not_caf() {
        // voice amp = osc * amp  (has a λ-param) -> not CAF
        let typed =
            ty_with("osc = sine 440 0.5 0; voice amp = osc * amp; foo = voice 0.5").unwrap();
        assert!(typed.cafs.contains("osc"));
        assert!(!typed.cafs.contains("voice"));
    }

    #[test]
    fn data_record_constructor_infers_value_channel() {
        let t =
            ty_of("data Point = { x: Double, y: Double }; foo = Point { x: 1.0, y: 2.0 }").unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
            ValueTy::Con("Point".into(), vec![])
        );
    }

    #[test]
    fn field_project_produces_float_value() {
        let t =
            ty_of("data Point = { x: Double, y: Double }; p = Point { x: 1.0, y: 2.0 }; foo = p.x")
                .unwrap();
        assert_eq!(t.def_schemes["foo"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn sum_match_branches_on_constructor() {
        let t = ty_of(
            r"data Shape = Circle Double | Rect Double Double; foo = case Circle 1.0 of { Circle r -> r; Rect w h -> w; }",
        )
        .unwrap();
        assert_eq!(t.def_schemes["foo"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn field_update_preserves_record_type() {
        let t = ty_of(
            "data Point = { x: Double, y: Double }; p = Point { x: 1.0, y: 2.0 }; foo = p.x := 3.0",
        )
        .unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
            ValueTy::Con("Point".into(), vec![])
        );
    }

    #[test]
    fn match_arms_must_be_value_expressions() {
        // A bare literal arm body is value-compatible (`Circle r -> 1.0`);
        // arms are value channels, so a bare literal is accepted.
        assert!(
            ty_of(r"data Shape = Circle Double; foo = case Circle 1.0 of { Circle r -> 1.0 }")
                .is_ok()
        );
    }

    #[test]
    fn match_scrutinee_must_be_value() {
        assert!(ty_of(r"data Shape = Circle Double; foo = case 3 of { Circle r -> r }").is_err());
    }

    #[test]
    fn match_arms_same_sum_type() {
        assert!(
            ty_of(r"data A = C Double; data B = C Int; foo = case C 1.0 of { C r -> r }").is_err()
        );
    }

    #[test]
    fn match_too_many_arm_bindings_is_error() {
        assert!(
            ty_of(r"data Shape = Circle Double; foo = case Circle 1.0 of { Circle r s -> r }")
                .is_err()
        );
    }

    #[test]
    fn guarded_match_arm_type_checks_and_is_non_exhaustive_alone() {
        // A guarded ctor arm (`Circle r | r > 0.0 => r`) type-checks its guard
        // as a Bool value, but a guarded arm does NOT cover its constructor for
        // exhaustiveness: without a wildcard the match must fail loudly, not
        // silently drop the guard.
        let res = ty_of(
            r"data Shape = Circle Double | Rect Double Double; foo = case Circle 1.0 of { Circle r | r > 0.0 -> r; };",
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
            r"data Shape = Circle Double | Rect Double Double; foo = case Circle 1.0 of { Circle r | r > 0.0 -> r; _ -> 0.0; };"
        )
        .is_ok());
        // A non-Bool guard is a type error.
        let res = ty_of(
            r"data Shape = Circle Double | Rect Double Double; foo = case Circle 1.0 of { Circle r | r + 1.0 -> r; _ -> 0.0; };",
        );
        assert!(
            res.is_err(),
            "a non-Bool guard must be a compile error, not silently compiled"
        );
    }

    #[test]
    fn record_ctor_missing_field_is_error() {
        assert!(ty_of("data Point = { x: Double, y: Double }; foo = Point { x: 1.0 }").is_err());
    }

    #[test]
    fn record_ctor_duplicate_field_is_error() {
        assert!(ty_of(
            "data Point = { x: Double, y: Double }; foo = Point { x: 1.0, x: 2.0, y: 3.0 }"
        )
        .is_err());
    }

    #[test]
    fn if_infers_bool_condition_and_branch_types() {
        let t = ty_of("foo = if 1.0 < 2.0 then 1.0 else 2.0").unwrap();
        assert_eq!(t.def_schemes["foo"].ty.vty, ValueTy::Double);
        assert!(ty_of("foo = if 1.0 then 1.0 else 2.0").is_err());
        assert!(ty_of("foo = if 1.0 < 2.0 then 1 else 2").is_ok());
        // GHC 9.10.3: the arms are unresolved `Num a`, so `default (Integer,
        // Double)` takes its FIRST entry — `Integer`, not `Int`.
        let t2 = ty_of("foo = if 1.0 < 2.0 then 1 else 2").unwrap();
        assert_eq!(t2.def_schemes["foo"].ty.vty, ValueTy::Integer);
    }

    #[test]
    fn scalar_match_infers_literal_patterns_and_totality() {
        let t = ty_of(r"foo = case 0 of { 0 -> 1.0; _ -> 2.0; }").unwrap();
        assert_eq!(t.def_schemes["foo"].ty.vty, ValueTy::Double);
        assert!(ty_of(r"foo = case 0 of { 0 -> 1.0 }").is_err());
        let t2 = ty_of(r"foo = case true of { true -> 1.0; false -> 2.0 }").unwrap();
        assert_eq!(t2.def_schemes["foo"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn bare_int_literal_scrutinee_infers_as_value() {
        // `match 0 of {...}`: a bare Int literal is a signal-rate channel in v1
        // but value-compatible in value positions (mirroring arm_result_vty).
        let t = ty_of(r"foo = case 0 of { 0 -> 1.0; _ -> 2.0; }").unwrap();
        assert_eq!(t.def_schemes["foo"].ty.vty, ValueTy::Double);
        // GHC 9.10.3: `case 0.0 of { 0.0 -> 1; _ -> 2 }` is `Integer` —
        // the arms' unresolved `Num a` takes `default (Integer, Double)`'s
        // FIRST entry, not `Int` (Task 7's stage 3).
        let t2 = ty_of(r"foo = case 0.0 of { 0.0 -> 1; _ -> 2; }").unwrap();
        assert_eq!(t2.def_schemes["foo"].ty.vty, ValueTy::Integer);
    }

    #[test]
    fn nested_cross_sum_ctor_pattern_infers() {
        // `Just (Left x)`: the nested `Left` constructor belongs to `Either`,
        // NOT `Maybe`. The sum context must be re-derived from the payload type
        // at each pattern level.
        let t = ty_of(r"foo = case Just (Left 2.0) of { Just (Left x) -> x; _ -> 0.0; }").unwrap();
        assert_eq!(t.def_schemes["foo"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn nested_user_sum_ctor_pattern_infers() {
        // Same nesting for a USER sum: `Wrap (Foo x)` — `Foo` is looked up in
        // `Inner`, not `Outer`.
        let t = ty_of(
            r"data Inner = Foo Double; data Outer = Wrap Inner | Empty; foo = case Wrap (Foo 2.0) of { Wrap (Foo x) -> x; _ -> 0.0; }",
        )
        .unwrap();
        assert_eq!(t.def_schemes["foo"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn type_after_data_resolves() {
        // Regression: the synonym is declared AFTER the data type that uses it.
        // Registration must be order-independent (two-phase: aliases first).
        let t = ty_of("data P = { x: Angles }; type Angles = Double; foo = P { x: 1.0 }").unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
            ValueTy::Con("P".into(), vec![])
        );
    }

    #[test]
    fn newtype_after_data_resolves() {
        // The newtype is declared AFTER the data type that uses it. The field
        // must resolve to `Newtype("Hz")` (not `Data("Hz")`), so constructing
        // it requires the explicit `Hz 440.0` wrapper.
        let t = ty_of("data P = { f: Hz }; newtype Hz = Double; foo = P { f: Hz 440.0 }").unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
            ValueTy::Con("P".into(), vec![])
        );
        // Newtypes are distinct wrappers: a bare Double does not satisfy a Hz
        // field (no automatic wrapping) — this is what the ordering fix buys
        // (a pre-fix `Data("Hz")` field would accept nothing, not even `Hz 440.0`).
        assert!(ty_of("data P = { f: Hz }; newtype Hz = Double; foo = P { f: 440.0 }").is_err());
    }

    #[test]
    fn chained_alias_resolves() {
        let t =
            ty_of("data P = { f: A }; type A = B; type B = Double; foo = P { f: 1.0 }").unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
            ValueTy::Con("P".into(), vec![])
        );
    }

    #[test]
    fn recursive_data_type_is_compile_error() {
        // A self-referential type would materialise an unbounded arena subtree
        // and exhaust the fixed capacity (spec §9.1 strict acyclicity).
        assert!(
            ty_of("data List = Cons Double List | End Double; foo = Cons 1.0 (End 2.0)").is_err()
        );
    }

    #[test]
    fn recursive_newtype_is_compile_error() {
        assert!(ty_of("newtype A = B; newtype B = A; foo = A 1.0").is_err());
    }

    #[test]
    fn acyclic_data_type_still_compiles() {
        let t =
            ty_of("data Point = { x: Double, y: Double }; foo = Point { x: 1.0, y: 2.0 }").unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
            ValueTy::Con("Point".into(), vec![])
        );
    }

    #[test]
    fn lambda_def_infers_as_func() {
        // A bare Def::Eval whose body is a lambda literal infers to a Func value
        // channel via the normal def-body inference path.
        let t = ty_of(r"double = \x -> x; foo = double").unwrap();
        assert!(matches!(t.def_schemes["foo"].ty.vty, ValueTy::Thunk(_, _)));
    }

    #[test]
    fn lambda_body_value_tuple_is_pair() {
        // `(x , x)` with a value λ-param is a channel tuple → `Pair`, a single
        // 0→1 value channel, so the lambda body is well-typed (the tuple no
        // longer parses as two signal channels).
        let t = ty_of(r"foo = \x -> (x , x)").unwrap();
        match &t.def_schemes["foo"].ty.vty {
            ValueTy::Thunk(_, ret) => {
                assert_eq!(ret.len(), 1);
                match &ret[0] {
                    ValueTy::Con(name, _) => {
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
        let t = ty_of(r"double = \x -> x * 2.0; foo = double").unwrap();
        match &t.def_schemes["foo"].ty.vty {
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
        let t = ty_of(r"add = \a -> \b -> a + b; add3 = add 3.0; foo = add3").unwrap();
        assert_eq!(
            t.def_schemes["foo"].ty.vty,
            ValueTy::Thunk(vec![ValueTy::Double], vec![ValueTy::Double])
        );
        let t = ty_of(r"add = \a -> \b -> a + b; add3 = add 3.0; foo = add3 4.0").unwrap();
        assert_eq!(t.def_schemes["foo"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn named_def_partial_application_types_as_func() {
        // add5 = add2 5.0 (partial application of a named def) types as
        // Func([Double], [Double]).
        let t = ty_of("add2 a b = a + b; add5 = add2 5.0; foo = add5 2.0").unwrap();
        assert_eq!(t.def_schemes["foo"].ty.vty, ValueTy::Double);
    }

    #[test]
    fn opaque_type_is_nameable_but_not_constructible() {
        // `data World;` declares a nameable type: a field/type-alias may
        // mention it — but nothing in source can CONSTRUCT one. `World {}` is
        // a compile-time error ("no constructors").
        let err = match ty_with("data World; foo = World {}") {
            Ok(_) => panic!("constructing an opaque type must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string()
                .contains("opaque type `World` has no constructors"),
            "unexpected error: {err}"
        );
        assert!(
            ty_with("data World; data Box = { w: World }; foo = 1.0").is_ok(),
            "an opaque type must be nameable in a type position"
        );
        assert!(
            ty_with("data World; type Alias = World; foo = 1.0").is_ok(),
            "an opaque type must be usable through a type alias"
        );
    }

    #[test]
    fn opaque_type_has_no_projection_or_patterns() {
        // An opaque value has no fields to project and no constructors to
        // match against: both are compile-time errors. The record wrapper
        // pins the binder's type so the projection/match is not deferred.
        let err = match ty_with(
            "data World; data Box = { w: World }; name b = let v = b.w in v.name; foo = 1.0",
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
            "data W; data Box = { w: W }; first b = let v = b.w in case v of { W -> 1.0 }; foo = 1.0",
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
        let res = ty_of(r"foo = \f -> f f");
        assert!(res.is_err());
    }

    #[test]
    fn collect_ref_names_walks_a_kernel_arguments() {
        // `collect_ref_names` feeds the complete-mode value-`rerate` of the
        // top-level defs a body REFERENCES. A kernel node has no callee name —
        // there is nothing to collect for the node itself — but its ARGUMENTS
        // are ordinary expressions that can name a def. An arm that stops at
        // the node leaves that def on its signal-rate placeholder.
        //
        // MEASURED: no program in the suite discriminates this at the public
        // surface (a partner reachable only through a kernel argument compiles
        // and runs identically with and without the arm, because the retry only
        // re-rates schemes a later instantiation would have freshened anyway).
        // The walk is pinned here instead of pretending a golden value covers it.
        // The body must contain a real `Expr::Kernel`: the PRELUDE's `gcd` is
        // an ordinary definition over `__gcd`, so a program-level `gcd` call is
        // still a name-based application. `__gcd` is the reserved spelling the
        // parser binds to the node.
        let src = r"f = __gcd (h 1) (h 2); h x = x; foo = f";
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        let body = p.defs.iter().find(|d| d.name() == "f").unwrap().body();
        let mut refs = HashSet::new();
        collect_ref_names(body, &mut refs);
        assert!(
            refs.contains("h"),
            "a name used only inside a kernel argument must be collected: {refs:?}"
        );
        assert!(
            !refs.contains(&crate::ir::GCD_PRIMITIVE.to_string()),
            "the kernel itself is not a name and must not be collected: {refs:?}"
        );
    }
}
