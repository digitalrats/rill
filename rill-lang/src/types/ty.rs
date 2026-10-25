//! Arrow type model: a program is a transform over value types (linear lambda
//! model — all data is equal; there is no signal/value split).
//!
//! An expression's type is a single [`ValueTy`] wrapped in [`Ty`]; channel
//! arity is gone (`ins`/`outs` vectors were removed with the block track). A
//! signal is an ordinary value type ([`ValueTy::Buffer`]), executed as
//! `Value::Buffer` on the value track. [`ArrowTy`] is a back-compat alias for
//! [`Ty`]; [`Block`] a back-compat alias for [`ValueTy`].

use std::collections::{HashMap, HashSet};

use crate::ast::{Def, Expr};
use crate::error::{CompileError, Span};

use super::unify::unify_value;

/// A unification variable identifier.
pub type TypeVarId = u32;

/// Upper bound on `resolve_value` chain-following depth. Legitimate nesting —
/// function types (`Func([Func([...])], ...)`) and the argument lists of
/// `App`/`Data`/`Newtype` applications — stays well below this; a value type
/// deeper than this is either a cyclic binding or a pathological program, and
/// resolving it must degrade to the unresolved var — never a stack overflow.
/// The occurs-check in `unify_value` rejects cycles at the source.
pub(crate) const MAX_VALUE_RESOLVE_DEPTH: usize = 64;

/// Type of an arena value. All data is equal — there is no signal/value split
/// (linear lambda model); a signal buffer is the ordinary `ValueTy::Buffer`.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueTy {
    /// Integer value.
    Int,
    /// Floating point value.
    Float,
    /// Boolean value type.
    Bool,
    /// String value type.
    String,
    /// A signal buffer (a block of samples). An ordinary value type, like
    /// `List`/`Map`; executed as `Value::Buffer` on the value track.
    Buffer,
    /// The unit type `()`. The only inhabitant is `Value::Void` (allocated by
    /// the value track's `ValueConstVoid`); used for `IO ()` actions.
    Void,
    /// A named data record, parameterized by its type arguments.
    Data(String, Vec<ValueTy>),
    /// A newtype wrapping another value type.
    Newtype(String, Vec<ValueTy>),
    /// Builtin constructor application: `List Float`.
    App(String, Vec<ValueTy>),
    /// A function/expression type (unary): capture types and result type.
    /// `Thunk([], [t])` is a constant `t`; `Thunk([a], [b])` is `\x:a -> b`.
    /// Replaces the legacy `Func` (same two-list shape, unary core model).
    Thunk(Vec<ValueTy>, Vec<ValueTy>),
    /// Unresolved value-type unification variable.
    Var(TypeVarId),
    /// Type-constructor variable (kind `* -> *` or higher), bound by a
    /// typeclass class variable.
    TyConVar(TypeVarId),
    /// Application of a type-constructor *variable* to arguments: `m b` in a
    /// data-field type (`data K m a b = { f: a -> m b }`). The head is the id of
    /// the data-type's type parameter (same id space as `Var`/`TyConVar`); when it
    /// resolves to a concrete constructor the application becomes `App(c, args)`.
    TyConApp(TypeVarId, Vec<ValueTy>),
}

impl ValueTy {
    /// The type of a unary function capturing `captures` and producing
    /// `result`: `\x:a -> b` is `Thunk([a], [b])`.
    pub fn thunk(captures: Vec<ValueTy>, result: Vec<ValueTy>) -> ValueTy {
        ValueTy::Thunk(captures, result)
    }

    /// The type of a constant of type `t`: a nullary thunk `Thunk([], [t])`.
    pub fn const_ty(t: ValueTy) -> ValueTy {
        ValueTy::Thunk(vec![], vec![t])
    }
}

/// `(a -> List b) -> List a -> List b`: pick the result element type for
/// `concat_map` from the closure's RETURN list element, falling back to the
/// source list element when the return is not a `List`.
pub(crate) fn concat_map_result_elem(closure_ty: &ValueTy, source_elem: ValueTy) -> ValueTy {
    match closure_ty {
        ValueTy::Thunk(_, rets) => match rets.first() {
            Some(ValueTy::App(name, inner)) if name == "List" => {
                inner.first().cloned().unwrap_or(source_elem)
            }
            _ => source_elem,
        },
        _ => source_elem,
    }
}

/// The type of a single expression: a unary core value type.
///
/// Every `infer_expr` produces one `Ty` (a single value); there is no channel
/// arity any more (`ins`/`outs` vectors are gone). A function value carries a
/// `ValueTy::Thunk(captures, result)`; a constant carries its leaf/scalar type.
#[derive(Debug, Clone, PartialEq)]
pub struct Ty {
    /// The value type of the expression.
    pub vty: ValueTy,
}

impl Ty {
    /// A plain value of type `vty`.
    pub fn value(vty: ValueTy) -> Ty {
        Ty { vty }
    }
    /// Back-compat constructor: a value channel is just a single value.
    pub fn value_channel(vty: ValueTy) -> Ty {
        Ty { vty }
    }
    /// Back-compat constructor for `(0 → 1)` transforms; a non-unary
    /// `(n_in → n_out)` shape no longer exists — both are collapsed to the
    /// single value `t`.
    pub fn uniform(_n_in: usize, _n_out: usize, t: ValueTy) -> Ty {
        Ty { vty: t }
    }
    /// Input arity: always 0 — a `Ty` is a single value (channel arity is gone).
    pub fn arity_in(&self) -> usize {
        0
    }
    /// Output arity: always 1 (a `Ty` is a single value).
    pub fn arity_out(&self) -> usize {
        1
    }
    /// The single output value type (replaces the old `outs[0]`).
    pub fn out(&self) -> &ValueTy {
        &self.vty
    }
}

/// Back-compat alias for the unary expression type.
pub type ArrowTy = Ty;

/// Back-compat alias: every channel is a value type.
pub type Block = ValueTy;

/// A polymorphic type scheme `∀ vars. ty`.
///
/// `lam_count` is the number of λ-parameters for this definition (user-defined
/// function arguments). With the unary core the parameters live inside the
/// `Thunk` value type (`f = \a -> \b -> r` has type
/// `Thunk([a], Thunk([b], [r]))`); `lam_count` is retained for the placeholder
/// and lowering paths that count λ-parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Scheme {
    /// Number of λ-parameters.
    pub lam_count: usize,
    /// Quantified type variables.
    pub vars: Vec<TypeVarId>,
    /// The generalized value type.
    pub ty: Ty,
}

/// A substitution mapping type variables to value types.
#[derive(Debug, Clone, Default)]
pub struct Subst {
    /// The value-type mapping. Value-type variables resolve structurally:
    /// unifying `Var(v)` with a concrete `ValueTy` (e.g. a `Func` signature for
    /// a lambda parameter used as a callee) records the binding here, so HOF
    /// parameter types stay concrete by the time lowering needs them (record
    /// projection needs the Data type, a function parameter needs its Func
    /// signature).
    pub value_map: HashMap<TypeVarId, ValueTy>,
}

/// Shape of a declared data type.
#[derive(Debug, Clone)]
pub enum DataInfo {
    /// A product type: field name → value type, plus the strictness flag
    /// (a strict field's argument is forced to WHNF at construction).
    Record(Vec<(String, ValueTy, bool)>),
    /// A sum type: constructor name → payload value types, each carrying its
    /// strictness flag (a strict payload is forced to WHNF at construction).
    Sum(Vec<(String, Vec<(ValueTy, bool)>)>),
    /// An ABSTRACT (opaque) type: nameable in signatures, but with no
    /// constructors and no fields. Source programs cannot build, project, or
    /// pattern-match one — only compiler-provided primitives produce values of
    /// these types (`World`, `STState`, `STRef s a`).
    Opaque,
}

/// A `typeclass` declaration: its type variable and method dictionary.
#[derive(Debug, Clone)]
pub struct TypeclassInfo {
    /// The class type variable (e.g. `a` in `typeclass Show a`).
    pub var: String,
    /// Kind arity of the class variable, inferred from its use in method
    /// signatures (`f a` -> 1, `f a b` -> 2, bare `a` -> 0).
    pub arity: usize,
    /// Method dictionary: method name → signature type expression.
    pub methods: Vec<(String, crate::ast::TypeExpr)>,
    /// Default method bodies: method name → (params, body). An instance that
    /// omits a method uses its class default (precedence: instance body >
    /// default > error).
    pub defaults: HashMap<String, (Vec<String>, Expr)>,
}

/// A concrete `instance` declaration: which class it implements, the concrete
/// type bound to the class variable, and the method bodies it provides.
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    /// The class this instance implements.
    pub class: String,
    /// The instance head as parsed (`List`, `Kleisli m`, `Pair Int Int`). Used
    /// to derive the class variable's concrete value type (the instance head's
    /// full type) for method-body validation.
    pub head: crate::ast::TypeExpr,
    /// The registry key. A partial head (`Kleisli m`) keys by its constructor;
    /// a fully concrete head keys by its FULL applied type (`Pair Int Int`).
    pub ty: String,
    /// Constraint list: (class, type var), e.g. `(Monad, "m")`.
    pub constraints: Vec<(String, String)>,
    /// Type-constructor args bound by a PARTIAL instance head: `Kleisli m` →
    /// `["m"]` (arity 3 total, 1 bound ⇒ 2 remaining). Empty for a bare or
    /// fully concrete head.
    pub head_args: Vec<String>,
    /// Method bodies: method name → (parameter bindings, body).
    pub methods: HashMap<String, (Vec<String>, Expr)>,
    /// True when this instance was declared by a compiler prelude
    /// ([`PRELUDE`]) or derived
    /// from one (a superclass template, a structural `Eq`/`Ord` marker).
    ///
    /// Such bodies are trusted compiler machinery and are not re-validated
    /// against the USER's data environment: a user `data` declaration may
    /// legally shadow a builtin record name (a program declaring its own
    /// `Pair`), which would make a prelude body's field names untypeable
    /// against the shadowed shape. Their bodies still type at every CALL SITE.
    ///
    /// The flag lives on the DECLARATION, not on the `(class, type)` key: a
    /// user instance that replaces a prelude key (`instance Monad IO`) gets a
    /// fresh `InstanceInfo` with `from_prelude: false`, so it is validated like
    /// any other user instance.
    pub from_prelude: bool,
}

impl InstanceInfo {
    /// The value type an instance head binds to the class variable, in the
    /// signature converter's spelling (a bare head resolves through the type
    /// environment, an applied one becomes a constructor application).
    pub(crate) fn class_var_ty(&self, env: &TypeEnv) -> ValueTy {
        crate::types::infer::data_field_vty(env, &[], &self.head)
    }
}

/// A class-constrained function definition (`mk p v :: Ix i => Pair i i -> …`).
///
/// Such a definition is NOT a plain function: its body mentions methods of the
/// constrained class (`rangeSize` in `rangeSize p`), which cannot be resolved
/// until a call site fixes the class variable to a concrete type. The body is
/// therefore never inferred in isolation — each call site specializes it with
/// the concrete class-variable type and the bound instance (compile-time
/// β-substitution, never a runtime dictionary). See
/// [`TypeEnv::constrained_defs`] and `lower::lower_constrained_apply`.
///
/// Limits of the per-call-site model (all deliberate, all diagnosed):
///
/// - A definition that references itself from its own body cannot be
///   specialized: the recursive call's class variable is only solved by the
///   enclosing call site, which is exactly the expansion in flight. Direct
///   self-recursion is already rejected by the general fragment-recursion check
///   (`recursive function call: …`); recursion that reaches lowering through an
///   indirection (a `where` binding) is caught by the specialization guard
///   (`recursive class-constrained definition …`) instead of looping.
/// - The specialization is an ordinary value construct. There is no separate
///   buffer-`Apply` path: a buffer is a value, so a call that cannot be
///   specialized is reported as an ordinary lowering failure.
/// - A BARE reference (`f = mk`) has no call site at which to solve the class
///   variable; lowering reports `cannot use class-constrained definition …
///   without applying it`. The reducer likewise never inlines such a reference
///   (β-reducing the deferred body would lower it against an unsolved variable).
#[derive(Debug, Clone)]
pub struct ConstrainedDef {
    /// The declared signature, a [`crate::ast::TypeExpr::TConstrained`] node
    /// carrying the class context.
    pub sig: crate::ast::TypeExpr,
    /// The definition's formal parameters, recovered from the parser's curried
    /// body (see `split_curried_params`): multi-parameter definitions are
    /// stored as nested lambdas with an empty `Def::Eval::params` list.
    pub params: Vec<crate::ast::Param>,
    /// The definition body with the formal parameters peeled off (it is the
    /// body captured at registration, i.e. before `reduce`; references inside
    /// it still resolve through the program's reduced definition map at
    /// lowering time).
    pub body: Expr,
    /// The definition's `where`-block definitions.
    pub where_defs: Vec<Def>,
}

