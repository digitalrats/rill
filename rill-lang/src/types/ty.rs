//! Arrow type model: a program is a transform over value types (linear lambda
//! model — all data is equal; there is no signal/value split).
//!
//! An expression's type is a single [`ValueTy`] wrapped in [`Ty`]; channel
//! arity is gone (`ins`/`outs` vectors were removed with the block track). A
//! signal is an ordinary value type ([`ValueTy::Buffer`], carrying its sample
//! element), executed as
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
/// (linear lambda model); a signal buffer is the ordinary `ValueTy::Buffer`,
/// which carries its element (`FixedBuffer elem`).
#[derive(Debug, Clone, PartialEq)]
pub enum ValueTy {
    /// Integer value.
    Int,
    /// Floating point value.
    Double,
    /// Boolean value type.
    Bool,
    /// String value type.
    String,
    /// A signal buffer over an element type (a block of `elem` samples):
    /// `FixedBuffer elem`. An ordinary value type, like `List`/`Map`; executed
    /// as `Value::Buffer` on the value track.
    ///
    /// The payload is the buffer's SAMPLE ELEMENT, and it is checked: two
    /// buffers unify exactly when their elements unify, and the ONE legal
    /// element today is the registry's buffer element (`Double` — see
    /// [`TypeEnv::is_buffer_element`]). This is the only representation of a
    /// buffer type; `App("FixedBuffer", [elem])` is not a second shape.
    Buffer(Box<ValueTy>),
    /// The unit type `()`. The only inhabitant is `Value::Void` (allocated by
    /// the value track's `ValueConstVoid`); used for `IO ()` actions.
    Void,
    /// A named data record, parameterized by its type arguments.
    Data(String, Vec<ValueTy>),
    /// A newtype wrapping another value type.
    Newtype(String, Vec<ValueTy>),
    /// Builtin constructor application: `List Double`.
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
    /// The type of a DICTIONARY parameter: the runtime witness a constrained
    /// function takes for one of its class constraints (`f :: Num a => a -> a`
    /// carries one `Dict` for `Num a`).
    ///
    /// The payload is the CLASS NAME. That is the dictionary's identity for
    /// unification, and it lives in the COMPILER only: the runtime dictionary
    /// is a positional vtable with no names (design §3.1), so the class name is
    /// never looked up or compared at run time — it is the static evidence a
    /// `Dict` type exists to make explicit. Two `Dict`s unify exactly when
    /// their class names are equal; a `Dict` never unifies with a data type.
    Dict(String),
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

/// A polymorphic type scheme `∀ vars. C vars => ty`.
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
    /// The class context the scheme REQUIRES: `(class, its type variable)`
    /// pairs, e.g. `Ix i => …` is `[("Ix", i)]`.
    ///
    /// This is the System FC constraint the caller must discharge with a
    /// dictionary. The variable is the id the constraint's variable has in
    /// [`Self::ty`] (and therefore in [`Self::vars`]), so the context and the
    /// type always name the same variable. Only a constraint whose variable the
    /// signature actually mentions is recorded: a context over a declared type
    /// (`Ix Int => …`) is already discharged, and an AMBIGUOUS context
    /// (`C i => Double`, where `i` occurs in no argument or result) has no
    /// variable to pair a dictionary with. Empty for an unconstrained scheme.
    pub constraints: Vec<(String, TypeVarId)>,
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

/// A predefined (compiler-known) scalar type: the value type its NAME denotes,
/// and whether the FFI layer may name it as a compile-time scalar parameter.
///
/// `Double`, `Int`, `Bool` and `String` have a host scalar representation. A
/// buffer (`Buffer`) is an ordinary value that an FFI channel is made of — the
/// channel type is `FixedBuffer`, not a scalar — and the unit type `()` has no
/// host representation at all, so both are registered with
/// `ffi_scalar: false`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScalarType {
    /// The value type the name denotes.
    pub vty: ValueTy,
    /// Whether a `foreign` signature may name this type as a SCALAR parameter.
    pub ffi_scalar: bool,
    /// Whether this scalar may be the SAMPLE ELEMENT of a buffer
    /// (`FixedBuffer elem` / `Tape elem`).
    ///
    /// One property of the registry entry, not a list in the checker: a buffer
    /// element is exactly a registered scalar marked here, and marking a
    /// second scalar is a one-line change that needs no other edit. `Double` is
    /// the only marked element today because it is the only element a program
    /// can name or observe (a buffer's samples enter the language as
    /// `Value::Double`).
    pub buffer_element: bool,
}

/// The registry's answer for one type-level NAME: what, if anything, that name
/// denotes.
///
/// One table answers every question about a type name ([`TypeEnv::type_entry`]),
/// so the declaration check, the resolver, the derived-instance seed, the FFI
/// classification, and the diagnostic renderer can never disagree about which
/// names are types.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeEntry {
    /// A predefined scalar/primitive value type (`Int`, `Double`, `Bool`,
    /// `String`, `Buffer`, `()`).
    Scalar(ScalarType),
    /// A type constructor with its type-argument arity: a builtin container
    /// (`List` → 1), a builtin data shape (`Maybe` → 1), or a program
    /// `data`/abstract declaration (`data Box a` → 1, a monomorphic
    /// `data Point` → 0).
    TypeCtor {
        /// Number of type arguments the constructor takes.
        arity: usize,
    },
    /// A `newtype` wrapper.
    Newtype,
    /// A `type` synonym (`type Angles = Double`) and the name it stands for.
    Alias(String),
    /// A type variable: a lowercase name (Haskell 2010 §3.1), which is the
    /// binder / `forall` / class-variable space. Known, but NOT a type — an
    /// unknown type does not exist here, it is a variable.
    Variable,
}

impl TypeEntry {
    /// Whether this entry denotes a TYPE (`false` only for a type variable).
    pub fn is_type(&self) -> bool {
        !matches!(self, TypeEntry::Variable)
    }
}

/// The kind of a type-level name or expression: how many type arguments it
/// still expects before it is a type (`*`).
///
/// Haskell 2010's Report types need only `*`, `* -> *`, `* -> * -> *`, so a
/// kind is fully described by that count. There is no kind signature, no kind
/// polymorphism and no promotion in the language, which is exactly why the
/// count can be DERIVED: it is the type-argument arity the registry already
/// records ([`TypeEnv::ctor_kinds`], [`TypeEnv::data_arities`], and the scalar
/// registry), never a second table of names.
///
/// The one thing this representation cannot express is a constructor whose
/// PARAMETER is itself higher-kinded (`Kleisli m a b` with `m :: * -> *`,
/// whose true kind is `(* -> *) -> * -> * -> *`). A parameter's kind is written
/// by a kind signature, which the language does not have; such a parameter is
/// spelled as a type VARIABLE and keeps the kind its context gives it (see
/// [`TypeEnv::require_type_kind`]).
///
/// [`TypeEnv::ctor_kinds`]: TypeEnv::ctor_kinds
/// [`TypeEnv::data_arities`]: TypeEnv::data_arities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kind {
    /// Number of type arguments the entity still expects: `0` is `*`, `1` is
    /// `* -> *`, `2` is `* -> * -> *`.
    arity: usize,
}

impl Kind {
    /// The kind of a type: `*`.
    pub const TYPE: Kind = Kind { arity: 0 };

    /// The kind of a constructor expecting `arity` more type arguments
    /// (`arity` 0 is [`Kind::TYPE`]).
    pub fn of_arity(arity: usize) -> Kind {
        Kind { arity }
    }

    /// How many type arguments this kind still expects (`0` for `*`).
    pub fn arity(self) -> usize {
        self.arity
    }

    /// Whether this kind is `*` — i.e. the entity is a type.
    pub fn is_type(self) -> bool {
        self.arity == 0
    }

    /// Apply this kind to `n` type arguments, if it accepts that many.
    ///
    /// `None` when the application supplies MORE arguments than the kind
    /// expects (`Pair Int Int Int`), which is a kind error: the head is not a
    /// constructor of that arity.
    pub fn applied_to(self, n: usize) -> Option<Kind> {
        Some(Kind {
            arity: self.arity.checked_sub(n)?,
        })
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("*")?;
        for _ in 0..self.arity {
            f.write_str(" -> *")?;
        }
        Ok(())
    }
}

