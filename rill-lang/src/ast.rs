//! Abstract syntax tree produced by the parser.

use crate::error::Span;
use crate::mode::Mode;
use num_bigint::BigInt;
use num_rational::BigRational;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A type name reference in a declaration (`Double`, `Hz`, `Point`, ...).
pub type TypeName = String;

/// A type expression in a declaration: concrete names, type variables,
/// constructor application, function types, and capacity literals.
/// A type-expression node in a declaration signature.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TypeExpr {
    /// A concrete type name or a type variable.
    TName(String),
    /// A constructor application: `f a`, `List Double`.
    TApp(String, Vec<TypeExpr>),
    /// A curried function type: `a -> b`.
    TFunc(Vec<TypeExpr>, Box<TypeExpr>),
    /// Explicit universal quantification: `forall a b. T`.
    TForall(Vec<String>, Box<TypeExpr>),
    /// A class-constrained type: `C v => T`, `(C v, C w) => T`. Each
    /// constraint is a `(class name, type variable)` pair and constrains the
    /// type VARIABLE it names; the body type follows. Generic over the class
    /// (`Ix` is not special).
    TConstrained(Vec<(String, String)>, Box<TypeExpr>),
}

impl TypeExpr {
    /// The number of function arguments (0 for a non-function type).
    pub fn arg_count(&self) -> usize {
        match self {
            TypeExpr::TFunc(args, _) => args.len(),
            TypeExpr::TForall(_, inner) => inner.arg_count(),
            TypeExpr::TConstrained(_, inner) => inner.arg_count(),
            _ => 0,
        }
    }
    /// The argument type expressions (empty for a non-function type).
    pub fn arg_types(&self) -> &[TypeExpr] {
        match self {
            TypeExpr::TFunc(args, _) => args,
            TypeExpr::TForall(_, inner) => inner.arg_types(),
            TypeExpr::TConstrained(_, inner) => inner.arg_types(),
            _ => &[],
        }
    }
    /// The number of arguments a method signature accepts: the arity of a
    /// `TFunc`, `1` for a bare (class-var selector) signature, delegating
    /// through a `TForall` binder or a class context.
    pub fn sig_arg_count(&self) -> usize {
        match self {
            TypeExpr::TFunc(args, _) => args.len(),
            TypeExpr::TForall(_, inner) => inner.sig_arg_count(),
            TypeExpr::TConstrained(_, inner) => inner.sig_arg_count(),
            _ => 1,
        }
    }
    /// Whether this signature carries a class context (`C v => T`), looking
    /// through an explicit `forall` binder (`forall v. C v => T`).
    ///
    /// This is a SHAPE test, not a typing rule: a constrained definition's body
    /// is type-checked once, under its given constraints (design §3.0). It is
    /// used to recognize the declarations whose class variable stays ABSTRACT
    /// until a call site supplies the dictionary.
    pub fn is_constrained(&self) -> bool {
        match self {
            TypeExpr::TConstrained(..) => true,
            TypeExpr::TForall(_, inner) => inner.is_constrained(),
            _ => false,
        }
    }
}

/// A match pattern. The case convention: an uppercase-initial identifier is a
/// constructor, a lowercase-initial identifier is a variable binding, `_` is a
/// wildcard.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Pattern {
    /// Binds the whole matched value to a name (`x`).
    Var(String),
    /// Matches anything, binds nothing (`_`).
    Wild,
    /// Integer literal, at arbitrary precision. A case-pattern integer is
    /// matched against the scrutinee's `Num` type, so lowering converts it with
    /// the same `fromInteger` conversion the expression form uses.
    IntegerLit(BigInt),
    /// Fractional literal, at arbitrary precision: the EXACT value the source
    /// digits denote, plus the source spelling.
    ///
    /// Haskell 2010 §6.4.1 makes a fractional literal a `fromRational` of the
    /// exact decimal fraction, so `0.1` is `1/10` and NOT the nearest
    /// `Double` (`3602879701896397/36028797018963968`). The payload is built
    /// from the digits, and the text is what `render` reproduces. The ratio is
    /// boxed for the same size reason as the expression's variant.
    FractionalLit(Box<BigRational>, String),
    /// String literal.
    LitStr(String),
    /// Constructor application with (possibly nested) argument patterns.
    ///
    /// `True` / `False` are ordinary constructor patterns of the Prelude's
    /// `data Bool = False | True`, so this is the ONLY pattern form a boolean
    /// match uses — there is no separate boolean-literal pattern.
    Ctor(String, Vec<Pattern>),
    /// Bang pattern `!pat` — force the matched value to WHNF before matching
    /// the inner pattern (GHC-style extension; no deep strictness).
    Bang(Box<Pattern>),
}

impl Pattern {
    /// This pattern with any leading bang markers stripped: `!x` → `x`,
    /// `!!x` → `x`. Shape-based analyses (constructor/variable tests,
    /// exhaustiveness) look through the markers; lowering re-emits the force.
    pub fn strip_bang(&self) -> &Pattern {
        match self {
            Pattern::Bang(inner) => inner.strip_bang(),
            other => other,
        }
    }
}