/// Recover a definition's formal parameters from its curried body.
///
/// The parser turns `f a b = e` into a bare binding whose body is
/// `\a -> \b -> e` and clears the definition's `params` (see
/// `parser::curry_defs_in`), so the formal parameters of a multi-parameter
/// definition live in the body's leading lambdas. `arity` is the definition
/// signature's argument count — it says exactly how many lambdas belong to the
/// definition head rather than to a function-valued body — and the returned
/// body has them peeled off.
///
/// Shared by constrained-definition registration and by the eager inference
/// path, which types an annotated multi-parameter definition's λ-chain from its
/// signature.
pub(crate) fn split_curried_params(
    arity: usize,
    params: &[crate::ast::Param],
    body: &Expr,
) -> (Vec<crate::ast::Param>, Expr) {
    let mut params = params.to_vec();
    let mut body = body.clone();
    while params.len() < arity {
        let (first, rest) = match &body {
            Expr::Lambda {
                params: lam,
                body: inner,
                span,
            } if !lam.is_empty() => {
                let first = lam[0].clone();
                let rest = if lam.len() == 1 {
                    (**inner).clone()
                } else {
                    Expr::Lambda {
                        params: lam[1..].to_vec(),
                        body: inner.clone(),
                        span: *span,
                    }
                };
                (first, rest)
            }
            _ => break,
        };
        params.push(first);
        body = rest;
    }
    (params, body)
}

/// The compile-time type environment: aliases, newtypes, data-type shapes, and
/// the typeclass/instance dictionary.
///
/// Built once during inference and carried to lowering so both phases resolve
/// type names identically (no order dependence, no duplicated resolution).
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    /// Type synonyms (alias name → target name).
    pub type_aliases: HashMap<String, String>,
    /// Newtype wrappers (wrapper name → inner type name).
    pub newtypes: HashMap<String, String>,
    /// Data type declarations: name → shape.
    pub data_types: HashMap<String, DataInfo>,
    /// Typeclass declarations: class name → method dictionary.
    pub typeclasses: HashMap<String, TypeclassInfo>,
    /// Instances grouped by class, then by bound type name.
    ///
    /// An entry carries its declaration's provenance in
    /// [`InstanceInfo::from_prelude`]: a user `instance` replaces a prelude
    /// entry wholesale, so the trust flag must travel with the declaration
    /// rather than with the key.
    pub instances: HashMap<String, HashMap<String, InstanceInfo>>,
    /// Builtin type constructors: name → value arity. `List a` has arity 1,
    /// `Map k v` arity 2. Open collections carry no capacity.
    pub ctor_kinds: HashMap<String, usize>,
    /// User-declared parameterized data types: name → number of type
    /// parameters (`data Box a` → 1). Used to kind-check constructor instances
    /// against a class's arity (`Functor f` needs `Box a`, not `Pair a b`).
    ///
    /// v1 registers only parameterized RECORDS here, not sums. A parameterized
    /// user sum (`data Opt a = Some a | None`) works as an ordinary data type
    /// (construction + match both stay monomorphic `Data(name, [])`), but its
    /// match-pin path is not slot-carrying like a record's field projection, so
    /// an instance body would hit the confusing `Data("Opt", [Var(_)])` vs
    /// `Data("Opt", [])` unify error. Leaving sums out of this table makes such
    /// an instance fail the kind check with a clean "not a type constructor"
    /// / arity message instead. See `validate_instances`.
    pub data_arities: HashMap<String, usize>,
    /// Foreign function declarations: name → declared `TypeExpr` signature.
    /// The language-side contract for Rust-implemented builtins (SP-3a FFI layer).
    pub foreign_sigs: HashMap<String, crate::ast::TypeExpr>,
    /// Built-in polymorphic function signatures carried by the environment
    /// (`runST : (forall s. ST s a) -> a`). Distinct from [`Self::foreign_sigs`]
    /// (the FFI signature conversion rejects `forall`) and from typeclass
    /// methods (these are ordinary functions, not instance-dispatched): a
    /// `forall`-bound argument makes the callee's application rank-N.
    pub builtin_sigs: HashMap<String, crate::ast::TypeExpr>,
    /// Ordinary function definitions declared by the prelude (the Buffer-arrow
    /// combinators `seq`/`par`/`split`/`merge` over Buffer arrows). Unlike
    /// typeclasses/instances/foreign decls, these are plain `Def::Eval`s — they
    /// must be visible to inference as top-level definitions, so
    /// [`infer_program_with`](crate::types::infer::infer_program_with) threads
    /// them into the program's defs before typing.
    pub prelude_defs: Vec<crate::ast::Def>,
    /// Class-constrained definitions (`mk :: Ix i => …`), keyed by name.
    ///
    /// Their bodies are deferred: the constrained class variable is abstract
    /// until a call site solves it, so typing the body at load would dispatch
    /// the body's method calls against an unresolved type. Inference types the
    /// parameters from the signature and lowering specializes the body per call
    /// site (see [`ConstrainedDef`]).
    pub constrained_defs: HashMap<String, ConstrainedDef>,
}

/// The single language prelude, declared in rill-lang itself and registered by
/// [`TypeEnv::with_builtins`]. Users never redeclare it but may add their own
/// instances. `instance Monad T` auto-derives `Applicative T` and
/// `Functor T` (see [`TypeEnv::derive_superclass_instances`]).
///
/// One text, one parse unit: there is no separate "category" / "signal" /
/// "standard" prelude. A buffer is an ordinary value; `Buffer` is an ordinary
/// type class next to `Functor`/`Monad`/`Monoid`.
pub(crate) const PRELUDE: &str = r#"
typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
typeclass Applicative f where { pure: a -> f a; ap: f (a -> b) -> f a -> f b; }
typeclass Monad m where { return: a -> m a; bind: m a -> (a -> m b) -> m b; }
typeclass Monoid m where { mempty: m; mappend: m -> m -> m; }

--  Index types (`Data.Ix`): the offset/range vocabulary the array surface is
--  written against. `range` enumerates the index set in ROW-MAJOR order (last
--  coordinate fastest), `index` gives an element's flat offset, `inRange` tests
--  membership and `rangeSize` counts the elements — its value is also the
--  required element count of `listArray`. Instances are declared in the
--  standard prelude, and every offset they compute is an `Int`,
--  so the flat array primitives only ever see integers.
typeclass Ix i where {
    range: Pair i i -> List i;
    index: Pair i i -> i -> Int;
    inRange: Pair i i -> i -> Bool;
    rangeSize: Pair i i -> Int;
}

data Kleisli m a b = { unKleisli: a -> m b };

typeclass Arrow a where {
    arr: (b -> c) -> a b c;
    first: a b c -> a (Pair b d) (Pair c d);
    compose: a b c -> a c d -> a b d;
    second: a b c -> a (Pair d b) (Pair d c) =
        k (compose (compose (arr (\ p -> Pair { first: p.second, second: p.first })) (first k)) (arr (\ p -> Pair { first: p.second, second: p.first })));
    both: a b c -> a d e -> a (Pair b d) (Pair c e) =
        f g (compose (first f) (second g));
    fan: a b c -> a b d -> a b (Pair c d) =
        f g (compose (arr (\ x -> Pair { first: x, second: x })) (both f g));
}

instance (Monad m) => Arrow (Kleisli m) where {
    arr f = Kleisli (\ x -> return (f x));
    first k = Kleisli (\ p -> bind (k.unKleisli p.first) (\ z -> return (Pair { first: z, second: p.second })));
    compose k1 k2 = Kleisli (\ x -> bind (k1.unKleisli x) (\ y -> k2.unKleisli y));
}

--  --- Buffer values -------------------------------------------------------
--  A buffer is an ordinary first-class VALUE, exactly like a scalar: there is no
--  separate "signal path" and no signal/value split. `Buffer` is an ordinary
--  type class; arithmetic over buffers comes from its instances (P3 of the
--  typeclass layer).
typeclass Buffer b where { }
instance Buffer (FixedBuffer a) where { }
data Tape a = { };
instance Buffer (Tape f32) where { }

--  Legacy Buffer-arrow combinators (`Buffer -> Buffer` as first-class
--  values), retained as plain value functions after the signal surface
--  syntax (`:` / `,` / `<:` / `:>` and the `~` / `@` stateful operators)
--  was removed. `seq`/`par`/`split`/`merge` compose Buffer arrows directly;
--  they use the builtin `Pair a b` record (`first`/`second`) to carry two
--  channels.

--  `seq f g` runs f then g on one channel: `\ x -> g (f x)`.
seq = \ f -> \ g -> \ x -> g (f x);
--  `par f g` runs f and g independently on a two-channel Pair:
--  `\ p -> Pair { first: f p.first, second: g p.second }`.
par = \ f -> \ g -> \ p -> Pair { first: f p.first, second: g p.second };
--  `split f g` fans one channel out to both f and g:
--  `\ x -> Pair { first: f x, second: g x }`.
split = \ f -> \ g -> \ x -> Pair { first: f x, second: g x };
--  `merge f g` sums the two channels through f and g into one:
--  `\ p -> f p.first + g p.second`.
merge = \ f -> \ g -> \ p -> f p.first + g p.second;

--  `otherwise` — the GHC Prelude's guard fallback is an ordinary Bool VALUE,
--  not a keyword: `otherwise = True`. A definition here (rather than a token in
--  the lexer) lets a guard read `| otherwise = e` exactly like any other guard,
--  and lets the totality rule recognise the final unconditional alternative
--  without special-casing surface syntax.
otherwise = true;


--  --- Standard library -----------------------------------------------------
--  The primitive opaque tokens (`World`/`STState`/`STRef`), the `ST`/`IO`
--  monads and their instances, and the opaque io-streams handles
--  (`InputStream`/`OutputStream`).
-- Primitive opaque tokens. `World` is the real-world token (the GHC
-- `State# RealWorld` analog), `STState` the per-region state token threaded by
-- the ST monad. Neither has constructors or fields: only the runtime creates
-- them.
data World;
data STState;
-- An opaque cell handle. Only `newSTRef` creates one; the region phantom `s`
-- keeps it from escaping its `runST` region.
data STRef s a;
-- An opaque mutable-array handle, mirroring `STRef`. The ST effects create one
-- (`newArrayFlat`) and `runSTArray` freezes it into an immutable `Array`; the
-- region phantom `s` keeps an `STArray` from escaping its `runST` region.
data STArray s i e;
-- The state-threading monad: a state token in, the advanced token AND the
-- result out. `runST` supplies the initial token and discards the final one.
data ST s a = { unST: STState -> Pair STState a };
-- The state-threading IO monad: `IO a` is an action from the real-world token
-- to the advanced token AND the result. The runner supplies `realWorld`; the
-- `World` token itself is an opaque value only the runtime holds.
data IO a = { unIO: World -> Pair World a };
-- Opaque stream handles: only runtime effects create them, so no program text
-- can construct, project, or match one (`fromList`/`stdout` are values/effects
-- that yield a handle).
data InputStream  a;
data OutputStream a;

-- Haskell 2010 §4.4.2 / the Prelude: `(++) :: [a] -> [a] -> [a]` is an
-- ordinary function, MONOMORPHIC to lists (it is NOT `Monoid.mappend`
-- generalized). Defined here so `(++)` is usable as a first-class value
-- (`fold (++) …`, `f = (++)`), not only as a parser-level infix. The infix
-- `a ++ b` desugars to this same name.
(++) a b = append_list a b;

instance Monoid Float where { mempty = 0.0; mappend a b = a + b; }
instance Monoid String where { mempty = ""; mappend a b = concat_string a b; }
-- Haskell 2010's `instance Monoid [a]` delegates TO `++` (`mappend = (++)`),
-- not the other way round: `++` is the primitive and the instance is the
-- derived form.
instance Monoid List where { mempty = list; mappend a b = a ++ b; }

instance Functor List where { fmap g xs = map g xs; }
instance Functor Maybe where {
    fmap g m = case m of { Nothing -> Nothing; Just x -> Just (g x); };
}

instance Monad Maybe where {
    return x = Just x;
    bind m f = case m of { Nothing -> Nothing; Just x -> f x; };
}
instance Monad List where {
    return x = cons x (list);
    bind xs f = concat_map f xs;
}

instance Monad (ST s) where {
    return x = ST { unST: \s -> Pair { first: s, second: x } };
    bind m f = ST { unST: \s -> let p = m.unST s in let q = f p.second in q.unST p.first };
};

instance Monad IO where {
    return x = IO { unIO: \w -> Pair { first: w, second: x } };
    bind m f = IO { unIO: \w -> let io_state = m.unIO w in let io_next = f io_state.second in io_next.unIO io_state.first };
};

