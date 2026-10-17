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
    /// `true` keyword.
    KwTrue,
    /// `false` keyword.
    KwFalse,
    /// `do` keyword — monadic sequencing block.
    KwDo,
    /// `<-` — do-block monadic binding.
    LArrow,
    /// `\` — lambda literal `\x -> body`.
    Backslash,
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
                "data" if !followed_by_paren => Tok::KwData,
                "type" if !followed_by_paren => Tok::KwType,
                "newtype" if !followed_by_paren => Tok::KwNewtype,
                "typeclass" if !followed_by_paren => Tok::KwTypeclass,
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
                "true" if !followed_by_paren => Tok::KwTrue,
                "false" if !followed_by_paren => Tok::KwFalse,
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
                Tok::KwTrue,
                Tok::KwFalse,
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
                Tok::Float(1.0),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_dash_dash_comment() {
        assert_eq!(
            kinds("1.0 -- a comment\n + 2.0"),
            vec![
                Tok::Float(1.0),
                Tok::Newline,
                Tok::Plus,
                Tok::Float(2.0),
                Tok::Eof
            ]
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
                Tok::Float(1.0),
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
            kinds("x :: Float"),
            vec![
                Tok::Ident("x".into()),
                Tok::DoubleColon,
                Tok::Ident("Float".into()),
                Tok::Eof,
            ]
        );
    }
}
