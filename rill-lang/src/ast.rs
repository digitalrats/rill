//! Abstract syntax tree produced by the parser.

use crate::error::Span;

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
}

impl TypeExpr {
    /// The number of function arguments (0 for a non-function type).
    pub fn arg_count(&self) -> usize {
        match self {
            TypeExpr::TFunc(args, _) => args.len(),
            _ => 0,
        }
    }
    /// The argument type expressions (empty for a non-function type).
    pub fn arg_types(&self) -> &[TypeExpr] {
        match self {
            TypeExpr::TFunc(args, _) => args,
            _ => &[],
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
}

/// One `match` arm: a pattern, then a sequence of `(guard, body)` alternatives.
/// The first alternative's guard is `true` only for the bare `=> body` form; a
/// guarded arm (`pat | g => body`) carries the user-written guard `g` as its
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
    /// Imaginary literal, e.g. `3i`, `2.5i`.
    Imag(f64, Span),
    /// Identity wire `_` (arity 1→1).
    Wire(Span),
    /// Cut `!` (arity 1→0).
    Cut(Span),
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
    // --- arrow combinators (first-class) ---
    /// Sequential composition `A : B`.
    Seq(Box<Expr>, Box<Expr>, Span),
    /// Parallel composition `A , B`.
    Par(Box<Expr>, Box<Expr>, Span),
    /// Fan-out `A <: B` (split).
    Split(Box<Expr>, Box<Expr>, Span),
    /// Fan-in `A :> B` (merge/sum).
    Merge(Box<Expr>, Box<Expr>, Span),
    /// Feedback `A ~ B` (1-block delayed).
    Loop(Box<Expr>, Box<Expr>, Span),
    /// Integer delay `A @ n`.
    Delay(Box<Expr>, Box<Expr>, Span),
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
            | Expr::Imag(_, s)
            | Expr::Wire(s)
            | Expr::Cut(s)
            | Expr::Str(_, s)
            | Expr::Ref(_, s)
            | Expr::Neg(_, s)
            | Expr::Seq(_, _, s)
            | Expr::Par(_, _, s)
            | Expr::Split(_, _, s)
            | Expr::Merge(_, _, s)
            | Expr::Loop(_, _, s)
            | Expr::Delay(_, _, s) => *s,
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
}

/// A definition — top-level, `where`-block, or `let`-block.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Def {
    /// `name p1 p2 = body` — an anchor with parameters.
    Anchor {
        /// Definition name.
        name: String,
        /// Formal parameters.
        params: Vec<Param>,
        /// Right-hand side.
        body: Expr,
        /// Optional where-block definitions.
        where_defs: Vec<Def>,
        /// Span of the whole definition.
        span: Span,
    },
    /// `name = body` — a local binding (no params).
    Local {
        /// Definition name.
        name: String,
        /// Right-hand side.
        body: Expr,
        /// Optional where-block definitions.
        where_defs: Vec<Def>,
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
        /// Span.
        span: Span,
    },
    /// `instance C T where { m p1 p2 = body; }` — concrete instance. A method
    /// body binds zero or more parameters (`show f = f`, `fmap g xs = ...`),
    /// β-substituted at each call site at compile time.
    Instance {
        /// Class name.
        class: String,
        /// Concrete type the instance is for.
        ty: TypeName,
        /// Optional constraint list: (class, type variable) before `=>`,
        /// e.g. `Monad m` in `instance Monad m => Arrow (Kleisli m)`.
        constraints: Vec<(String, String)>,
        /// Head type-constructor args (partial application). `Kleisli m` →
        /// head `Kleisli`, `head_args = ["m"]`; a plain `List` → `head_args = []`.
        head_args: Vec<String>,
        /// Method bodies: (method name, parameter bindings, body expr).
        method_bodies: Vec<(String, Vec<Param>, Expr)>,
        /// Span.
        span: Span,
    },
}

impl Def {
    /// Returns the identifier name of this definition.
    pub fn name(&self) -> &str {
        match self {
            Def::Anchor { name, .. } => name,
            Def::Local { name, .. } => name,
            Def::Data { name, .. } => name,
            Def::Sum { name, .. } => name,
            Def::TypeAlias { name, .. } => name,
            Def::Newtype { name, .. } => name,
            Def::Typeclass { name, .. } => name,
            Def::Instance { class, .. } => class,
        }
    }

    /// Returns the body expression of this definition.
    ///
    /// Declaration variants (`is_decl()`) carry no expression body — callers
    /// must skip them via [`Def::is_decl`] before calling this.
    pub fn body(&self) -> &Expr {
        match self {
            Def::Anchor { body, .. } => body,
            Def::Local { body, .. } => body,
            _ => unreachable!("declaration variant has no body"),
        }
    }

    /// Returns the parameters of this definition (empty for Local).
    pub fn params(&self) -> &[Param] {
        match self {
            Def::Anchor { params, .. } => params,
            _ => &[],
        }
    }

    /// Returns the where-block definitions of this definition.
    pub fn where_defs(&self) -> &[Def] {
        match self {
            Def::Anchor { where_defs, .. } => where_defs,
            Def::Local { where_defs, .. } => where_defs,
            _ => &[],
        }
    }

    /// Whether this definition is a type declaration (`data`, `type`,
    /// `newtype`, `typeclass`, `instance`) rather than a signal expression
    /// definition. Declaration variants carry no inferable body — the
    /// inference/lowering pipeline skips them via this flag.
    pub fn is_decl(&self) -> bool {
        matches!(
            self,
            Def::Data { .. }
                | Def::Sum { .. }
                | Def::TypeAlias { .. }
                | Def::Newtype { .. }
                | Def::Typeclass { .. }
                | Def::Instance { .. }
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
}