--  ---------------------------------------------------------------------------
--  Data.Array — the constrained language-level array surface.
--
--  It is written entirely against `Ix` (declared in this prelude), on top
--  of the flat storage primitives (`arrayFromList`, `arrayIndexFlat`,
--  `arrayUpdate`, `arrayAccum`, `arrayFromAssocs`, `arraySeed`, the `STArray`
--  effects, and the buffer bridge).
--
--  OFFSETS for construction/update/accum (`arrayFromAssocs`, `arrayUpdate`,
--  `arrayAccum`) are now resolved BY THE PRIMITIVES against the stored bounds
--  for the two prelude index shapes (`Int`, `Pair Int Int`) — a structural
--  limitation: building an array over a user-defined index type faults
--  "unsupported index type". READS stay language-side, through
--  `checkedOffset`/`index`, so user `Ix` instances keep working for `!`
--  and the ST wrappers; only `array`-construction over a non-prelude index
--  shape is out of reach.
--
--  Construction details worth spelling out:
--    * `array`'s unlisted positions are UNDEFINED elements — one shared lazy
--      callable that latches the clean fault "array: undefined array element"
--      when forced (GHC's `undefined`), which makes `array` generic in the
--      element type. Associations are applied in order; a repeated index
--      leaves the last value. `accumArray` seeds every position with its
--      initial value and therefore has no holes.
--
--  `listArray` / `newListArray` follow GHC's lenient construction (no length
--  check): a list SHORTER than `rangeSize` pads the tail with the same shared
--  lazy hole as `array`'s unlisted positions (forcing it faults "array:
--  undefined array element"); a list LONGER than `rangeSize` is silently
--  truncated to the array's size. This is exactly GHC's `arrEleBottom` model
--  shared by `array`, `listArray`, and `newListArray`.
--
--  Fault policy: every CELL reference is bounds-checked, not just the flat
--  offset. `checkedOffset` turns an out-of-range cell into `-1`, which the flat
--  primitives reject with their own clean out-of-range fault — so `!`/`index`
--  and every association's index fault instead of wrapping into a neighbouring
--  row (a flat offset cannot tell the storage layer which cell it came from).
--
--  `range` enumerates ROW-MAJOR (last coordinate fastest), which is exactly the
--  order `Ix.index` flattens to, so `indices`/`assocs` and the element list of
--  a 2-D `listArray` all agree.
--  ---------------------------------------------------------------------------

--  1-D indices: `rangeSize` is defined per instance (rather than as a class
--  default over `length (range b)`) so an instance body never has to invoke a
--  sibling method of its own class; `range` itself is the definition of the
--  enumeration, so counting it would be circular in spirit.
instance Ix Int where {
    range b = listRange b;
    index b i = i - b.first;
    inRange b i = (b.first <= i) && (i <= b.second);
    rangeSize b = b.second - b.first + 1;
}

--  2-D indices, row-major over `Pair Int Int`. The instance is SELF-CONTAINED on
--  purpose: the per-coordinate arithmetic is inlined instead of dispatching the
--  `Ix Int` sibling method, so an instance body never depends on instance
--  resolution of its own class (the same class variable would have to be solved
--  twice). `range` builds the grid with `concat_map`/`map` over two inclusive
--  `listRange`s — no recursion, which strict mode bans.
instance Ix (Pair Int Int) where {
    range b = concat_map (\ i1 -> map (\ i2 -> Pair { first: i1, second: i2 }) (listRange (Pair { first: (b.first).second, second: (b.second).second }))) (listRange (Pair { first: (b.first).first, second: (b.second).first }));
    index b i = (i.first - (b.first).first) * ((b.second).second - (b.first).second + 1) + (i.second - (b.first).second);
    inRange b i = ((b.first).first <= i.first) && (i.first <= (b.second).first) && ((b.first).second <= i.second) && (i.second <= (b.second).second);
    rangeSize b = ((b.second).first - (b.first).first + 1) * ((b.second).second - (b.first).second + 1);
}

--  --- Immutable arrays ------------------------------------------------------
--  `bounds` and `elems` are plain definitions (not builtins): the ST read-backs
--  `getBounds`/`getElems` are the mutable-handle counterparts and stay distinct.

--  Both carry EXPLICIT signatures: without one the inferred parameter type is
--  the numeric fallback, so `arrayBounds` inside the body saw a non-array and
--  typed the bounds as a flat `Pair Int Int` (right by accident for 1-D, wrong
--  for a nested index). With the signature the index type is a signature
--  variable, which never solves an `Ix` instance — a caller that indexes with
--  `bounds a` therefore routes to the array definition, never to a bogus
--  `Ix Int` method.
bounds a :: Array i e -> Pair i i = arrayBounds a;
elems a :: Array i e -> List e = arrayElems a;

--  `index` is DOUBLE DUTY: the `Ix` offset method (`Pair i i -> i -> Int`) and
--  this array accessor (`Array i e -> i -> e`, the `!` operator). The dispatch is
--  type-directed on the first argument — a `Pair` bounds value selects the
--  method, an `Array` selects this definition — and the body below relies on
--  exactly that to reach the METHOD for its offset. The offset is computed by
--  `checkedOffset` (the PRIMITIVE `arrayBounds`, not the `bounds` definition: a
--  plain definition is lowered through its (polymorphic) scheme, so its index
--  type would stay a variable and the METHOD dispatch would be lost).
index a ix :: Ix i => Array i e -> i -> e = arrayIndexFlat a (checkedOffset (arrayBounds a) ix);

--  `checkedOffset bounds ix`: the flat offset of `ix`, or `-1` when the cell is
--  outside the bounds. Every flat primitive rejects a negative offset, so an
--  out-of-range CELL takes the same clean fault channel as an out-of-range flat
--  offset. Without the check a 2-D cell such as `(0, 3)` in a 3x3 grid would
--  compute the in-range offset `3` and silently read the next row's first
--  element — a wrapped read the storage layer cannot detect, because a flat
--  offset carries no information about the cell it came from.
checkedOffset b ix :: Ix i => Pair i i -> i -> Int = if inRange b ix then index b ix else -1;

