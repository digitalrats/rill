//! Hand-written tokeniser. Produces `Token`s carrying source spans.

use crate::error::{CompileError, Span};
use num_bigint::BigInt;
use num_rational::BigRational;

/// A lexical token kind.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// Numeric literal that contains a `.` or exponent — a fractional literal.
    /// Carries the EXACT value its digits spell plus the source text, so `0.1`
    /// is `1/10` rather than the nearest `f64`.
    FractionalLit(BigRational, String),
    /// Numeric literal with no `.` — an integer. Carries arbitrary precision:
    /// `123456789012345678901234567890` is a valid literal (GHC accepts it as
    /// an `Integer`).
    Int(BigInt),
    /// Identifier / keyword (`sin`, `min`, `process`, user names).
    Ident(String),
    /// String literal, e.g. `"cutoff"`.
    Str(String),
    /// `_`
    Wire,
    /// `!` — a bang marker (`!x`) when attached to its operand, otherwise the
    /// infix indexing operator (`a ! i`).
    Bang,
    /// `:`
    Colon,
    /// `<:`
    Split,
    /// `:>`
    Merge,
    /// `~`
    Tilde,
    /// `@`
    At,
    /// `,`
    Comma,
    /// `+`
    Plus,
    /// `++` — list append (Prelude, `infixr 5`).
    PlusPlus,
    /// `<>` — the "diamond" operator: Semigroup append in Haskell 2010
    /// (`infixr 6`). Named for its shape, which is how it reads in source; the
    /// token also spells the `<>` class METHOD reference.
    Diamond,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `=`
    Eq,
    /// `;`
    Semi,
    /// `where` keyword — optional definition block.
    KwWhere,
    /// `let` keyword — expression-level mutually-recursive bindings.
    KwLet,
    /// `in` keyword — separator in `let defs in expr`.
    KwIn,
    /// `data` keyword — product or sum type declaration.
    KwData,
    /// `type` keyword — type synonym declaration.
    KwType,
    /// `newtype` keyword — distinct wrapper type declaration.
    KwNewtype,
    /// `typeclass` keyword — method dictionary declaration.
    KwTypeclass,
    /// `instance` keyword — concrete typeclass instance.
    KwInstance,
    /// `foreign` keyword — foreign declaration.
    KwForeign,
    /// `forall` keyword — explicit universal quantification in a type.
    KwForall,
    /// `of` keyword — separator in `case x of { .. }`.
    KwOf,
    /// `if` keyword — conditional expression.
    KwIf,
    /// `then` keyword — `if` branch separator.
    KwThen,
    /// `else` keyword — `if` branch separator.
    KwElse,
    /// `->` thin arrow — function-type, lambda, and case-alternative arrow.
    ThinArrow,
    /// `=>` fat arrow — class-constraint arrow (`instance C a => T`).
    FatArrow,
    /// `|` — sum-type constructor separator.
    Pipe,
    /// `.` — field access.
    Dot,
    /// `:=` — field update assignment.
    ColonEq,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `?`
    Question,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `do` keyword — monadic sequencing block.
    KwDo,
    /// `<-` — do-block monadic binding.
    LArrow,
    /// `\` — lambda literal `\x -> body`.
    Backslash,
    /// `` ` `` — backtick infix application (`` a `f` b `` ≡ `f a b`).
    Backtick,
    /// `case` keyword — pattern matching (`case e of { .. }`).
    KwCase,
    /// `::` — type signature annotation (`name :: TypeExpr`).
    DoubleColon,
    /// Newline — layout (offside rule) boundary.
    Newline,
    /// End of input.
    Eof,
    /// Imaginary literal, e.g. `3i`, `2.5i`.
    Imag(f64),
    /// `{-# name value #-}` pragma directive.
    Pragma {
        /// Pragma name, e.g. `MODE`.
        name: String,
        /// Pragma value, e.g. `strict` | `complete`.
        value: String,
    },
}