/// One `case` arm: a pattern, then a sequence of `(guard, body)` alternatives.
///
/// A guard-less alternative (`pattern -> body`) carries `None` — Haskell 2010
/// §3.13 gives it no guard expression at all (GHC's `GRHS` holds an empty guard
/// list), so there is no "always true" sentinel a user could shadow. A guarded
/// alternative (`pattern | g -> body`) carries `Some(g)`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MatchArm {
    /// The pattern tested against the scrutinee.
    pub pattern: Pattern,
    /// `(guard_expr, body_expr)` alternatives, evaluated in order; the first
    /// whose guard holds runs its body. `None` is an unconditional alternative.
    pub guards: Vec<(Option<Expr>, Expr)>,
    /// Span of the whole arm (for diagnostics).
    pub span: Span,
}

/// A rill-lang expression node.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Expr {
    /// Integer literal, at arbitrary precision — `123456789012345678901234567890`
    /// is a valid literal. The value IS the Haskell 2010 `Integer` the Report's
    /// literal denotes (§6.4.1), so the node names no *type*: which `Num` type
    /// it lands on is decided at lowerering by the `fromInteger` application it
    /// becomes (`fromInteger (42 :: Integer)`), and by unification with the
    /// position's expected type.
    IntegerLit(BigInt, Span),
    /// Fractional literal, at arbitrary precision: the EXACT value the source
    /// digits denote, plus the source spelling.
    ///
    /// Haskell 2010 §6.4.1: a fractional literal is `fromRational` applied to
    /// the exact decimal fraction its digits spell, so `0.1` is `1 % 10`, not
    /// the nearest `Double` (`3602879701896397/36028797018963968`). The
    /// payload is built from the DIGITS (never from an `f64`), and the text is
    /// kept beside it so `render` reproduces the source spelling (`1e3`
    /// stays `1e3`).
    ///
    /// The ratio is BOXED so this variant stays within the AST's existing
    /// size: a bare `BigRational` is 64 bytes, which pushed `Expr` from 72 to
    /// 104 bytes and overflowed the recursion stack of the lowering pass on a
    /// nested program (the same ceiling earlier stages hit).
    FractionalLit(Box<BigRational>, String, Span),
    /// Imaginary literal is gone (Haskell 2010 has no imaginary literal).
    /// Unit literal `()` — the sole inhabitant of `ValueTy::Void`.
    Void(Span),
    /// String literal, e.g. `"cutoff"`.
    Str(String, Span),
    /// A HANDLE to a name — a definition or a bound parameter.
    ///
    /// This is what GHC Core calls a `Var`: the thing β-reduction substitutes
    /// for, a CAF read, a captured binding. It is NOT an application, and the
    /// distinction is GHC's own (`-ddump-ds`, measured):
    ///
    /// ```haskell
    /// g = \y -> y     -- definition: a lambda
    /// h = g (IS 1#)   -- application: App(Var g, arg)
    /// k = g           -- reference: Var g, with NO App at all
    /// ```
    ///
    /// Haskell 2010 has no zero-argument application: `f` and `f ()` differ by an
    /// ARGUMENT, and `()` is an ordinary value (the unit constructor), not the
    /// absence of one. A CONSTANT is not an application either — `x = 42` is a
    /// CAF, `x = I# 42#` in Core and `IS! [42#]` in STG.
    ///
    /// [`Expr::Apply`] therefore always carries at least one argument; an
    /// `Apply` with an empty `args` is not a "reference", it is the malformed
    /// state this node exists to prevent.
    Ref(String, Span),
    /// Application `name arg1 arg2` — `App` in GHC Core. `args` is NEVER empty
    /// (see [`Expr::Ref`]): a bare name is a reference, not an application.
    Apply {
        /// Function name.
        name: String,
        /// Argument expressions — always at least one.
        args: Vec<Expr>,
        /// Full span of the application.
        span: Span,
    },
    /// Application of an arbitrary expression (not just a name) to arguments:
    /// `(k.unKleisli) p.first`, `(fn x -> x) 1.0`.
    ApplyExpr {
        /// The callee expression (a closure-valued projection, lambda, …).
        callee: Box<Expr>,
        /// Argument expressions.
        args: Vec<Expr>,
        /// Full span.
        span: Span,
    },
    /// Unary negation `-expr`.
    Neg(Box<Expr>, Span),
    /// `let defs in body` — expression-level mutually-recursive bindings.
    Let {
        /// Definitions (may contain Anchors and Locals).
        defs: Vec<Def>,
        /// The expression these bindings are visible in.
        body: Box<Expr>,
        /// Full span.
        span: Span,
    },
    /// Record literal, e.g. `{ channels: 3, gain: 0.8 }`.
    Record(Vec<(String, Expr)>, Span),
    /// Late-binding actor parameter: `?name` or `?name=default`.
    ActorParam {
        /// Parameter name (without `?` prefix).
        name: String,
        /// Optional default value expression.
        default: Option<Box<Expr>>,
        /// Source span.
        span: Span,
    },
    /// Field projection `record.field`.
    FieldProject {
        /// Record expression.
        record: Box<Expr>,
        /// Field name.
        field: String,
        /// Span.
        span: Span,
    },
    /// COW field mutation `record.field := value`.
    FieldUpdate {
        /// Record expression.
        record: Box<Expr>,
        /// Field name.
        field: String,
        /// New value expression.
        value: Box<Expr>,
        /// Span.
        span: Span,
    },
    /// Pattern matching over a value (a sum or a scalar).
    Match {
        /// Scrutinee expression.
        scrutinee: Box<Expr>,
        /// Arms.
        arms: Vec<MatchArm>,
        /// Span.
        span: Span,
    },
    /// Conditional expression `if cond then a else b`.
    If {
        /// Condition (must be a Bool value).
        cond: Box<Expr>,
        /// Taken when the condition is true.
        then: Box<Expr>,
        /// Taken when the condition is false.
        els: Box<Expr>,
        /// Span.
        span: Span,
    },
    /// Lambda literal `fn p1 p2 ... -> body`.
    Lambda {
        /// Parameters.
        params: Vec<Param>,
        /// Body expression.
        body: Box<Expr>,
        /// Span.
        span: Span,
    },
    /// Expression type annotation `expr :: TypeExpr` (Haskell 2010 §3.12).
    ///
    /// A type-level construct: inference pins the inner expression's type to
    /// the annotation, and lowering erases the node (the inner expression
    /// lowers unchanged). Desugaring keeps it, because inference runs after
    /// desugar — erasing it there would discard the only record of the
    /// annotation before anything reads it.
    Annot {
        /// The annotated expression.
        expr: Box<Expr>,
        /// The declared type.
        ty: TypeExpr,
        /// Full span, from the expression through the type.
        span: Span,
    },
    /// List literal `[e1, e2]`.
    ListLit(Vec<Expr>, Span),
    /// Map literal `{ "k": v, ... }` (string keys).
    MapLit(Vec<(String, Expr)>, Span),
    /// An application of one of the PRELUDE's internal kernels, bound at
    /// PARSE time rather than resolved by name in a later pass.
    ///
    /// This node exists because a reserved SPELLING is not enough. `__gcd`,
    /// `__ratio_percent` and `__fault` were compared literally in `infer` and
    /// in `lower`, but `reduce` runs BETWEEN them (`lib.rs`), and a user
    /// definition of a kernel spelling that carries a type signature was
    /// β-reduced into the Prelude's body before lowering ever saw the name:
    ///
    /// ```text
    /// __gcd :: Integer -> Integer -> Integer; __gcd a b = 99;
    /// gcd 12 18   --  expected 6, answered 99
    /// ```
    ///
    /// The same defect class had already been fixed twice by adding a third
    /// name comparison (Task 3 rounds 2 and 3). A name is the wrong carrier:
    /// every pass that resolves names has to know the list, and every new pass
    /// can forget it. Binding the operation to a NODE removes the carrier —
    /// there is no `Ref` for a pass to look up, substitute, or inline, so
    /// `infer`, `reduce` and `lower` are all interception-proof by
    /// construction, and a future pass inherits that without knowing the
    /// spellings exist.
    ///
    /// The conversion happens ONCE, in the parser's currying walk (see
    /// `parser::curry_expr`), which is also how the PRELUDE text itself gets
    /// its nodes. That is the only place the reserved spellings appear as
    /// strings.
    Kernel {
        /// Which kernel is applied.
        kernel: Kernel,
        /// Arguments, in source order. Arity is checked by `infer`, not here,
        /// so the diagnostic for a wrong arity is the same one the name-based
        /// path produced.
        args: Vec<Expr>,
        /// Full span of the application.
        span: Span,
    },
}

