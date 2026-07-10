//! Abstract syntax tree produced by the parser.

use crate::error::Span;

/// Binary block-diagram combinators and arithmetic operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `:` sequential composition.
    Seq,
    /// `,` parallel composition.
    Par,
    /// `<:` split / fan-out.
    Split,
    /// `:>` merge / fan-in.
    Merge,
    /// `~` feedback (implicit 1-sample delay).
    Feedback,
    /// `@` integer delay.
    Delay,
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

/// A rill-lang expression node.
#[derive(Debug, Clone, PartialEq)]
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
    /// Unary negation `-expr`.
    Neg(Box<Expr>, Span),
    /// A binary combinator/operator.
    Bin {
        /// The operator.
        op: BinOp,
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
            | Expr::Neg(_, s) => *s,
            Expr::Apply { span, .. }
            | Expr::Bin { span, .. }
            | Expr::Let { span, .. }
            | Expr::Record(_, span)
            | Expr::ActorParam { span, .. } => *span,
        }
    }
}

/// A parameter declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// Parameter name.
    pub name: String,
    /// Source span.
    pub span: Span,
}

/// A definition — top-level, `where`-block, or `let`-block.
#[derive(Debug, Clone, PartialEq)]
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
}

impl Def {
    /// Returns the identifier name of this definition.
    pub fn name(&self) -> &str {
        match self {
            Def::Anchor { name, .. } => name,
            Def::Local { name, .. } => name,
        }
    }

    /// Returns the body expression of this definition.
    pub fn body(&self) -> &Expr {
        match self {
            Def::Anchor { body, .. } => body,
            Def::Local { body, .. } => body,
        }
    }

    /// Returns the parameters of this definition (empty for Local).
    pub fn params(&self) -> &[Param] {
        match self {
            Def::Anchor { params, .. } => params,
            Def::Local { .. } => &[],
        }
    }

    /// Returns the where-block definitions of this definition.
    pub fn where_defs(&self) -> &[Def] {
        match self {
            Def::Anchor { where_defs, .. } => where_defs,
            Def::Local { where_defs, .. } => where_defs,
        }
    }
}

/// A whole program: a list of mutually-recursive definitions.
/// Exactly one must be named `main`.
#[derive(Debug, Clone, PartialEq)]
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
