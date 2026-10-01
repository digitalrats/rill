//! Arrow type model: a program is a block transform.
//!
//! Signal types are structured in three levels: the per-sample scalar type
//! (`Scalar`), one channel (`Channel`), and a block transform (`ArrowTy`,
//! n input channels → m output channels). Arities (channel counts) are
//! synthesized separately (see `infer.rs`) because `<:`/`:>` divisibility is
//! not expressible by unification.
//!
//! Channels carry a **rate**: [`Rate::Signal`] (a block of samples per tick,
//! the existing SIMD path) or [`Rate::Value`] (one arena value per tick).
//! [`Block`] is a back-compat alias for a signal-rate `Channel`.

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

/// The scalar (element) type of a sample.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    /// Integer.
    Int,
    /// Floating point (the runtime `T`).
    Float,
    /// Unresolved unification variable.
    Var(TypeVarId),
}

/// Wire rate: block-rate signal vs per-block value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rate {
    /// A block of samples processed per tick (existing SIMD path).
    Signal,
    /// One arena value per tick.
    Value,
}

/// Type of an arena value (per-block value channel).
#[derive(Debug, Clone, PartialEq)]
pub enum ValueTy {
    /// Integer value.
    Int,
    /// Floating point value.
    Float,
    /// Boolean value type (value track only).
    Bool,
    /// String value type (value track only).
    String,
    /// A named data record, parameterized by its type arguments.
    Data(String, Vec<ValueTy>),
    /// A newtype wrapping another value type.
    Newtype(String, Vec<ValueTy>),
    /// Builtin constructor application: `List Float`.
    App(String, Vec<ValueTy>),
    /// Function type: value-argument types and value-result types. Signal-wire
    /// arguments are positional wire-captures at the call site, not part of the
    /// type.
    Func(Vec<ValueTy>, Vec<ValueTy>),
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

/// A signal or value channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// Wire rate: signal (block of samples) or value (one per tick).
    pub rate: Rate,
    /// Scalar type of the samples (Signal rate only).
    pub elem: Scalar,
    /// Arena value type (Value rate only).
    pub vty: ValueTy,
}

impl Channel {
    /// A signal-rate channel whose samples have scalar type `elem`.
    pub fn signal(elem: Scalar) -> Self {
        Self {
            rate: Rate::Signal,
            elem,
            vty: ValueTy::Int,
        }
    }
    /// A value-rate channel carrying arena values of type `vty`.
    pub fn value(vty: ValueTy) -> Self {
        Self {
            rate: Rate::Value,
            elem: Scalar::Int,
            vty,
        }
    }
    /// The scalar element type (Signal rate) — panics for Value rate.
    pub fn scalar(&self) -> &Scalar {
        debug_assert_eq!(self.rate, Rate::Signal);
        &self.elem
    }
    /// Back-compat constructor: a Signal-rate channel with scalar `elem`.
    pub fn new(elem: Scalar) -> Self {
        Self::signal(elem)
    }
}

/// Back-compat alias for [`Channel`] (a signal-rate channel).
pub type Block = Channel;

/// A block transform: n input channels → m output channels.
///
/// The vector *lengths* are the arities (channel counts). During inference we
/// usually know the arities as concrete integers; unification touches the
/// `Block::elem` scalars, and value-type unification (`unify_value`) lands with
/// value channels (Task 6+).
#[derive(Debug, Clone, PartialEq)]
pub struct ArrowTy {
    /// Scalar type of each input channel (len = input arity).
    pub ins: Vec<Block>,
    /// Scalar type of each output channel (len = output arity).
    pub outs: Vec<Block>,
}

impl ArrowTy {
    /// A (n_in → n_out) transform where every channel has the same scalar `s`.
    pub fn uniform(n_in: usize, n_out: usize, s: Scalar) -> ArrowTy {
        ArrowTy {
            ins: vec![Block::new(s.clone()); n_in],
            outs: vec![Block::new(s); n_out],
        }
    }
    /// A value channel (per-block value): 0 inputs → 1 output.
    pub fn value_channel(vty: ValueTy) -> ArrowTy {
        ArrowTy {
            ins: vec![],
            outs: vec![Channel::value(vty)],
        }
    }
    /// Input arity (channel count).
    pub fn arity_in(&self) -> usize {
        self.ins.len()
    }
    /// Output arity (channel count).
    pub fn arity_out(&self) -> usize {
        self.outs.len()
    }
}

/// A polymorphic type scheme `∀ vars. ty`.
///
/// `lam_count` is the number of λ-parameters for this definition
/// (user-defined function arguments — distinct from signal port inputs).
/// These appear as the **first** `lam_count` elements of `ty.ins`.
/// `ty.ins[lam_count..]` are signal port inputs (from `_` references in the body).
#[derive(Debug, Clone, PartialEq)]
pub struct Scheme {
    /// Number of λ-parameters.
    pub lam_count: usize,
    /// Quantified type variables.
    pub vars: Vec<TypeVarId>,
    /// The generalized diagram type.
    pub ty: ArrowTy,
}

