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
    /// Identity wire `_` (arity 1→1).
    Wire(Span),
    /// Cut `!` (arity 1→0).
    Cut(Span),
    /// A reference to a definition or a bound parameter.
    Ref(String, Span),
    /// Application `name(arg, ...)`.
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
}

impl Expr {
    /// The source span of this node.
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Wire(s)
            | Expr::Cut(s)
            | Expr::Ref(_, s)
            | Expr::Neg(_, s) => *s,
            Expr::Apply { span, .. } | Expr::Bin { span, .. } => *span,
        }
    }
}

/// A top-level definition: `name(params) = body;` (params may be empty).
#[derive(Debug, Clone, PartialEq)]
pub struct Def {
    /// Definition name.
    pub name: String,
    /// Formal parameter names (empty for a plain alias).
    pub params: Vec<String>,
    /// Right-hand side.
    pub body: Expr,
    /// Span of the whole definition.
    pub span: Span,
}

/// A whole program: an ordered list of definitions. One MUST be named `process`.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The definitions in source order.
    pub defs: Vec<Def>,
}
