//! Abstract syntax tree produced by the parser.

use crate::error::Span;
use crate::mode::Mode;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A type name reference in a declaration (`Float`, `Hz`, `Point`, ...).
pub type TypeName = String;

/// A type expression in a declaration: concrete names, type variables,
/// constructor application, function types, and capacity literals.
/// A type-expression node in a declaration signature.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TypeExpr {
    /// A concrete type name or a type variable.
    TName(String),
    /// A constructor application: `f a`, `List Float`.
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
    /// through an explicit `forall` binder (`forall v. C v => T`). A
    /// class-constrained definition has no standalone body: its class variable
    /// is abstract until a call site solves it.
    pub fn is_constrained(&self) -> bool {
        match self {
            TypeExpr::TConstrained(..) => true,
            TypeExpr::TForall(_, inner) => inner.is_constrained(),
            _ => false,
        }
    }
}

/// Arithmetic operators (elementwise, 2→1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ArithOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
}

/// A value-track comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CmpOp {
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
}

/// A value-track boolean logic operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LogicOp {
    /// `&&`
    And,
    /// `||`
    Or,
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
    /// Integer literal.
    LitInt(i64),
    /// Float literal.
    LitFloat(f64),
    /// Boolean literal.
    LitBool(bool),
    /// String literal.
    LitStr(String),
    /// Constructor application with (possibly nested) argument patterns.
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
/// The first alternative's guard is `true` only for the bare `-> body` form; a
/// guarded arm (`pat | g -> body`) carries the user-written guard `g` as its
/// first alternative.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MatchArm {
    /// The pattern tested against the scrutinee.
    pub pattern: Pattern,
    /// `(guard_expr, body_expr)` pairs, evaluated in order; the first that is
    /// true runs its body.
    pub guards: Vec<(Expr, Expr)>,
    /// Span of the whole arm (for diagnostics).
    pub span: Span,
}

/// A rill-lang expression node.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Expr {
    /// Integer literal.
    Int(i64, Span),
    /// Float literal.
    Float(f64, Span),
    /// Imaginary literal is gone (Haskell 2010 has no imaginary literal).
    /// Unit literal `()` — the sole inhabitant of `ValueTy::Void`.
    Void(Span),
    /// String literal, e.g. `"cutoff"`.
    Str(String, Span),
    /// A reference to a definition or a bound parameter.
    Ref(String, Span),
    /// Application `name(arg, ...)` or juxtaposed `name arg1 arg2`.
    Apply {
        /// Function name.
        name: String,
        /// Argument expressions.
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
    // --- arithmetic ---
    /// Elementwise arithmetic `lhs op rhs`.
    Arith {
        /// The operator.
        op: ArithOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
        /// Full span.
        span: Span,
    },
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
    /// Boolean literal `true` / `false`.
    Bool(bool, Span),
    /// List literal `[e1, e2]`.
    ListLit(Vec<Expr>, Span),
    /// Map literal `{ "k": v, ... }` (string keys).
    MapLit(Vec<(String, Expr)>, Span),
    /// Value-track comparison `a < b` (only valid in value position).
    Cmp {
        /// The operator.
        op: CmpOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
        /// Full span.
        span: Span,
    },
    /// Value-track logic `a && b` / `a || b`.
    Logic {
        /// The operator.
        op: LogicOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
        /// Full span.
        span: Span,
    },
}

impl Expr {
    /// The source span of this node.
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Void(s)
            | Expr::Str(_, s)
            | Expr::Ref(_, s)
            | Expr::Neg(_, s) => *s,
            Expr::Apply { span, .. }
            | Expr::ApplyExpr { span, .. }
            | Expr::Arith { span, .. }
            | Expr::Let { span, .. }
            | Expr::Record(_, span)
            | Expr::ActorParam { span, .. }
            | Expr::FieldProject { span, .. }
            | Expr::FieldUpdate { span, .. }
            | Expr::Match { span, .. }
            | Expr::If { span, .. }
            | Expr::Lambda { span, .. }
            | Expr::Bool(_, span)
            | Expr::ListLit(_, span)
            | Expr::MapLit(_, span)
            | Expr::Cmp { span, .. }
            | Expr::Logic { span, .. } => *span,
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

/// A definition — top-level, `where`-block, or `let`-block.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Def {
    /// `name p1 p2 = body` — one definition form (params optional; multi-param
    /// is currying sugar). `sig` is an optional `::` type annotation
    /// (inference-only hint/constraint). Named so it reads as «evaluable
    /// definition» in contrast to the `is_decl()` declarations. Top-level,
    /// `where`-block, and `let`-block bindings are all this one form (a bare
    /// binding has no params).
    Eval {
        /// Definition name.
        name: String,
        /// Formal parameters (curried).
        params: Vec<Param>,
        /// Right-hand side.
        body: Expr,
        /// Optional where-block definitions.
        where_defs: Vec<Def>,
        /// Optional `:: TypeExpr` signature annotation.
        sig: Option<TypeExpr>,
        /// Bang binding (`let !x = e`): force the right-hand side to WHNF at
        /// the binding point (GHC-style extension; no deep strictness).
        #[cfg_attr(feature = "serde", serde(default))]
        bang: bool,
        /// Span of the whole definition.
        span: Span,
    },
    /// `data Name tv1 tv2 = { f1: T1, f2: T2 }` — product type.
    Data {
        /// Type name.
        name: String,
        /// Type parameters (e.g. `a` in `data Box a`).
        tyvars: Vec<String>,
        /// Fields: (field name, type expression).
        fields: Vec<(String, TypeExpr)>,
        /// Span.
        span: Span,
    },
    /// `data Name tv = C1 T1 | C2 T2 T3` — sum type with constructors.
    Sum {
        /// Type name.
        name: String,
        /// Type parameters (e.g. `a` in `data Box a`).
        tyvars: Vec<String>,
        /// Constructors: (ctor name, payload type expressions).
        ctors: Vec<(String, Vec<TypeExpr>)>,
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
    TypeAlias {
        /// Alias name.
        name: String,
        /// Target type name.
        target: TypeName,
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
    Typeclass {
        /// Class name.
        name: String,
        /// Type variable (e.g. `a`).
        var: String,
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
        /// Carried type signature: `FixedBuffer f32 -> Float -> ... -> FixedBuffer f32`.
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

    /// Returns the parameters of this definition (empty for a bare binding).
    pub fn params(&self) -> &[Param] {
        match self {
            Def::Eval { params, .. } => params,
            _ => &[],
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
            _ => None,
        }
    }

    /// Whether this definition is a type declaration (`data`, `type`,
    /// `newtype`, `typeclass`, `instance`, `foreign`) rather than a signal
    /// expression definition. Declaration variants carry no inferable body —
    /// the inference/lowering pipeline skips them via this flag.
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
}