/// The role a buffer type CONSTRUCTOR (`FixedBuffer elem`, `Tape elem`) plays
/// in the language.
///
/// A buffer is an ordinary value of either role; the role only says how the FFI
/// layer classifies a parameter that names it. Recorded next to the name in
/// [`TypeEnv::buffer_ctors`] so the FFI classifier asks the registry instead of
/// keeping its own list of buffer spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferRole {
    /// A signal channel: `FixedBuffer elem`. Contributes one input arity.
    Channel,
    /// A shared-buffer resource handle: `Tape elem`. A symbolic reference, not
    /// a channel.
    Resource,
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
    /// Superclass context: `(class, its type variable)` pairs declared in the
    /// `(C1 a, C2 a) =>` prefix, in declaration order. These are the DIRECT
    /// superclasses; see [`TypeEnv::entailed_classes`] for the transitive
    /// closure.
    ///
    /// System FC: a superclass constraint is a FIELD of the subclass
    /// dictionary, so `Ord a`'s dictionary holds `Eq a`'s — that is what makes
    /// `Eq`'s methods resolvable wherever `Ord a` is in scope.
    pub superclasses: Vec<(String, String)>,
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
    /// True when this instance's method bodies are COMPILER KERNELS rather
    /// than source equations: a primitive numeric instance (`Num Int`,
    /// `Num Double`, `Fractional Double`), a buffer instance (element-wise
    /// kernels), or a structurally derived `Eq`/`Ord` instance.
    ///
    /// Dispatch for such an instance does not look for a body — it selects the
    /// kernel for the (class, type, method) triple. This is the System FC
    /// analogue of a primitive dictionary.
    pub primitive: bool,
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
/// Such a definition is NOT a plain function at the CALL SITE: its body mentions
/// methods of the constrained class (`rangeSize` in `rangeSize p`), which are
/// resolved against the GIVEN constraint and have no one concrete instance until
/// a call site fixes the class variable. Its body IS type-checked once, at load,
/// under the abstract dictionary (design §3.0) — an ill-typed constrained body is
/// rejected immediately, and every method call inside it records the given
/// witness in [`TypedProgram::dict_selections`].
///
/// [`TypedProgram::dict_selections`]: crate::types::infer::TypedProgram::dict_selections
///
/// What remains per call site is DISPATCH: each call specializes the body with
/// the concrete class-variable type and the bound instance (compile-time
/// β-substitution, never a runtime dictionary). That specialisation is the
/// transitional model this registry exists for; the dictionary design replaces
/// it with a passed dictionary argument (design §3.7 step 8). See
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
    /// Predefined scalar/primitive types, in registration order: the ONE table
    /// that says `Int`, `Double`, `Bool`, `String`, `Buffer` and `()` are types.
    /// Seeded once by [`TypeEnv::with_builtins`] and extended by
    /// [`TypeEnv::register_scalar`], so no other code needs a list of scalar
    /// names (the defect this table exists to remove).
    ///
    /// Order is significant only for the reverse lookup
    /// ([`TypeEnv::scalar_name_of`]): a later registration for the same value
    /// type renames it.
    pub scalars: Vec<(String, ScalarType)>,
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
    /// The buffer type CONSTRUCTORS and the role each plays (`FixedBuffer` a
    /// signal channel, `Tape` a shared-buffer resource). A buffer VALUE type is
    /// [`ValueTy::Buffer`] carrying its element; this table says which NAMES may
    /// appear as a buffer constructor in a signature, and how the FFI classifies
    /// them. Seeded by [`TypeEnv::with_builtins`].
    pub buffer_ctors: HashMap<String, BufferRole>,
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
    /// Their bodies ARE type-checked once, at load, under the GIVEN constraints
    /// (design §3.0): the class variable is assumed, so a method call inside the
    /// body resolves against the abstract dictionary and its witness is recorded
    /// in [`TypedProgram::dict_selections`]. The registry itself is lowering's
    /// transitional dispatch table: each call site still specializes the body
    /// with the concrete class-variable type (see [`ConstrainedDef`]).
    ///
    /// [`TypedProgram::dict_selections`]:
    ///     crate::types::infer::TypedProgram::dict_selections
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
typeclass (Functor f) => Applicative f where { pure: a -> f a; ap: f (a -> b) -> f a -> f b; }
typeclass (Applicative m) => Monad m where { return: a -> m a; bind: m a -> (a -> m b) -> m b; }
typeclass Monoid m where { mempty: m; mappend: m -> m -> m; }

--  Equality and ordering. Haskell 2010: `class Eq a` with `==`/`/=`, and
--  `class Eq a => Ord a` with `compare`/`<`/`<=`/`>`/`>=`/`min`/`max`. The
--  operator spellings ARE the method names, so an infix `a == b` and a bare
--  `(==)` resolve to the same binding — that is what makes operators sugar
--  over methods.
typeclass Eq a where {
    (==) : a -> a -> Bool;
    (/=) : a -> a -> Bool = \ x -> \ y -> if x == y then False else True;
}
--  `compare` returns an ORDER ENCODING: -1 / 0 / 1 (see DOCS-GAPS). The
--  defaults are written against it, so an instance need only provide
--  `compare`.
typeclass (Eq a) => Ord a where {
    compare : a -> a -> Int;
    (<)  : a -> a -> Bool = \ x -> \ y -> compare x y < 0;
    (<=) : a -> a -> Bool = \ x -> \ y -> compare x y <= 0;
    (>)  : a -> a -> Bool = \ x -> \ y -> compare x y > 0;
    (>=) : a -> a -> Bool = \ x -> \ y -> compare x y >= 0;
    min  : a -> a -> a = \ x -> \ y -> if x <= y then x else y;
    max  : a -> a -> a = \ x -> \ y -> if x <= y then y else x;
}

--  Numeric tower. `Num` carries the operators; `Fractional` adds `/`.
--  `Num` is Eq- and Ord-bounded: Haskell's numeric tower requires comparison
--  (`Real a => Ord a`), and rill keeps one flat tower so the numeric operators
--  and the ordering operators can never disagree about a receiver's class.
typeclass (Eq a, Ord a) => Num a where {
    (+) : a -> a -> a;
    (-) : a -> a -> a;
    (*) : a -> a -> a;
    negate : a -> a;
    abs : a -> a;
    signum : a -> a;
    fromInteger : Int -> a;
}
typeclass (Num a) => Fractional a where {
    (/) : a -> a -> a;
    recip : a -> a;
}

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
--  Primitive numeric instances. The bodies are the compiler's arithmetic
--  kernels (there are no source equations for a primitive), so the instance
--  declarations are markers; the kernel is selected by the class + type pair.
instance Num Int where { }
instance Num Double where { }
instance Fractional Double where { }

--  A buffer is an ordinary value and a NUMERIC one: `Buffer` is a subclass of
--  `Num` (and therefore of `Eq`/`Ord`), so it uses the SAME operator vocabulary
--  with no re-declaration and no possibility of a name collision. An instance's
--  body emits the whole-buffer kernel. There is no buffer-specific operator.
typeclass (Num b) => Buffer b where { }
data Tape a = { };
--  `Buffer` is a marker subclass of `Num`; the arithmetic comes from the
--  `Num` instance. Today the prelude declares the marker instances only — the
--  whole-buffer `Num` bodies are emitted by the compiler for these types (the
--  instance body is the kernel call), which is why no equations appear here.
--  The heads are MONOMORPHIC in the element (`FixedBuffer Double`, `Tape
--  Double`) because `Double` is the one legal buffer element today: `a` here
--  would advertise an instance for every element while the element is checked,
--  which is a lie. When a second element exists, the variable-headed
--  `instance (Floating a) => Num (FixedBuffer a)` is an ADDITIVE change.
instance Num (FixedBuffer Double) where { }
instance Num (Tape Double) where { }
instance Buffer (FixedBuffer Double) where { }
instance Buffer (Tape Double) where { }

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

instance Monoid Double where { mempty = 0.0; mappend a b = a + b; }
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
/// (`mixer`'s leading `List (FixedBuffer Double)` is a VariadicSignal followed
/// only by a record — [`crate::types::ffi::ffi_sig_from_typeexpr`] accepts the
/// shape). `tape_loop` (`Int -> Tape Double`) is a resource CONSTRUCTOR: its
/// `Tape Double` result is not a `FixedBuffer`/`Pair`, so
/// [`crate::types::ffi::ffi_sig_from_typeexpr`] rejects it by design — lowering
/// recognizes `tape_loop <capacity>` directly (like the legacy `TapeLoop`
/// declaration) and produces the tape index, which flows to `write_head` /
/// `read_head` via their `Tape Double` resource params (Task 11).
pub(crate) const BUILTIN_FOREIGN_DECLS: &str = r#"
foreign sine : Double -> Double -> Double -> FixedBuffer Double;
foreign saw : Double -> Double -> Double -> FixedBuffer Double;
foreign square : Double -> Double -> Double -> FixedBuffer Double;
foreign triangle : Double -> Double -> Double -> FixedBuffer Double;
foreign noise : Double -> Double -> FixedBuffer Double;
foreign ay38910 : Double -> Double -> FixedBuffer Double;
foreign sampler : Double -> Double -> Double -> Double -> Double -> FixedBuffer Double;

foreign integrator : FixedBuffer Double -> FixedBuffer Double;
foreign leaky_integrator : Double -> FixedBuffer Double -> FixedBuffer Double;
foreign onepole : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign moog : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign lowpass : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign highpass : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign biquad : Double -> Double -> Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign delay : Double -> Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign distortion : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign limiter : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign graphic_eq : Double -> FixedBuffer Double -> FixedBuffer Double;
foreign spectralgate : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign spectraldelay : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign convolver : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign analog_moog : Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign lofi : Double -> Double -> Double -> Double -> Double -> Double -> Double -> FixedBuffer Double -> FixedBuffer Double;
foreign eq_parametric : EqConfig -> FixedBuffer Double -> FixedBuffer Double;

foreign tape_loop : Int -> Tape Double;
foreign read_head : Tape Double -> Double -> FixedBuffer Double;

data EqBand = { freq: Double, q: Double, gain_db: Double, band_type: Int };
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