--  Lenient constructor (GHC-faithful): a list SHORTER than `rangeSize` pads
--  the tail with the shared lazy hole (forcing it faults "array: undefined
--  array element"); a list LONGER than `rangeSize` is silently truncated.
--  `arrayFromListChecked` carries the pre-computed size to the interpreter,
--  which pads or truncates.
listArray b es :: Ix i => Pair i i -> List e -> Array i e = arrayFromListChecked b (rangeSize b) es;

--  `array assocs`: every unlisted position is ONE SHARED lazy hole whose
--  FORCE latches the clean fault "array: undefined array element" (GHC's
--  `undefined` for unlisted positions — not a `0.0` fill). Associations are
--  applied in order; a repeated index leaves the last value. The interpreter
--  resolves the index-typed associations against the construction bounds for
--  the two prelude index shapes (`Int`, `Pair Int Int`).
array b xs :: Ix i => Pair i i -> List (Pair i e) -> Array i e = arrayFromAssocs b xs;

--  `accumArray f init assocs`: seed every position with `init`, then fold the
--  associations in order (`f old new`). `arraySeed` fills all positions, so
--  an `accumArray` has no holes.
accumArray f init b xs :: Ix i => (e -> (e -> e)) -> e -> Pair i i -> List (Pair i e) -> Array i e
                          = arrayAccum (arraySeed b init) f xs;

--  `(//)` — apply (index, value) associations, last wins, source untouched.
update a xs :: Ix i => Array i e -> List (Pair i e) -> Array i e = arrayUpdate a xs;

--  `accum f` — fold associations into an existing array.
accum f a xs :: Ix i => (e -> (e -> e)) -> Array i e -> List (Pair i e) -> Array i e = arrayAccum a f xs;

indices a :: Ix i => Array i e -> List i = range (arrayBounds a);

assocs a :: Ix i => Array i e -> List (Pair i e) = map (\ ix -> Pair { first: ix, second: index a ix }) (indices a);

--  `ixmap b f a`: reindex — the result is indexed by `Pair i i`, and position
--  `ix` takes `a`'s element at `f ix`. TWO `Ix` instances are in play, one per
--  index type.
ixmap b f a :: (Ix i, Ix j) => Pair i i -> (i -> j) -> Array j e -> Array i e = arrayFromList b (map (\ ix -> index a (f ix)) (range b));

--  --- Mutable arrays (ST) ---------------------------------------------------
--  The wrappers compute the flat offset with `Ix.index` on the handle's own
--  bounds (`getBounds`) and then call the corresponding `…Flat` effect.

newArray b e :: Ix i => Pair i i -> e -> ST s (STArray s i e) = newArrayFlat b (rangeSize b) e;

--  Lenient, like `listArray`: a short list pads the tail with the shared lazy
--  hole; a long list is silently truncated to `rangeSize`.
newListArray b es :: Ix i => Pair i i -> List e -> ST s (STArray s i e) = newArrayFromListFlat b (rangeSize b) es;

readArray arr ix :: Ix i => STArray s i e -> i -> ST s e = bind (getBounds arr) (\ b -> readArrayFlat arr (index b ix));

writeArray arr ix v :: Ix i => STArray s i e -> i -> e -> ST s () = bind (getBounds arr) (\ b -> writeArrayFlat arr (index b ix) v);

modifyArray arr ix f :: Ix i => STArray s i e -> i -> (e -> e) -> ST s () = bind (getBounds arr) (\ b -> modifyArrayFlat arr (index b ix) f);

from n = cons n (from (n + 1.0));

--  Syntax terminator: `register_prelude` parses this text as a program, and a
--  program needs at least one definition. The name is reserved (`__` prefix) so
--  it can NEVER collide with, or shadow, a user program's `main` — the prelude
--  must not participate in the entry-point contract.
__prelude_end = ();
"#;

/// Inline catalog of foreign builtin declarations auto-registered into
/// [`TypeEnv::foreign_sigs`] (and their record `data` types into
/// [`TypeEnv::data_types`]). Programs resolve these names without a
/// user-written `foreign` per program — the single signature source since
/// the legacy `BuiltinSig` machinery was removed (SP-3b Task 12).
///
/// Scope note: signal-input builtins are listed here so a program can apply
/// them to a signal value directly — `onepole 200.0 0.7 buf` applies the
/// partial application to the buffer. The record-param builtins
/// (`mixer`/`eq_parametric`/`dry_wet`) declare their record configs here too
/// (`mixer`'s leading `List (FixedBuffer f32)` is a VariadicSignal followed
/// only by a record — [`crate::types::ffi::ffi_sig_from_typeexpr`] accepts the
/// shape). `tape_loop` (`Int -> Tape f32`) is a resource CONSTRUCTOR: its
/// `Tape f32` result is not a `FixedBuffer`/`Pair`, so
/// [`crate::types::ffi::ffi_sig_from_typeexpr`] rejects it by design — lowering
/// recognizes `tape_loop <capacity>` directly (like the legacy `TapeLoop`
/// declaration) and produces the tape index, which flows to `write_head` /
/// `read_head` via their `Tape f32` resource params (Task 11).
pub(crate) const BUILTIN_FOREIGN_DECLS: &str = r#"
foreign sine : Float -> Float -> Float -> FixedBuffer f32;
foreign saw : Float -> Float -> Float -> FixedBuffer f32;
foreign square : Float -> Float -> Float -> FixedBuffer f32;
foreign triangle : Float -> Float -> Float -> FixedBuffer f32;
foreign noise : Float -> Float -> FixedBuffer f32;
foreign ay38910 : Float -> Float -> FixedBuffer f32;
foreign sampler : Float -> Float -> Float -> Float -> Float -> FixedBuffer f32;

foreign integrator : FixedBuffer f32 -> FixedBuffer f32;
foreign leaky_integrator : Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign onepole : Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign moog : Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign lowpass : Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign highpass : Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign biquad : Float -> Float -> Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign delay : Float -> Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign distortion : Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign limiter : Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign graphic_eq : Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign spectralgate : Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign spectraldelay : Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign convolver : Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign analog_moog : Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign lofi : Float -> Float -> Float -> Float -> Float -> Float -> Float -> FixedBuffer f32 -> FixedBuffer f32;
foreign eq_parametric : EqConfig -> FixedBuffer f32 -> FixedBuffer f32;

foreign tape_loop : Int -> Tape f32;
foreign read_head : Tape f32 -> Float -> FixedBuffer f32;

data EqBand = { freq: Float, q: Float, gain_db: Float, band_type: Int };
data EqConfig = { bands: List EqBand };

main = ();
"#;

/// Derived instance bodies for the superclass chain, written in rill-lang.
/// `instance Monad T` ⇒ `Applicative T` (`pure` = `return`, `ap` via `bind`) and
/// `Functor T` (`fmap` via `bind`); `instance Applicative T` ⇒ `Functor T`
/// (`fmap` via `ap`/`pure`). Referenced by [`TypeEnv::derive_superclass_instances`].
const APPLICATIVE_FROM_MONAD: &str =
    "pure x = return x; ap mf mx = bind mf (\\ f -> bind mx (\\ x -> return (f x)));";
const FUNCTOR_FROM_MONAD: &str = "fmap g x = bind x (\\ y -> return (g y));";
const FUNCTOR_FROM_APPLICATIVE: &str = "fmap g x = ap (pure g) x;";

/// Parse an `instance C T where { <template> }` source fragment and build the
/// [`InstanceInfo`]. Used by superclass auto-derivation.
///
/// `from_prelude` copies the provenance of the instance the template derives
/// from: a `Monad` instance declared in a prelude yields trusted superclass
/// templates, while a user `Monad` instance yields templates that
/// [`validate_instances`](crate::types::infer::validate_instances) checks like
/// any user body.
fn instance_from_template(
    ty: &str,
    class: &str,
    bodies: &str,
    head_args: &[String],
    from_prelude: bool,
) -> InstanceInfo {
    let head = if head_args.is_empty() {
        ty.to_string()
    } else {
        let joined: String = head_args
            .iter()
            .map(|h| h.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        format!("({} {})", ty, joined)
    };
    let src = format!("instance {class} {head} where {{ {bodies} }}; main = ();");
    let toks = crate::lexer::tokenize(&src);
    debug_assert!(toks.is_ok(), "derived instance template must lex");
    let program = crate::parser::parse(&toks.ok().unwrap(), src.as_bytes());
    debug_assert!(program.is_ok(), "derived instance template must parse");
    let defs = program.ok().unwrap().defs;
    let inst = defs
        .iter()
        .find(|d| matches!(d, Def::Instance { .. }))
        .cloned()
        .unwrap();
    let Def::Instance {
        head,
        ty,
        head_args,
        method_bodies,
        ..
    } = inst
    else {
        unreachable!("derived instance template produced an instance")
    };
    let mut methods: HashMap<String, (Vec<String>, Expr)> = HashMap::new();
    for (mname, params, body) in method_bodies {
        let bindings = params.iter().map(|p| p.name.clone()).collect();
        methods.insert(mname.clone(), (bindings, body));
    }
    InstanceInfo {
        class: class.to_string(),
        head,
        ty,
        constraints: vec![],
        head_args,
        methods,
        from_prelude,
    }
}

/// Parse a prelude/catalog fragment and register its defs: `Def::Data` through
/// the phase-2 path ([`crate::types::infer::data_field_vty`], the same path
/// user data declarations use) and every other declaration
/// (`typeclass`/`instance`/`foreign`) through
/// [`TypeEnv::register_decls`]. A parse failure is a compiler bug (the
/// constants are fixed) — assert loudly.
fn register_prelude(env: &mut TypeEnv, src: &str) {
    let toks = crate::lexer::tokenize(src);
    debug_assert!(toks.is_ok(), "prelude must lex");
    let program = crate::parser::parse(&toks.ok().unwrap(), src.as_bytes());
    debug_assert!(
        program.is_ok(),
        "prelude must parse: {:?}",
        program.as_ref().err()
    );
    let defs = program.ok().unwrap().defs;
    // Prelude `data` declarations (`data Kleisli m a b = …`, the catalog's
    // record types) and ABSTRACT declarations (`data World;` — the primitive
    // opaque tokens) register here. User `data` declarations register in infer
    // phase 2 (`infer_program_with`) — `register_decls` deliberately does NOT
    // handle `Def::Data`/`Def::Abstract`, so a declaration reaches exactly one
    // registration path (no double-register).
    for def in &defs {
        if let Def::Data {
            name,
            tyvars,
            fields,
            ..
        } = def
        {
            let fields_ty = fields
                .iter()
                .map(|(f, t, strict)| {
                    let ft = crate::types::infer::data_field_vty(env, tyvars, t);
                    (f.clone(), ft, *strict)
                })
                .collect();
            env.data_types
                .insert(name.clone(), DataInfo::Record(fields_ty));
            if !tyvars.is_empty() {
                env.data_arities.insert(name.clone(), tyvars.len());
            }
        }
        if let Def::Abstract { name, tyvars, .. } = def {
            env.data_types.insert(name.clone(), DataInfo::Opaque);
            if !tyvars.is_empty() {
                env.data_arities.insert(name.clone(), tyvars.len());
            }
        }
    }
    env.register_decls(&defs);
    // Mark the instances this fragment declares as trusted compiler machinery;
    // their bodies are not re-validated against the user's (possibly
    // shadowing) data declarations — see [`InstanceInfo::from_prelude`]. The
    // flag is per DECLARATION: a user instance with the same `(class, type)`
    // key replaces this entry with its own `from_prelude: false`.
    for def in &defs {
        if let Def::Instance { class, ty, .. } = def {
            if let Some(info) = env
                .instances
                .get_mut(class.as_str())
                .and_then(|by_ty| by_ty.get_mut(ty.as_str()))
            {
                info.from_prelude = true;
            }
        }
    }
    // Plain function definitions in a prelude (`seq = \ f -> \ g -> …`)
    // become top-level defs visible to inference — see
    // [`TypeEnv::prelude_defs`]. `Def::Foreign`/`Typeclass`/`Instance`/`Data`
    // are registered by [`Self::register_decls`]; only `Eval` defs (the
    // combinator sugar over Buffer arrows) land here.
    for def in &defs {
        if matches!(
            def,
            Def::Eval { .. } | Def::TypeAlias { .. } | Def::Newtype { .. } | Def::Sum { .. }
        ) && def.name() != "main"
        {
            env.prelude_defs.push(def.clone());
        }
    }
}

impl TypeEnv {
    /// A `TypeEnv` with the builtin constructor table, the builtin
    /// `Maybe`/`Pair`/`Either` type shapes, and the category-theory prelude
    /// (`Functor`/`Applicative`/`Monad`/`Monoid` classes + instances)
    /// registered.
    pub fn with_builtins() -> Self {
        let ctor_kinds = [
            ("List".to_string(), 1usize),
            ("Maybe".to_string(), 1usize),
            ("Set".to_string(), 1usize),
            ("Map".to_string(), 2usize),
            ("Pair".to_string(), 2usize),
            ("Either".to_string(), 2usize),
            ("Array".to_string(), 2usize),   // i e
            ("STArray".to_string(), 3usize), // s i e
            ("FixedBuffer".to_string(), 1usize),
        ]
        .into_iter()
        .collect();
        // NOTE: the `Var(1)`/`Var(2)` ids below are NOT unification variables —
        // they are placeholder parameter positions inside the builtin type
        // shapes (`Maybe a`, `Pair a b`, `Either a b`). They collide with the
        // live inference var space (`Ctx::next` counts from 0), so any Phase
        // 7/8 consumer that resolves these shapes MUST instantiate them with
        // fresh ids before unifying against a live `Subst`.
        let data_types = HashMap::from([
            (
                "Maybe".to_string(),
                DataInfo::Sum(vec![
                    ("Just".to_string(), vec![(ValueTy::Var(1), false)]),
                    ("Nothing".to_string(), vec![]),
                ]),
            ),
            (
                "Pair".to_string(),
                DataInfo::Record(vec![
                    ("first".to_string(), ValueTy::Var(1), false),
                    ("second".to_string(), ValueTy::Var(2), false),
                ]),
            ),
            (
                "Either".to_string(),
                DataInfo::Sum(vec![
                    ("Left".to_string(), vec![(ValueTy::Var(1), false)]),
                    ("Right".to_string(), vec![(ValueTy::Var(2), false)]),
                ]),
            ),
        ]);
        let typeclasses = HashMap::from([
            (
                "Eq".to_string(),
                TypeclassInfo {
                    var: "a".to_string(),
                    arity: 0,
                    methods: vec![(
                        "eq".to_string(),
                        crate::ast::TypeExpr::TFunc(
                            vec![crate::ast::TypeExpr::TName("a".into())],
                            Box::new(crate::ast::TypeExpr::TName("Bool".into())),
                        ),
                    )],
                    defaults: HashMap::new(),
                },
            ),
            (
                "Ord".to_string(),
                TypeclassInfo {
                    var: "a".to_string(),
                    arity: 0,
                    methods: vec![(
                        "lt".to_string(),
                        crate::ast::TypeExpr::TFunc(
                            vec![crate::ast::TypeExpr::TName("a".into())],
                            Box::new(crate::ast::TypeExpr::TName("Bool".into())),
                        ),
                    )],
                    defaults: HashMap::new(),
                },
            ),
        ]);
        let mut env = TypeEnv {
            ctor_kinds,
            data_types,
            typeclasses,
            ..TypeEnv::default()
        };
        // The single language prelude: classes, instances, the primitive
        // tokens, and the buffer type — declared in rill-lang itself so they are
        // first-class entities. A parse failure here is a compiler bug (the
        // constant is fixed) — assert loudly.
        register_prelude(&mut env, PRELUDE);
        // Every `Monad` instance derives its `Applicative`/`Functor`
        // superclasses; one derivation pass after the whole prelude is parsed.
        env.derive_superclass_instances();
        // Builtin foreign catalog: the migrated legacy builtins' declarations
        // plus the record types their params need. Parsed the same way as the
        // preludes — a parse failure here is a compiler bug.
        register_prelude(&mut env, BUILTIN_FOREIGN_DECLS);
        // Builtin polymorphic functions: `runST : (forall s. ST s a) -> a` is a
        // forall-typed ordinary function (NOT a foreign decl — FFI conversion
        // hits its `other` arm on `forall`, and it is not instance-dispatched).
        // The `ST s a` type is declared in `PRELUDE`; rank-N application
        // of the signature lives in `infer_apply_impl`.
        env.builtin_sigs.insert(
            "runST".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![crate::ast::TypeExpr::TForall(
                    vec!["s".to_string()],
                    Box::new(crate::ast::TypeExpr::TApp(
                        "ST".to_string(),
                        vec![
                            crate::ast::TypeExpr::TName("s".into()),
                            crate::ast::TypeExpr::TName("a".into()),
                        ],
                    )),
                )],
                Box::new(crate::ast::TypeExpr::TName("a".into())),
            ),
        );
        // The ST cell primitives. Their signatures are `TForall`-bound over the
        // region phantom `s` (and the element type `a`): the ordinary
        // builtin-sig path instantiates each bound var to a fresh inference
        // variable, so a call inside `runST (\s -> …)` unifies that variable
        // with the enclosing region's `TyConVar` — that is what makes an
        // escaping `STRef s a` a compile-time error.
        //
        //   newSTRef    :: a -> ST s (STRef s a)
        //   readSTRef   :: STRef s a -> ST s a
        //   writeSTRef  :: STRef s a -> a -> ST s ()
        //   modifySTRef :: STRef s a -> (a -> a) -> ST s ()
        let st =
            |args: Vec<crate::ast::TypeExpr>| crate::ast::TypeExpr::TApp("ST".to_string(), args);
        let stref =
            |args: Vec<crate::ast::TypeExpr>| crate::ast::TypeExpr::TApp("STRef".to_string(), args);
        let tv = |n: &str| crate::ast::TypeExpr::TName(n.to_string());
        let forall_s = |inner: crate::ast::TypeExpr| {
            crate::ast::TypeExpr::TForall(vec!["s".to_string()], Box::new(inner))
        };
        env.builtin_sigs.insert(
            "newSTRef".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![tv("a")],
                Box::new(forall_s(st(vec![tv("s"), stref(vec![tv("s"), tv("a")])]))),
            ),
        );
        env.builtin_sigs.insert(
            "readSTRef".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![stref(vec![tv("s"), tv("a")])],
                Box::new(forall_s(st(vec![tv("s"), tv("a")]))),
            ),
        );
        env.builtin_sigs.insert(
            "writeSTRef".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![stref(vec![tv("s"), tv("a")]), tv("a")],
                Box::new(forall_s(st(vec![tv("s"), tv("()")]))),
            ),
        );
        env.builtin_sigs.insert(
            "modifySTRef".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![
                    stref(vec![tv("s"), tv("a")]),
                    crate::ast::TypeExpr::TFunc(vec![tv("a")], Box::new(tv("a"))),
                ],
                Box::new(forall_s(st(vec![tv("s"), tv("()")]))),
            ),
        );
        // The STArray primitives. Flat, type-agnostic operations mirroring the
        // STRef family: every flat size and offset is computed by the LANGUAGE
        // (the `Ix` class lands in a later task), so there is no `Ix i =>`
        // constraint and no range math in the interpreter. `s` is quantified in
        // result position exactly like the STRef signatures, which links each
        // operation to the enclosing `runST` region's phantom — that is what
        // makes an escaping `STArray s i e` a compile-time error.
        //
        //   newArrayFlat    :: Pair i i -> Int -> e -> ST s (STArray s i e)
        //   readArrayFlat   :: STArray s i e -> Int -> ST s e
        //   writeArrayFlat  :: STArray s i e -> Int -> e -> ST s ()
        //   modifyArrayFlat :: STArray s i e -> Int -> (e -> e) -> ST s ()
        //   getBounds       :: STArray s i e -> ST s (Pair i i)
        //   getElems        :: STArray s i e -> ST s (List e)
        let starray = |args: Vec<crate::ast::TypeExpr>| {
            crate::ast::TypeExpr::TApp("STArray".to_string(), args)
        };
        let pair_of =
            |args: Vec<crate::ast::TypeExpr>| crate::ast::TypeExpr::TApp("Pair".to_string(), args);
        let list_of =
            |arg: crate::ast::TypeExpr| crate::ast::TypeExpr::TApp("List".to_string(), vec![arg]);
        let arr_ie = || crate::ast::TypeExpr::TApp("Array".to_string(), vec![tv("i"), tv("e")]);
        let sa_ie = || starray(vec![tv("s"), tv("i"), tv("e")]);
        env.builtin_sigs.insert(
            "newArrayFlat".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![pair_of(vec![tv("i"), tv("i")]), tv("Int"), tv("e")],
                Box::new(forall_s(st(vec![tv("s"), sa_ie()]))),
            ),
        );
        env.builtin_sigs.insert(
            "readArrayFlat".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![sa_ie(), tv("Int")],
                Box::new(forall_s(st(vec![tv("s"), tv("e")]))),
            ),
        );
        env.builtin_sigs.insert(
            "writeArrayFlat".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![sa_ie(), tv("Int"), tv("e")],
                Box::new(forall_s(st(vec![tv("s"), tv("()")]))),
            ),
        );
        env.builtin_sigs.insert(
            "modifyArrayFlat".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![
                    sa_ie(),
                    tv("Int"),
                    crate::ast::TypeExpr::TFunc(vec![tv("e")], Box::new(tv("e"))),
                ],
                Box::new(forall_s(st(vec![tv("s"), tv("()")]))),
            ),
        );
        env.builtin_sigs.insert(
            "getBounds".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![sa_ie()],
                Box::new(forall_s(st(vec![tv("s"), pair_of(vec![tv("i"), tv("i")])]))),
            ),
        );
        env.builtin_sigs.insert(
            "getElems".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![sa_ie()],
                Box::new(forall_s(st(vec![tv("s"), list_of(tv("e"))]))),
            ),
        );
        // `newArrayFromListFlat` — the strict `newListArray` allocation:
        //
        //   newArrayFromListFlat :: Pair i i -> Int -> List e -> ST s (STArray s i e)
        //
        // The `Int` is the language-computed `rangeSize`; the interpreter
        // compares it with the list length and faults on a mismatch.
        env.builtin_sigs.insert(
            "newArrayFromListFlat".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![pair_of(vec![tv("i"), tv("i")]), tv("Int"), list_of(tv("e"))],
                Box::new(forall_s(st(vec![tv("s"), sa_ie()]))),
            ),
        );
        // `runSTArray` — the freeze. Its signature is the rank-N shape of
        // `runST` with the region's payload pinned to an `STArray`:
        //
        //   runSTArray :: (forall s. ST s (STArray s i e)) -> Array i e
        //
        // The region phantom is quantified in the ARGUMENT (like `runST`), so
        // the body's `s` cannot escape into `Array i e`. The rank-2 application
        // itself lives in `infer_apply_impl` (mirroring `runST`); this entry is
        // the signature of record and the shape the application detects.
        env.builtin_sigs.insert(
            "runSTArray".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![crate::ast::TypeExpr::TForall(
                    vec!["s".to_string()],
                    Box::new(st(vec![tv("s"), sa_ie()])),
                )],
                Box::new(arr_ie()),
            ),
        );
        // IO / io-streams primitives. `IO` is the state-threading monad
        // declared in `PRELUDE` (`IO a = { unIO: World -> Pair World a }`);
        // `()` is `ValueTy::Void` (`TName "()"` resolves to it in
        // `type_expr_to_vty`). The element type of a memory stream is generic,
        // so each polymorphic signature is a `TForall`: the ordinary
        // builtin-sig path instantiates the binder once per call site (Task 1),
        // keeping the input/output element types linked.
        //
        //   fromList        :: List a -> IO (InputStream a)
        //   toList          :: InputStream a -> IO (List a)
        //   write           :: Maybe a -> OutputStream a -> IO ()
        //   read            :: InputStream a -> IO (Maybe a)
        //   atEnd           :: InputStream a -> IO Bool
        //   unread          :: a -> InputStream a -> IO ()
        //   connect         :: InputStream a -> OutputStream a -> IO ()
        //   makeInputStream :: IO (Maybe a) -> IO (InputStream a)
        //   makeOutputStream:: (Maybe a -> IO ()) -> IO (OutputStream a)
        //   stdout          :: OutputStream String
        //   stderr          :: OutputStream String
        //   stdin           :: InputStream String
        //   show_float      :: Float -> String
        //   show_int        :: Int -> String
        //   show_bool       :: Bool -> String
        let forall_a = |inner: crate::ast::TypeExpr| {
            crate::ast::TypeExpr::TForall(vec!["a".to_string()], Box::new(inner))
        };
        let io =
            |inner: crate::ast::TypeExpr| crate::ast::TypeExpr::TApp("IO".to_string(), vec![inner]);
        let stream = |name: &str, arg: crate::ast::TypeExpr| {
            crate::ast::TypeExpr::TApp(name.to_string(), vec![arg])
        };
        env.builtin_sigs.insert(
            "fromList".to_string(),
            forall_a(crate::ast::TypeExpr::TFunc(
                vec![crate::ast::TypeExpr::TApp(
                    "List".to_string(),
                    vec![tv("a")],
                )],
                Box::new(io(stream("InputStream", tv("a")))),
            )),
        );
        env.builtin_sigs.insert(
            "toList".to_string(),
            forall_a(crate::ast::TypeExpr::TFunc(
                vec![stream("InputStream", tv("a"))],
                Box::new(io(crate::ast::TypeExpr::TApp(
                    "List".to_string(),
                    vec![tv("a")],
                ))),
            )),
        );
        env.builtin_sigs.insert(
            "write".to_string(),
            forall_a(crate::ast::TypeExpr::TFunc(
                vec![
                    crate::ast::TypeExpr::TApp("Maybe".to_string(), vec![tv("a")]),
                    stream("OutputStream", tv("a")),
                ],
                Box::new(io(tv("()"))),
            )),
        );
        env.builtin_sigs.insert(
            "stdout".to_string(),
            crate::ast::TypeExpr::TFunc(vec![], Box::new(stream("OutputStream", tv("String")))),
        );
        env.builtin_sigs.insert(
            "stderr".to_string(),
            crate::ast::TypeExpr::TFunc(vec![], Box::new(stream("OutputStream", tv("String")))),
        );
        // The console input: line-oriented `String` text (rill has no
        // `ByteString`), realized against the runner's injected console lines.
        env.builtin_sigs.insert(
            "stdin".to_string(),
            crate::ast::TypeExpr::TFunc(vec![], Box::new(stream("InputStream", tv("String")))),
        );
        env.builtin_sigs.insert(
            "read".to_string(),
            forall_a(crate::ast::TypeExpr::TFunc(
                vec![stream("InputStream", tv("a"))],
                Box::new(io(crate::ast::TypeExpr::TApp(
                    "Maybe".to_string(),
                    vec![tv("a")],
                ))),
            )),
        );
        env.builtin_sigs.insert(
            "atEnd".to_string(),
            forall_a(crate::ast::TypeExpr::TFunc(
                vec![stream("InputStream", tv("a"))],
                Box::new(io(tv("Bool"))),
            )),
        );
        env.builtin_sigs.insert(
            "unread".to_string(),
            forall_a(crate::ast::TypeExpr::TFunc(
                vec![tv("a"), stream("InputStream", tv("a"))],
                Box::new(io(tv("()"))),
            )),
        );
        env.builtin_sigs.insert(
            "connect".to_string(),
            forall_a(crate::ast::TypeExpr::TFunc(
                vec![
                    stream("InputStream", tv("a")),
                    stream("OutputStream", tv("a")),
                ],
                Box::new(io(tv("()"))),
            )),
        );
        env.builtin_sigs.insert(
            "makeInputStream".to_string(),
            forall_a(crate::ast::TypeExpr::TFunc(
                vec![io(crate::ast::TypeExpr::TApp(
                    "Maybe".to_string(),
                    vec![tv("a")],
                ))],
                Box::new(io(stream("InputStream", tv("a")))),
            )),
        );
        env.builtin_sigs.insert(
            "makeOutputStream".to_string(),
            forall_a(crate::ast::TypeExpr::TFunc(
                vec![crate::ast::TypeExpr::TFunc(
                    vec![crate::ast::TypeExpr::TApp(
                        "Maybe".to_string(),
                        vec![tv("a")],
                    )],
                    Box::new(io(tv("()"))),
                )],
                Box::new(io(stream("OutputStream", tv("a")))),
            )),
        );
        env.builtin_sigs.insert(
            "show_float".to_string(),
            crate::ast::TypeExpr::TFunc(vec![tv("Float")], Box::new(tv("String"))),
        );
        env.builtin_sigs.insert(
            "show_int".to_string(),
            crate::ast::TypeExpr::TFunc(vec![tv("Int")], Box::new(tv("String"))),
        );
        env.builtin_sigs.insert(
            "show_bool".to_string(),
            crate::ast::TypeExpr::TFunc(vec![tv("Bool")], Box::new(tv("String"))),
        );
        // Immutable array storage primitives: the flat, type-agnostic surface
        // (design §Storage primitives, immutable list). Every INDEX computation
        // lives in the interpreter for the two prelude shapes (`Int`,
        // `Pair Int Int`), resolved against the STORED bounds of the array; the
        // language's `Ix` instances stay for READS (`checkedOffset`, `index`)
        // and for user-defined index types over the read surface. These
        // signatures are deliberately UNCONSTRAINED — no `Ix i =>` context and
        // no range math in the interpreter. They are polymorphic over the index
        // type `i` and the element type `e`, bound with an explicit `TForall` so
        // the ordinary builtin-sig path instantiates one fresh variable per
        // call site (keeping the argument and result types linked).
        //
        //   arrayFromList  :: Pair i i -> List e -> Array i e
        //   arrayBounds    :: Array i e -> Pair i i
        //   arrayElems     :: Array i e -> List e
        //   arrayIndexFlat :: Array i e -> Int -> e
        //   arrayUpdate    :: Array i e -> List (Pair i e) -> Array i e
        //   arrayAccum     :: Array i e -> (e -> e -> e) -> List (Pair i e)
        //                     -> Array i e
        //
        //   arrayFromAssocs :: Pair i i -> List (Pair i e) -> Array i e
        //   arraySeed       :: Pair i i -> e -> Array i e
        //
        // Construction and update associations (`arrayFromAssocs`,
        // `arrayUpdate`, `arrayAccum`) are INDEX-TYPED — the interpreter
        // resolves each one against the bounds shape shared by every arm
        // (`array_shape`/`ArrayShape::offset`), reproducing the prelude `Ix`
        // instances for the two supported shapes. Reads stay language-side:
        // `checkedOffset`/`index` go through `arrayIndexFlat` with a flat
        // offset computed by the `Ix` instances.
        // The accumulator of `arrayAccum` is written CURRIED (`e -> e -> e`
        // right-associates to `e -> (e -> e)`), matching the language's
        // representation of a binary closure as a nest of unary lambdas.
        let forall_ie = |inner: crate::ast::TypeExpr| {
            crate::ast::TypeExpr::TForall(vec!["i".to_string(), "e".to_string()], Box::new(inner))
        };
        let array =
            |args: Vec<crate::ast::TypeExpr>| crate::ast::TypeExpr::TApp("Array".to_string(), args);
        let pair =
            |args: Vec<crate::ast::TypeExpr>| crate::ast::TypeExpr::TApp("Pair".to_string(), args);
        let list =
            |arg: crate::ast::TypeExpr| crate::ast::TypeExpr::TApp("List".to_string(), vec![arg]);
        let arr_ie = || array(vec![tv("i"), tv("e")]);
        // `List (Pair i e)`: the index-typed (cell, value) association list.
        let assocs_ie = || list(pair(vec![tv("i"), tv("e")]));
        env.builtin_sigs.insert(
            "arrayFromList".to_string(),
            forall_ie(crate::ast::TypeExpr::TFunc(
                vec![pair(vec![tv("i"), tv("i")]), list(tv("e"))],
                Box::new(arr_ie()),
            )),
        );
        env.builtin_sigs.insert(
            "arrayBounds".to_string(),
            forall_ie(crate::ast::TypeExpr::TFunc(
                vec![arr_ie()],
                Box::new(pair(vec![tv("i"), tv("i")])),
            )),
        );
        env.builtin_sigs.insert(
            "arrayElems".to_string(),
            forall_ie(crate::ast::TypeExpr::TFunc(
                vec![arr_ie()],
                Box::new(list(tv("e"))),
            )),
        );
        env.builtin_sigs.insert(
            "arrayIndexFlat".to_string(),
            forall_ie(crate::ast::TypeExpr::TFunc(
                vec![arr_ie(), tv("Int")],
                Box::new(tv("e")),
            )),
        );
        env.builtin_sigs.insert(
            "arrayUpdate".to_string(),
            forall_ie(crate::ast::TypeExpr::TFunc(
                vec![arr_ie(), assocs_ie()],
                Box::new(arr_ie()),
            )),
        );
        env.builtin_sigs.insert(
            "arrayAccum".to_string(),
            forall_ie(crate::ast::TypeExpr::TFunc(
                vec![
                    arr_ie(),
                    crate::ast::TypeExpr::TFunc(
                        vec![tv("e")],
                        Box::new(crate::ast::TypeExpr::TFunc(
                            vec![tv("e")],
                            Box::new(tv("e")),
                        )),
                    ),
                    assocs_ie(),
                ],
                Box::new(arr_ie()),
            )),
        );
        // Index-typed construction ops (the M8 shape): `arrayFromAssocs` takes
        // an index-typed `List (Pair i e)` association list whose offsets are
        // resolved against the construction bounds by the interpreter;
        // `arraySeed` fills every position with one value. Both are
        // UNCONSTRAINED like their siblings — the language defs carry the
        // `Ix i =>` context.
        env.builtin_sigs.insert(
            "arrayFromAssocs".to_string(),
            forall_ie(crate::ast::TypeExpr::TFunc(
                vec![
                    pair(vec![tv("i"), tv("i")]),
                    list(pair(vec![tv("i"), tv("e")])),
                ],
                Box::new(arr_ie()),
            )),
        );
        env.builtin_sigs.insert(
            "arraySeed".to_string(),
            forall_ie(crate::ast::TypeExpr::TFunc(
                vec![pair(vec![tv("i"), tv("i")]), tv("e")],
                Box::new(arr_ie()),
            )),
        );
        // Buffer bridge: concrete over the `Int`-indexed `Float` array the
        // `FixedBuffer f32` maps to. The value-level type of a signal buffer is
        // `Buffer` (that is what the FFI apply path produces for a fully applied
        // generator and what a buffer-returning `main` yields), so these are
        // written against `Buffer` rather than the `FixedBuffer f32` spelling
        // used inside `foreign` declarations.
        env.builtin_sigs.insert(
            "arrayFromBuffer".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![tv("Buffer")],
                Box::new(array(vec![tv("Int"), tv("Float")])),
            ),
        );
        env.builtin_sigs.insert(
            "bufferFromArray".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![array(vec![tv("Int"), tv("Float")])],
                Box::new(tv("Buffer")),
            ),
        );
        // The two primitives the language-level array surface needs but the
        // flat storage layer cannot express:
        //
        //   listRange           :: Pair Int Int -> List Int
        //   arrayFromListChecked :: Pair i i -> Int -> List e -> Array i e
        //
        // `listRange` is the only list-GENERATING primitive (strict mode bans
        // recursion and there is no `enumFromTo`), so `Ix.range` builds on it.
        // `arrayFromListChecked` is `arrayFromList` plus the strict length check
        // behind `listArray` (and `newArrayFromListFlat` is its ST twin).
        env.builtin_sigs.insert(
            "listRange".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![pair(vec![tv("Int"), tv("Int")])],
                Box::new(list(tv("Int"))),
            ),
        );
        env.builtin_sigs.insert(
            "arrayFromListChecked".to_string(),
            forall_ie(crate::ast::TypeExpr::TFunc(
                vec![pair(vec![tv("i"), tv("i")]), tv("Int"), list(tv("e"))],
                Box::new(arr_ie()),
            )),
        );

        env
    }

    /// Register declaration defs (`typeclass`/`instance`/`foreign`) into the
    /// env. Extracted from inference phase 1 so the category prelude (parsed in
    /// [`Self::with_builtins`]) and user declarations share one registration
    /// path.
    pub fn register_decls(&mut self, defs: &[Def]) {
        for def in defs {
            match def {
                Def::Typeclass {
                    name,
                    var,
                    methods,
                    defaults,
                    ..
                } => {
                    self.typeclasses.insert(
                        name.clone(),
                        TypeclassInfo {
                            var: var.clone(),
                            arity: methods
                                .iter()
                                .map(|(_, sig)| Self::class_var_arity(var, sig))
                                .max()
                                .unwrap_or(0),
                            methods: methods.clone(),
                            defaults: defaults
                                .iter()
                                .map(|(m, ps, b)| {
                                    (
                                        m.clone(),
                                        (ps.iter().map(|p| p.name.clone()).collect(), b.clone()),
                                    )
                                })
                                .collect(),
                        },
                    );
                }
                Def::Instance {
                    class,
                    head,
                    ty,
                    constraints,
                    head_args,
                    method_bodies,
                    ..
                } => {
                    let mut methods: HashMap<String, (Vec<String>, Expr)> = HashMap::new();
                    for (mname, params, body) in method_bodies {
                        let bindings = params.iter().map(|p| p.name.clone()).collect();
                        methods.insert(mname.clone(), (bindings, body.clone()));
                    }
                    self.instances.entry(class.clone()).or_default().insert(
                        ty.clone(),
                        InstanceInfo {
                            class: class.clone(),
                            head: head.clone(),
                            ty: ty.clone(),
                            constraints: constraints.clone(),
                            head_args: head_args.clone(),
                            methods,
                            // A declaration registered here comes from user
                            // source; `register_prelude` flips the flag for the
                            // prelude fragments it registers.
                            from_prelude: false,
                        },
                    );
                }
                Def::Foreign { name, sig, .. } => {
                    self.foreign_sigs.insert(name.clone(), sig.clone());
                }
                // A class-constrained definition (`mk :: Ix i => …`, also under
                // a `forall` binder): register it for per-call-site
                // specialization. Its body is NOT typed here (nor by the eager
                // body pass) — the class variable is abstract until a call site
                // solves it.
                Def::Eval {
                    name,
                    sig: Some(sig),
                    params,
                    body,
                    where_defs,
                    ..
                } if crate::types::infer::is_constrained_sig(sig) => {
                    // The parser curries a multi-parameter definition into
                    // nested lambdas and clears `params` (`curry_defs_in`), so
                    // the formal parameters of `f a b :: …` live in the body's
                    // leading lambdas. The signature's arity says exactly how
                    // many to recover.
                    let (params, body) = split_curried_params(sig.arg_count(), params, body);
                    self.constrained_defs.insert(
                        name.clone(),
                        ConstrainedDef {
                            sig: sig.clone(),
                            params,
                            body,
                            where_defs: where_defs.clone(),
                        },
                    );
                }
                _ => {}
            }
        }
    }

    /// Superclass auto-derivation: `instance Monad T` synthesizes
    /// `Applicative T` and `Functor T`; `instance Applicative T` synthesizes
    /// `Functor T`. Explicit instances always win. The derived bodies are the
    /// standard definitions, parsed from templates. A partial head
    /// (`instance Monad (ST s)`) carries its bound head args into the derived
    /// instances so the kind check sees the same remaining arity.
    pub(crate) fn derive_superclass_instances(&mut self) {
        let monad_insts = self
            .instances
            .get("Monad")
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or(vec![]);
        let monad_tys: Vec<String> = monad_insts.iter().map(|(t, _)| t.clone()).collect();
        let applicative_tys = self
            .instances
            .get("Applicative")
            .map(|m| m.keys().cloned().collect())
            .unwrap_or(vec![]);
        for t in monad_tys {
            let Some(src_info) = monad_insts
                .iter()
                .find(|(ty, _)| *ty == t)
                .map(|(_, info)| info.clone())
            else {
                continue;
            };
            let head_args = src_info.head_args.clone();
            // A template derived from a prelude instance stays trusted; one
            // derived from a USER instance is validated like any user body.
            let from_prelude = src_info.from_prelude;
            if !self
                .instances
                .entry("Applicative".to_string())
                .or_default()
                .contains_key(&t)
            {
                self.instances
                    .entry("Applicative".to_string())
                    .or_default()
                    .insert(
                        t.clone(),
                        instance_from_template(
                            t.as_str(),
                            "Applicative",
                            APPLICATIVE_FROM_MONAD,
                            &head_args,
                            from_prelude,
                        ),
                    );
            }
            if !self
                .instances
                .entry("Functor".to_string())
                .or_default()
                .contains_key(&t)
            {
                self.instances
                    .entry("Functor".to_string())
                    .or_default()
                    .insert(
                        t.clone(),
                        instance_from_template(
                            t.as_str(),
                            "Functor",
                            FUNCTOR_FROM_MONAD,
                            &head_args,
                            from_prelude,
                        ),
                    );
            }
        }
        let applicative_insts = self
            .instances
            .get("Applicative")
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or(vec![]);
        for t in applicative_tys {
            if !self
                .instances
                .entry("Functor".to_string())
                .or_default()
                .contains_key(&t)
            {
                let Some(src_info) = applicative_insts
                    .iter()
                    .find(|(ty, _)| *ty == t)
                    .map(|(_, info)| info.clone())
                else {
                    continue;
                };
                let head_args = src_info.head_args.clone();
                self.instances
                    .entry("Functor".to_string())
                    .or_default()
                    .insert(
                        t.clone(),
                        instance_from_template(
                            t.as_str(),
                            "Functor",
                            FUNCTOR_FROM_APPLICATIVE,
                            &head_args,
                            src_info.from_prelude,
                        ),
                    );
            }
        }
    }

    /// The kind arity of a class variable as used in a method signature: the
    /// max number of type arguments applied to the variable (`f a` -> 1,
    /// `f a b` -> 2, bare `a` -> 0). Used to typecheck `instance` heads.
    pub(crate) fn class_var_arity(var: &str, sig: &crate::ast::TypeExpr) -> usize {
        fn depth(var: &str, te: &crate::ast::TypeExpr) -> usize {
            match te {
                crate::ast::TypeExpr::TName(n) if n == var => 0,
                crate::ast::TypeExpr::TApp(head, args) if head == var => args.len(),
                crate::ast::TypeExpr::TApp(_, args) => {
                    args.iter().map(|a| depth(var, a)).max().unwrap_or(0)
                }
                crate::ast::TypeExpr::TConstrained(_, inner) => depth(var, inner),
                crate::ast::TypeExpr::TFunc(args, ret) => args
                    .iter()
                    .map(|a| depth(var, a))
                    .chain(std::iter::once(depth(var, ret)))
                    .max()
                    .unwrap_or(0),
                _ => 0,
            }
        }
        depth(var, sig)
    }

    /// The index of the argument in a method signature whose type applies the
    /// class variable (`f a` in `fmap: (a -> b) -> f a -> f b`). The instance
    /// is selected by that argument's concrete type. `None` for arity-0
    /// signatures (bare class var).
    pub(crate) fn class_var_arg_index(
        &self,
        sig: &crate::ast::TypeExpr,
        class_var: &str,
    ) -> Option<usize> {
        match sig {
            crate::ast::TypeExpr::TConstrained(_, inner) => {
                self.class_var_arg_index(inner, class_var)
            }
            crate::ast::TypeExpr::TFunc(args, _) => args
                .iter()
                .position(|a| matches!(a, crate::ast::TypeExpr::TApp(h, _) if h == class_var)),
            _ => None,
        }
    }

    /// Solve an arity-0 class variable from a method call's selector argument:
    /// unify the method's class-var parameter pattern (`Pair i i`) against the
    /// selector's concrete value type (`Pair Int Int` → `i := Int`).
    ///
    /// This is what makes `Ix Int` and `Ix (Pair Int Int)` dispatch distinctly:
    /// the selector's head constructor (`Pair`) cannot tell them apart, but the
    /// SOLVED class variable's full type can (render it with
    /// [`value_type_key`]). `class_var_var` must be a fresh id the caller's
    /// `fresh` supply will not reissue.
    pub(crate) fn class_var_solved_ty(
        &self,
        sig: &crate::ast::TypeExpr,
        class_var: &str,
        selector_ty: &ValueTy,
        class_var_var: TypeVarId,
        fresh: impl FnMut() -> ValueTy,
    ) -> Option<ValueTy> {
        let idx = class_var_param_pattern(sig, class_var)?;
        let sig = match sig {
            crate::ast::TypeExpr::TConstrained(_, inner)
            | crate::ast::TypeExpr::TForall(_, inner) => inner.as_ref(),
            other => other,
        };
        let arg = match sig {
            crate::ast::TypeExpr::TFunc(args, _) => args.get(idx)?,
            _ => return None,
        };
        let pattern =
            crate::types::infer::class_var_pattern_vty(self, class_var, class_var_var, arg, fresh);
        let mut subst = Subst::default();
        // The value side spells a concrete constructor application `App`
        // (builtin ctor) or `Data`/`Newtype` (declared shape), while an instance
        // head's pattern is always an application: an `App`-vs-`Data` pair with
        // the same head is the same type to the registry, but the unifier
        // compares variants strictly. Normalize both sides before unifying.
        unify_value(
            &normalize_ctor_app(&pattern),
            &normalize_ctor_app(selector_ty),
            &mut subst,
            Span::new(0, 0),
        )
        .ok()?;
        let solved = subst.resolve_value(&ValueTy::Var(class_var_var));
        // Still an unresolved variable: no concrete class variable.
        match solved {
            ValueTy::Var(_) | ValueTy::TyConVar(_) | ValueTy::TyConApp(..) => None,
            other => Some(other),
        }
    }

    /// Register a derived (structural) `Eq`/`Ord` instance for every concrete
    /// data type currently in the env — user data types, the builtin shapes
    /// (`Maybe`/`Pair`/`Either`) and the builtin ctor kinds (`List`/`Set`/`Map`,
    /// whose structural order the interpreter's `value_cmp` implements) — plus
    /// newtypes (derived by their inner, spec §2.5; `value_cmp` unwraps them)
    /// and the scalar leaves. `Func` types get no instance. Derived instances
    /// are markers: their method bodies are not run — the interpreter's
    /// `value_cmp` implements the order.
    pub fn derive_eq_ord(&mut self) {
        let mut names: Vec<String> = self.data_types.keys().cloned().collect();
        names.extend(self.ctor_kinds.keys().cloned());
        names.extend(self.newtypes.keys().cloned());
        names.extend(
            ["Int", "Float", "Bool", "String"]
                .iter()
                .map(|s| s.to_string()),
        );
        for class in ["Eq", "Ord"] {
            let by_ty = self.instances.entry(class.to_string()).or_default();
            for n in &names {
                by_ty.entry(n.clone()).or_insert_with(|| InstanceInfo {
                    class: class.to_string(),
                    head: crate::ast::TypeExpr::TName(n.clone()),
                    ty: n.clone(),
                    constraints: vec![], // derived marker — no constraint list
                    head_args: vec![],   // derived marker — full application
                    methods: HashMap::new(), // derived marker — empty body
                    // Compiler-derived structural marker: no body to validate.
                    from_prelude: true,
                });
            }
        }
    }

    /// Value arity of a builtin constructor (`None` if not a builtin).
    pub fn ctor_arity(&self, name: &str) -> Option<usize> {
        self.ctor_kinds.get(name).copied()
    }
    /// The number of VALUE arguments a constructor takes. Builtin constructors
    /// come from [`Self::ctor_kinds`]; user parameterized data types
    /// (`data Box a`) from [`Self::data_arities`].
    pub fn ctor_value_arity(&self, name: &str) -> Option<usize> {
        if let Some(a) = self.ctor_kinds.get(name) {
            return Some(*a);
        }
        self.data_arities.get(name).copied()
    }

    /// Arity remaining after the instance head's bound args: `Kleisli m` (total 3,
    /// bound 1) → 2. For a non-partial head, `total`.
    ///
    /// Wired up by SP-2 Task 6 (constraint-instance resolution); no callers
    /// yet, so the transient `dead_code` is allowed here rather than suppressed
    /// globally.
    #[allow(dead_code)]
    pub(crate) fn instance_head_arity(&self, ty: &str, head_args: &[String]) -> Option<usize> {
        let total = self.ctor_value_arity(ty)?;
        Some(total.saturating_sub(head_args.len()))
    }

    /// Resolve a DSL type name to a value type, following type synonyms and
    /// newtype wrappers. Alias chains resolve iteratively with a bounded loop
    /// (cycle-safe): each pass follows one link and there are at most
    /// `len(aliases)` distinct links to follow.
    pub fn vty_of_name(&self, name: &str) -> ValueTy {
        let mut cur = name.to_string();
        for _ in 0..=self.type_aliases.len() {
            match self.type_aliases.get(&cur) {
                Some(target) => cur = target.clone(),
                None => break,
            }
        }
        match cur.as_str() {
            "Float" => ValueTy::Float,
            "Int" => ValueTy::Int,
            "Bool" => ValueTy::Bool,
            "String" => ValueTy::String,
            "Buffer" => ValueTy::Buffer,
            "()" => ValueTy::Void,
            n => {
                if self.newtypes.contains_key(n) {
                    ValueTy::Newtype(n.to_string(), vec![])
                } else {
                    ValueTy::Data(n.to_string(), vec![])
                }
            }
        }
    }

    /// Whether `method` is declared with zero arguments (e.g. `mempty: m`).
    pub(crate) fn is_nullary_method(&self, class: &str, method: &str) -> bool {
        self.typeclasses
            .get(class)
            .and_then(|c| c.methods.iter().find(|(m, _)| m == method))
            .map(|(_, s)| s.arg_count() == 0)
            .unwrap_or(false)
    }

    /// Find the class whose method dictionary declares `method`. Returns
    /// `None` when no class declares it (the name may be a definition or
    /// builtin) and when several classes declare the same method name
    /// (ambiguous — the caller reports it as unresolvable).
    pub fn class_of_method(&self, method: &str) -> Option<String> {
        let mut found: Option<String> = None;
        for (cname, info) in &self.typeclasses {
            if info.methods.iter().any(|(m, _)| m == method) {
                if found.is_some() {
                    return None;
                }
                found = Some(cname.clone());
            }
        }
        found
    }

    /// The DSL type name of a concrete value type, used to look up instances
    /// (`Float`, `Int`, a data type name, a newtype name, a builtin ctor
    /// application's head like `List`/`Map`). `None` for unresolved type
    /// variables and function types — a method call or Ord-constrained key
    /// over such an argument cannot select an instance at compile time.
    pub fn type_name_of_vty(&self, v: &ValueTy) -> Option<String> {
        match v {
            ValueTy::Int => Some("Int".to_string()),
            ValueTy::Float => Some("Float".to_string()),
            ValueTy::Bool => Some("Bool".to_string()),
            ValueTy::String => Some("String".to_string()),
            ValueTy::Data(n, _) => Some(n.clone()),
            ValueTy::Newtype(n, _) => Some(n.clone()),
            ValueTy::App(n, _) => Some(n.clone()),
            _ => None,
        }
    }

    /// Match a class-var signature pattern against a concrete value type.
    /// `f a` (pattern head is the class var) matches `App("List", [Int])`
    /// by binding the class var to the constructor and unifying the remaining
    /// pattern args with the concrete's args positionally. Open collections
    /// carry no capacity slot. Returns the bound constructor name on success.
    pub fn match_ctor_pattern(
        &self,
        class_var: &str,
        pat: &ValueTy,
        concrete: &ValueTy,
        subst: &mut Subst,
    ) -> Option<String> {
        if let ValueTy::App(f, p_args) = pat {
            if f == class_var {
                // The concrete side is either a builtin constructor
                // (`App("List", [..])` / `App("Maybe", [t])`) or a user data
                // type (`Data("Box", [..])`).
                let (c, c_args) = match concrete {
                    ValueTy::App(c, a) | ValueTy::Data(c, a) => (c, a),
                    _ => return None,
                };
                // Match the pattern args positionally against the concrete's.
                if p_args.len() != c_args.len() {
                    return None;
                }
                for (pa, ca) in p_args.iter().zip(c_args.iter()) {
                    if unify_value(pa, ca, subst, Span::new(0, 0)).is_err() {
                        return None;
                    }
                }
                return Some(c.clone());
            }
        }
        None
    }

    /// Validate that all data types and newtypes are acyclic. A data type that
    /// (transitively) references itself through a field, payload, or newtype
    /// inner is rejected: v1 guarantees acyclicity at compile time so the
    /// arena-capacity bound (`subtree_size`) is exact and RC is sound
    /// (spec §9.1, the `strict` contract).
    pub fn check_acyclic(&self) -> Result<(), CompileError> {
        let mut visiting = HashSet::new();
        for name in self.data_types.keys() {
            self.check_acyclic_name(name, &mut visiting)?;
        }
        visiting.clear();
        for name in self.newtypes.keys() {
            self.check_acyclic_newtype(name, &mut visiting)?;
        }
        Ok(())
    }

    fn check_acyclic_name(
        &self,
        name: &str,
        visiting: &mut HashSet<String>,
    ) -> Result<(), CompileError> {
        if !visiting.insert(name.to_string()) {
            return Err(CompileError::Type {
                msg: format!(
                    "recursive data type `{name}` is not supported in v1 (acyclicity is required)"
                ),
                span: Span::new(0, 0),
            });
        }
        match self.data_types.get(name) {
            Some(DataInfo::Record(fields)) => {
                for (_, t, _) in fields {
                    match t {
                        ValueTy::Data(inner, _) => self.check_acyclic_name(inner, visiting)?,
                        ValueTy::Newtype(inner, _) => {
                            self.check_acyclic_newtype(inner, visiting)?
                        }
                        _ => {}
                    }
                }
            }
            Some(DataInfo::Sum(ctors)) => {
                for (_, payload) in ctors {
                    for (t, _) in payload {
                        match t {
                            ValueTy::Data(inner, _) => self.check_acyclic_name(inner, visiting)?,
                            ValueTy::Newtype(inner, _) => {
                                self.check_acyclic_newtype(inner, visiting)?
                            }
                            _ => {}
                        }
                    }
                }
            }
            Some(DataInfo::Opaque) | None => {}
        }
        visiting.remove(name);
        Ok(())
    }

    fn check_acyclic_newtype(
        &self,
        name: &str,
        visiting: &mut HashSet<String>,
    ) -> Result<(), CompileError> {
        if !visiting.insert(name.to_string()) {
            return Err(CompileError::Type {
                msg: format!(
                    "recursive newtype `{name}` is not supported in v1 (acyclicity is required)"
                ),
                span: Span::new(0, 0),
            });
        }
        let res = match self.newtypes.get(name) {
            Some(inner) => match self.vty_of_name(inner) {
                ValueTy::Data(data_name, _) => self.check_acyclic_name(&data_name, visiting),
                ValueTy::Newtype(nw_name, _) => self.check_acyclic_newtype(&nw_name, visiting),
                _ => Ok(()),
            },
            None => Ok(()),
        };
        visiting.remove(name);
        res
    }

    /// Resolve a typeclass method call: the class declaring `method`, the
    /// concrete type name `ty_name`, and the matching instance's method body
    /// `(parameter bindings, body)`. Returns `None` when no class declares
    /// `method` or no instance binds `ty_name`.
    pub fn resolve_method(
        &self,
        method: &str,
        ty_name: &str,
    ) -> Option<(String, Vec<String>, Expr)> {
        if let Some(cname) = self.class_of_method(method) {
            if let Some(instance) = self
                .instances
                .get(cname.as_str())
                .and_then(|by_ty| by_ty.get(ty_name))
            {
                if let Some((params, body)) = instance.methods.get(method).cloned() {
                    return Some((cname, params, body));
                }
            }
        }
        None
    }

    /// The default body for `method` of `class`, if the class declares one.
    /// Precedence: instance body > default > error.
    pub(crate) fn class_default(&self, class: &str, method: &str) -> Option<(Vec<String>, Expr)> {
        self.typeclasses
            .get(class)
            .and_then(|c| c.defaults.get(method).cloned())
    }
}