/// One of the PRELUDE's internal kernels — the operations that must not be
/// reachable through a name a program can redefine.
///
/// These are the bodies behind surface bindings that Haskell 2010 requires to
/// stay SHADOWABLE: `(%)` and `gcd` are Prelude functions, and `Ratio`'s
/// arithmetic normalises through them. A program may replace `%` or `gcd`;
/// it must not thereby replace what `a / b` computes, nor what `recip (0/1)`
/// does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Kernel {
    /// The Prelude-internal bottom (`__fault message`).
    Fault,
    /// `Ratio` normalisation (`__ratio_percent n d`).
    RatioPercent,
    /// Arbitrary-precision integer gcd (`__gcd a b`).
    Gcd,
}

impl Kernel {
    /// The reserved SOURCE spelling of this kernel.
    ///
    /// Used for rendering only — resolution no longer goes through a name
    /// (see [`Expr::Kernel`]). Kept in step with the `ir` constants by
    /// [`Kernel::from_reserved_spelling`], the ONE mapping in the compiler.
    pub fn spelling(self) -> &'static str {
        match self {
            Kernel::Fault => crate::ir::VALUE_FAULT_PRIMITIVE,
            Kernel::RatioPercent => crate::ir::RATIO_PERCENT_PRIMITIVE,
            Kernel::Gcd => crate::ir::GCD_PRIMITIVE,
        }
    }

    /// The kernel a reserved spelling denotes, or `None` for any other name.
    ///
    /// The ONLY place the reserved spellings are recognised. Every pass sees
    /// [`Expr::Kernel`] instead of a name, so this list cannot be forgotten by
    /// a later pass.
    pub fn from_reserved_spelling(name: &str) -> Option<Kernel> {
        match name {
            crate::ir::VALUE_FAULT_PRIMITIVE => Some(Kernel::Fault),
            crate::ir::RATIO_PERCENT_PRIMITIVE => Some(Kernel::RatioPercent),
            crate::ir::GCD_PRIMITIVE => Some(Kernel::Gcd),
            _ => None,
        }
    }
}

