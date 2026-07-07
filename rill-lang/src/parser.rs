//! Recursive-descent + Pratt (operator-precedence) parser.

use crate::ast::{BinOp, Def, Expr, Program};
use crate::error::{CompileError, Span};
use crate::lexer::{Tok, Token};

struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
}

/// Binding powers. Higher = binds tighter. Returns (op, left_bp, right_bp).
/// Left-associative ops use right_bp = left_bp + 1.
fn infix_binding_power(t: &Tok) -> Option<(BinOp, u8, u8)> {
    Some(match t {
        Tok::Tilde => (BinOp::Feedback, 1, 2),
        Tok::Colon => (BinOp::Seq, 3, 4),
        Tok::Merge => (BinOp::Merge, 5, 6),
        Tok::Split => (BinOp::Split, 7, 8),
        Tok::Comma => (BinOp::Par, 9, 10),
        Tok::Plus => (BinOp::Add, 11, 12),
        Tok::Minus => (BinOp::Sub, 11, 12),
        Tok::Star => (BinOp::Mul, 13, 14),
        Tok::Slash => (BinOp::Div, 13, 14),
        Tok::Percent => (BinOp::Rem, 13, 14),
        Tok::At => (BinOp::Delay, 15, 16),
        _ => return None,
    })
}