/// A substitution mapping type variables to scalars (signal element types)
/// and value types (arena value types).
#[derive(Debug, Clone, Default)]
pub struct Subst {
    /// The scalar mapping (signal element types).
    pub map: HashMap<TypeVarId, Scalar>,
    /// The value-type mapping (arena value types). Value-type variables
    /// resolve structurally: unifying `Var(v)` with a concrete `ValueTy`
    /// (e.g. a `Func` signature for a lambda parameter used as a callee)
    /// records the binding here, so HOF parameter types stay concrete by the
    /// time lowering needs them (record projection needs the Data type, a
    /// function parameter needs its Func signature).
    pub value_map: HashMap<TypeVarId, ValueTy>,
}

/// Shape of a declared data type.
#[derive(Debug, Clone)]
pub enum DataInfo {
    /// A product type: field name → value type.
    Record(Vec<(String, ValueTy)>),
    /// A sum type: constructor name → payload value types.
    Sum(Vec<(String, Vec<ValueTy>)>),
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
    /// The concrete type name bound to the class variable.
    pub ty: String,
    /// Constraint list: (class, type var), e.g. `(Monad, "m")`.
    pub constraints: Vec<(String, String)>,
    /// Type-constructor args bound by the instance head (partial application):
    /// `Kleisli m` → `["m"]` (arity 3 total, 1 bound ⇒ 2 remaining).
    pub head_args: Vec<String>,
    /// Method bodies: method name → (parameter bindings, body).
    pub methods: HashMap<String, (Vec<String>, Expr)>,
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
}

/// Built-in category-theory typeclasses, declared in rill-lang itself and
/// registered by [`TypeEnv::with_builtins`]. Users never redeclare them but may
/// add their own instances. `instance Monad T` auto-derives `Applicative T` and
/// `Functor T` (see [`TypeEnv::derive_superclass_instances`]).
pub(crate) const CATEGORY_PRELUDE: &str = r#"
typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
typeclass Applicative f where { pure: a -> f a; ap: f (a -> b) -> f a -> f b; }
typeclass Monad m where { return: a -> m a; bind: m a -> (a -> m b) -> m b; }
typeclass Monoid m where { mempty: m; mappend: m -> m -> m; }

instance Monoid Float where { mempty = 0.0; mappend a b = a + b; }
instance Monoid String where { mempty = ""; mappend a b = concat_string a b; }
instance Monoid List where { mempty = list; mappend a b = append_list a b; }

instance Functor List where { fmap g xs = map g xs; }
instance Functor Maybe where {
    fmap g m = match m of { Nothing => Nothing; Just x => Just (g x); };
}

instance Monad Maybe where {
    return x = Just x;
    bind m f = match m of { Nothing => Nothing; Just x => f x; };
}
instance Monad List where {
    return x = cons x (list);
    bind xs f = concat_map f xs;
}

main = _;
"#;

/// Derived instance bodies for the superclass chain, written in rill-lang.
/// `instance Monad T` ⇒ `Applicative T` (`pure` = `return`, `ap` via `bind`) and
/// `Functor T` (`fmap` via `bind`); `instance Applicative T` ⇒ `Functor T`
/// (`fmap` via `ap`/`pure`). Referenced by [`TypeEnv::derive_superclass_instances`].
const APPLICATIVE_FROM_MONAD: &str =
    "pure x = return x; ap mf mx = bind mf (fn f -> bind mx (fn x -> return (f x)));";
const FUNCTOR_FROM_MONAD: &str = "fmap g x = bind x (fn y -> return (g y));";
const FUNCTOR_FROM_APPLICATIVE: &str = "fmap g x = ap (pure g) x;";