/// A token plus its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// The token kind.
    pub tok: Tok,
    /// Where it came from.
    pub span: Span,
}

/// The EXACT rational value a fractional literal's decimal text denotes.
///
/// Haskell 2010 §6.4.1: a fractional literal is `fromRational` of the decimal
/// fraction it spells, so this builds the ratio from the DIGITS — never from
/// an `f64`, whose rounding would turn `0.1` into
/// `3602879701896397/36028797018963968`. The grammar accepted here is the
/// Report's `decimal → digits . digits [exponent]` (either digit run may be
/// empty, as in `1.` or `1.e3`), with `exponent → (e|E) [+|-] decimal`.
///
/// `None` for text the scanner should never have produced; the caller turns it
/// into a lex error rather than a silent value.
pub(crate) fn decimal_rational(text: &str) -> Option<BigRational> {
    let (mantissa, exp) = match text.find(['e', 'E']) {
        Some(at) => {
            let exp: i64 = text[at + 1..].parse().ok()?;
            (&text[..at], exp)
        }
        None => (text, 0),
    };
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(at) => (&mantissa[..at], &mantissa[at + 1..]),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    // The fraction is `(int ‖ frac) / 10^len(frac)`, scaled by `10^exp`: an
    // exact power of ten in each direction, so no rounding is involved.
    let digits = format!("{int_part}{frac_part}");
    let numer: BigInt = digits.parse().ok()?;
    let scale = exp - frac_part.len() as i64;
    let ten = BigInt::from(10);
    // `BigInt::pow` takes a `u32`. A scale that does not fit is not a
    // representable literal — an exact value would need more digits than the
    // address space holds — so it is a LOUD rejection rather than a silently
    // truncated exponent (`1e4294967296` must never become `1e0`).
    let power = u32::try_from(scale.unsigned_abs()).ok()?;
    Some(if scale >= 0 {
        BigRational::from_integer(numer * ten.pow(power))
    } else {
        BigRational::new(numer, ten.pow(power))
    })
}