/// The derived body for `instance <sup> <ty>` when `<sub> <ty>` holds, keyed by
/// the (subclass, superclass) pair. `None` when this tower level has no
/// derivable body — the superclass instance must then be written explicitly by
/// the user (a class may require a superclass without being able to synthesise
/// its methods).
///
/// Only the Functor/Applicative tower has synthesised bodies; the numeric
/// towers (`Ord ⊑ Eq`, `Fractional ⊑ Num`, …) are *constraints*, not
/// derivations — a value has both dictionaries from its own instance
/// declarations, they are not built from one another.
fn superclass_template(sub: &str, sup: &str) -> Option<&'static str> {
    match (sub, sup) {
        ("Monad", "Applicative") => Some(APPLICATIVE_FROM_MONAD),
        ("Monad", "Functor") => Some(FUNCTOR_FROM_MONAD),
        ("Applicative", "Functor") => Some(FUNCTOR_FROM_APPLICATIVE),
        _ => None,
    }
}

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
        // A template-derived instance has a real body, so it is not a primitive.
        primitive: false,
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
            let mut free: Vec<String> = Vec::new();
            let fields_ty = fields
                .iter()
                .map(|(f, t, strict)| {
                    let ft =
                        crate::types::infer::data_field_vty_with_free(env, tyvars, t, &mut free);
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
    // Every type position in the fragment must have kind `*` (and every
    // instance head the class's kind). The prelude and the builtin catalog are
    // COMPILER-OWNED and never pass the program-level walk, so this is where
    // their kind contract is enforced: a `* -> *` constructor in a `*` position
    // in the prelude is a compiler bug, reported at load rather than silently
    // accepted by the converters that later consume these signatures.
    if let Err(e) = env.validate_type_names(&defs).and_then(|()| {
        for def in &defs {
            if let Def::Instance {
                class, head, span, ..
            } = def
            {
                env.check_instance_head(class, head, *span)?;
            }
        }
        Ok(())
    }) {
        panic!("compiler-owned prelude/catalog is ill-kinded: {e}");
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
    /// A `TypeEnv` with the predefined scalar registry, the builtin
    /// constructor table, the builtin `Maybe`/`Pair`/`Either` type shapes, and
    /// the category-theory prelude (`Functor`/`Applicative`/`Monad`/`Monoid`
    /// classes + instances) registered.
    ///
    /// The scalars are seeded ONCE here; a program's own declarations extend
    /// this same environment ([`crate::types::infer::infer_program_with`]), so
    /// there is no second name table anywhere.
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
        // The buffer type constructors and their FFI role. `FixedBuffer elem`
        // is a signal channel, `Tape elem` a shared-buffer resource; both are
        // registered here so the FFI classifier asks the registry.
        let buffer_ctors = HashMap::from([
            ("FixedBuffer".to_string(), BufferRole::Channel),
            ("Tape".to_string(), BufferRole::Resource),
        ]);
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
        // Classes are declared in the PRELUDE text ONLY: one source of truth.
        // (`Eq`/`Ord` used to be built here as well, with wrong unary `eq`/`lt`
        // methods, which made operators impossible to express as methods.)
        let typeclasses: HashMap<String, TypeclassInfo> = HashMap::new();
        let mut env = TypeEnv {
            ctor_kinds,
            buffer_ctors,
            data_types,
            typeclasses,
            ..TypeEnv::default()
        };
        // The predefined scalar/primitive types: Haskell 2010 §6.4.1 spells the
        // numeric leaves `Int`/`Double`, `Bool` and `String` are the Prelude's,
        // `Buffer` is rill's buffer value, and `()` is the unit type (Haskell
        // 2010 spells the unit type as the empty tuple; there is no `Void`
        // type name). This is the ONE place that says which names are types —
        // every consumer asks [`TypeEnv::type_entry`].
        for (name, vty, ffi_scalar, buffer_element) in [
            ("Int", ValueTy::Int, true, false),
            ("Double", ValueTy::Double, true, true),
            ("Bool", ValueTy::Bool, true, false),
            ("String", ValueTy::String, true, false),
            ("()", ValueTy::Void, false, false),
        ] {
            env.register_scalar(
                name,
                ScalarType {
                    vty,
                    ffi_scalar,
                    buffer_element,
                },
            );
        }
        // `Buffer` is rill's buffer VALUE type (`FixedBuffer elem`), not a
        // scalar: it is registered separately so its name denotes the
        // element-carrying constructor rather than a leaf.
        env.register_scalar(
            "Buffer",
            ScalarType {
                vty: ValueTy::Buffer(Box::new(ValueTy::Double)),
                ffi_scalar: false,
                buffer_element: false,
            },
        );
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
        //   show_double      :: Double -> String
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
            "show_double".to_string(),
            crate::ast::TypeExpr::TFunc(vec![tv("Double")], Box::new(tv("String"))),
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
        // Buffer bridge: concrete over the `Int`-indexed `Double` array the
        // `FixedBuffer Double` maps to. The value-level type of a signal buffer is
        // `Buffer` (that is what the FFI apply path produces for a fully applied
        // generator and what a buffer-returning `main` yields), so these are
        // written against `Buffer` rather than the `FixedBuffer Double` spelling
        // used inside `foreign` declarations.
        env.builtin_sigs.insert(
            "arrayFromBuffer".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![tv("Buffer")],
                Box::new(array(vec![tv("Int"), tv("Double")])),
            ),
        );
        env.builtin_sigs.insert(
            "bufferFromArray".to_string(),
            crate::ast::TypeExpr::TFunc(
                vec![array(vec![tv("Int"), tv("Double")])],
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

        // The `builtin_sigs` catalog is assembled in Rust, so it never passes
        // the program-level kind walk — hold it to the SAME contract here. A
        // `* -> *` constructor in a `*` position in a builtin signature is a
        // compiler bug, not something a converter downstream may accept.
        if let Err(e) = env
            .builtin_sigs
            .values()
            .try_for_each(|sig| env.check_type_kind(sig, Span::new(0, 0)))
        {
            panic!("a builtin signature is ill-kinded: {e}");
        }

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
                    supers,
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
                            // Direct superclasses, in declaration order.
                            superclasses: supers.clone(),
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
                            // A body-less declaration is a MARKER: the class's
                            // methods are compiler kernels for that type
                            // (`instance Num Int where { }`). A declaration
                            // with equations carries a real body.
                            primitive: methods.is_empty(),
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
        // General walk over DECLARED superclasses (System FC: a superclass
        // constraint is a field of the subclass dictionary, so an instance of
        // the subclass yields an instance of every superclass). The old code
        // hard-coded `Monad -> Applicative -> Functor`; the table below supplies
        // the template bodies for those three derivations, but the DRIVER is the
        // declaration, so a new tower needs no code change here.
        //
        // Repeat until a fixpoint: a chain `Integral -> Real -> ...` may need
        // several passes because the superclass instance must exist before the
        // next level is derived.
        loop {
            let mut derived_any = false;
            // Snapshot the declarations so the loop can mutate `self`.
            let classes: Vec<(String, Vec<(String, String)>)> = self
                .typeclasses
                .iter()
                .map(|(name, info)| (name.clone(), info.superclasses.clone()))
                .collect();
            for (sub, supers) in classes {
                let Some(insts) = self.instances.get(&sub).map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<Vec<_>>()
                }) else {
                    continue;
                };
                for supers in supers.iter() {
                    let sup = supers.0.clone();
                    for (ty, src_info) in &insts {
                        let mut already = self
                            .instances
                            .get(&sup)
                            .map(|m| m.contains_key(ty))
                            .unwrap_or(false);
                        if already {
                            continue;
                        }
                        let Some(template) = superclass_template(&sub, &sup) else {
                            continue;
                        };
                        let head_args = src_info.head_args.clone();
                        let info = instance_from_template(
                            ty.as_str(),
                            &sup,
                            template,
                            &head_args,
                            src_info.from_prelude,
                        );
                        self.instances
                            .entry(sup.clone())
                            .or_default()
                            .insert(ty.clone(), info);
                        already = true;
                        let _ = already;
                        derived_any = true;
                    }
                }
            }
            if !derived_any {
                break;
            }
        }
    }

    /// The transitive superclass closure of `class` (including `class` itself),
    /// in breadth-first order. Cycle-safe (a malformed declaration cannot loop).
    ///
    /// System FC: projecting the superclass fields of a dictionary repeatedly.
    pub fn entailed_classes(&self, class: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut queue: Vec<String> = vec![class.to_string()];
        while let Some(c) = queue.pop() {
            if out.iter().any(|x| x == &c) {
                continue;
            }
            out.push(c.clone());
            if let Some(info) = self.typeclasses.get(&c) {
                for (sup, _var) in &info.superclasses {
                    if !out.iter().any(|x| x == sup) {
                        queue.push(sup.clone());
                    }
                }
            }
        }
        out
    }

    /// Whether `class` entails `target` (reflexive and transitive).
    pub fn entails(&self, class: &str, target: &str) -> bool {
        class == target || self.entailed_classes(class).iter().any(|c| c == target)
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
    /// and every registered SCALAR leaf. `Func` types get no instance. Derived
    /// instances are markers: their method bodies are not run — the
    /// interpreter's `value_cmp` implements the order.
    ///
    /// The scalar leaves come from [`Self::scalars`] — the one registry — not
    /// from a private list here: a name added to the registry is seeded here
    /// without touching this function.
    pub fn derive_eq_ord(&mut self) {
        let mut names: Vec<String> = self.scalars.iter().map(|(n, _)| n.clone()).collect();
        names.extend(self.data_types.keys().cloned());
        names.extend(self.ctor_kinds.keys().cloned());
        names.extend(self.newtypes.keys().cloned());
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
                    // Structural comparison is a compiler kernel.
                    primitive: true,
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
    /// Register a predefined scalar type under `name`. Registering a second
    /// name for the SAME value type RENAMES it: the earlier spelling is
    /// dropped, so one value type never has two registered names and
    /// [`Self::scalar_name_of`] stays a function.
    pub fn register_scalar(&mut self, name: impl Into<String>, ty: ScalarType) {
        let name = name.into();
        self.scalars.retain(|(n, s)| *n == name || s.vty != ty.vty);
        if let Some(slot) = self.scalars.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = ty;
        } else {
            self.scalars.push((name, ty));
        }
    }

    /// The registry entry for a type-level NAME — the single answer to "what
    /// does this name denote".
    ///
    /// Resolution order (Haskell 2010 §3.1 lexical rules come first):
    ///
    /// 1. a registered predefined scalar (`Int`, `Double`, `Bool`, `String`,
    ///    `Buffer`, `()`);
    /// 2. a `type` synonym, followed to its final target;
    /// 3. a `newtype` wrapper;
    /// 4. a declared `data`/abstract type or a builtin constructor, with its
    ///    type-argument arity;
    /// 5. any other LOWERCASE name — a type VARIABLE (it is not a type, but it
    ///    is a well-formed name in the type language);
    /// 6. otherwise `None`: an UPPERCASE name that no declaration introduced
    ///    does not exist (`unknown type name`).
    pub fn type_entry(&self, name: &str) -> Option<TypeEntry> {
        if let Some((_, scalar)) = self.scalars.iter().find(|(n, _)| n == name) {
            return Some(TypeEntry::Scalar(scalar.clone()));
        }
        // A synonym denotes whatever its FINAL target denotes; a chain that
        // loops (or ends at a name that is not a type) denotes nothing. The
        // walk is bounded by the alias count, so a cycle terminates.
        if self.type_aliases.contains_key(name) {
            let mut cur = name;
            let mut hops = 0usize;
            while let Some(target) = self.type_aliases.get(cur) {
                hops += 1;
                if hops > self.type_aliases.len() {
                    return None;
                }
                cur = target.as_str();
            }
            return match self.type_entry(cur)? {
                TypeEntry::Variable | TypeEntry::Alias(_) => None,
                _ => Some(TypeEntry::Alias(cur.to_string())),
            };
        }
        if self.newtypes.contains_key(name) {
            return Some(TypeEntry::Newtype);
        }
        if let Some(arity) = self.type_ctor_arity(name) {
            return Some(TypeEntry::TypeCtor { arity });
        }
        if name.chars().next().is_some_and(char::is_lowercase) {
            return Some(TypeEntry::Variable);
        }
        None
    }

    /// Whether `name` denotes a TYPE: a registry entry that is not a type
    /// variable. This is the ONE declaration predicate.
    ///
    /// A lowercase name is a type variable by Haskell 2010's lexical rules, so
    /// it is NOT a declared type even though the registry recognizes it as a
    /// well-formed variable name; an undeclared uppercase name is simply not a
    /// type at all.
    pub fn is_type_name(&self, name: &str) -> bool {
        self.type_entry(name).is_some_and(|e| e.is_type())
    }

    /// Whether `name` is a type VARIABLE: a lowercase name that no declaration
    /// shadows (Haskell 2010 §3.1).
    pub fn is_type_variable(&self, name: &str) -> bool {
        matches!(self.type_entry(name), Some(TypeEntry::Variable))
    }

    /// The type-argument arity of a declared type constructor (builtin
    /// container, builtin data shape, `data`, or abstract type). `None` when
    /// `name` is not a constructor.
    pub fn type_ctor_arity(&self, name: &str) -> Option<usize> {
        if let Some(a) = self.ctor_kinds.get(name) {
            return Some(*a);
        }
        if let Some(a) = self.data_arities.get(name) {
            return Some(*a);
        }
        // A monomorphic declaration (`data Point`, `data World`) has arity 0.
        self.data_types.get(name).map(|_| 0)
    }

    /// The KIND of a type-level name — the ONE place that answers "what is
    /// this name's kind".
    ///
    /// The answer is a pure PROJECTION of the arity the registry already
    /// records, never a second table of names:
    ///
    /// | registry entry | kind |
    /// |---|---|
    /// | scalar (`Int`, `Double`, `Bool`, `String`, `Buffer`, `()`) | `*` |
    /// | `newtype` wrapper | `*` |
    /// | `type` synonym | the target's kind |
    /// | [`Self::ctor_kinds`] / [`Self::data_arities`] entry, arity `n` | `* -> … -> *` (`n` arrows) |
    /// | lowercase name (a type variable) | unknown — its binder gives it |
    ///
    /// `None` means "not a kind this table knows": a type VARIABLE (a lowercase
    /// name, whose kind its binder supplies — see the class-`arity` rule) or a
    /// declared type whose parameters the registry does not record (a
    /// parameterized SUM, and any declaration reached only through
    /// [`Self::data_types`]). An unknown kind is never an error: it is the
    /// caller's context that would have to judge it, and this table cannot.
    pub fn kind_of(&self, name: &str) -> Option<Kind> {
        match self.type_entry(name)? {
            TypeEntry::Scalar(_) | TypeEntry::Newtype => Some(Kind::TYPE),
            TypeEntry::Alias(target) => self.kind_of(&target),
            TypeEntry::TypeCtor { arity } => {
                // `type_ctor_arity` reports `0` for two very different things:
                // an EXACT zero (a builtin `data_arities` entry with no
                // parameters) and a declared `data` type whose parameters the
                // registry does not record. Only an exact arity is a kind.
                if self.ctor_kinds.contains_key(name) || self.data_arities.contains_key(name) {
                    Some(Kind::of_arity(arity))
                } else {
                    None
                }
            }
            TypeEntry::Variable => None,
        }
    }

    /// The kind of a type EXPRESSION, or `None` when the expression involves a
    /// type variable (whose kind only its binder knows).
    ///
    /// An over-application (a constructor given more arguments than its kind
    /// accepts) is the one hard error here: `Pair Int Int Int` has no kind at
    /// all. Every argument position is itself required to have kind `*`.
    fn type_expr_kind(
        &self,
        te: &crate::ast::TypeExpr,
        span: Span,
    ) -> Result<Option<Kind>, CompileError> {
        use crate::ast::TypeExpr;
        match te {
            TypeExpr::TName(n) => Ok(self.kind_of(n)),
            TypeExpr::TApp(head, args) => {
                for a in args {
                    self.check_type_kind(a, span)?;
                }
                let Some(head_kind) = self.kind_of(head) else {
                    // A variable head (`m b`): the binder's kind decides.
                    return Ok(None);
                };
                match head_kind.applied_to(args.len()) {
                    Some(k) => Ok(Some(k)),
                    None => Err(CompileError::Type {
                        msg: format!(
                            "too many arguments in `{}`; `{head}` has kind `{head_kind}`",
                            self.type_expr_spelling(te)
                        ),
                        span,
                    }),
                }
            }
            TypeExpr::TFunc(args, ret) => {
                for a in args {
                    self.check_type_kind(a, span)?;
                }
                self.check_type_kind(ret, span)?;
                Ok(Some(Kind::TYPE))
            }
            TypeExpr::TForall(_, inner) | TypeExpr::TConstrained(_, inner) => {
                self.type_expr_kind(inner, span)
            }
        }
    }

    /// Require `te` to have kind `*` — the only kind a TYPE POSITION accepts
    /// (a signature, a data field, a constructor payload, a class method's
    /// argument, an instance body's annotation).
    ///
    /// This is the single implementation of Haskell 2010 §4.1's kind rule for
    /// type positions; every enforcement point calls it, so there is exactly
    /// one wording and exactly one rule.
    pub fn check_type_kind(
        &self,
        te: &crate::ast::TypeExpr,
        span: Span,
    ) -> Result<(), CompileError> {
        match self.type_expr_kind(te, span)? {
            Some(kind) if !kind.is_type() => Err(kind_position_error(self, te, kind, span)),
            _ => Ok(()),
        }
    }

    /// Check an `instance` HEAD against the class it implements, in kind
    /// vocabulary.
    ///
    /// The class variable's kind is [`Kind::of_arity`] of the class's `arity`
    /// (the number of type arguments the method signatures apply it to), and
    /// the head must have exactly that kind:
    ///
    /// * `instance Functor List` → `List` has kind `* -> *` ✔;
    /// * `instance Functor Pair` → `Pair` has kind `* -> * -> *` ✘;
    /// * `instance Monad IO` → `IO` has kind `* -> *` ✔;
    /// * `instance (Monad m) => Arrow (Kleisli m)` → `Kleisli m` leaves two
    ///   arguments, so `* -> * -> *` ✔;
    /// * `instance Functor (Pair Int)` → `* -> *` ✔ (`instance Functor ((,)
    ///   Int)` in GHC).
    ///
    /// A class whose variable is a TYPE (`Monoid m`, `Ix i`, arity `0`) binds
    /// the head's whole type, so the head must have kind `*`. rill spells a
    /// constructor head for such a class SATURATED — `instance Monoid List` is
    /// Haskell 2010's `instance Monoid [a]` (head `[a]`, kind `*`), and
    /// `instance Monoid (List a)` is accepted the same way. A bare constructor
    /// head is therefore read as its saturation and passes.
    ///
    /// A lowercase head (`instance C a`) is a type VARIABLE with the class
    /// variable's kind, so its binder decides and this check defers; an
    /// UPPERCASE name with no recorded kind is not a type constructor at all.
    pub fn check_instance_head(
        &self,
        class: &str,
        head: &crate::ast::TypeExpr,
        span: Span,
    ) -> Result<(), CompileError> {
        let Some(class_arity) = self.typeclasses.get(class).map(|c| c.arity) else {
            // An undeclared class is reported by class registration, not here.
            return Ok(());
        };
        let expected = Kind::of_arity(class_arity);
        // A malformed head (an over-applied constructor) has no kind at all.
        let got = self.type_expr_kind(head, span)?;
        // A class whose variable is a TYPE (`Monoid m`, `Eq a`, `Ix i`) binds
        // the head's whole type, so the head must have kind `*`. rill spells a
        // constructor head for such a class by SATURATING it — `instance Monoid
        // List` is Haskell 2010's `instance Monoid [a]` (head `[a]`, kind `*`)
        // — so a bare constructor/declaration name is accepted here.
        if class_arity == 0 {
            // A BARE constructor name under a type-variable class is that
            // constructor SATURATED (`Monoid List` ≡ `Monoid [a]`, and the
            // derived `Eq List`/`Ord Array` markers name their full type the
            // same way). GHC writes the applied form; rill's registry keys a
            // fully-applied head and a bare constructor head alike.
            if matches!(head, crate::ast::TypeExpr::TName(n) if self.is_known_type_name(n)) {
                return Ok(());
            }
            match got {
                Some(k) if k.is_type() => return Ok(()),
                Some(k) => {
                    return Err(CompileError::Type {
                        msg: format!(
                            "expecting a type in the head of `{class}`; `{}` has kind `{k}`",
                            self.type_expr_spelling(head)
                        ),
                        span,
                    })
                }
                // Unknown kind: a type variable (bound at the class's own kind)
                // or a declared nominal type whose parameters the registry does
                // not record. Both are types; only an over-application above
                // is an error.
                None => return Ok(()),
            }
        }
        let satisfied = match got {
            Some(k) => k == expected,
            None => match head {
                // A type-variable head binds at the class variable's kind.
                crate::ast::TypeExpr::TName(n) if self.is_type_variable(n) => true,
                // A variable APPLICATION (`instance C (m a)`) is a constructor
                // applied to a variable; the context (or the instance thereby
                // declaring `m` abstract) supplies its kind.
                crate::ast::TypeExpr::TApp(n, _) if self.is_type_variable(n) => true,
                _ => false,
            },
        };
        if !satisfied {
            let spelling = self.type_expr_spelling(head);
            return Err(match got {
                Some(k) => CompileError::Type {
                    msg: format!(
                        "`{class}` expects a type constructor of kind `{expected}`, but \
                         `{spelling}` has kind `{k}`"
                    ),
                    span,
                },
                None => CompileError::Type {
                    msg: format!(
                        "`{spelling}` is not a type constructor (`{class}` expects kind \
                         `{expected}`)"
                    ),
                    span,
                },
            });
        }
        Ok(())
    }

    /// The source spelling of a type expression for diagnostics
    /// (`[Int]`, `(a, b)`, `Maybe Int`) — the form a program writes.
    ///
    /// This is NOT the instance-registry key: that one
    /// ([`crate::render::render_type_key`]) must agree byte for byte with
    /// [`value_type_key`], which keys a selector's value type by the nominal
    /// constructor names. Diagnostics quote the source form instead.
    pub fn type_expr_spelling(&self, te: &crate::ast::TypeExpr) -> String {
        crate::render::render_type_spelling(te)
    }

    /// The source spelling of a scalar value type (`Int`, `Double`, `Bool`,
    /// `String`, `Buffer`, `()`), read from the registry rather than a private
    /// table. `None` for a non-scalar value type.
    pub fn scalar_name_of(&self, v: &ValueTy) -> Option<String> {
        self.scalars
            .iter()
            .find(|(_, s)| &s.vty == v)
            .map(|(n, _)| n.clone())
    }

    /// Whether a `foreign` signature may name `name` as a SCALAR parameter.
    /// A typed registry miss (`Foo` not declared) is not a scalar either.
    pub fn is_ffi_scalar(&self, name: &str) -> bool {
        matches!(
            self.type_entry(name),
            Some(TypeEntry::Scalar(ScalarType {
                ffi_scalar: true,
                ..
            }))
        )
    }

    /// Whether a buffer may carry the type named `name` as its SAMPLE ELEMENT
    /// (`FixedBuffer name` / `Tape name`).
    ///
    /// Data-driven: the answer is a property of the registry entry
    /// ([`ScalarType::buffer_element`]), so supporting a second element is a
    /// one-line change at the seed, not a new list in the checker. `Double` is
    /// the only marked element today — it is the only element a program can
    /// name or observe.
    pub fn is_buffer_element(&self, name: &str) -> bool {
        matches!(
            self.type_entry(name),
            Some(TypeEntry::Scalar(ScalarType {
                buffer_element: true,
                ..
            }))
        )
    }

    /// The element type a buffer's samples are checked against, from the
    /// registry's marked buffer element(s). The FIRST marked scalar in
    /// registration order is the element a buffer value's samples have: the
    /// runtime materialises every sample as `Value::Double`, so a buffer value
    /// types as `FixedBuffer <that element>`.
    pub fn buffer_element_ty(&self) -> Option<ValueTy> {
        self.scalars
            .iter()
            .find(|(_, s)| s.buffer_element)
            .map(|(_, s)| s.vty.clone())
    }

    /// The buffer VALUE type a bare buffer expression has
    /// (`FixedBuffer <element>`), or the element-free fallback when no scalar is
    /// marked as a buffer element (a program could not observe a buffer at all
    /// in that state).
    pub fn buffer_vty(&self) -> ValueTy {
        ValueTy::Buffer(Box::new(
            self.buffer_element_ty().unwrap_or(ValueTy::Double),
        ))
    }

    /// Build the value type a type CONSTRUCTOR application denotes.    ///
    /// This is the ONE place a `<head> <args…>` application becomes a value
    /// type, so every converter (signatures, data fields, instance heads,
    /// builtin signatures) agrees:
    ///
    /// * a buffer CHANNEL constructor (`FixedBuffer elem`) becomes the buffer
    ///   value type [`ValueTy::Buffer`] — never a second `App("FixedBuffer", …)`
    ///   shape;
    /// * a declared parameterized data type (`IO a`, `ST s a`, a user
    ///   `data Box a`) keeps the `Data` flavour it is constructed and matched
    ///   with;
    /// * every other application (a builtin container: `List`, `Pair`, `Array`)
    ///   is `App`.
    pub fn ctor_application(&self, head: &str, args: Vec<ValueTy>) -> ValueTy {
        if let Some(buf) = self.channel_ctor_vty(head, &args) {
            return buf;
        }
        if self.data_arities.contains_key(head) {
            ValueTy::Data(head.to_string(), args)
        } else {
            ValueTy::App(head.to_string(), args)
        }
    }

    /// The [`BufferRole`] of a buffer type constructor name (`FixedBuffer`,
    /// `Tape`), or `None` when the name is not a buffer constructor.
    pub fn buffer_role(&self, name: &str) -> Option<BufferRole> {
        self.buffer_ctors.get(name).copied()
    }

    /// Whether a CONCRETE value type may be a buffer's sample element.
    ///
    /// The registry decides via [`ScalarType::buffer_element`], so this is not a
    /// private list: `Double` is the only element today, and marking a second
    /// scalar is one edit at the seed.
    pub fn is_buffer_element_ty(&self, v: &ValueTy) -> bool {
        self.scalars
            .iter()
            .any(|(_, s)| s.buffer_element && &s.vty == v)
    }

    /// The registered buffer element spellings, in registration order, for a
    /// diagnostic ("the only buffer element is `Double`").
    pub fn buffer_element_names(&self) -> Vec<String> {
        self.scalars
            .iter()
            .filter(|(_, s)| s.buffer_element)
            .map(|(n, _)| n.clone())
            .collect()
    }

    /// The value type a buffer CHANNEL constructor application denotes:
    /// `FixedBuffer elem` → `ValueTy::Buffer(elem)`.
    ///
    /// `None` when `head` is not a channel constructor at all, so a caller can
    /// fall through to its ordinary constructor-application path. Composite
    /// elements (a buffer of buffers) are not expressible; validation rejects
    /// them, and a concrete non-registered element is reported there. This is
    /// the ONE translation into the buffer value type — no caller builds
    /// `App("FixedBuffer", [elem])` as a second shape.
    pub fn channel_ctor_vty(&self, head: &str, args: &[ValueTy]) -> Option<ValueTy> {
        match self.buffer_role(head)? {
            BufferRole::Channel if args.len() == 1 => {
                Some(ValueTy::Buffer(Box::new(args[0].clone())))
            }
            _ => None,
        }
    }

    /// Whether an instance-registry KEY names a buffer type — either the bare
    /// constructor (`FixedBuffer`) or the full applied key the monomorphic
    /// prelude head registers (`FixedBuffer Double`, `Tape Double`).
    ///
    /// Data-driven off [`Self::buffer_ctors`], so the lowering kernel selector
    /// does not carry its own list of buffer spellings.
    pub fn instance_key_is_buffer(&self, key: &str) -> bool {
        self.buffer_role(key.split(' ').next().unwrap_or(key))
            .is_some()
    }

    /// Whether `name` is a WELL-FORMED type-level name: a registered type or a
    /// type variable (a lowercase name). `false` only for an uppercase name no
    /// declaration introduced.
    pub fn is_known_type_name(&self, name: &str) -> bool {
        self.type_entry(name).is_some()
    }

    /// Reject every type expression in `defs` that names a type the registry
    /// does not have.
    ///
    /// This is the enforcement point of Haskell 2010 §3.1: an uppercase
    /// identifier in a type position MUST name a declared type, so
    /// `f :: Integer -> Integer` is an error rather than a function over a
    /// phantom type nothing can inhabit. A lowercase name is a type variable
    /// and always passes.
    ///
    /// Every place a program can write a type expression is walked: definition
    /// signatures (including `where`/`let` blocks), expression annotations,
    /// `data`/`newtype`/`type`/abstract declarations, constructor payloads,
    /// typeclass method and default signatures, and `instance` heads and
    /// contexts. The prelude and the builtin `foreign` catalog are not walked —
    /// they are compiler-owned and correct by construction.
    pub fn validate_type_names(&self, defs: &[Def]) -> Result<(), CompileError> {
        for def in defs {
            self.validate_def_type_names(def)?;
        }
        Ok(())
    }

    /// [`Self::validate_type_names`] for one definition, recursing into
    /// `where` blocks and the type annotations of its body.
    fn validate_def_type_names(&self, def: &Def) -> Result<(), CompileError> {
        match def {
            Def::Data {
                tyvars,
                fields,
                span,
                ..
            } => {
                for (_, t, _) in fields {
                    self.validate_type_expr(t, tyvars, *span)?;
                }
            }
            Def::Sum {
                tyvars,
                ctors,
                span,
                ..
            } => {
                for (_, payload) in ctors {
                    for (t, _) in payload {
                        self.validate_type_expr(t, tyvars, *span)?;
                    }
                }
            }
            // `type A = B` / `newtype A = B`: `B` must denote a type, because
            // the wrapper's inner type is resolved from it. A synonym may name
            // a synonym declared later (the env registers every alias before
            // this runs), and a chain that never reaches a type is unknown.
            Def::TypeAlias { target, span, .. } | Def::Newtype { target, span, .. } => {
                if !self.is_known_type_name(target) || !self.is_type_name(target) {
                    return Err(unknown_type_error(target, *span));
                }
                // The alias's inner type is used as a TYPE, so it must have
                // kind `*`: `type A = List` is `type A = []`, rejected by GHC.
                let inner = crate::ast::TypeExpr::TName(target.clone());
                self.check_type_kind(&inner, *span)?;
            }
            Def::Abstract { .. } => {}
            Def::Typeclass {
                var,
                methods,
                defaults,
                span,
                ..
            } => {
                let tyvars = vec![var.clone()];
                for (_, sig) in methods {
                    self.validate_type_expr(sig, &tyvars, *span)?;
                }
                for (_, _, body) in defaults {
                    self.validate_body_type_names(body, &tyvars)?;
                }
            }
            Def::Instance {
                class,
                head,
                constraints,
                method_bodies,
                span,
                ..
            } => {
                // An instance head binds its own type variables (`Kleisli m`,
                // `Pair Int Int`); a context binds its class variables.
                let mut tyvars: Vec<String> = Vec::new();
                collect_type_app_vars(head, &mut tyvars);
                tyvars.extend(constraints.iter().map(|(_, v)| v.clone()));
                // The head is kind-checked against the CLASS's expected kind,
                // not against `*`: `Functor List` binds a `* -> *` constructor,
                // `Monoid (List a)` binds a type.
                self.validate_instance_head(class, head, &tyvars, *span)?;
                for (_, _, body) in method_bodies {
                    self.validate_body_type_names(body, &tyvars)?;
                }
            }
            Def::Foreign { sig, span, .. } => {
                self.validate_type_expr(sig, &[], *span)?;
            }
            Def::Eval {
                sig,
                body,
                where_defs,
                span,
                ..
            } => {
                if let Some(sig) = sig {
                    self.validate_type_expr(sig, &[], *span)?;
                }
                for w in where_defs {
                    self.validate_def_type_names(w)?;
                }
                self.validate_body_type_names(body, &[])?;
            }
            // The parser's equation form: the bodies live in the equations,
            // each with its own `where` block. `desugar` turns them into a
            // `Def::Eval` before inference, so walking them here is what makes
            // an unknown type in `f 0 = …` caught either way.
            Def::Equations {
                sig,
                equations,
                span,
                ..
            } => {
                if let Some(sig) = sig {
                    self.validate_type_expr(sig, &[], *span)?;
                }
                for eq in equations {
                    for w in &eq.where_defs {
                        self.validate_def_type_names(w)?;
                    }
                    for (g, b) in &eq.guards {
                        self.validate_body_type_names(g, &[])?;
                        self.validate_body_type_names(b, &[])?;
                    }
                }
            }
            Def::Signature { sig, span, .. } => {
                self.validate_type_expr(sig, &[], *span)?;
            }
        }
        Ok(())
    }

    /// Reject an unknown type name anywhere in one type expression, and require
    /// the expression to have kind `*` (it is in a TYPE position).
    ///
    /// `tyvars` are the locally bound type-variable names (`data Box a` binds
    /// `a`, a typeclass binds its class variable, `forall`/`instance` bind
    /// their own); a name there is a variable whatever its case, exactly as
    /// Haskell's scoping works. With no binder in scope a LOWERCASE name is
    /// still a type variable by the lexical rule.
    ///
    /// The kind rule is [`TypeEnv::check_type_kind`] — the ONE implementation of
    /// Haskell 2010 §4.1's kind requirement for a type position, shared with the
    /// converters so no path can disagree.
    fn validate_type_expr(
        &self,
        te: &crate::ast::TypeExpr,
        tyvars: &[String],
        span: Span,
    ) -> Result<(), CompileError> {
        self.validate_type_names_in(te, tyvars, span)?;
        self.check_type_kind(te, span)
    }

    /// Validate an `instance` HEAD: every name it mentions must exist, and the
    /// head must have the kind its class expects
    /// ([`TypeEnv::check_instance_head`]). A head is NOT a type position (it
    /// binds a constructor for a constructor class), so the kind rule here is
    /// the class's expected kind, never `*`.
    fn validate_instance_head(
        &self,
        class: &str,
        head: &crate::ast::TypeExpr,
        tyvars: &[String],
        span: Span,
    ) -> Result<(), CompileError> {
        self.validate_type_names_in(head, tyvars, span)?;
        self.check_instance_head(class, head, span)
    }

    /// [`Self::validate_type_expr`]'s NAME rule only: reject an unknown type
    /// name anywhere in one type expression.
    fn validate_type_names_in(
        &self,
        te: &crate::ast::TypeExpr,
        tyvars: &[String],
        span: Span,
    ) -> Result<(), CompileError> {
        use crate::ast::TypeExpr;
        match te {
            TypeExpr::TName(n) => {
                if tyvars.iter().any(|v| v == n) || self.is_known_type_name(n) {
                    Ok(())
                } else {
                    Err(unknown_type_error(n, span))
                }
            }
            TypeExpr::TApp(head, args) => {
                if !tyvars.iter().any(|v| v == head) && !self.is_known_type_name(head) {
                    return Err(unknown_type_error(head, span));
                }
                // A buffer constructor's element is CHECKED here: the one legal
                // element today is the registry's marked scalar (`Double`), so
                // `FixedBuffer Int` / `FixedBuffer Bool` name an element the
                // language does not have. The requirement is named in the error,
                // and the rule is a registry property — not a name list.
                if self.buffer_role(head).is_some() && args.len() == 1 {
                    self.validate_buffer_element(&args[0], tyvars, span)?;
                }
                for a in args {
                    self.validate_type_names_in(a, tyvars, span)?;
                }
                Ok(())
            }
            TypeExpr::TFunc(args, ret) => {
                for a in args {
                    self.validate_type_names_in(a, tyvars, span)?;
                }
                self.validate_type_names_in(ret, tyvars, span)
            }
            TypeExpr::TForall(bound, inner) => {
                let mut inner_vars: Vec<String> = tyvars.to_vec();
                inner_vars.extend(bound.iter().cloned());
                self.validate_type_names_in(inner, &inner_vars, span)
            }
            TypeExpr::TConstrained(constraints, inner) => {
                // A context's variables are bound by the context itself.
                let mut inner_vars: Vec<String> = tyvars.to_vec();
                inner_vars.extend(constraints.iter().map(|(_, v)| v.clone()));
                self.validate_type_names_in(inner, &inner_vars, span)
            }
        }
    }

    /// Reject a buffer ELEMENT type the language does not have.
    ///
    /// `FixedBuffer elem` / `Tape elem` require `elem` to be a registered buffer
    /// element ([`Self::is_buffer_element`]). A concrete wrong element
    /// (`Int`, `Bool`, `String`) is an error naming the requirement; a TYPE
    /// VARIABLE element (`FixedBuffer a` inside a polymorphic signature) is a
    /// constraint the signature makes, not a claim about a concrete element, so
    /// it passes — the call site's element is checked against the buffer's
    /// actual type when the two meet.
    fn validate_buffer_element(
        &self,
        elem: &crate::ast::TypeExpr,
        tyvars: &[String],
        span: Span,
    ) -> Result<(), CompileError> {
        use crate::ast::TypeExpr;
        match elem {
            // A type variable element: a constraint on the signature, deferred
            // to the call site.
            TypeExpr::TName(n) if tyvars.iter().any(|v| v == n) || self.is_type_variable(n) => {
                Ok(())
            }
            TypeExpr::TName(n) if self.is_buffer_element(n) => Ok(()),
            // An UPPERCASE name nothing declares is not an element problem: it
            // is a name that does not exist (Haskell 2010 §3.1), and the
            // diagnostic must say so rather than blame the element rule.
            TypeExpr::TName(n) if !self.is_known_type_name(n) => Err(unknown_type_error(n, span)),
            // Any other concrete element (a declared type, a newtype, a scalar
            // the registry does not mark as a buffer element) is wrong.
            TypeExpr::TName(n) => Err(wrong_buffer_element_error(self, n, span)),
            other => Err(CompileError::Type {
                msg: format!(
                    "a buffer's element must be a scalar type; the only buffer element is `{}`",
                    self.buffer_element_names().join("`, `")
                ),
                span: {
                    let _ = other;
                    span
                },
            }),
        }
    }

    /// [`Self::validate_type_expr`] over every type annotation inside an
    /// expression (`e :: T`, `let …`, lambdas, `case` arms).
    fn validate_body_type_names(&self, e: &Expr, tyvars: &[String]) -> Result<(), CompileError> {
        match e {
            Expr::Annot { expr, ty, span } => {
                self.validate_type_expr(ty, tyvars, *span)?;
                self.validate_body_type_names(expr, tyvars)
            }
            Expr::Let { defs, body, .. } => {
                for d in defs {
                    self.validate_def_type_names(d)?;
                }
                self.validate_body_type_names(body, tyvars)
            }
            Expr::Lambda { body, .. } => self.validate_body_type_names(body, tyvars),
            Expr::Apply { args, .. } => {
                for a in args {
                    self.validate_body_type_names(a, tyvars)?;
                }
                Ok(())
            }
            Expr::ApplyExpr { callee, args, .. } => {
                self.validate_body_type_names(callee, tyvars)?;
                for a in args {
                    self.validate_body_type_names(a, tyvars)?;
                }
                Ok(())
            }
            Expr::Neg(inner, _) => self.validate_body_type_names(inner, tyvars),
            Expr::Arith { lhs, rhs, .. }
            | Expr::Cmp { lhs, rhs, .. }
            | Expr::Logic { lhs, rhs, .. } => {
                self.validate_body_type_names(lhs, tyvars)?;
                self.validate_body_type_names(rhs, tyvars)
            }
            Expr::If {
                cond, then, els, ..
            } => {
                self.validate_body_type_names(cond, tyvars)?;
                self.validate_body_type_names(then, tyvars)?;
                self.validate_body_type_names(els, tyvars)
            }
            Expr::Record(fields, _) => {
                for (_, fe) in fields {
                    self.validate_body_type_names(fe, tyvars)?;
                }
                Ok(())
            }
            Expr::ListLit(elems, _) => {
                for el in elems {
                    self.validate_body_type_names(el, tyvars)?;
                }
                Ok(())
            }
            Expr::MapLit(entries, _) => {
                for (_, ve) in entries {
                    self.validate_body_type_names(ve, tyvars)?;
                }
                Ok(())
            }
            Expr::FieldProject { record, .. } => self.validate_body_type_names(record, tyvars),
            Expr::FieldUpdate { record, value, .. } => {
                self.validate_body_type_names(record, tyvars)?;
                self.validate_body_type_names(value, tyvars)
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.validate_body_type_names(scrutinee, tyvars)?;
                for arm in arms {
                    for (g, b) in &arm.guards {
                        self.validate_body_type_names(g, tyvars)?;
                        self.validate_body_type_names(b, tyvars)?;
                    }
                }
                Ok(())
            }
            Expr::ActorParam { default, .. } => match default {
                Some(d) => self.validate_body_type_names(d, tyvars),
                None => Ok(()),
            },
            Expr::Int(..)
            | Expr::Double(..)
            | Expr::Void(..)
            | Expr::Str(..)
            | Expr::Ref(..)
            | Expr::Bool(..) => Ok(()),
        }
    }

    /// Resolve a DSL type name to a value type, following type synonyms.
    ///
    /// `None` when the name denotes no type (an undeclared uppercase name) —
    /// callers report it as `unknown type name`. A lowercase name that is not a
    /// declared type is a TYPE VARIABLE and also returns `None` here: it
    /// resolves to a fresh unification variable, which is the caller's job
    /// (see [`Self::is_type_name`], the predicate that separates the two cases).
    pub fn vty_of_name(&self, name: &str) -> Option<ValueTy> {
        match self.type_entry(name)? {
            TypeEntry::Scalar(s) => Some(s.vty),
            TypeEntry::Alias(target) => self.vty_of_name(&target),
            TypeEntry::Newtype => Some(ValueTy::Newtype(name.to_string(), vec![])),
            TypeEntry::TypeCtor { .. } => Some(ValueTy::Data(name.to_string(), vec![])),
            TypeEntry::Variable => None,
        }
    }

    /// A user-facing spelling of a value type for diagnostics: nominals by
    /// name, SCALARS by their registry spelling, and type variables by letter
    /// (`a`, `b`, …) so a partially-resolved function type still reads like a
    /// type.
    ///
    /// The scalar spellings come from [`Self::scalars`], so a name added to the
    /// registry renders under that name with no separate table to update.
    pub fn type_spelling(&self, t: &ValueTy) -> String {
        let mut names: HashMap<TypeVarId, String> = HashMap::new();
        self.type_spelling_named(t, &mut names)
    }

    /// [`Self::type_spelling`] with a shared variable-naming map, so the same
    /// variable keeps its letter throughout one rendered type.
    fn type_spelling_named(&self, t: &ValueTy, names: &mut HashMap<TypeVarId, String>) -> String {
        let var_name = |id: TypeVarId, names: &mut HashMap<TypeVarId, String>| {
            let next = names.len();
            names
                .entry(id)
                .or_insert_with(|| {
                    let letter = (b'a' + (next % 26) as u8) as char;
                    letter.to_string()
                })
                .clone()
        };
        match t {
            // A buffer is spelled by its CONSTRUCTOR and its element:
            // `FixedBuffer Double`. The bare registry name `Buffer` is an
            // alias-level spelling for a buffer value, never a diagnostic that
            // hides which element was expected.
            ValueTy::Buffer(elem) => {
                format!("FixedBuffer {}", self.type_spelling_named(elem, names))
            }
            ValueTy::Int | ValueTy::Double | ValueTy::Bool | ValueTy::String | ValueTy::Void => {
                self.scalar_name_of(t).unwrap_or_else(|| format!("{t:?}"))
            }
            // A dictionary is not a data type, so it has no nominal name — spell
            // it as the class witness it is.
            ValueTy::Dict(class) => format!("Dict {class}"),
            ValueTy::Data(n, args) | ValueTy::Newtype(n, args) | ValueTy::App(n, args) => {
                if args.is_empty() {
                    n.clone()
                } else if let Some(spelling) = self.source_spelling_named(n, args, names) {
                    // The built-in list/tuple CONSTRUCTORS are spelled with the
                    // syntax the user wrote (`[Int]`, `(a, b)`), not with the
                    // internal constructor name — a diagnostic must quote the
                    // form the signature is written in.
                    spelling
                } else {
                    let rendered: Vec<String> = args
                        .iter()
                        .map(|a| self.type_spelling_named(a, names))
                        .collect();
                    format!("{n} {}", rendered.join(" "))
                }
            }
            ValueTy::Thunk(args, rets) => {
                // A function type, rendered `A -> B` (curried chains flatten),
                // so a higher-order mismatch names what the parameter really
                // demands.
                let (cap, res) = flatten_thunk(&ValueTy::Thunk(args.clone(), rets.clone()));
                if cap.is_empty() && res.is_empty() {
                    // A bare named definition reference types as an EMPTY
                    // `Thunk` until it is applied, so it has no signature to
                    // spell out.
                    return "a function value".to_string();
                }
                let mut parts: Vec<String> = cap
                    .iter()
                    .map(|a| {
                        let s = self.type_spelling_named(a, names);
                        if s.contains(' ') && !s.starts_with('(') {
                            format!("({s})")
                        } else {
                            s
                        }
                    })
                    .collect();
                for r in &res {
                    parts.push(self.type_spelling_named(r, names));
                }
                parts.join(" -> ")
            }
            ValueTy::Var(v) | ValueTy::TyConVar(v) => var_name(*v, names),
            ValueTy::TyConApp(f, args) => {
                let head = var_name(*f, names);
                let rendered: Vec<String> = args
                    .iter()
                    .map(|a| self.type_spelling_named(a, names))
                    .collect();
                format!("{head} {}", rendered.join(" "))
            }
        }
    }

    /// The SOURCE spelling of a built-in list or tuple CONSTRUCTOR application —
    /// `[Int]` for `List Int`, `(a, b)` for `Pair a b` — or `None` when the head
    /// is not one of those or is not fully applied.
    ///
    /// Haskell 2010 §3.7 spells these two constructors with bracket/comma
    /// syntax, and the parser desugars that syntax to the nominal application
    /// (see [`crate::parser`]), so a diagnostic that printed the nominal form
    /// would quote a spelling no program contains. A partially applied head
    /// (`List`, `Pair a`) has no source spelling: the nominal form is the only
    /// thing that can name it.
    fn source_spelling_named(
        &self,
        head: &str,
        args: &[ValueTy],
        names: &mut HashMap<TypeVarId, String>,
    ) -> Option<String> {
        match (head, args) {
            ("List", [elem]) => Some(format!("[{}]", self.type_spelling_named(elem, names))),
            ("Pair", [first, second]) => Some(format!(
                "({}, {})",
                self.type_spelling_named(first, names),
                self.type_spelling_named(second, names)
            )),
            _ => None,
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
    /// (`Double`, `Int`, a data type name, a newtype name, a builtin ctor
    /// application's head like `List`/`Map`). `None` for unresolved type
    /// variables and function types — a method call or Ord-constrained key
    /// over such an argument cannot select an instance at compile time.
    ///
    /// A scalar's spelling comes from [`Self::scalars`] (the one registry).
    /// `FixedBuffer` instead of the registered `Buffer` is deliberate: a buffer
    /// value's instance head is the constructor that provides its `Num`
    /// dictionary.
    pub fn type_name_of_vty(&self, v: &ValueTy) -> Option<String> {
        match v {
            // A buffer value's nominal type is `FixedBuffer`: that is the
            // instance head that provides its `Num` dictionary. The element is
            // checked, but instance lookup is keyed by the CONSTRUCTOR (the
            // prelude declares `instance Num (FixedBuffer Double)`, a
            // monomorphic head whose key is the full applied type — see
            // [`value_type_key`]).
            ValueTy::Buffer(_) => Some("FixedBuffer".to_string()),
            ValueTy::Int | ValueTy::Double | ValueTy::Bool | ValueTy::String | ValueTy::Void => {
                self.scalar_name_of(v)
            }
            ValueTy::Data(n, _) => Some(n.clone()),
            ValueTy::Newtype(n, _) => Some(n.clone()),
            ValueTy::App(n, _) => Some(n.clone()),
            // A dictionary is not a nominal data type. Its payload is the
            // CLASS name, which names a class — never a type an instance can be
            // registered for, so it must not select an instance.
            ValueTy::Dict(_) => None,
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
                Some(ValueTy::Data(data_name, _)) => self.check_acyclic_name(&data_name, visiting),
                Some(ValueTy::Newtype(nw_name, _)) => {
                    self.check_acyclic_newtype(&nw_name, visiting)
                }
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
/// `pick: Bool -> Pair a a -> Double`). Empty when no argument mentions it (a
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
///
/// A buffer needs no arm here: a buffer VALUE is already
/// `ValueTy::Buffer(elem)` and an instance head is converted to the same shape
/// by [`TypeEnv::ctor_application`], so the two sides are comparable as they
/// stand — there is exactly one buffer representation.
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
        // A dictionary has no constructor application to normalize: it is not
        // an instance head, so it passes through unchanged (its class id is
        // already canonical).
        ValueTy::Dict(class) => ValueTy::Dict(class.clone()),
        other => other.clone(),
    }
}

/// The compile error for a type name nothing declares.
/// The compile error for a type expression whose kind is not `*` where a TYPE
/// is required: the GHC wording, naming the kind and how many arguments are
/// missing.
///
/// `Expecting one more argument to 'Pair Int'; Expected a type, but 'Pair Int'
/// has kind '* -> *'` — the count comes from the kind, so a bare `Pair` reports
/// two missing and `Pair Int` one.
fn kind_position_error(
    env: &TypeEnv,
    te: &crate::ast::TypeExpr,
    kind: Kind,
    span: Span,
) -> CompileError {
    let spelling = env.type_expr_spelling(te);
    let missing = kind.arity();
    let noun = if missing == 1 {
        "argument"
    } else {
        "arguments"
    };
    CompileError::Type {
        msg: format!(
            "expecting {missing} more {noun} to `{spelling}`; expected a type, but `{spelling}` \
             has kind `{kind}`"
        ),
        span,
    }
}

fn unknown_type_error(name: &str, span: Span) -> CompileError {
    CompileError::Type {
        msg: format!("unknown type name `{name}`"),
        span,
    }
}

/// The compile error for an unknown type name where no source span is at hand
/// (a name inside a compiler-built signature).
pub(crate) fn unknown_type_name(name: &str) -> CompileError {
    unknown_type_error(name, Span::new(0, 0))
}

/// The compile error for a buffer element the language does not have: it names
/// the requirement, which is the registry's marked element(s) — not a name list
/// duplicated here.
fn wrong_buffer_element_error(env: &TypeEnv, declared: &str, span: Span) -> CompileError {
    let allowed = env.buffer_element_names().join("`, `");
    CompileError::Type {
        msg: format!("wrong buffer element `{declared}`: the only buffer element is `{allowed}`"),
        span,
    }
}

/// Flatten a curried unary-thunk nest into its full argument and result lists.
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

/// Collect the type-variable names an instance head binds: every lowercase
/// name used as a bare type or as an applied head (`Kleisli m` binds `m`,
/// `Pair Int Int` binds nothing).
fn collect_type_app_vars(te: &crate::ast::TypeExpr, out: &mut Vec<String>) {
    use crate::ast::TypeExpr;
    match te {
        TypeExpr::TName(n) | TypeExpr::TApp(n, _) => {
            if n.chars().next().is_some_and(char::is_lowercase) && !out.iter().any(|v| v == n) {
                out.push(n.clone());
            }
            if let TypeExpr::TApp(_, args) = te {
                for a in args {
                    collect_type_app_vars(a, out);
                }
            }
        }
        TypeExpr::TFunc(args, ret) => {
            for a in args {
                collect_type_app_vars(a, out);
            }
            collect_type_app_vars(ret, out);
        }
        TypeExpr::TForall(bound, inner) => {
            for b in bound {
                if !out.iter().any(|v| v == b) {
                    out.push(b.clone());
                }
            }
            collect_type_app_vars(inner, out);
        }
        TypeExpr::TConstrained(constraints, inner) => {
            for (_, v) in constraints {
                if !out.iter().any(|x| x == v) {
                    out.push(v.clone());
                }
            }
            collect_type_app_vars(inner, out);
        }
    }
}

/// Canonical spelling of a concrete value type: `Int`, `Pair Int Int`,
/// `Pair Int (Pair Int Int)`. A nested application is parenthesized so the
/// spelling matches the parser's instance-head rendering
/// ([`TypeEnv::render_type_key`]'s counterpart in `render`).
///
/// This is the INSTANCE-REGISTRY KEY: a `ValueTy` and an `instance C T` head
/// spell a type the same way only if both go through one function, which is why
/// scalars are spelled from [`TypeEnv::scalars`] and not from a private list
/// here.
pub fn value_type_key(env: &TypeEnv, v: &ValueTy) -> Option<String> {
    fn key(env: &TypeEnv, v: &ValueTy) -> Option<String> {
        Some(match v {
            // A buffer's registry key is the CONSTRUCTOR applied to its element
            // — `FixedBuffer Double` — exactly what an `instance Num
            // (FixedBuffer Double)` head renders as. The key therefore
            // distinguishes elements: there is no `Buffer`-spelled key.
            ValueTy::Buffer(elem) => format!("FixedBuffer {}", key(env, elem)?),
            ValueTy::Int | ValueTy::Double | ValueTy::Bool | ValueTy::String | ValueTy::Void => {
                env.scalar_name_of(v)?
            }
            // A dictionary is never a registry key: it is compiler-side
            // evidence of a constraint, not a type an instance is declared for.
            ValueTy::Dict(_) => return None,
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
                        out.push_str(&key(env, a)?);
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
    key(env, v)
}

/// A stable ordinal RANK for a value type: a total order over the type FORMS.
///
/// This is the ordering CONTRACT for any code that must put value types in a
/// deterministic order (diagnostic listing, tie-breaking): rank ascending breaks
/// every tie the same way. It is not a type judgement — two distinct types share
/// a rank (`Data("A")` and `Data("B")` are both rank 6), so a rank comparison
/// can give a stable order but can never prove equality.
///
/// Every variant has an EXPLICIT rank (no wildcard), so a new type form must be
/// given one deliberately instead of silently inheriting another form's place
/// in the order. The ranks below are a fixed table: a new form is appended with
/// the next unused rank, so no established ordering is ever reshuffled.
///
/// A dictionary ranks ABOVE every data form: it is a constraint witness, not a
/// value the program's data operations see, so it must never interleave with
/// the data kinds in an ordering.
///
/// Not to be confused with the interpreter's `Value`-level `kind_rank`, which
/// orders runtime VALUES by their representation kind; this one orders TYPES by
/// their form. The two answer different questions and never share a table.
pub fn kind_rank(v: &ValueTy) -> i8 {
    match v {
        ValueTy::Int => 0,
        ValueTy::Double => 1,
        ValueTy::Bool => 2,
        ValueTy::String => 3,
        // A buffer ranks by its FORM (the element it carries does not give it a
        // different rank: rank orders forms, not types).
        ValueTy::Buffer(_) => 4,
        ValueTy::Void => 5,
        ValueTy::Data(..) => 6,
        ValueTy::Newtype(..) => 7,
        ValueTy::App(..) => 8,
        ValueTy::Thunk(..) => 9,
        ValueTy::Var(_) => 10,
        ValueTy::TyConVar(_) => 11,
        ValueTy::TyConApp(..) => 12,
        ValueTy::Dict(_) => 13,
    }
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
        let f = ValueTy::Thunk(
            vec![ValueTy::Double, ValueTy::Double],
            vec![ValueTy::Double],
        );
        assert_eq!(
            f,
            ValueTy::Thunk(
                vec![ValueTy::Double, ValueTy::Double],
                vec![ValueTy::Double]
            )
        );
        let g = ValueTy::Thunk(vec![ValueTy::Double], vec![ValueTy::Double]);
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
        let t = ValueTy::App("List".into(), vec![ValueTy::Double]);
        assert!(matches!(t, ValueTy::App(..)));
    }

    #[test]
    fn bool_string_are_leaves() {
        assert_ne!(ValueTy::Bool, ValueTy::Double);
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
        assert_eq!(env.vty_of_name("Bool"), Some(ValueTy::Bool));
        assert_eq!(env.vty_of_name("String"), Some(ValueTy::String));
        assert_eq!(env.vty_of_name("Double"), Some(ValueTy::Double));
        assert_eq!(env.vty_of_name("Int"), Some(ValueTy::Int));
        // A name nothing declares is not a type (Haskell 2010 §3.1): a
        // phantom `Data("Integer", [])` would make `f :: Integer -> Integer`
        // typecheck over a type no value can inhabit.
        assert_eq!(env.vty_of_name("Integer"), None);
        assert_eq!(env.vty_of_name("Void"), None);
        assert_eq!(env.vty_of_name("()"), Some(ValueTy::Void));
    }

    #[test]
    fn the_prelude_and_builtin_catalog_pass_their_own_name_check() {
        // The prelude declares `World`, `STState`, `STRef`, `STArray`, `ST`,
        // `IO`, `InputStream`, `OutputStream`, `Kleisli`, `Tape`, and the
        // catalog adds its record types. Every one of them MUST resolve through
        // the same registry a program's names do — otherwise the prelude would
        // be a second, privileged name space and `World` would be a type only
        // by accident.
        let env = TypeEnv::with_builtins();
        for name in [
            "World",
            "STState",
            "STRef",
            "STArray",
            "ST",
            "IO",
            "InputStream",
            "OutputStream",
            "Kleisli",
            "Tape",
            "Buffer",
            "FixedBuffer",
            "EqBand",
            "EqConfig",
        ] {
            assert!(
                env.is_type_name(name),
                "prelude/catalog type `{name}` must be a registry entry"
            );
        }
        // The prelude's Buffer instances name `FixedBuffer a` / `Tape Double`
        // where `f32` is a type VARIABLE, so the prelude's own name use is
        // exactly what the program-level rule permits.
        assert_eq!(env.type_entry("f32"), Some(TypeEntry::Variable));
        // And the prelude's declarations pass the program-level check verbatim:
        // every name it mentions is registered, including forward references
        // inside one prelude text (the whole text is registered before the
        // check would run for a program).
        assert_eq!(
            env.type_ctor_arity("Kleisli"),
            Some(3),
            "Kleisli m a b has arity 3"
        );
        assert_eq!(env.type_ctor_arity("ST"), Some(2));
        assert_eq!(env.type_ctor_arity("World"), Some(0));

        // The strongest form of the same guarantee: the prelude TEXT passes the
        // program-level name check verbatim. If a prelude declaration ever
        // mentions a name the registry does not carry, this fails — which is
        // exactly the "rejecting undeclared names broke a prelude declaration"
        // failure mode, caught here instead of at a user's first compile.
        let toks = crate::lexer::tokenize(PRELUDE).unwrap();
        let prog = crate::parser::parse(&toks, PRELUDE.as_bytes()).unwrap();
        env.validate_type_names(&prog.defs)
            .expect("the prelude must pass the same name check a program does");

        let toks = crate::lexer::tokenize(BUILTIN_FOREIGN_DECLS).unwrap();
        let prog = crate::parser::parse(&toks, BUILTIN_FOREIGN_DECLS.as_bytes()).unwrap();
        env.validate_type_names(&prog.defs)
            .expect("the builtin catalog must pass the same name check a program does");
    }

    #[test]
    fn match_ctor_pattern_rejects_extra_pattern_args() {
        let env = TypeEnv::with_builtins();
        let mut subst = Subst::default();
        // Pattern `f a b` (two type args) against `App("List", [Double])`
        // (one slot): the extra `b` slot must reject the match.
        let pat = ValueTy::App("f".into(), vec![ValueTy::Var(0), ValueTy::Var(1)]);
        let concrete = ValueTy::App("List".into(), vec![ValueTy::Double]);
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
        let concrete = ValueTy::App("List".into(), vec![ValueTy::Double]);
        assert_eq!(
            env.match_ctor_pattern("f", &pat, &concrete, &mut subst),
            Some("List".to_string())
        );
        // `f a b` (two args) against a Pair (two slots) matches.
        let pat2 = ValueTy::App("f".into(), vec![ValueTy::Var(1), ValueTy::Var(2)]);
        let concrete2 = ValueTy::App("Pair".into(), vec![ValueTy::Double, ValueTy::Int]);
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
        // Linear lambda model: no Channel/Rate — a signal is just a buffer value
        // type, which CARRIES its element.
        let sig = ValueTy::Buffer(Box::new(ValueTy::Double));
        let val = ValueTy::Data("Point".into(), vec![]);
        assert_eq!(sig, ValueTy::Buffer(Box::new(ValueTy::Double)));
        assert_ne!(sig, val);
        // The element is part of the type: a different element is a different
        // type.
        assert_ne!(sig, ValueTy::Buffer(Box::new(ValueTy::Int)));
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
    fn instance_head_arity_is_expressed_as_a_kind() {
        // The instance-head check answers in KINDS now: a head's remaining
        // argument count is its kind, applied to the arguments it binds. The
        // former `instance_head_arity` helper is gone — one answer, not two.
        let env = TypeEnv::with_builtins();
        // `Pair a` — total 2, one bound ⇒ `* -> *` remaining.
        assert_eq!(env.kind_of("Pair"), Some(Kind::of_arity(2)));
        assert_eq!(
            env.kind_of("Pair").and_then(|k| k.applied_to(1)),
            Some(Kind::of_arity(1))
        );
        // `List a` — total 1, one bound ⇒ fully applied (`*`).
        assert_eq!(
            env.kind_of("List").and_then(|k| k.applied_to(1)),
            Some(Kind::TYPE)
        );
        // Unknown constructor: no kind at all.
        assert_eq!(env.kind_of("Nope"), None);
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
            DataInfo::Record(vec![("x".to_string(), ValueTy::Double, false)]),
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
        assert!(by_ty.contains_key("Double"));
        assert!(by_ty.contains_key("Int"));
        assert!(by_ty.contains_key("Bool"));
        assert!(by_ty.contains_key("String"));
        assert!(by_ty.contains_key("Point"));
        assert!(by_ty.contains_key("List"));
        assert!(!by_ty.contains_key("Thunk"), "Thunk has no derived Ord");
    }
}