impl Expr {
    /// Whether this node IS an integer literal — the syntactic class whose
    /// runtime representation (`Int`, `Integer` or `Double`) is decided by its
    /// expected type alone.
    ///
    /// `-3` counts: the parser folds the sign into an [`Expr::Neg`] over
    /// [`Expr::IntegerLit`], and Haskell 2010 §3.8's negative literal is one literal,
    /// not an application of `negate` to a literal. This is the ONE definition
    /// of "integer literal" in the compiler — `reduce`'s annotation carrier,
    /// inference's widening rule and the `fromInteger` typing of a literal all
    /// consult it, so they cannot disagree about what a literal is (a
    /// disagreement is how `f 2` was accepted at `Double` while `f (-2)` was
    /// rejected).
    ///
    /// A parenthesised/annotated literal (`(2 :: Integer)`, `-3 :: Integer`) is
    /// NOT included: [`Expr::Annot`] supplies its own expected type at both the
    /// inference and lowering sites, which is stronger than the widening rule.
    pub fn is_int_literal(&self) -> bool {
        match self {
            Expr::IntegerLit(_, _) => true,
            Expr::Neg(inner, _) => inner.is_int_literal(),
            _ => false,
        }
    }

    /// The arbitrary-precision value of this node when it is an integer literal
    /// ([`Self::is_int_literal`]), with the sign already applied.
    ///
    /// This is the shared constant fold: `Expr::Neg` negates in `BigInt`, so
    /// `-9223372036854775808` is the exact value `-(9223372036854775808)` and
    /// only the FINAL `Int`/`Double` conversion truncates or rounds.
    pub fn int_literal_value(&self) -> Option<BigInt> {
        match self {
            Expr::IntegerLit(v, _) => Some(v.clone()),
            Expr::Neg(inner, _) => inner.int_literal_value().map(|v| -v),
            _ => None,
        }
    }

    /// The source span of this node.
    pub fn span(&self) -> Span {
        match self {
            Expr::IntegerLit(_, s)
            | Expr::FractionalLit(_, _, s)
            | Expr::Void(s)
            | Expr::Str(_, s)
            | Expr::Ref(_, s)
            | Expr::Neg(_, s) => *s,
            Expr::Apply { span, .. }
            | Expr::ApplyExpr { span, .. }
            | Expr::Kernel { span, .. }
            | Expr::Let { span, .. }
            | Expr::Record(_, span)
            | Expr::ActorParam { span, .. }
            | Expr::FieldProject { span, .. }
            | Expr::FieldUpdate { span, .. }
            | Expr::Match { span, .. }
            | Expr::If { span, .. }
            | Expr::Lambda { span, .. }
            | Expr::Annot { span, .. }
            | Expr::ListLit(_, span)
            | Expr::MapLit(_, span) => *span,
        }
    }
}

/// A parameter declaration.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Param {
    /// Parameter name.
    pub name: String,
    /// Source span.
    pub span: Span,
    /// Whether this is a bang parameter (`f !x = …`, `\ !x -> …`): the
    /// argument is forced to WHNF on application entry.
    #[cfg_attr(feature = "serde", serde(default))]
    pub bang: bool,
}

impl Param {
    /// Wrap `body` in one unary λ per parameter, curried left to right:
    /// `[x, !y]` over `e` is `\x -> (\ !y -> e)`.
    ///
    /// This is Haskell 2010 §4.4.3.1's shortening read backwards — the ONE
    /// place a parameter list becomes the body it denotes, so no caller can
    /// invent a second place to keep parameters.
    pub fn lambda_wrap(params: &[Param], body: Expr) -> Expr {
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
}

/// One equation of a function-definition group, as written in the source.
///
/// This is the intermediate form the parser produces for the Haskell 2010
/// equation surface (guards, argument patterns). It is consumed by
/// [`crate::desugar`], which rewrites the whole group into a plain
/// [`Def::Eval`]; it never reaches inference or lowering.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Equation {
    /// The argument patterns, one per formal parameter (`f 0.0 y = …` has two).
    pub patterns: Vec<Pattern>,
    /// The `(guard, body)` alternatives in source order. A plain `= body`
    /// equation is one alternative with NO guard (`None`); a guarded equation
    /// (`| g1 = e1 | g2 = e2`) carries one alternative per guard (Haskell 2010
    /// §4.4.3). Never empty.
    pub guards: Vec<(Option<Expr>, Expr)>,
    /// The equation's own `where` block (empty when absent).
    pub where_defs: Vec<Def>,
    /// Span of the whole equation.
    pub span: Span,
}

