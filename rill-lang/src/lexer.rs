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
    /// End of input.
    Eof,
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
            if is_float {
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
            let tok = if text == "_" {
                Tok::Wire
            } else {
                Tok::Ident(text.to_string())
            };
            out.push(Token { tok, span });
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
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
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
            kinds("_ : + <: :> ~ @ , * / % ! ( ) = ;"),
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
                Tok::LParen,
                Tok::RParen,
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
}