/// Every argument index in a method signature whose type MENTIONS the class
/// variable (`[0, 1]` for `mappend: m -> m -> m`, `[1]` for
/// `pick: Bool -> Pair a a -> Float`). Empty when no argument mentions it (a
/// nullary method such as `mempty: m`).
///
/// Callers select the dispatch selector from these indices: the class variable
/// must be solved from an argument whose signature type constrains it, not just
/// from whichever argument happens to come first.
pub(crate) fn class_var_param_indices(sig: &crate::ast::TypeExpr, class_var: &str) -> Vec<usize> {
    match sig {
        crate::ast::TypeExpr::TConstrained(_, inner) | crate::ast::TypeExpr::TForall(_, inner) => {
            class_var_param_indices(inner, class_var)
        }
        crate::ast::TypeExpr::TFunc(args, _) => args
            .iter()
            .enumerate()
            .filter(|(_, a)| crate::types::infer::type_expr_mentions(a, class_var))
            .map(|(i, _)| i)
            .collect(),
        _ => vec![],
    }
}

/// The index of the first argument in a method signature whose type MENTIONS
/// the class variable (`Pair i i` in `index: Pair i i -> i -> Int`), i.e. the
/// argument whose pattern is unified against the call site's concrete selector
/// type to solve the class variable. `None` when no argument mentions it (a
/// nullary method such as `mempty: m`).
pub(crate) fn class_var_param_pattern(
    sig: &crate::ast::TypeExpr,
    class_var: &str,
) -> Option<usize> {
    class_var_param_indices(sig, class_var).into_iter().next()
}