/// A definition — top-level, `where`-block, or `let`-block.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Def {
    /// `name p1 p2 = body` — one definition form. `sig` is an optional `::`
    /// type annotation (inference-only hint/constraint). Named so it reads as
    /// «evaluable definition» in contrast to the `is_decl()` declarations.
    /// Top-level, `where`-block, and `let`-block bindings are all this one form.
    ///
    /// There is NO parameter list here. Haskell 2010 §4.4.3.1: `f x = e` is
    /// SHORTENING for `f = \x -> e`, so a definition binds a NAME to an
    /// EXPRESSION and the only carrier of parameters is the body's leading
    /// [`Expr::Lambda`] nest (one argument each, curried). A constant is the
    /// arity-0 case — its body builds no λ. [`Def::arity`] is that one arity.
    Eval {
        /// Definition name.
        name: String,
        /// Right-hand side — the definition's whole value, parameters included
        /// as its leading λ-nest.
        body: Expr,
        /// Optional where-block definitions.
        where_defs: Vec<Def>,
        /// Optional `:: TypeExpr` signature annotation.
        sig: Option<TypeExpr>,
        /// Bang binding (`let !x = e`): force the right-hand side to WHNF at
        /// the binding point (GHC-style extension; no deep strictness).
        #[cfg_attr(feature = "serde", serde(default))]
        bang: bool,
        /// True when the source WROTE a parameter list (`f x = e`, or
        /// `f x y = e` before currying).
        ///
        /// This is the Report's `funlhs` versus `pat` distinction, and Haskell
        /// 2010 §4.4.3.1 says it matters even though the two spellings denote
        /// the same value: §4.5.5's monomorphism restriction applies to a
        /// SIMPLE PATTERN BINDING — one with no parameter list — and not to a
        /// function binding. GHC 9.10.3, measured on a COMPILED module (GHCi's
        /// `ExtendedDefaultRules` default the display and hide the difference):
        ///
        /// ```haskell
        /// double = \x -> x * 2.0   -- pat    → Fractional a => a -> a MONOMORPHIC
        /// double x = x * 2.0       -- funlhs → Fractional a => a -> a GENERALISED
        /// add    = \a -> \b -> a + b   -- pat    → monomorphic
        /// add    = \a b -> a + b       -- (same)
        /// add a b = a + b              -- funlhs → (Num a) => a -> a -> a
        /// ```
        ///
        /// Using the pattern-bound `double` at two incompatible types is a TYPE
        /// ERROR; using the function-bound one at the same two types is legal.
        /// Both facts are load-bearing, so the classification is recorded.
        ///
        /// It is NOT a second carrier of arity: arity stays [`Def::arity`], the
        /// body's λ-nest depth. This is a fact about the SOURCE SPELLING, which
        /// the Report makes semantically relevant.
        #[cfg_attr(feature = "serde", serde(default))]
        wrote_params: bool,
        /// Span of the whole definition.
        span: Span,
    },
    /// `name p1 p2 | g = e | …` — a Haskell 2010 function equation, in the
    /// intermediate form the parser records for the whole equation surface
    /// (argument patterns, guards, per-equation `where`). Consecutive
    /// equations of one name form a group; [`crate::desugar`] rewrites the
    /// group into a single [`Def::Eval`] whose body is a `case`-shaped
    /// [`Expr::Match`]. This variant never reaches inference or lowering.
    Equations {
        /// Definition name.
        name: String,
        /// The equations of this name, in source order (at least one).
        equations: Vec<Equation>,
        /// Optional `:: TypeExpr` signature annotation written inline
        /// (`f x :: Double -> Double = …`). A standalone signature
        /// ([`Def::Signature`]) is attached to its group by
        /// [`crate::desugar`].
        sig: Option<TypeExpr>,
        /// Span of the whole equation.
        span: Span,
    },
    /// `f :: T;` — a standalone Haskell 2010 type signature. It carries no body
    /// of its own; [`crate::desugar`] attaches it to the binding(s) of the same
    /// name that follow it. Never reaches inference or lowering.
    Signature {
        /// The signed name.
        name: String,
        /// The declared type.
        sig: TypeExpr,
        /// Span of the declaration.
        span: Span,
    },
    /// `data Name tv1 tv2 = { f1: T1, f2: T2 }` — product type.
    Data {
        /// Type name.
        name: String,
        /// Type parameters (e.g. `a` in `data Box a`).
        tyvars: Vec<String>,
        /// Fields: (field name, type expression, strictness flag). A strict
        /// field (`{ x: !Double }`) forces its argument to WHNF at construction
        /// (Haskell 2010 strictness flag).
        fields: Vec<(String, TypeExpr, bool)>,
        /// Span.
        span: Span,
    },
    /// `data Name tv = C1 T1 | C2 T2 T3` — sum type with constructors.
    Sum {
        /// Type name.
        name: String,
        /// Type parameters (e.g. `a` in `data Box a`).
        tyvars: Vec<String>,
        /// Constructors: (ctor name, payload (type expression, strictness
        /// flag)). A strict payload type (`C !Int`) forces its argument to
        /// WHNF at construction (Haskell 2010 strictness flag).
        ctors: Vec<(String, Vec<(TypeExpr, bool)>)>,
        /// Span.
        span: Span,
    },
    /// `data Name tv1 tv2;` — an ABSTRACT (opaque) type: nameable in
    /// signatures and usable as a value type, but with no constructors and no
    /// fields, so no program text can build, project, or pattern-match one. The
    /// compiler (not the surface language) produces values of these types.
    Abstract {
        /// Type name.
        name: String,
        /// Type parameters (e.g. `a` in `data STRef s a`).
        tyvars: Vec<String>,
        /// Span.
        span: Span,
    },
    /// `type Name = T` — synonym (pure substitution).
    ///
    /// Haskell 2010 §4.1.2 allows any `type` on the right-hand side, so the
    /// target is a full type EXPRESSION: `type Rational = Ratio Integer` is a
    /// constructor application, exactly like a signature's body.
    TypeAlias {
        /// Alias name.
        name: String,
        /// Target type expression (`Ratio Integer`, `Double`, `Pair Int Int`).
        target: TypeExpr,
        /// Span.
        span: Span,
    },
    /// `newtype Name = T` — distinct wrapper.
    Newtype {
        /// Wrapper name.
        name: String,
        /// Inner type name.
        target: TypeName,
        /// Span.
        span: Span,
    },
    /// `typeclass C a where { m: sig; }` — method dictionary.
    ///
    /// The Haskell 2010 superclass form `typeclass (C1 a, C2 a) => D a where
    /// { … }` declares the same dictionary plus a superclass context: every
    /// `D a` dictionary CONTAINS a `C1 a` and a `C2 a` dictionary (System FC:
    /// a superclass constraint is a field of the subclass dictionary). The
    /// context is empty for the plain form.
    Typeclass {
        /// Class name.
        name: String,
        /// Type variable (e.g. `a`).
        var: String,
        /// Superclass context: `(class, its type variable)` pairs from the
        /// `(C1 a, C2 a) =>` prefix, in declaration order. Empty for a class
        /// with no superclasses.
        supers: Vec<(String, String)>,
        /// Methods: (method name, signature type expression).
        methods: Vec<(String, TypeExpr)>,
        /// Default method bodies: (method name, parameter bindings, body expr).
        /// `second k = …` — a default the class provides for instances that omit
        /// the method (precedence: instance body > default > error).
        defaults: Vec<(String, Vec<Param>, Expr)>,
        /// Span.
        span: Span,
    },
    /// `instance C T where { m p1 p2 = body; }` — concrete instance. A method
    /// body binds zero or more parameters (`show f = f`, `fmap g xs = ...`),
    /// β-substituted at each call site at compile time.
    Instance {
        /// Class name.
        class: String,
        /// The instance head as parsed: a bare type (`List`, `Int`) or an
        /// applied one (`(Kleisli m)`, `(Pair Int Int)`).
        head: TypeExpr,
        /// Instance registry key, derived from `head`. A head that binds type
        /// variables is a PARTIAL instance and keys by its constructor
        /// (`Kleisli m` → `Kleisli`); a head whose arguments are all concrete
        /// types is keyed by the FULL applied type (`Pair Int Int` → `Pair Int
        /// Int`) so `Ix Int` and `Ix (Pair Int Int)` do not collide.
        ty: TypeName,
        /// Optional constraint list: (class, type variable) before `=>`,
        /// e.g. `Monad m` in `instance Monad m => Arrow (Kleisli m)`.
        constraints: Vec<(String, String)>,
        /// Bound type variables of a partial head, in order (`Kleisli m` →
        /// `["m"]`); empty for a bare or fully concrete head.
        head_args: Vec<String>,
        /// Method bodies: (method name, parameter bindings, body expr).
        method_bodies: Vec<(String, Vec<Param>, Expr)>,
        /// Span.
        span: Span,
    },
    /// `foreign name : TypeExpr;` — a foreign (Rust-implemented) builtin whose
    /// signature is declared in the language. The language owns the contract; a
    /// runtime factory bound by name provides the implementation.
    Foreign {
        /// Foreign function name (the builtin's registry name).
        name: String,
        /// Carried type signature: `FixedBuffer Double -> Double -> ... -> FixedBuffer Double`.
        sig: TypeExpr,
        /// Span.
        span: Span,
    },
}