impl<'a> Parser<'a> {
    fn new(toks: &'a [Token]) -> Self {
        Self { toks, pos: 0 }
    }
    fn peek(&self) -> &Token {
        &self.toks[self.pos]
    }
    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }
    fn eat(&mut self, want: &Tok) -> Result<Token, CompileError> {
        if &self.peek().tok == want {
            Ok(self.bump())
        } else {
            let p = self.peek();
            Err(CompileError::Parse {
                msg: format!("expected {want:?}, found {:?}", p.tok),
                span: p.span,
            })
        }
    }

    fn parse_program(&mut self) -> Result<Program, CompileError> {
        let mut defs = Vec::new();
        while self.peek().tok != Tok::Eof {
            defs.push(self.parse_def()?);
        }
        if defs.is_empty() {
            return Err(CompileError::Parse {
                msg: "empty program (expected at least `process = ...;`)".into(),
                span: Span::new(0, 0),
            });
        }
        Ok(Program { defs })
    }

    fn parse_def(&mut self) -> Result<Def, CompileError> {
        let name_tok = self.peek().clone();
        let (name, start) = match &name_tok.tok {
            Tok::Ident(n) => (n.clone(), name_tok.span.start),
            _ => {
                return Err(CompileError::Parse {
                    msg: format!("expected definition name, found {:?}", name_tok.tok),
                    span: name_tok.span,
                })
            }
        };
        self.bump();
        let mut params = Vec::new();
        if self.peek().tok == Tok::LParen {
            self.bump();
            loop {
                match &self.peek().tok {
                    Tok::Ident(p) => {
                        params.push(p.clone());
                        self.bump();
                    }
                    other => {
                        return Err(CompileError::Parse {
                            msg: format!("expected parameter name, found {other:?}"),
                            span: self.peek().span,
                        })
                    }
                }
                match self.peek().tok {
                    Tok::Comma => {
                        self.bump();
                    }
                    Tok::RParen => break,
                    _ => {
                        return Err(CompileError::Parse {
                            msg: "expected `,` or `)` in parameter list".into(),
                            span: self.peek().span,
                        })
                    }
                }
            }
            self.eat(&Tok::RParen)?;
        }
        self.eat(&Tok::Eq)?;
        let body = self.parse_expr(0, false)?;
        let semi = self.eat(&Tok::Semi)?;
        Ok(Def {
            name,
            params,
            body,
            span: Span::new(start, semi.span.end),
        })
    }

    /// Pratt loop. When `no_comma` is set, a top-level `,` terminates the
    /// expression instead of being parsed as the `Par` combinator — used inside
    /// an application's argument list where `,` is a separator. Grouping parens
    /// reset this so `,` means `Par` again.
    fn parse_expr(&mut self, min_bp: u8, no_comma: bool) -> Result<Expr, CompileError> {
        let mut lhs = self.parse_prefix(no_comma)?;
        while let Some((op, l_bp, r_bp)) = infix_binding_power(&self.peek().tok) {
            if no_comma && op == BinOp::Par {
                break;
            }
            if l_bp < min_bp {
                break;
            }
            self.bump();
            let rhs = self.parse_expr(r_bp, no_comma)?;

            // Desugar `Float + Imag` / `Int + Imag` / `Float - Imag` → complex(re, im)
            if matches!(op, BinOp::Add | BinOp::Sub) {
                let re = match &lhs {
                    Expr::Float(v, _) => Some(*v),
                    Expr::Int(v, _) => Some(*v as f64),
                    _ => None,
                };
                let im = match &rhs {
                    Expr::Imag(v, _) => Some(if matches!(op, BinOp::Sub) { -*v } else { *v }),
                    _ => None,
                };
                if let (Some(re), Some(im)) = (re, im) {
                    let span = lhs.span().merge(rhs.span());
                    lhs = Expr::Apply {
                        name: "complex".to_string(),
                        args: vec![
                            Expr::Float(re, Span::new(0, 0)),
                            Expr::Float(im, Span::new(0, 0)),
                        ],
                        span,
                    };
                    continue;
                }
            }

            let span = lhs.span().merge(rhs.span());
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_prefix(&mut self, no_comma: bool) -> Result<Expr, CompileError> {
        let t = self.peek().clone();
        match t.tok {
            Tok::Minus => {
                self.bump();
                let inner = self.parse_expr(15, no_comma)?;
                let span = t.span.merge(inner.span());
                Ok(Expr::Neg(Box::new(inner), span))
            }
            _ => self.parse_atom(),
        }
    }

    fn parse_atom(&mut self) -> Result<Expr, CompileError> {
        let t = self.bump();
        match t.tok {
            Tok::Int(v) => Ok(Expr::Int(v, t.span)),
            Tok::Float(v) => Ok(Expr::Float(v, t.span)),
            Tok::Imag(v) => Ok(Expr::Imag(v, t.span)),
            Tok::Wire => Ok(Expr::Wire(t.span)),
            Tok::Cut => Ok(Expr::Cut(t.span)),
            Tok::Str(s) => Ok(Expr::Str(s, t.span)),
            Tok::Plus => Ok(Expr::Ref("+".into(), t.span)),
            Tok::Minus => Ok(Expr::Ref("-".into(), t.span)),
            Tok::Star => Ok(Expr::Ref("*".into(), t.span)),
            Tok::Slash => Ok(Expr::Ref("/".into(), t.span)),
            Tok::Percent => Ok(Expr::Ref("%".into(), t.span)),
            Tok::Ident(name) => {
                if self.peek().tok == Tok::LParen {
                    self.bump();
                    let mut args = Vec::new();
                    if self.peek().tok != Tok::RParen {
                        loop {
                            args.push(self.parse_expr(0, true)?);
                            match self.peek().tok {
                                Tok::Comma => {
                                    self.bump();
                                }
                                _ => break,
                            }
                        }
                    }
                    let rp = self.eat(&Tok::RParen)?;
                    Ok(Expr::Apply {
                        name,
                        args,
                        span: t.span.merge(rp.span),
                    })
                } else {
                    Ok(Expr::Ref(name, t.span))
                }
            }
            Tok::LParen => {
                let inner = self.parse_expr(0, false)?;
                self.eat(&Tok::RParen)?;
                Ok(inner)
            }
            other => Err(CompileError::Parse {
                msg: format!("unexpected token {other:?}"),
                span: t.span,
            }),
        }
    }
}

/// Parse a complete program (`name = expr; ...`).
pub fn parse(tokens: &[Token]) -> Result<Program, CompileError> {
    Parser::new(tokens).parse_program()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn prog(src: &str) -> Program {
        parse(&tokenize(src).unwrap()).unwrap()
    }
    fn body(src: &str) -> Expr {
        let p = prog(src);
        p.defs
            .into_iter()
            .find(|d| d.name == "process")
            .unwrap()
            .body
    }

    #[test]
    fn parses_single_def() {
        let p = prog("process = _;");
        assert_eq!(p.defs.len(), 1);
        assert_eq!(p.defs[0].name, "process");
        assert!(matches!(p.defs[0].body, Expr::Wire(_)));
    }

    #[test]
    fn arithmetic_binds_tighter_than_par() {
        match body("process = _ * 2 , _;") {
            Expr::Bin {
                op: BinOp::Par,
                lhs,
                ..
            } => {
                assert!(matches!(*lhs, Expr::Bin { op: BinOp::Mul, .. }));
            }
            other => panic!("expected top Par, got {other:?}"),
        }
    }

    #[test]
    fn feedback_binds_loosest() {
        match body("process = + ~ _;") {
            Expr::Bin {
                op: BinOp::Feedback,
                lhs,
                rhs,
                ..
            } => {
                assert!(matches!(*lhs, Expr::Ref(_, _)) || matches!(*lhs, Expr::Bin { .. }));
                assert!(matches!(*rhs, Expr::Wire(_)));
            }
            other => panic!("expected Feedback at top, got {other:?}"),
        }
    }

    #[test]
    fn seq_is_left_associative() {
        match body("process = _ : _ : _;") {
            Expr::Bin {
                op: BinOp::Seq,
                lhs,
                ..
            } => {
                assert!(matches!(*lhs, Expr::Bin { op: BinOp::Seq, .. }));
            }
            other => panic!("expected Seq at top, got {other:?}"),
        }
    }

    #[test]
    fn application_uses_comma_as_arg_separator() {
        let p = prog("gain(x, y) = x; process = gain(_, 2);");
        let call = p.defs.iter().find(|d| d.name == "process").unwrap();
        match &call.body {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "gain");
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn grouping_paren_is_parallel_inside() {
        match body("process = (_ , _) :> _;") {
            Expr::Bin {
                op: BinOp::Merge,
                lhs,
                ..
            } => {
                assert!(matches!(*lhs, Expr::Bin { op: BinOp::Par, .. }));
            }
            other => panic!("expected Merge, got {other:?}"),
        }
    }

    #[test]
    fn application_arg_may_be_a_composed_expression() {
        let p = prog("f(a, b) = a; process = f(_ : _, 2);");
        let call = p.defs.iter().find(|d| d.name == "process").unwrap();
        match &call.body {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "f");
                assert_eq!(args.len(), 2);
                assert!(matches!(args[0], Expr::Bin { op: BinOp::Seq, .. }));
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_semicolon() {
        assert!(parse(&tokenize("process = _").unwrap()).is_err());
    }

    #[test]
    fn parses_string_arg() {
        let p = parse(&tokenize(r#"process = f("x");"#).unwrap()).unwrap();
        let call = p.defs.iter().find(|d| d.name == "process").unwrap();
        match &call.body {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "f");
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0], Expr::Str(s, _) if s == "x"));
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }
}