/// Normalize a value type for instance-head matching: every concrete
/// constructor application becomes an `App` (`Data`/`Newtype` → `App`), with the
/// arguments normalized too. The value side distinguishes a builtin ctor
/// application (`App`) from a declared shape (`Data`/`Newtype`), but the
/// registry treats `Pair Int Int` as one type; [`unify_value`] compares variants
/// strictly, so both sides of an instance-head match are normalized first.
fn normalize_ctor_app(v: &ValueTy) -> ValueTy {
    match v {
        ValueTy::Data(n, args) | ValueTy::Newtype(n, args) | ValueTy::App(n, args) => {
            ValueTy::App(n.clone(), args.iter().map(normalize_ctor_app).collect())
        }
        ValueTy::TyConApp(f, args) => {
            ValueTy::TyConApp(*f, args.iter().map(normalize_ctor_app).collect())
        }
        ValueTy::Thunk(args, rets) => ValueTy::Thunk(
            args.iter().map(normalize_ctor_app).collect(),
            rets.iter().map(normalize_ctor_app).collect(),
        ),
        other => other.clone(),
    }
}

/// Canonical registry-key spelling of a concrete value type: `Int`,
/// `Pair Int Int`, `Pair Int (Pair Int Int)`. A nested application is
/// parenthesized so the spelling matches the parser's instance-head rendering
/// ([`crate::render::render_type_key`]).
///
/// `None` when the type is not concrete (an unresolved variable, a
/// type-constructor variable, a function type): such a selector cannot name an
/// instance, and the caller falls back to the head-constructor lookup.
pub(crate) fn value_type_key(v: &ValueTy) -> Option<String> {
    fn key(v: &ValueTy) -> Option<String> {
        Some(match v {
            ValueTy::Int => "Int".to_string(),
            ValueTy::Float => "Float".to_string(),
            ValueTy::Bool => "Bool".to_string(),
            ValueTy::String => "String".to_string(),
            ValueTy::Buffer => "Buffer".to_string(),
            ValueTy::Void => "()".to_string(),
            ValueTy::Data(n, args) | ValueTy::Newtype(n, args) | ValueTy::App(n, args) => {
                if args.is_empty() {
                    n.clone()
                } else {
                    let mut out = n.clone();
                    for a in args {
                        out.push(' ');
                        let nested = matches!(
                            a,
                            ValueTy::Data(_, inner)
                                | ValueTy::Newtype(_, inner)
                                | ValueTy::App(_, inner)
                                if !inner.is_empty()
                        );
                        if nested {
                            out.push('(');
                        }
                        out.push_str(&key(a)?);
                        if nested {
                            out.push(')');
                        }
                    }
                    out
                }
            }
            _ => return None,
        })
    }
    key(v)
}