impl Def {
    /// Returns the identifier name of this definition.
    pub fn name(&self) -> &str {
        match self {
            Def::Eval { name, .. } => name,
            Def::Equations { name, .. } => name,
            Def::Signature { name, .. } => name,
            Def::Data { name, .. } => name,
            Def::Sum { name, .. } => name,
            Def::Abstract { name, .. } => name,
            Def::TypeAlias { name, .. } => name,
            Def::Newtype { name, .. } => name,
            Def::Typeclass { name, .. } => name,
            Def::Instance { class, .. } => class,
            Def::Foreign { name, .. } => name,
        }
    }

    /// Returns the body expression of this definition.
    ///
    /// Declaration variants (`is_decl()`) carry no expression body — callers
    /// must skip them via [`Def::is_decl`] before calling this.
    pub fn body(&self) -> &Expr {
        match self {
            Def::Eval { body, .. } => body,
            _ => unreachable!("declaration variant has no body"),
        }
    }

    /// The definition's ARITY: how many arguments its body's leading λ-nest
    /// binds (0 for a constant).
    ///
    /// This is the ONE arity of a definition. Haskell 2010 §4.4.3.1 makes
    /// `f x = e` a shortening for `f = \x -> e`, so a definition binds a name
    /// to an EXPRESSION and the only carrier of parameters is the λ — there is
    /// no separate parameter list to keep in step with it. A caller that needs
    /// the parameters themselves reads the λ-nest
    /// ([`crate::types::ty::split_curried_params`]).
    pub fn arity(&self) -> usize {
        match self {
            Def::Eval { body, .. } => crate::types::infer::leading_lambda_count(body),
            _ => 0,
        }
    }

