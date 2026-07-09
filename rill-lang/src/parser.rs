//! Recursive-descent + Pratt (operator-precedence) parser.

use crate::ast::{BinOp, Def, Expr, Param, Program};
use crate::error::{CompileError, Span};
use crate::lexer::{Tok, Token};

struct Parser<'a> {
    toks: &'a [Token],
    src: &'a [u8],
    pos: usize,
}

/// Binding powers. Higher = binds tighter. Returns (op, left_bp, right_bp).
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

/// Check if a token kind can start an atom (for juxtaposed application args).
fn is_atom_start(tok: &Tok) -> bool {
    matches!(
        tok,
        Tok::Ident(_)
            | Tok::Int(_)
            | Tok::Float(_)
            | Tok::Wire
            | Tok::Cut
            | Tok::Str(_)
            | Tok::LParen
            | Tok::LBrace
            | Tok::Minus
    )
}

impl<'a> Parser<'a> {
    fn new(toks: &'a [Token], src: &'a [u8]) -> Self {
        Self { toks, src, pos: 0 }
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

    fn expect_ident(&mut self) -> Result<(String, Span), CompileError> {
        let t = self.peek().clone();
        match t.tok {
            Tok::Ident(name) => {
                self.bump();
                Ok((name, t.span))
            }
            _ => Err(CompileError::Parse {
                msg: format!("expected identifier, found {:?}", t.tok),
                span: t.span,
            }),
        }
    }

    fn cur_col(&self) -> usize {
        let off = self.peek().span.start.min(self.src.len());
        let line_start = self.src[..off]
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        off - line_start
    }

    fn span_from(&self, start: usize) -> Span {
        if self.pos > 0 {
            Span::new(start, self.toks[self.pos - 1].span.end)
        } else {
            Span::new(start, start)
        }
    }

    fn error(&self, msg: &str) -> CompileError {
        CompileError::Parse {
            msg: msg.into(),
            span: self.peek().span,
        }
    }

    fn parse_program(&mut self) -> Result<Program, CompileError> {
        let mut defs = Vec::new();
        while self.peek().tok != Tok::Eof {
            defs.push(self.parse_top_def()?);
            if self.peek().tok == Tok::Semi {
                self.bump();
            }
            if self.peek().tok == Tok::Eof {
                break;
            }
        }
        if defs.is_empty() {
            return Err(CompileError::Parse {
                msg: "empty program (expected at least one definition)".into(),
                span: self.peek().span,
            });
        }
        if !defs.iter().any(|d| d.name() == "main") {
            return Err(CompileError::Parse {
                msg: "program must contain a `main` definition".into(),
                span: Span::new(0, self.src.len()),
            });
        }
        Ok(Program { defs })
    }

    fn parse_top_def(&mut self) -> Result<Def, CompileError> {
        let start = self.peek().span.start;
        let name = match &self.peek().tok {
            Tok::Ident(n) => {
                let n = n.clone();
                self.bump();
                n
            }
            Tok::KwMain => {
                self.bump();
                "main".to_string()
            }
            other => {
                return Err(CompileError::Parse {
                    msg: format!("expected definition name, found {other:?}"),
                    span: self.peek().span,
                })
            }
        };
        let mut params = Vec::new();
        while let Tok::Ident(_) = self.peek().tok {
            let (pname, pspan) = self.expect_ident()?;
            params.push(Param {
                name: pname,
                span: pspan,
            });
        }
        self.eat(&Tok::Eq)?;
        let body = self.parse_expr(0, false)?;

        let where_defs = if self.peek().tok == Tok::KwWhere {
            self.bump();
            self.parse_where_block()?
        } else {
            vec![]
        };

        let span = Span::new(start, body.span().end);

        if params.is_empty() {
            Ok(Def::Local {
                name,
                body,
                where_defs,
                span,
            })
        } else {
            Ok(Def::Anchor {
                name,
                params,
                body,
                where_defs,
                span,
            })
        }
    }

    fn parse_def(&mut self) -> Result<Def, CompileError> {
        self.parse_top_def()
    }

    fn parse_where_block(&mut self) -> Result<Vec<Def>, CompileError> {
        let mut defs = Vec::new();
        if self.peek().tok == Tok::LBrace {
            self.bump();
            loop {
                if self.peek().tok == Tok::RBrace {
                    break;
                }
                let d = self.parse_def()?;
                self.eat(&Tok::Semi)?;
                defs.push(d);
                if self.peek().tok == Tok::RBrace {
                    break;
                }
            }
            self.eat(&Tok::RBrace)?;
        } else {
            let layout_col = self.cur_col();
            while self.peek().tok != Tok::Eof
                && self.peek().tok != Tok::KwIn
                && self.cur_col() >= layout_col
            {
                let d = self.parse_def()?;
                defs.push(d);
                if self.peek().tok == Tok::Semi {
                    self.bump();
                } else if self.peek().tok == Tok::Eof
                    || self.peek().tok == Tok::KwIn
                    || self.cur_col() < layout_col
                {
                    break;
                }
            }
        }
        Ok(defs)
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
            Tok::KwLet => {
                self.bump();
                let defs = self.parse_where_block()?;
                self.eat(&Tok::KwIn)?;
                let body = self.parse_expr(0, no_comma)?;
                let span = t.span.merge(body.span());
                Ok(Expr::Let {
                    defs,
                    body: Box::new(body),
                    span,
                })
            }
            Tok::Minus => {
                self.bump();
                let inner = self.parse_expr(15, no_comma)?;
                let span = t.span.merge(inner.span());
                Ok(Expr::Neg(Box::new(inner), span))
            }
            Tok::Ident(name) => {
                self.bump();
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
                } else if is_atom_start(&self.peek().tok) {
                    let mut args = Vec::new();
                    while is_atom_start(&self.peek().tok) {
                        args.push(self.parse_atom()?);
                    }
                    let span = t.span.merge(args.last().unwrap().span());
                    Ok(Expr::Apply { name, args, span })
                } else {
                    Ok(Expr::Ref(name, t.span))
                }
            }
            _ => self.parse_atom(),
        }
    }

    fn parse_record(&mut self) -> Result<Expr, CompileError> {
        let start = self.eat(&Tok::LBrace)?.span.start;
        let mut fields = Vec::new();

        if self.peek().tok == Tok::RBrace {
            self.bump();
            return Ok(Expr::Record(fields, self.span_from(start)));
        }

        loop {
            let (key, _) = self.expect_ident()?;
            self.eat(&Tok::Colon)?;
            let val = self.parse_expr(0, true)?;
            fields.push((key, val));

            if self.peek().tok == Tok::Comma {
                self.bump();
                if self.peek().tok == Tok::RBrace {
                    break;
                }
            } else if self.peek().tok == Tok::RBrace {
                break;
            } else {
                return Err(self.error("expected ',' or '}' in record literal"));
            }
        }

        self.eat(&Tok::RBrace)?;
        Ok(Expr::Record(fields, self.span_from(start)))
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
            Tok::LBrace => {
                // rewind — parse_record handles the opening brace
                self.pos -= 1;
                self.parse_record()
            }
            other => Err(CompileError::Parse {
                msg: format!("unexpected token {other:?}"),
                span: t.span,
            }),
        }
    }
}