impl Subst {
    /// Follow the substitution chain for a single value type to its
    /// representative, resolving variables inside `Func` signatures too.
    ///
    /// Bounded by [`MAX_VALUE_RESOLVE_DEPTH`]: a (lowering-bug or
    /// already-rejected) cyclic binding degrades to the unresolved var rather
    /// than overflowing the stack. The occurs-check in `unify_value` rejects
    /// cycles at the source, so this is pure defense-in-depth.
    pub fn resolve_value(&self, t: &ValueTy) -> ValueTy {
        self.resolve_value_depth(t, 0)
    }

    /// Depth-bounded core of [`Self::resolve_value`].
    fn resolve_value_depth(&self, t: &ValueTy, depth: usize) -> ValueTy {
        if depth > MAX_VALUE_RESOLVE_DEPTH {
            return t.clone();
        }
        match t {
            ValueTy::Var(v) => match self.value_map.get(v) {
                Some(inner) => self.resolve_value_depth(inner, depth + 1),
                None => t.clone(),
            },
            ValueTy::TyConVar(v) => match self.value_map.get(v) {
                Some(inner) => self.resolve_value_depth(inner, depth + 1),
                None => t.clone(),
            },
            ValueTy::Thunk(args, rets) => ValueTy::Thunk(
                args.iter()
                    .map(|a| self.resolve_value_depth(a, depth + 1))
                    .collect(),
                rets.iter()
                    .map(|r| self.resolve_value_depth(r, depth + 1))
                    .collect(),
            ),
            ValueTy::TyConApp(f, args) => {
                // Resolve the head variable; a bound head (a concrete
                // constructor) rewrites the application to `App(c, args)`.
                let resolved_args: Vec<ValueTy> = args
                    .iter()
                    .map(|a| self.resolve_value_depth(a, depth + 1))
                    .collect();
                let resolved = self.resolve_value_depth(&ValueTy::TyConVar(*f), depth + 1);
                match resolved {
                    // Follow the head var's union-find chain to its
                    // representative. Without this, `TyConApp(10, ..)` with
                    // `10 -> Var(7)` keeps the STALE id 10: `instantiate`
                    // freshens the generalized var 7 but leaves 10 untouched,
                    // severing a monad's parameter↔result link
                    // (`Kleisli m a b -> a -> m b` becomes `Kleisli m' a b ->
                    // a -> m b`), so a result-directed chain never learns its
                    // container.
                    ValueTy::TyConVar(w) => ValueTy::TyConApp(w, resolved_args),
                    ValueTy::Var(w) => ValueTy::TyConApp(w, resolved_args),
                    ValueTy::App(c, _) => ValueTy::App(c, resolved_args),
                    _other => ValueTy::TyConApp(*f, resolved_args),
                }
            }
            ValueTy::Data(name, args) | ValueTy::Newtype(name, args) | ValueTy::App(name, args) => {
                let resolved: Vec<ValueTy> = args
                    .iter()
                    .map(|a| self.resolve_value_depth(a, depth + 1))
                    .collect();
                match t {
                    ValueTy::Data(..) => ValueTy::Data(name.clone(), resolved),
                    ValueTy::Newtype(..) => ValueTy::Newtype(name.clone(), resolved),
                    _ => ValueTy::App(name.clone(), resolved),
                }
            }
            _ => t.clone(),
        }
    }
    /// Apply the substitution across a whole type.
    ///
    /// Every channel is a value type (lambda-parameter variables become the
    /// concrete types bound by call-site unification).
    pub fn apply(&self, t: &ArrowTy) -> ArrowTy {
        Ty {
            vty: self.resolve_value(&t.vty),
        }
    }
}