    /// Whether the source WROTE a parameter list (`f x = e`).
    ///
    /// Haskell 2010 §4.4.3.1 makes `f x = e` and `f = \x -> e` the same VALUE,
    /// but §4.5.5's monomorphism restriction distinguishes them: it applies to
    /// a SIMPLE PATTERN BINDING — one with no parameter list. So
    /// `double = \x -> x * 2.0` stays monomorphic (one shared type variable for
    /// every use) while `double x = x * 2.0` generalises to
    /// `Fractional a => a -> a`. GHC 9.10.3, compiled module: the pattern form
    /// used at two incompatible types is an error, the function form is not.
    ///
    /// A declaration and a desugared internal binding are both `false`: neither
    /// is a user-written pattern binding.
    pub fn wrote_params(&self) -> bool {
        match self {
            Def::Eval { wrote_params, .. } => *wrote_params,
            _ => false,
        }
    }

    /// Whether this is a bang binding (`let !x = e`): the right-hand side is
    /// forced to WHNF at the binding point.
    pub fn is_bang(&self) -> bool {
        matches!(self, Def::Eval { bang: true, .. })
    }

    /// Returns the where-block definitions of this definition.
    pub fn where_defs(&self) -> &[Def] {
        match self {
            Def::Eval { where_defs, .. } => where_defs,
            _ => &[],
        }
    }

    /// Returns the optional `::` signature of this definition (`None` for a
    /// declaration variant or an unannotated binding).
    pub fn sig(&self) -> Option<&TypeExpr> {
        match self {
            Def::Eval { sig, .. } => sig.as_ref(),
            Def::Equations { sig, .. } => sig.as_ref(),
            Def::Signature { sig, .. } => Some(sig),
            _ => None,
        }
    }

    /// Whether this definition is a type declaration (`data`, `type`,
    /// `newtype`, `typeclass`, `instance`, `foreign`) rather than a signal
    /// expression definition. Declaration variants carry no inferable body —
    /// the inference/lowering pipeline skips them via this flag.
    ///
    /// The parser's intermediate forms ([`Def::Equations`], [`Def::Signature`])
    /// count as declarations too: they carry no `Def::Eval` body, and
    /// [`crate::desugar`] removes them before inference. A leak past that pass
    /// is then skipped rather than misread as an empty binding.
    pub fn is_decl(&self) -> bool {
        matches!(
            self,
            Def::Data { .. }
                | Def::Sum { .. }
                | Def::Abstract { .. }
                | Def::TypeAlias { .. }
                | Def::Newtype { .. }
                | Def::Typeclass { .. }
                | Def::Instance { .. }
                | Def::Foreign { .. }
                | Def::Equations { .. }
                | Def::Signature { .. }
        )
    }
}

/// A whole program: a list of mutually-recursive definitions.
/// Exactly one must be named `main`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Program {
    /// Top-level definitions.
    pub defs: Vec<Def>,
    /// Evaluation mode selected by a leading `{-# MODE ... #-}` pragma.
    #[cfg_attr(feature = "serde", serde(default))]
    pub mode: Mode,
    /// Whether `mode` came from an explicit `{-# MODE ... #-}` pragma (as
    /// opposed to the default). When true, the pragma wins over any
    /// API-supplied mode; when false, an API-supplied mode is the default.
    #[cfg_attr(feature = "serde", serde(default))]
    pub mode_explicit: bool,
}

