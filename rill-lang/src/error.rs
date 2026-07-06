//! Source spans and the unified compile error type.

use thiserror::Error;

/// A half-open byte range `[start, end)` into the original source string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
}

impl Span {
    /// Construct a span from a start and end byte offset.
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// A span covering both `self` and `other`.
    pub fn merge(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

/// Any error produced while compiling rill-lang source.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum CompileError {
    /// The lexer hit a character it cannot start a token with.
    #[error("lex error at {span:?}: {msg}")]
    Lex {
        /// Human-readable cause.
        msg: String,
        /// Location in source.
        span: Span,
    },
    /// The parser encountered unexpected or missing tokens.
    #[error("parse error at {span:?}: {msg}")]
    Parse {
        /// Human-readable cause.
        msg: String,
        /// Location in source.
        span: Span,
    },
    /// The type checker rejected the program (arity or scalar mismatch, etc.).
    #[error("type error at {span:?}: {msg}")]
    Type {
        /// Human-readable cause.
        msg: String,
        /// Location in source.
        span: Span,
    },
    /// A well-typed program that the MVP backend cannot lower/run.
    #[error("unsupported: {0}")]
    Unsupported(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_merge_covers_both() {
        let a = Span::new(2, 5);
        let b = Span::new(8, 10);
        assert_eq!(a.merge(b), Span::new(2, 10));
    }
}