#[cfg(test)]
mod funcsig_tests {
    use super::*;

    #[test]
    fn func_signature_carries_arg_and_result_types() {
        let f = ValueTy::Thunk(vec![ValueTy::Float, ValueTy::Float], vec![ValueTy::Float]);
        assert_eq!(
            f,
            ValueTy::Thunk(vec![ValueTy::Float, ValueTy::Float], vec![ValueTy::Float])
        );
        let g = ValueTy::Thunk(vec![ValueTy::Float], vec![ValueTy::Float]);
        assert_ne!(f, g);
    }
}

#[cfg(test)]
mod builtin_array_sig_tests {
    use super::*;

    /// Navigate a builtin sig entry `forall i e. TFunc(args, ret)` down to its
    /// argument list, panicking on any other shape.
    fn tfunc_args(sig: &crate::ast::TypeExpr) -> &[crate::ast::TypeExpr] {
        match sig {
            crate::ast::TypeExpr::TForall(_, inner) => match inner.as_ref() {
                crate::ast::TypeExpr::TFunc(args, _) => args.as_slice(),
                other => panic!("expected a TFunc under the forall, got {other:?}"),
            },
            other => panic!("expected a TForall sig, got {other:?}"),
        }
    }

    #[test]
    fn array_construction_ops_register_index_typed_signatures() {
        let env = TypeEnv::with_builtins();
        for name in ["arrayFromAssocs", "arraySeed"] {
            let sig = env
                .builtin_sigs
                .get(name)
                .unwrap_or_else(|| panic!("{name} must be in the builtin sig table"));
            let args = tfunc_args(sig);
            assert_eq!(args.len(), 2, "{name} takes (bounds, init/assocs)");
            // Arg 0 is the bounds pair `Pair i i`.
            assert!(
                matches!(&args[0], crate::ast::TypeExpr::TApp(n, inner)
                    if n == "Pair" && inner.len() == 2),
                "{name}: first argument must be the `Pair i i` bounds"
            );
        }
        // arrayFromAssocs's second argument is the index-typed assoc list.
        let sig = env
            .builtin_sigs
            .get("arrayFromAssocs")
            .expect("arrayFromAssocs sig");
        let args = tfunc_args(sig);
        assert!(
            matches!(
                &args[1],
                crate::ast::TypeExpr::TApp(n, inner)
                    if n == "List" && inner.len() == 1
                        && matches!(&inner[0], crate::ast::TypeExpr::TApp(p, el)
                            if p == "Pair" && el.len() == 2)
            ),
            "arrayFromAssocs: second argument must be `List (Pair i e)`, got {:?}",
            args[1]
        );
    }

    #[test]
    fn array_update_accum_signatures_are_index_typed() {
        // The M8 flip: `arrayUpdate`/`arrayAccum` now take INDEX-typed
        // associations (`List (Pair i e)`), resolved against the source array's
        // stored bounds by the interpreter — not flat `(Int offset, value)`.
        let env = TypeEnv::with_builtins();
        for name in ["arrayUpdate", "arrayAccum"] {
            let sig = env
                .builtin_sigs
                .get(name)
                .unwrap_or_else(|| panic!("{name} must be in the builtin sig table"));
            let args = tfunc_args(sig);
            let assoc_idx = if name == "arrayUpdate" { 1 } else { 2 };
            assert!(
                matches!(
                    &args[assoc_idx],
                    crate::ast::TypeExpr::TApp(n, inner)
                        if n == "List" && inner.len() == 1
                            && matches!(&inner[0], crate::ast::TypeExpr::TApp(p, el)
                                if p == "Pair" && el.len() == 2
                                    && !matches!(&el[0], crate::ast::TypeExpr::TName(t)
                                        if t == "Int"))
                ),
                "{name}: association list must be index-typed `List (Pair i e)` (not flat `List (Pair Int e)`), got {:?}",
                args[assoc_idx]
            );
        }
    }
}

#[cfg(test)]
mod hkt_value_ty_tests {
    use super::*;

    #[test]
    fn app_constructs() {
        let t = ValueTy::App("List".into(), vec![ValueTy::Float]);
        assert!(matches!(t, ValueTy::App(..)));
    }

    #[test]
    fn bool_string_are_leaves() {
        assert_ne!(ValueTy::Bool, ValueTy::Float);
        assert_ne!(ValueTy::String, ValueTy::Bool);
    }

    #[test]
    fn vty_of_name_resolves_primitive_bool_string() {
        // Regression: `vty_of_name` must resolve `Bool`/`String` to the
        // primitive value types, not to `Data("Bool"/"String", [])`. Before
        // the fix, a `data` field of type `String`
        // became `Data("String", [])`, which failed to unify with string
        // literals (`ValueTy::String`).
        let env = TypeEnv::with_builtins();
        assert_eq!(env.vty_of_name("Bool"), ValueTy::Bool);
        assert_eq!(env.vty_of_name("String"), ValueTy::String);
        assert_eq!(env.vty_of_name("Float"), ValueTy::Float);
        assert_eq!(env.vty_of_name("Int"), ValueTy::Int);
    }

    #[test]
    fn match_ctor_pattern_rejects_extra_pattern_args() {
        let env = TypeEnv::with_builtins();
        let mut subst = Subst::default();
        // Pattern `f a b` (two type args) against `App("List", [Float])`
        // (one slot): the extra `b` slot must reject the match.
        let pat = ValueTy::App("f".into(), vec![ValueTy::Var(0), ValueTy::Var(1)]);
        let concrete = ValueTy::App("List".into(), vec![ValueTy::Float]);
        assert_eq!(
            env.match_ctor_pattern("f", &pat, &concrete, &mut subst),
            None
        );
    }

    #[test]
    fn match_ctor_pattern_matches_consumed_pattern_args() {
        let env = TypeEnv::with_builtins();
        let mut subst = Subst::default();
        // `f a` (one arg) against a List (one slot) matches.
        let pat = ValueTy::App("f".into(), vec![ValueTy::Var(0)]);
        let concrete = ValueTy::App("List".into(), vec![ValueTy::Float]);
        assert_eq!(
            env.match_ctor_pattern("f", &pat, &concrete, &mut subst),
            Some("List".to_string())
        );
        // `f a b` (two args) against a Pair (two slots) matches.
        let pat2 = ValueTy::App("f".into(), vec![ValueTy::Var(1), ValueTy::Var(2)]);
        let concrete2 = ValueTy::App("Pair".into(), vec![ValueTy::Float, ValueTy::Int]);
        assert_eq!(
            env.match_ctor_pattern("f", &pat2, &concrete2, &mut subst),
            Some("Pair".to_string())
        );
    }
}

#[cfg(test)]
mod channel_tests {
    use super::*;

    #[test]
    fn buffer_is_an_ordinary_value_type() {
        // Linear lambda model: no Channel/Rate — a signal is just ValueTy::Buffer.
        let sig = ValueTy::Buffer;
        let val = ValueTy::Data("Point".into(), vec![]);
        assert_eq!(sig, ValueTy::Buffer);
        assert_ne!(sig, val);
    }

    #[test]
    fn apply_preserves_value_types() {
        let s = Subst::default();
        let t = ArrowTy::value(ValueTy::Int);
        let r = s.apply(&t);
        assert_eq!(r.vty, ValueTy::Int);
    }
}

#[cfg(test)]
mod ctor_table_tests {
    use super::*;

    #[test]
    fn builtin_ctor_kinds_and_arities() {
        let env = TypeEnv::with_builtins();
        assert!(env.ctor_arity("List") == Some(1)); // elem
        assert!(env.ctor_arity("Maybe") == Some(1));
        assert!(env.ctor_arity("Set") == Some(1));
        assert!(env.ctor_arity("Map") == Some(2));
        assert!(env.ctor_arity("Pair") == Some(2));
        assert!(env.ctor_arity("Either") == Some(2));
        assert!(env.ctor_arity("Array") == Some(2)); // i e
        assert!(env.ctor_arity("STArray") == Some(3)); // s i e
        assert!(env.ctor_arity("Nope").is_none());
        // `STArray`'s arity lives in BOTH tables: `ctor_kinds` above AND the
        // abstract prelude declaration (`data STArray s i e;` registers
        // `data_arities`). Pin the declarations to each other so they cannot
        // drift apart.
        assert_eq!(env.data_arities.get("STArray"), Some(&3));
    }

    #[test]
    fn builtin_shapes_are_acyclic() {
        // The injected Maybe/Pair/Either shapes must satisfy the v1 acyclicity
        // contract (the arena-capacity bound depends on it).
        TypeEnv::with_builtins().check_acyclic().unwrap();
    }

    #[test]
    fn instance_head_arity_after_bound_args() {
        let env = TypeEnv::with_builtins();
        // `(Pair a)` — total 2, bound 1 ⇒ 1 remaining.
        assert_eq!(env.instance_head_arity("Pair", &["a".to_string()]), Some(1));
        // `(List a)` — total 1, bound 1 ⇒ fully applied.
        assert_eq!(env.instance_head_arity("List", &["a".to_string()]), Some(0));
        // A non-partial head: total arity unchanged.
        assert_eq!(env.instance_head_arity("Pair", &[]), Some(2));
        // Unknown constructor: None.
        assert_eq!(env.instance_head_arity("Nope", &[]), None);
    }
}

#[cfg(test)]
mod eq_ord_tests {
    use super::*;

    #[test]
    fn builtin_eq_ord_registered_and_derived() {
        let mut env = TypeEnv::with_builtins();
        env.data_types.insert(
            "Point".to_string(),
            DataInfo::Record(vec![("x".to_string(), ValueTy::Float, false)]),
        );
        env.data_types.insert(
            "List".to_string(),
            DataInfo::Sum(vec![
                (
                    "Cons".to_string(),
                    vec![(ValueTy::Var(1), false), (ValueTy::Var(2), false)],
                ),
                ("Nil".to_string(), vec![]),
            ]),
        );
        env.derive_eq_ord();
        assert!(env.typeclasses.contains_key("Eq"));
        assert!(env.typeclasses.contains_key("Ord"));
        let by_ty = &env.instances["Ord"];
        assert!(by_ty.contains_key("Float"));
        assert!(by_ty.contains_key("Int"));
        assert!(by_ty.contains_key("Bool"));
        assert!(by_ty.contains_key("String"));
        assert!(by_ty.contains_key("Point"));
        assert!(by_ty.contains_key("List"));
        assert!(!by_ty.contains_key("Thunk"), "Thunk has no derived Ord");
    }
}