/// Parse a complete program (list of mutually-recursive top-level definitions).
pub fn parse(tokens: &[Token], src: &[u8]) -> Result<Program, CompileError> {
    Parser::new(tokens, src).parse_program()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn prog(src: &str) -> Program {
        parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap()
    }
    fn body(src: &str) -> Expr {
        let p = prog(src);
        let main = p.main_def().expect("no main def");
        main.body().clone()
    }

    #[test]
    fn parses_main_without_params() {
        let p = prog("main = _ * 0.5");
        let main = p.main_def().unwrap();
        assert_eq!(main.params().len(), 0);
    }

    #[test]
    fn parses_main_with_params() {
        let p = prog("main regs x = _ * 0.5");
        let main = p.main_def().unwrap();
        assert_eq!(main.params().len(), 2);
        assert_eq!(main.params()[0].name, "regs");
        assert_eq!(main.params()[1].name, "x");
    }

    #[test]
    fn parses_main_with_where_block() {
        let p = prog(
            "main = osc : lpf where { osc freq = sin freq * 0.5; lpf cut = lowpass _ cut 0.7; }",
        );
        let main = p.main_def().unwrap();
        assert_eq!(main.where_defs().len(), 2);
        match &main.where_defs()[0] {
            Def::Anchor { name, params, .. } => {
                assert_eq!(name, "osc");
                assert_eq!(params.len(), 1);
            }
            _ => panic!("expected Anchor"),
        }
        match &main.where_defs()[1] {
            Def::Anchor { name, params, .. } => {
                assert_eq!(name, "lpf");
                assert_eq!(params.len(), 1);
            }
            _ => panic!("expected Anchor"),
        }
    }

    #[test]
    fn parses_where_local_binding() {
        let p = prog("main = osc where { freq = 440; }");
        let main = p.main_def().unwrap();
        assert_eq!(main.where_defs().len(), 1);
        matches!(&main.where_defs()[0], Def::Local { name, .. } if name == "freq");
    }

    #[test]
    fn arithmetic_binds_tighter_than_par() {
        match body("main = _ * 2 , _") {
            Expr::Bin { op: BinOp::Par, .. } => {}
            other => panic!("expected Par, got {other:?}"),
        }
    }

    #[test]
    fn feedback_binds_loosest() {
        match body("main = + ~ _") {
            Expr::Bin {
                op: BinOp::Feedback,
                ..
            } => {}
            other => panic!("expected Feedback, got {other:?}"),
        }
    }

    #[test]
    fn seq_is_left_associative() {
        match body("main = _ : _ : _") {
            Expr::Bin { op: BinOp::Seq, .. } => {}
            other => panic!("expected Seq, got {other:?}"),
        }
    }

    #[test]
    fn application_uses_comma_as_arg_separator() {
        let p = prog("main = gain(_ , 2)");
        let main = p.main_def().unwrap();
        match main.body() {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "gain");
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn grouping_paren_is_parallel_inside() {
        match body("main = (_ , _) :> _") {
            Expr::Bin {
                op: BinOp::Merge, ..
            } => {}
            other => panic!("expected Merge, got {other:?}"),
        }
    }

    #[test]
    fn application_arg_may_be_a_composed_expression() {
        let p = prog("main = f(_ : _, 2)");
        match p.main_def().unwrap().body() {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "f");
                assert_eq!(args.len(), 2);
                assert!(matches!(args[0], Expr::Bin { op: BinOp::Seq, .. }));
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn juxtaposition_parse() {
        let p = prog("main regs = ay38910 1750000.0 regs : lofi 8 44100 0.75 1.0 1 0 1");
        match p.main_def().unwrap().body() {
            Expr::Bin { op: BinOp::Seq, .. } => {}
            other => panic!("expected Seq, got {other:?}"),
        }
    }

    #[test]
    fn parses_string_arg() {
        let p = parse(
            &tokenize(r#"main = f("x")"#).unwrap(),
            r#"main = f("x")"#.as_bytes(),
        )
        .unwrap();
        match p.main_def().unwrap().body() {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "f");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_main() {
        assert!(parse(&tokenize("_ * 0.5").unwrap(), "_ * 0.5".as_bytes()).is_err());
    }

    #[test]
    fn parses_top_level_multi_def() {
        let p = prog("sq x = x * x; main = sq _");
        assert_eq!(p.defs.len(), 2);
        assert_eq!(p.defs[0].name(), "sq");
        assert_eq!(p.defs[1].name(), "main");
    }

    #[test]
    fn parses_top_level_multi_def_no_semicolon() {
        let p = prog("gain = _ * 0.5; main = gain");
        assert_eq!(p.defs.len(), 2);
    }

    #[test]
    fn parses_let_expression() {
        let p = prog("main = let gain = _ * 0.5 in gain");
        let main = p.main_def().unwrap();
        match main.body() {
            Expr::Let { defs, body, .. } => {
                assert_eq!(defs.len(), 1);
                assert_eq!(defs[0].name(), "gain");
                match body.as_ref() {
                    Expr::Ref(name, _) => assert_eq!(name, "gain"),
                    _ => panic!("expected Ref"),
                }
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn parses_let_with_braces() {
        let p = prog("main = let { g = _ * 0.5; } in g");
        let main = p.main_def().unwrap();
        assert!(matches!(main.body(), Expr::Let { .. }));
    }

    #[test]
    fn main_with_where_and_top_level() {
        let p = prog("gain = _ * 0.5; main = gain where { x = 1; }");
        assert_eq!(p.defs.len(), 2);
        let main = p.main_def().unwrap();
        assert_eq!(main.where_defs().len(), 1);
    }

    #[test]
    fn parse_simple_record() {
        match body("main = mixer { channels: 3 }") {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "mixer");
                assert_eq!(args.len(), 1);
                match &args[0] {
                    Expr::Record(fields, _) => {
                        assert_eq!(fields.len(), 1);
                        assert_eq!(fields[0].0, "channels");
                        assert!(matches!(fields[0].1, Expr::Int(3, _)));
                    }
                    other => panic!("expected Record, got {other:?}"),
                }
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn parse_nested_record() {
        match body("main = mixer { ch: { vol: 0.8 } }") {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "mixer");
                match &args[0] {
                    Expr::Record(fields, _) => {
                        assert_eq!(fields.len(), 1);
                        assert_eq!(fields[0].0, "ch");
                        match &fields[0].1 {
                            Expr::Record(inner, _) => {
                                assert_eq!(inner.len(), 1);
                                assert_eq!(inner[0].0, "vol");
                            }
                            other => panic!("expected nested Record, got {other:?}"),
                        }
                    }
                    other => panic!("expected Record, got {other:?}"),
                }
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn parse_empty_record() {
        match body("main = mixer { }") {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "mixer");
                match &args[0] {
                    Expr::Record(fields, _) => {
                        assert_eq!(fields.len(), 0);
                    }
                    other => panic!("expected Record, got {other:?}"),
                }
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn parse_multi_field_record() {
        match body("main = mixer { channels: 3, gain: 0.8 }") {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "mixer");
                match &args[0] {
                    Expr::Record(fields, _) => {
                        assert_eq!(fields.len(), 2);
                        assert_eq!(fields[0].0, "channels");
                        assert_eq!(fields[1].0, "gain");
                    }
                    other => panic!("expected Record, got {other:?}"),
                }
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn parse_record_with_trailing_comma() {
        match body("main = mixer { channels: 3, }") {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "mixer");
                match &args[0] {
                    Expr::Record(fields, _) => {
                        assert_eq!(fields.len(), 1);
                    }
                    other => panic!("expected Record, got {other:?}"),
                }
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }
}