/// Tokenise `src` into a vector terminated by a single [`Tok::Eof`].
///
/// Whitespace is skipped. `//` starts a line comment.
pub fn tokenize(src: &str) -> Result<Vec<Token>, CompileError> {
    let bytes = src.as_bytes();
    let mut i = 0usize;
    let mut out = Vec::new();

    let is_ident_start = |c: u8| c.is_ascii_alphabetic() || c == b'_';
    let is_ident_cont = |c: u8| c.is_ascii_alphanumeric() || c == b'_';

    // Whitespace (except newline) is skipped. `--` starts a line comment. A
    // newline is emitted as `Tok::Newline` so the parser can implement the
    // offside rule (layout) — each line boundary is a layout point.
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\n' {
            out.push(Token {
                tok: Tok::Newline,
                span: Span::new(i, i + 1),
            });
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        // `{-# … #-}` pragma. Matched BEFORE the block-comment path so
        // `{-# MODE complete #-}` is never swallowed as a `{- … -}` comment.
        if c == b'{' && i + 2 < bytes.len() && bytes[i + 1] == b'-' && bytes[i + 2] == b'#' {
            // `{-# name value #-}` — scan to the closing `#-}`.
            let mut j = i + 3;
            while j + 2 < bytes.len()
                && !(bytes[j] == b'#' && bytes[j + 1] == b'-' && bytes[j + 2] == b'}')
            {
                j += 1;
            }
            if j + 2 >= bytes.len() {
                return Err(CompileError::Lex {
                    msg: "unterminated pragma (expected `#-}`)".into(),
                    span: Span::new(start, bytes.len()),
                });
            }
            let end = j + 3;
            let content = &src[i + 3..j];
            // Split the inner text on whitespace into exactly `name value`.
            let mut words: Vec<&str> = Vec::new();
            let mut k = i + 3;
            while k < j {
                while k < j && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k >= j {
                    break;
                }
                let wstart = k;
                while k < j && !bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                words.push(&src[wstart..k]);
            }
            if words.len() != 2 {
                return Err(CompileError::Lex {
                    msg: format!("bad pragma `{{-# {content} #-}}` (expected `name value`)"),
                    span: Span::new(start, end),
                });
            }
            out.push(Token {
                tok: Tok::Pragma {
                    name: words[0].to_string(),
                    value: words[1].to_string(),
                },
                span: Span::new(start, end),
            });
            i = end;
            continue;
        }
        // `{- … -}` block comment (Haskell 2010 §2.2), nesting-aware. Only
        // reached for non-pragma `{-`, because the pragma branch above runs
        // first and consumes `{-#`. An unterminated comment is a lex error
        // rather than silently swallowing the rest of the file.
        if c == b'{' && bytes.get(i + 1) == Some(&b'-') {
            let start = i;
            let mut depth = 0usize;
            // A comment that spans a line break still separates the lines it
            // swallowed: without this the definition before the comment and the
            // one after it look like a single layout item.
            let mut line_break: Option<usize> = None;
            while i < bytes.len() {
                if bytes[i] == b'{' && bytes.get(i + 1) == Some(&b'-') {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'-' && bytes.get(i + 1) == Some(&b'}') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if bytes[i] == b'\n' && line_break.is_none() {
                        line_break = Some(i);
                    }
                    i += 1;
                }
            }
            if depth != 0 {
                return Err(CompileError::Lex {
                    msg: "unterminated block comment (expected `-}`)".into(),
                    span: Span::new(start, bytes.len()),
                });
            }
            if let Some(nl) = line_break {
                out.push(Token {
                    tok: Tok::Newline,
                    span: Span::new(nl, nl + 1),
                });
            }
            continue;
        }
        if c == b'+' && i + 1 < bytes.len() && bytes[i + 1] == b'+' {
            // `++` — list append. Must precede the single-character `+` so the
            // operator is one token, not two.
            i += 2;
            out.push(Token {
                tok: Tok::PlusPlus,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            // `<>` — Semigroup append. Two characters, so it must be matched
            // before the single-character comparison operators.
            i += 2;
            out.push(Token {
                tok: Tok::Diamond,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b':' {
            i += 2;
            out.push(Token {
                tok: Tok::Split,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b':' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            i += 2;
            out.push(Token {
                tok: Tok::Merge,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b':' && i + 1 < bytes.len() && bytes[i + 1] == b':' {
            i += 2;
            out.push(Token {
                tok: Tok::DoubleColon,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b':' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::ColonEq,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'=' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            i += 2;
            out.push(Token {
                tok: Tok::FatArrow,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            i += 2;
            out.push(Token {
                tok: Tok::ThinArrow,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'=' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::EqEq,
                span: Span::new(start, i),
            });
            continue;
        }
        // `/=` — Haskell 2010's not-equal. Must be matched before the
        // single-character `/`, and `/=` (not `!=`) is the standard spelling.
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::NotEq,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::NotEq,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::Le,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'>' && i + 1 < bytes.len() && bytes[i + 1] == b'=' {
            i += 2;
            out.push(Token {
                tok: Tok::Ge,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'&' && i + 1 < bytes.len() && bytes[i + 1] == b'&' {
            i += 2;
            out.push(Token {
                tok: Tok::AndAnd,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            // `<-` — do-block monadic binding (disjoint from `<:` and `<=`).
            i += 2;
            out.push(Token {
                tok: Tok::LArrow,
                span: Span::new(start, i),
            });
            continue;
        }
        if c == b'|' && i + 1 < bytes.len() && bytes[i + 1] == b'|' {
            i += 2;
            out.push(Token {
                tok: Tok::OrOr,
                span: Span::new(start, i),
            });
            continue;
        }
        if c.is_ascii_digit() {
            let mut is_float = false;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit()
                    || bytes[i] == b'.'
                    || bytes[i] == b'e'
                    || bytes[i] == b'E')
            {
                if bytes[i] == b'.' || bytes[i] == b'e' || bytes[i] == b'E' {
                    is_float = true;
                    // A decimal exponent may carry a sign (Haskell 2010 §2.5:
                    // `exponent → (e|E) [+|-] decimal`), so `1e-3` is ONE
                    // literal rather than `1e` followed by `- 3`.
                    if (bytes[i] == b'e' || bytes[i] == b'E')
                        && i + 1 < bytes.len()
                        && (bytes[i + 1] == b'+' || bytes[i + 1] == b'-')
                    {
                        i += 1;
                    }
                }
                i += 1;
            }
            let text = &src[start..i];
            let span = Span::new(start, i);
            if i < bytes.len() && bytes[i] == b'i' {
                i += 1;
                let span = Span::new(start, i);
                let v: f64 = text.parse().map_err(|_| CompileError::Lex {
                    msg: format!("invalid imaginary literal `{text}i`"),
                    span,
                })?;
                out.push(Token {
                    tok: Tok::Imag(v),
                    span,
                });
            } else if is_float {
                let v = decimal_rational(text).ok_or_else(|| CompileError::Lex {
                    msg: format!("invalid float literal `{text}`"),
                    span,
                })?;
                out.push(Token {
                    tok: Tok::FractionalLit(v, text.to_string()),
                    span,
                });
            } else {
                let v: BigInt = text.parse().map_err(|_| CompileError::Lex {
                    msg: format!("invalid int literal `{text}`"),
                    span,
                })?;
                out.push(Token {
                    tok: Tok::Int(v),
                    span,
                });
            }
            continue;
        }
        if is_ident_start(c) {
            while i < bytes.len() && is_ident_cont(bytes[i]) {
                i += 1;
            }
            let text = &src[start..i];
            let span = Span::new(start, i);

            // peek past whitespace to see if `(` follows — if so,
            // `param(`, `keep(`, `inline(` are function calls, not keywords
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let followed_by_paren = j < bytes.len() && bytes[j] == b'(';

            let tok = match text {
                "_" => Tok::Wire,
                "where" if !followed_by_paren => Tok::KwWhere,
                "let" if !followed_by_paren => Tok::KwLet,
                "in" if !followed_by_paren => Tok::KwIn,
                "data" if !followed_by_paren => Tok::KwData,
                "type" if !followed_by_paren => Tok::KwType,
                "newtype" if !followed_by_paren => Tok::KwNewtype,
                // `typeclass` is a DECLARATION keyword only: the
                // `(C a) =>` superclass form puts a `(` right after it, so no
                // call-guard may apply here (unlike `where`/`let`, which are
                // also usable as call heads).
                "typeclass" => Tok::KwTypeclass,
                // `instance` is always a keyword — its head may start with a
                // parenthesized constraint list (`instance (Monad m) => …`), so
                // the `followed_by_paren` call-guard must not apply here.
                "instance" => Tok::KwInstance,
                "foreign" if !followed_by_paren => Tok::KwForeign,
                "forall" if !followed_by_paren => Tok::KwForall,
                // `case` is always a keyword — its scrutinee may start with a
                // parenthesized expression (`case (f x) of …`), so the
                // `followed_by_paren` call-guard must not apply here.
                "case" => Tok::KwCase,
                "of" if !followed_by_paren => Tok::KwOf,
                "if" if !followed_by_paren => Tok::KwIf,
                "then" if !followed_by_paren => Tok::KwThen,
                "else" if !followed_by_paren => Tok::KwElse,
                "do" if !followed_by_paren => Tok::KwDo,
                _ => Tok::Ident(text.to_string()),
            };
            out.push(Token { tok, span });
            continue;
        }
        if c == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            if i >= bytes.len() {
                return Err(CompileError::Lex {
                    msg: "unterminated string literal".into(),
                    span: Span::new(start, bytes.len()),
                });
            }
            i += 1;
            let text = src[start + 1..i - 1].to_string();
            out.push(Token {
                tok: Tok::Str(text),
                span: Span::new(start, i),
            });
            continue;
        }
        let single = match c {
            b'\\' => Tok::Backslash,
            b'`' => Tok::Backtick,
            b':' => Tok::Colon,
            b'~' => Tok::Tilde,
            b'@' => Tok::At,
            b',' => Tok::Comma,
            b'+' => Tok::Plus,
            b'-' => Tok::Minus,
            b'*' => Tok::Star,
            b'/' => Tok::Slash,
            b'%' => Tok::Percent,
            b'!' => Tok::Bang,
            b'?' => Tok::Question,
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b'{' => Tok::LBrace,
            b'}' => Tok::RBrace,
            b'[' => Tok::LBracket,
            b']' => Tok::RBracket,
            b'<' => Tok::Lt,
            b'>' => Tok::Gt,
            b'=' => Tok::Eq,
            b';' => Tok::Semi,
            b'|' => Tok::Pipe,
            b'.' => Tok::Dot,
            other => {
                return Err(CompileError::Lex {
                    msg: format!("unexpected character `{}`", other as char),
                    span: Span::new(start, start + 1),
                })
            }
        };
        i += 1;
        out.push(Token {
            tok: single,
            span: Span::new(start, i),
        });
    }
    out.push(Token {
        tok: Tok::Eof,
        span: Span::new(src.len(), src.len()),
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        tokenize(src).unwrap().into_iter().map(|t| t.tok).collect()
    }

    /// A fractional token for `text`, with the exact value its digits spell.
    fn fl(text: &str) -> Tok {
        Tok::FractionalLit(decimal_rational(text).unwrap(), text.to_string())
    }

    #[test]
    fn lexes_combinators_and_ops() {
        assert_eq!(
            kinds("_ : + <: :> ~ @ , * / % ! ? ( ) { } = ;"),
            vec![
                Tok::Wire,
                Tok::Colon,
                Tok::Plus,
                Tok::Split,
                Tok::Merge,
                Tok::Tilde,
                Tok::At,
                Tok::Comma,
                Tok::Star,
                Tok::Slash,
                Tok::Percent,
                Tok::Bang,
                Tok::Question,
                Tok::LParen,
                Tok::RParen,
                Tok::LBrace,
                Tok::RBrace,
                Tok::Eq,
                Tok::Semi,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn distinguishes_int_and_float() {
        assert_eq!(
            kinds("3 3.5 10"),
            vec![Tok::Int(3.into()), fl("3.5"), Tok::Int(10.into()), Tok::Eof]
        );
    }

    #[test]
    fn a_fractional_token_carries_the_exact_value_and_the_source_text() {
        // The four conversions the plan fixes, plus a value whose exact
        // numerator/denominator are both large.
        for (src, n, d) in [
            ("0.1", 1i64, 10i64),
            ("3.14", 157, 50),
            ("1e3", 1000, 1),
            ("1.5e-2", 3, 200),
            ("1.00", 1, 1),
        ] {
            let toks = tokenize(src).unwrap();
            match &toks[0].tok {
                Tok::FractionalLit(v, text) => {
                    assert_eq!(v.numer(), &BigInt::from(n), "numerator of `{src}`");
                    assert_eq!(v.denom(), &BigInt::from(d), "denominator of `{src}`");
                    assert_eq!(text, src, "the source text is kept verbatim");
                }
                other => panic!("expected a fractional token for `{src}`, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_signed_exponent_is_part_of_the_literal() {
        // Haskell 2010 §2.5 allows a sign in `exponent`, so `1e-3` is ONE
        // literal. Read as `1e` minus `3` it would not even tokenise.
        assert_eq!(kinds("1e-3"), vec![fl("1e-3"), Tok::Eof]);
        assert_eq!(kinds("1E+3"), vec![fl("1E+3"), Tok::Eof]);
        assert_eq!(
            fl("1e-3"),
            Tok::FractionalLit(BigRational::new(1.into(), 1000.into()), "1e-3".into())
        );
    }

    #[test]
    fn an_invalid_fractional_literal_is_a_lex_error() {
        // `1e` has no exponent digits; the scanner must not invent a value.
        let err = tokenize("1e").unwrap_err();
        match err {
            CompileError::Lex { msg, .. } => {
                assert!(msg.contains("invalid float literal"), "{msg}")
            }
            other => panic!("expected a lex error, got {other}"),
        }
        // An exponent too large for the exact value is a LOUD rejection, not a
        // silently wrapped one.
        assert!(tokenize("1e4294967296").is_err());
    }

    #[test]
    fn lexes_idents_and_skips_comments() {
        assert_eq!(
            kinds("process -- a comment\n sin"),
            vec![
                Tok::Ident("process".into()),
                Tok::Newline,
                Tok::Ident("sin".into()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn split_and_merge_are_multichar() {
        assert_eq!(kinds(":>"), vec![Tok::Merge, Tok::Eof]);
        assert_eq!(kinds("<:"), vec![Tok::Split, Tok::Eof]);
    }

    #[test]
    fn rejects_unknown_char() {
        assert!(tokenize("$").is_err());
    }

    #[test]
    fn lexes_string_literal() {
        assert_eq!(
            kinds(r#""cutoff""#),
            vec![Tok::Str("cutoff".into()), Tok::Eof]
        );
    }

    #[test]
    fn rejects_unterminated_string() {
        assert!(tokenize(r#""abc"#).is_err());
    }

    #[test]
    fn lexes_pragma() {
        assert_eq!(
            kinds("{-# MODE complete #-}"),
            vec![
                Tok::Pragma {
                    name: "MODE".into(),
                    value: "complete".into()
                },
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn rejects_unterminated_pragma() {
        assert!(tokenize("{-# MODE complete").is_err());
    }

    #[test]
    fn rejects_malformed_pragma() {
        assert!(tokenize("{-# MODE #-}").is_err());
        assert!(tokenize("{-# MODE complete extra #-}").is_err());
    }

    #[test]
    fn main_is_an_ordinary_identifier() {
        // `main` is no longer a keyword: the entry name is a compile-time
        // pipeline parameter, not a lexer token.
        assert_eq!(
            kinds("main foo bar"),
            vec![
                Tok::Ident("main".into()),
                Tok::Ident("foo".into()),
                Tok::Ident("bar".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn main_is_not_keyword_when_followed_by_paren() {
        assert_eq!(
            kinds(r#"main("freq", 440)"#),
            vec![
                Tok::Ident("main".into()),
                Tok::LParen,
                Tok::Str("freq".into()),
                Tok::Comma,
                Tok::Int(440.into()),
                Tok::RParen,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_let_and_in_keywords() {
        assert_eq!(
            kinds("let x = 1 in x"),
            vec![
                Tok::KwLet,
                Tok::Ident("x".into()),
                Tok::Eq,
                Tok::Int(1.into()),
                Tok::KwIn,
                Tok::Ident("x".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn let_is_not_keyword_when_followed_by_paren() {
        assert_eq!(
            kinds("let(x, y)"),
            vec![
                Tok::Ident("let".into()),
                Tok::LParen,
                Tok::Ident("x".into()),
                Tok::Comma,
                Tok::Ident("y".into()),
                Tok::RParen,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_new_declaration_keywords() {
        assert_eq!(
            kinds("data type newtype typeclass instance foreign of"),
            vec![
                Tok::KwData,
                Tok::KwType,
                Tok::KwNewtype,
                Tok::KwTypeclass,
                Tok::KwInstance,
                Tok::KwForeign,
                Tok::KwOf,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn no_fn_or_match_keywords() {
        // Haskell 2010 surface: `fn` and `match` are ordinary identifiers.
        assert_eq!(
            kinds("fn x"),
            vec![Tok::Ident("fn".into()), Tok::Ident("x".into()), Tok::Eof,]
        );
        assert_eq!(
            kinds("match x"),
            vec![Tok::Ident("match".into()), Tok::Ident("x".into()), Tok::Eof,]
        );
    }

    #[test]
    fn lexes_new_punctuation() {
        assert_eq!(
            kinds("=> := | ."),
            vec![Tok::FatArrow, Tok::ColonEq, Tok::Pipe, Tok::Dot, Tok::Eof]
        );
    }

    #[test]
    fn distinguishes_thin_and_fat_arrow() {
        // Haskell 2010: `->` is the function-type/lambda/case arrow; `=>` is
        // reserved for class constraints.
        assert_eq!(
            kinds("a -> b"),
            vec![
                Tok::Ident("a".into()),
                Tok::ThinArrow,
                Tok::Ident("b".into()),
                Tok::Eof,
            ]
        );
        assert_eq!(
            kinds("a => b"),
            vec![
                Tok::Ident("a".into()),
                Tok::FatArrow,
                Tok::Ident("b".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn paren_following_keyword_is_not_a_call_for_other_keywords() {
        // Keywords stay paren-guarded (`data(y)` is a call-style identifier).
        // `match` is no longer a keyword — it lexes as a plain identifier.
        assert_eq!(
            kinds(r#"match(x) data(y)"#),
            vec![
                Tok::Ident("match".into()),
                Tok::LParen,
                Tok::Ident("x".into()),
                Tok::RParen,
                Tok::Ident("data".into()),
                Tok::LParen,
                Tok::Ident("y".into()),
                Tok::RParen,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_backslash_lambda_arrow() {
        assert_eq!(
            kinds(r"\x -> x"),
            vec![
                Tok::Backslash,
                Tok::Ident("x".into()),
                Tok::ThinArrow,
                Tok::Ident("x".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_if_then_else_keywords() {
        assert_eq!(
            kinds("if then else"),
            vec![Tok::KwIf, Tok::KwThen, Tok::KwElse, Tok::Eof]
        );
    }

    #[test]
    fn if_then_else_not_keywords_when_followed_by_paren() {
        // `else(`, `if(`, `then(` are call-style identifiers, not keywords.
        assert_eq!(
            kinds("if(x) then(y) else(z)"),
            vec![
                Tok::Ident("if".into()),
                Tok::LParen,
                Tok::Ident("x".into()),
                Tok::RParen,
                Tok::Ident("then".into()),
                Tok::LParen,
                Tok::Ident("y".into()),
                Tok::RParen,
                Tok::Ident("else".into()),
                Tok::LParen,
                Tok::Ident("z".into()),
                Tok::RParen,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_collection_and_comparison_tokens() {
        assert_eq!(
            kinds("[ ] == != < > <= >= && || true false"),
            vec![
                Tok::LBracket,
                Tok::RBracket,
                Tok::EqEq,
                Tok::NotEq,
                Tok::Lt,
                Tok::Gt,
                Tok::Le,
                Tok::Ge,
                Tok::AndAnd,
                Tok::OrOr,
                // `true` / `false` are ordinary IDENTIFIERS: they name the
                // Prelude constructors `True` / `False` (via the parser's
                // alias), not keywords.
                Tok::Ident("true".into()),
                Tok::Ident("false".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn fn_is_not_keyword_when_followed_by_paren() {
        assert_eq!(
            kinds("fn(x, y)"),
            vec![
                Tok::Ident("fn".into()),
                Tok::LParen,
                Tok::Ident("x".into()),
                Tok::Comma,
                Tok::Ident("y".into()),
                Tok::RParen,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_backslash_lambda() {
        assert_eq!(
            kinds(r"\x -> x + 1.0"),
            vec![
                Tok::Backslash,
                Tok::Ident("x".into()),
                Tok::ThinArrow,
                Tok::Ident("x".into()),
                Tok::Plus,
                fl("1.0"),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_dash_dash_comment() {
        assert_eq!(
            kinds("1.0 -- a comment\n + 2.0"),
            vec![fl("1.0"), Tok::Newline, Tok::Plus, fl("2.0"), Tok::Eof]
        );
    }

    #[test]
    fn lexes_case_keyword() {
        assert_eq!(
            kinds("case e of { }"),
            vec![
                Tok::KwCase,
                Tok::Ident("e".into()),
                Tok::KwOf,
                Tok::LBrace,
                Tok::RBrace,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn case_is_keyword_before_paren() {
        // `case (f x) of …` — a parenthesized scrutinee must not turn the
        // keyword into a call-style identifier.
        assert_eq!(
            kinds("case (C 1.0) of { }"),
            vec![
                Tok::KwCase,
                Tok::LParen,
                Tok::Ident("C".into()),
                fl("1.0"),
                Tok::RParen,
                Tok::KwOf,
                Tok::LBrace,
                Tok::RBrace,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_double_colon() {
        assert_eq!(
            kinds("x :: Double"),
            vec![
                Tok::Ident("x".into()),
                Tok::DoubleColon,
                Tok::Ident("Double".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_backtick() {
        assert_eq!(
            kinds("1.0 `add` 2.0"),
            vec![
                fl("1.0"),
                Tok::Backtick,
                Tok::Ident("add".into()),
                Tok::Backtick,
                fl("2.0"),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_plusplus_as_one_token() {
        // `++` must not degrade into two `Tok::Plus`, and `+` must still work.
        assert_eq!(
            kinds("a ++ b"),
            vec![
                Tok::Ident("a".into()),
                Tok::PlusPlus,
                Tok::Ident("b".into()),
                Tok::Eof,
            ]
        );
        assert_eq!(
            kinds("a + b"),
            vec![
                Tok::Ident("a".into()),
                Tok::Plus,
                Tok::Ident("b".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_nested_block_comment_as_whitespace() {
        assert_eq!(
            kinds("1.0 {- a {- b -} c -} + 2.0"),
            vec![fl("1.0"), Tok::Plus, fl("2.0"), Tok::Eof]
        );
    }

    #[test]
    fn block_comment_keeps_newlines() {
        // A multi-line block comment must not fuse the surrounding lines into
        // one: the newline inside it still separates layout items.
        assert_eq!(
            kinds("x = 1.0; {- a\nb -} y = 2.0;"),
            vec![
                Tok::Ident("x".into()),
                Tok::Eq,
                fl("1.0"),
                Tok::Semi,
                Tok::Newline,
                Tok::Ident("y".into()),
                Tok::Eq,
                fl("2.0"),
                Tok::Semi,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn rejects_unterminated_block_comment() {
        let err = tokenize("{- a {- b -}").unwrap_err();
        match err {
            CompileError::Lex { msg, .. } => {
                assert!(msg.contains("unterminated block comment"), "{msg}")
            }
            other => panic!("expected a lex error, got {other}"),
        }
    }

    #[test]
    fn pragma_is_not_a_block_comment() {
        // `{-# … #-}` is a pragma even though it starts with `{-`.
        assert_eq!(
            kinds("{-# MODE complete #-}\n1.0"),
            vec![
                Tok::Pragma {
                    name: "MODE".into(),
                    value: "complete".into()
                },
                Tok::Newline,
                fl("1.0"),
                Tok::Eof,
            ]
        );
    }
}
