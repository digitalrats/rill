//! Hand-written tokeniser. Produces `Token`s carrying source spans.

use crate::error::{CompileError, Span};

/// A lexical token kind.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// Numeric literal that contains a `.` or exponent — a float.
    Float(f64),
    /// Numeric literal with no `.` — an integer.
    Int(i64),
    /// Identifier / keyword (`sin`, `min`, `process`, user names).
    Ident(String),
    /// String literal, e.g. `"cutoff"`.
    Str(String),
    /// `_`
    Wire,
    /// `!`
    Cut,
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
    /// `main` keyword — entry point.
    KwMain,
    /// `where` keyword — optional definition block.
    KwWhere,
    /// `let` keyword — expression-level mutually-recursive bindings.
    KwLet,
    /// `in` keyword — separator in `let defs in expr`.
    KwIn,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `?`
    Question,
    /// End of input.
    Eof,
    /// Imaginary literal, e.g. `3i`, `2.5i`.
    Imag(f64),
}

/// A token plus its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// The token kind.
    pub tok: Tok,
    /// Where it came from.
    pub span: Span,
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

    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
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
                let v: f64 = text.parse().map_err(|_| CompileError::Lex {
                    msg: format!("invalid float literal `{text}`"),
                    span,
                })?;
                out.push(Token {
                    tok: Tok::Float(v),
                    span,
                });
            } else {
                let v: i64 = text.parse().map_err(|_| CompileError::Lex {
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
                "main" if !followed_by_paren => Tok::KwMain,
                "where" if !followed_by_paren => Tok::KwWhere,
                "let" if !followed_by_paren => Tok::KwLet,
                "in" if !followed_by_paren => Tok::KwIn,
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
            b':' => Tok::Colon,
            b'~' => Tok::Tilde,
            b'@' => Tok::At,
            b',' => Tok::Comma,
            b'+' => Tok::Plus,
            b'-' => Tok::Minus,
            b'*' => Tok::Star,
            b'/' => Tok::Slash,
            b'%' => Tok::Percent,
            b'!' => Tok::Cut,
            b'?' => Tok::Question,
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b'{' => Tok::LBrace,
            b'}' => Tok::RBrace,
            b'=' => Tok::Eq,
            b';' => Tok::Semi,
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
                Tok::Cut,
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
            vec![Tok::Int(3), Tok::Float(3.5), Tok::Int(10), Tok::Eof]
        );
    }

    #[test]
    fn lexes_idents_and_skips_comments() {
        assert_eq!(
            kinds("process // a comment\n sin"),
            vec![
                Tok::Ident("process".into()),
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
    fn lexes_main_keyword() {
        assert_eq!(
            kinds("main foo bar"),
            vec![
                Tok::KwMain,
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
                Tok::Int(440),
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
                Tok::Int(1),
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
}