/// Parse an `instance C T where { <template> }` source fragment and build the
/// [`InstanceInfo`]. Used by superclass auto-derivation.
fn instance_from_template(ty: &str, class: &str, bodies: &str) -> InstanceInfo {
    let src = format!("instance {class} {ty} where {{ {bodies} }}; main = _;");
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
    let Def::Instance { method_bodies, .. } = inst else {
        unreachable!("derived instance template produced an instance")
    };
    let mut methods: HashMap<String, (Vec<String>, Expr)> = HashMap::new();
    for (mname, params, body) in method_bodies {
        let bindings = params.iter().map(|p| p.name.clone()).collect();
        methods.insert(mname.clone(), (bindings, body));
    }
    InstanceInfo {
        class: class.to_string(),
        ty: ty.to_string(),
        constraints: vec![],
        head_args: vec![],
        methods,
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
                    ("Just".to_string(), vec![ValueTy::Var(1)]),
                    ("Nothing".to_string(), vec![]),
                ]),
            ),
            (
                "Pair".to_string(),
                DataInfo::Record(vec![
                    ("first".to_string(), ValueTy::Var(1)),
                    ("second".to_string(), ValueTy::Var(2)),
                ]),
            ),
            (
                "Either".to_string(),
                DataInfo::Sum(vec![
                    ("Left".to_string(), vec![ValueTy::Var(1)]),
                    ("Right".to_string(), vec![ValueTy::Var(2)]),
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
        // Category-theory prelude: declared in rill-lang itself so the classes
        // and instances are first-class entities. A parse failure here is a
        // compiler bug (the constant is fixed) — assert loudly.
        let toks = crate::lexer::tokenize(CATEGORY_PRELUDE);
        debug_assert!(toks.is_ok(), "category prelude must lex");
        let program = crate::parser::parse(&toks.ok().unwrap(), CATEGORY_PRELUDE.as_bytes());
        debug_assert!(program.is_ok(), "category prelude must parse");
        env.register_decls(&program.ok().unwrap().defs);
        env.derive_superclass_instances();
        env
    }

    /// Register declaration defs (`typeclass`/`instance`) into the env.
    /// Extracted from inference phase 1 so the category prelude (parsed in
    /// [`Self::with_builtins`]) and user declarations share one registration
    /// path.
    pub(crate) fn register_decls(&mut self, defs: &[Def]) {
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
                            ty: ty.clone(),
                            constraints: constraints.clone(),
                            head_args: head_args.clone(),
                            methods,
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
    /// standard definitions, parsed from templates.
    pub(crate) fn derive_superclass_instances(&mut self) {
        let monad_tys = self
            .instances
            .get("Monad")
            .map(|m| m.keys().cloned().collect())
            .unwrap_or(vec![]);
        let applicative_tys = self
            .instances
            .get("Applicative")
            .map(|m| m.keys().cloned().collect())
            .unwrap_or(vec![]);
        for t in monad_tys {
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
                        instance_from_template(t.as_str(), "Applicative", APPLICATIVE_FROM_MONAD),
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
                        instance_from_template(t.as_str(), "Functor", FUNCTOR_FROM_MONAD),
                    );
            }
        }
        for t in applicative_tys {
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
                        instance_from_template(t.as_str(), "Functor", FUNCTOR_FROM_APPLICATIVE),
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
            crate::ast::TypeExpr::TFunc(args, _) => args
                .iter()
                .position(|a| matches!(a, crate::ast::TypeExpr::TApp(h, _) if h == class_var)),
            _ => None,
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
                    ty: n.clone(),
                    constraints: vec![], // derived marker — no constraint list
                    head_args: vec![],   // derived marker — full application
                    methods: HashMap::new(), // derived marker — empty body
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
                for (_, t) in fields {
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
                    for t in payload {
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
            None => {}
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

impl Subst {
    /// Follow the substitution chain for a single scalar to its representative.
    pub fn resolve_scalar(&self, s: &Scalar) -> Scalar {
        match s {
            Scalar::Var(v) => match self.map.get(v) {
                Some(inner) => self.resolve_scalar(inner),
                None => s.clone(),
            },
            _ => s.clone(),
        }
    }
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
            ValueTy::Func(args, rets) => ValueTy::Func(
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
                    ValueTy::TyConVar(_) => ValueTy::TyConApp(*f, resolved_args),
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
    /// Signal-rate channels resolve their scalar element type; value-rate
    /// channels resolve their value type (lambda-parameter variables become
    /// the concrete types bound by call-site unification).
    pub fn apply(&self, t: &ArrowTy) -> ArrowTy {
        let block = |b: &Channel| match b.rate {
            Rate::Value => Channel::value(self.resolve_value(&b.vty)),
            Rate::Signal => Channel::signal(self.resolve_scalar(&b.elem)),
        };
        ArrowTy {
            ins: t.ins.iter().map(&block).collect(),
            outs: t.outs.iter().map(&block).collect(),
        }
    }
}

#[cfg(test)]
mod funcsig_tests {
    use super::*;

    #[test]
    fn func_signature_carries_arg_and_result_types() {
        let f = ValueTy::Func(vec![ValueTy::Float, ValueTy::Float], vec![ValueTy::Float]);
        assert_eq!(
            f,
            ValueTy::Func(vec![ValueTy::Float, ValueTy::Float], vec![ValueTy::Float])
        );
        let g = ValueTy::Func(vec![ValueTy::Float], vec![ValueTy::Float]);
        assert_ne!(f, g);
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
    fn channel_rates_are_distinct() {
        let sig = Channel::signal(Scalar::Float);
        let val = Channel::value(ValueTy::Data("Point".into(), vec![]));
        assert_eq!(sig.rate, Rate::Signal);
        assert_eq!(val.rate, Rate::Value);
        assert_eq!(sig.vty, ValueTy::Int);
    }

    #[test]
    fn apply_preserves_value_channels() {
        let s = Subst::default();
        let t = ArrowTy {
            ins: vec![Channel::value(ValueTy::Int)],
            outs: vec![],
        };
        let r = s.apply(&t);
        assert_eq!(r.ins[0].rate, Rate::Value);
        assert_eq!(r.ins[0].vty, ValueTy::Int);
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
        assert!(env.ctor_arity("Nope").is_none());
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
            DataInfo::Record(vec![("x".to_string(), ValueTy::Float)]),
        );
        env.data_types.insert(
            "List".to_string(),
            DataInfo::Sum(vec![
                ("Cons".to_string(), vec![ValueTy::Var(1), ValueTy::Var(2)]),
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
        assert!(!by_ty.contains_key("Func"), "Func has no derived Ord");
    }
}