impl Program {
    /// Returns the `main` definition, if present.
    pub fn main_def(&self) -> Option<&Def> {
        self.defs.iter().find(|d| d.name() == "main")
    }
}

#[cfg(test)]
mod type_expr_tests {
    use super::*;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn the_fractional_payload_stays_boxed() {
        // `Expr` and `Pattern` must keep the sizes they had BEFORE the exact
        // fraction arrived. A bare `BigRational` is 64 bytes, which makes them
        // 104 and 88 — and that alone re-arms the SIGABRT this task hit:
        // `tests/ix.rs::two_dimensional_assocs_pairs_each_cell_with_its_row_major_element`
        // overflows the 2 MiB test-thread stack inside
        // `lower_value_expected`'s recursion (SIGSEGV, 9 nested frames).
        // Unboxing the payload is therefore not a local change.
        //
        // 64-bit only: the absolute byte counts are the ones measured on the
        // x86-64 CI target. The POINT is that the payload costs one pointer,
        // and `a_bare_bigrational_would_not_fit` below states that portably.
        assert_eq!(
            std::mem::size_of::<Expr>(),
            72,
            "a larger `Expr` re-arms the stack overflow in `lower_value_expected`"
        );
        assert_eq!(
            std::mem::size_of::<Pattern>(),
            48,
            "a larger `Pattern` re-arms the same recursion overflow"
        );
    }

    #[test]
    fn a_bare_bigrational_would_not_fit() {
        // A COARSE portable sanity check, not a discriminator: on x86-64 an
        // unboxed `Expr` would be 104 bytes against a 64-byte `BigRational`, so
        // `104 < 128` still HOLDS and this assertion does NOT fail when the
        // payload is unboxed. The REAL guard is the exact-byte test above,
        // `the_fractional_payload_stays_boxed` (72 / 48), which is
        // `#[cfg(target_pointer_width = "64")]` because those counts are
        // x86-64 measurements. What is left here only catches gross growth —
        // a payload of two or more inline ratios — at any pointer width.
        assert!(
            std::mem::size_of::<Expr>() < std::mem::size_of::<BigRational>() * 2,
            "two inline ratios is gross growth; got {} for `Expr`",
            std::mem::size_of::<Expr>()
        );
        assert!(
            std::mem::size_of::<Pattern>() <= std::mem::size_of::<Expr>(),
            "`Pattern` must not outgrow `Expr`"
        );
    }

    #[test]
    fn type_expr_variants_construct() {
        let t = TypeExpr::TFunc(
            vec![TypeExpr::TName("a".into())],
            Box::new(TypeExpr::TApp(
                "List".into(),
                vec![TypeExpr::TName("a".into())],
            )),
        );
        assert!(matches!(t, TypeExpr::TFunc(..)));
    }

    #[test]
    fn constrained_type_delegates_arity_to_body() {
        // A class context constrains the body type; the arity queries must
        // answer for the BODY, not for the constraint list.
        let body = TypeExpr::TFunc(
            vec![
                TypeExpr::TApp(
                    "Pair".into(),
                    vec![TypeExpr::TName("i".into()), TypeExpr::TName("i".into())],
                ),
                TypeExpr::TApp("List".into(), vec![TypeExpr::TName("e".into())]),
            ],
            Box::new(TypeExpr::TApp(
                "Array".into(),
                vec![TypeExpr::TName("i".into()), TypeExpr::TName("e".into())],
            )),
        );
        let t = TypeExpr::TConstrained(vec![("Ix".into(), "i".into())], Box::new(body));
        assert_eq!(t.arg_count(), 2);
        assert_eq!(t.arg_types().len(), 2);
        assert_eq!(t.sig_arg_count(), 2);
    }

    #[test]
    fn constrained_bare_signature_delegates_selector_arity() {
        // A bare (non-function) body: a constrained `f : Ix i => a` still takes
        // the single class-method selector argument.
        let t = TypeExpr::TConstrained(
            vec![("Ix".into(), "i".into())],
            Box::new(TypeExpr::TName("a".into())),
        );
        assert_eq!(t.arg_count(), 0);
        assert!(t.arg_types().is_empty());
        assert_eq!(t.sig_arg_count(), 1);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn data_strictness_ast_serde_roundtrips() {
        let d = Def::Data {
            name: "P".into(),
            tyvars: vec![],
            fields: vec![
                ("x".to_string(), TypeExpr::TName("Double".into()), true),
                ("y".to_string(), TypeExpr::TName("Double".into()), false),
            ],
            span: Span::new(0, 10),
        };
        let s = Def::Sum {
            name: "T".into(),
            tyvars: vec![],
            ctors: vec![("C".to_string(), vec![(TypeExpr::TName("Int".into()), true)])],
            span: Span::new(0, 10),
        };
        for def in [d, s] {
            let json = serde_json::to_string(&def).unwrap();
            assert_eq!(serde_json::from_str::<Def>(&json).unwrap(), def);
        }
    }
}
