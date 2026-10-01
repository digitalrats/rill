//! Recursive-descent + Pratt (operator-precedence) parser.

use crate::ast::{ArithOp, CmpOp, Def, Expr, LogicOp, MatchArm, Param, Pattern, Program, TypeExpr};
use crate::error::{CompileError, Span};
use crate::lexer::{Tok, Token};

/// An infix operator: a block-diagram combinator or an arithmetic operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfixOp {
    Seq,
    Par,
    Split,
    Merge,
    Loop,
    Delay,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    CmpEq,
    CmpNe,
    CmpLt,
    CmpGt,
    CmpLe,
    CmpGe,
    LogicAnd,
    LogicOr,
}

impl InfixOp {
    fn is_arith(self) -> bool {
        matches!(
            self,
            InfixOp::Add | InfixOp::Sub | InfixOp::Mul | InfixOp::Div | InfixOp::Rem
        )
    }

    fn to_arith(self) -> ArithOp {
        match self {
            InfixOp::Add => ArithOp::Add,
            InfixOp::Sub => ArithOp::Sub,
            InfixOp::Mul => ArithOp::Mul,
            InfixOp::Div => ArithOp::Div,
            InfixOp::Rem => ArithOp::Rem,
            _ => unreachable!("not an arithmetic operator"),
        }
    }

    fn is_logic(self) -> bool {
        matches!(self, InfixOp::LogicAnd | InfixOp::LogicOr)
    }

    fn to_logic(self) -> LogicOp {
        match self {
            InfixOp::LogicAnd => LogicOp::And,
            InfixOp::LogicOr => LogicOp::Or,
            _ => unreachable!("not a logic operator"),
        }
    }

    fn is_cmp(self) -> bool {
        matches!(
            self,
            InfixOp::CmpEq
                | InfixOp::CmpNe
                | InfixOp::CmpLt
                | InfixOp::CmpGt
                | InfixOp::CmpLe
                | InfixOp::CmpGe
        )
    }

    fn to_cmp(self) -> CmpOp {
        match self {
            InfixOp::CmpEq => CmpOp::Eq,
            InfixOp::CmpNe => CmpOp::Ne,
            InfixOp::CmpLt => CmpOp::Lt,
            InfixOp::CmpGt => CmpOp::Gt,
            InfixOp::CmpLe => CmpOp::Le,
            InfixOp::CmpGe => CmpOp::Ge,
            _ => unreachable!("not a comparison operator"),
        }
    }
}

struct Parser<'a> {
    toks: &'a [Token],
    src: &'a [u8],
    pos: usize,
}

/// Binding powers. Higher = binds tighter. Returns (op, left_bp, right_bp).
fn infix_binding_power(t: &Tok) -> Option<(InfixOp, u8, u8)> {
    Some(match t {
        Tok::Tilde => (InfixOp::Loop, 1, 2),
        Tok::AndAnd => (InfixOp::LogicAnd, 1, 2),
        Tok::OrOr => (InfixOp::LogicOr, 1, 2),
        Tok::Colon => (InfixOp::Seq, 3, 4),
        Tok::EqEq => (InfixOp::CmpEq, 3, 4),
        Tok::NotEq => (InfixOp::CmpNe, 3, 4),
        Tok::Lt => (InfixOp::CmpLt, 3, 4),
        Tok::Gt => (InfixOp::CmpGt, 3, 4),
        Tok::Le => (InfixOp::CmpLe, 3, 4),
        Tok::Ge => (InfixOp::CmpGe, 3, 4),
        Tok::Merge => (InfixOp::Merge, 5, 6),
        Tok::Split => (InfixOp::Split, 7, 8),
        Tok::Comma => (InfixOp::Par, 9, 10),
        Tok::Plus => (InfixOp::Add, 11, 12),
        Tok::Minus => (InfixOp::Sub, 11, 12),
        Tok::Star => (InfixOp::Mul, 13, 14),
        Tok::Slash => (InfixOp::Div, 13, 14),
        Tok::Percent => (InfixOp::Rem, 13, 14),
        Tok::At => (InfixOp::Delay, 15, 16),
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
            | Tok::LBracket
            | Tok::KwTrue
            | Tok::KwFalse
            | Tok::Minus
            | Tok::Question
    )
}

/// Check if a token can begin a (sub)pattern.
fn is_pattern_start(tok: &Tok) -> bool {
    matches!(
        tok,
        Tok::Ident(_)
            | Tok::Int(_)
            | Tok::Float(_)
            | Tok::Str(_)
            | Tok::KwTrue
            | Tok::KwFalse
            | Tok::Wire
            | Tok::LParen
    )
}

/// One statement inside a `do { … }` block.
enum DoStmt {
    /// `x <- e` — monadic bind: `bind e (fn x -> rest)`.
    Bind(String, Expr),
    /// `let x = e` — inline binding: `let x = e in rest`.
    Let(String, Expr),
    /// `e;` — bare statement: `bind e (fn _ -> rest)`.
    Stmt(Expr),
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

    fn pos(&self) -> usize {
        self.pos
    }

    fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }

    fn parse_pattern(&mut self) -> Result<Pattern, CompileError> {
        let t = self.peek().clone();
        match &t.tok {
            Tok::Wire => {
                self.bump();
                Ok(Pattern::Wild)
            }
            Tok::Int(v) => {
                self.bump();
                Ok(Pattern::LitInt(*v))
            }
            Tok::Float(v) => {
                self.bump();
                Ok(Pattern::LitFloat(*v))
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Pattern::LitStr(s.clone()))
            }
            Tok::KwTrue => {
                self.bump();
                Ok(Pattern::LitBool(true))
            }
            Tok::KwFalse => {
                self.bump();
                Ok(Pattern::LitBool(false))
            }
            Tok::LParen => {
                self.bump();
                let p = self.parse_pattern()?;
                self.eat(&Tok::RParen)?;
                Ok(p)
            }
            Tok::Ident(name) => {
                let is_ctor = name.chars().next().is_some_and(|c| c.is_uppercase());
                self.bump();
                if is_ctor {
                    let mut args = Vec::new();
                    while is_pattern_start(&self.peek().tok) {
                        args.push(self.parse_pattern()?);
                    }
                    Ok(Pattern::Ctor(name.clone(), args))
                } else {
                    Ok(Pattern::Var(name.clone()))
                }
            }
            _ => Err(CompileError::Parse {
                msg: format!("expected a pattern, found {:?}", t.tok),
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
        let t = self.peek().clone();
        match t.tok {
            Tok::KwData => return self.parse_data_def(),
            Tok::KwType => return self.parse_type_alias_def(),
            Tok::KwNewtype => return self.parse_newtype_def(),
            Tok::KwTypeclass => return self.parse_typeclass_def(),
            Tok::KwInstance => return self.parse_instance_def(),
            _ => {}
        }
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

    /// `data Name = ...` — product or sum type declaration.
    fn parse_data_def(&mut self) -> Result<Def, CompileError> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident()?;
        let mut tyvars = Vec::new();
        while matches!(self.peek().tok, Tok::Ident(_)) {
            let (tv, _) = self.expect_ident()?;
            tyvars.push(tv);
        }
        self.eat(&Tok::Eq)?;
        if self.peek().tok == Tok::LBrace {
            // product: { f1: T1, f2: T2 }
            self.bump();
            let mut fields = Vec::new();
            while self.peek().tok != Tok::RBrace {
                let (fname, _) = self.expect_ident()?;
                self.eat(&Tok::Colon)?;
                let t = self.parse_type_expr()?;
                fields.push((fname, t));
                if self.peek().tok == Tok::Comma {
                    self.bump();
                }
            }
            self.eat(&Tok::RBrace)?;
            Ok(Def::Data {
                name,
                tyvars,
                fields,
                span: self.span_from(start),
            })
        } else {
            // sum: C1 T1 | C2 T2 T3
            let mut ctors = Vec::new();
            while self.peek().tok != Tok::Semi && self.peek().tok != Tok::Eof {
                let (cname, _) = self.expect_ident()?;
                let mut payload = Vec::new();
                while matches!(self.peek().tok, Tok::Ident(_) | Tok::Int(_)) {
                    payload.push(self.parse_type_single()?);
                }
                ctors.push((cname, payload));
                if self.peek().tok == Tok::Pipe {
                    self.bump();
                }
            }
            Ok(Def::Sum {
                name,
                tyvars,
                ctors,
                span: self.span_from(start),
            })
        }
    }

    /// Parse the inside of a `(…)` type grouping: a single grouped type
    /// expression, or `(b, d)` tuple sugar desugared to `TApp("Pair", [b, d])`.
    /// Only the binary tuple is supported. The opening `LParen` must already be
    /// consumed; this eats the closing `RParen`.
    fn parse_paren_type(&mut self) -> Result<TypeExpr, CompileError> {
        let first = self.parse_type_expr()?;
        if self.peek().tok == Tok::Comma {
            let mut items = vec![first];
            while self.peek().tok == Tok::Comma {
                self.bump();
                items.push(self.parse_type_single()?);
            }
            if items.len() != 2 {
                return Err(self.error("tuples in types are binary (use Pair)"));
            }
            self.eat(&Tok::RParen)?;
            return Ok(TypeExpr::TApp("Pair".into(), items));
        }
        self.eat(&Tok::RParen)?;
        Ok(first)
    }

    /// Parse a single, non-absorbing type atom: a bare type or type-variable
    /// name, or a parenthesized type expression. It does not consume following
    /// juxtaposed atoms — the caller's application and currying loops collect
    /// those.
    fn parse_type_single(&mut self) -> Result<TypeExpr, CompileError> {
        let t = self.peek().clone();
        match t.tok {
            Tok::LParen => {
                self.bump();
                self.parse_paren_type()
            }
            Tok::Ident(name) => {
                self.bump();
                Ok(TypeExpr::TName(name))
            }
            other => Err(self.error(&format!("expected type expression, found {other:?}"))),
        }
    }

    /// Parse a type atom: a single atom, or a name applied to juxtaposed atoms
    /// (`List Float`, `f a`). Juxtaposed arguments stay flat — each is one
    /// non-absorbing [`Parser::parse_type_single`].
    fn parse_type_atom(&mut self) -> Result<TypeExpr, CompileError> {
        let t = self.peek().clone();
        match t.tok {
            Tok::LParen => {
                self.bump();
                self.parse_paren_type()
            }
            Tok::Ident(name) => {
                self.bump();
                let mut args = Vec::new();
                while matches!(self.peek().tok, Tok::Ident(_) | Tok::LParen) {
                    args.push(self.parse_type_single()?);
                }
                if args.is_empty() {
                    Ok(TypeExpr::TName(name))
                } else {
                    Ok(TypeExpr::TApp(name, args))
                }
            }
            other => Err(self.error(&format!("expected type expression, found {other:?}"))),
        }
    }

    /// Parse a type expression: a chain of juxta-applied type names and type
    /// variables, `(T -> U -> V)` curried function types, and capacity ints.
    fn parse_type_expr(&mut self) -> Result<TypeExpr, CompileError> {
        let first = self.parse_type_atom()?;
        if self.peek().tok == Tok::FatArrow {
            let mut args = vec![first];
            while self.peek().tok == Tok::FatArrow {
                self.bump();
                args.push(self.parse_type_atom()?);
            }
            let ret = args.pop().expect("function type needs a result");
            Ok(TypeExpr::TFunc(args, Box::new(ret)))
        } else {
            Ok(first)
        }
    }

    /// `type Name = T` — synonym (pure substitution).
    fn parse_type_alias_def(&mut self) -> Result<Def, CompileError> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident()?;
        self.eat(&Tok::Eq)?;
        let (target, _) = self.expect_ident()?;
        Ok(Def::TypeAlias {
            name,
            target,
            span: self.span_from(start),
        })
    }

    /// `newtype Name = T` — distinct wrapper.
    fn parse_newtype_def(&mut self) -> Result<Def, CompileError> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident()?;
        self.eat(&Tok::Eq)?;
        let (target, _) = self.expect_ident()?;
        Ok(Def::Newtype {
            name,
            target,
            span: self.span_from(start),
        })
    }

    /// `typeclass C a where { m: sig; }` — method dictionary.
    fn parse_typeclass_def(&mut self) -> Result<Def, CompileError> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident()?;
        let (var, _) = self.expect_ident()?;
        self.eat(&Tok::KwWhere)?;
        self.eat(&Tok::LBrace)?;
        let mut methods = Vec::new();
        while self.peek().tok != Tok::RBrace {
            let (mname, _) = self.expect_ident()?;
            self.eat(&Tok::Colon)?;
            let sig = self.parse_type_expr()?;
            methods.push((mname, sig));
            self.eat(&Tok::Semi)?;
        }
        self.eat(&Tok::RBrace)?;
        Ok(Def::Typeclass {
            name,
            var,
            methods,
            span: self.span_from(start),
        })
    }

    /// `instance C T where { m p1 p2 = body; }` — concrete instance. Each
    /// method optionally binds one or more parameters (`show f = f`,
    /// `fmap g xs = map g xs`), β-substituted at each call site.
    fn parse_instance_def(&mut self) -> Result<Def, CompileError> {
        let start = self.bump().span.start;
        let (class, _) = self.expect_ident()?;
        let (ty, _) = self.expect_ident()?;
        self.eat(&Tok::KwWhere)?;
        self.eat(&Tok::LBrace)?;
        let mut method_bodies = Vec::new();
        while self.peek().tok != Tok::RBrace {
            let (mname, _) = self.expect_ident()?;
            // Zero or more parameter bindings before `=`: `show f = f`,
            // `fmap g xs = map g xs`.
            let mut params = Vec::new();
            while matches!(self.peek().tok, Tok::Ident(_)) {
                let (pname, pspan) = self.expect_ident()?;
                params.push(Param {
                    name: pname,
                    span: pspan,
                });
            }
            self.eat(&Tok::Eq)?;
            let body = self.parse_expr(0, true)?;
            method_bodies.push((mname, params, body));
            self.eat(&Tok::Semi)?;
        }
        self.eat(&Tok::RBrace)?;
        Ok(Def::Instance {
            class,
            ty,
            method_bodies,
            span: self.span_from(start),
        })
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
            if no_comma && op == InfixOp::Par {
                break;
            }
            if l_bp < min_bp {
                break;
            }
            self.bump();
            let rhs = self.parse_expr(r_bp, no_comma)?;

            if matches!(op, InfixOp::Add | InfixOp::Sub) {
                let re = match &lhs {
                    Expr::Float(v, _) => Some(*v),
                    Expr::Int(v, _) => Some(*v as f64),
                    Expr::Neg(inner, _) => match inner.as_ref() {
                        Expr::Float(v, _) => Some(-*v),
                        Expr::Int(v, _) => Some(-(*v as f64)),
                        _ => None,
                    },
                    _ => None,
                };
                let im = match &rhs {
                    Expr::Imag(v, _) => Some(if matches!(op, InfixOp::Sub) { -*v } else { *v }),
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
            lhs = if op.is_arith() {
                Expr::Arith {
                    op: op.to_arith(),
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span,
                }
            } else if op.is_logic() {
                Expr::Logic {
                    op: op.to_logic(),
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span,
                }
            } else if op.is_cmp() {
                Expr::Cmp {
                    op: op.to_cmp(),
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span,
                }
            } else {
                match op {
                    InfixOp::Seq => Expr::Seq(Box::new(lhs), Box::new(rhs), span),
                    InfixOp::Par => Expr::Par(Box::new(lhs), Box::new(rhs), span),
                    InfixOp::Split => Expr::Split(Box::new(lhs), Box::new(rhs), span),
                    InfixOp::Merge => Expr::Merge(Box::new(lhs), Box::new(rhs), span),
                    InfixOp::Loop => Expr::Loop(Box::new(lhs), Box::new(rhs), span),
                    InfixOp::Delay => Expr::Delay(Box::new(lhs), Box::new(rhs), span),
                    _ => unreachable!(),
                }
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
            Tok::KwIf => {
                self.bump();
                let cond = self.parse_expr(0, false)?;
                self.eat(&Tok::KwThen)?;
                let then = self.parse_expr(0, true)?;
                self.eat(&Tok::KwElse)?;
                let els = self.parse_expr(0, true)?;
                let span = t.span.merge(els.span());
                Ok(Expr::If {
                    cond: Box::new(cond),
                    then: Box::new(then),
                    els: Box::new(els),
                    span,
                })
            }
            Tok::KwMatch => {
                self.bump();
                let scrutinee = self.parse_expr(0, false)?;
                self.eat(&Tok::KwOf)?;
                self.eat(&Tok::LBrace)?;
                let mut arms = Vec::new();
                while self.peek().tok != Tok::RBrace {
                    let pat_span = self.peek().span;
                    let pattern = self.parse_pattern()?;
                    let mut guards = Vec::new();
                    if self.peek().tok == Tok::Pipe {
                        // Guarded arm: `pat | g1 => b1 | g2 => b2` — the first
                        // alternative's guard is written explicitly.
                        while self.peek().tok == Tok::Pipe {
                            self.bump();
                            let g = self.parse_expr(0, true)?;
                            self.eat(&Tok::FatArrow)?;
                            let b = self.parse_expr(0, true)?;
                            guards.push((g, b));
                        }
                    } else {
                        // Bare arm: `pat => body | g1 => b1 | ...` — the first
                        // alternative is unconditional (guard `true`).
                        self.eat(&Tok::FatArrow)?;
                        let first = self.parse_expr(0, true)?;
                        guards.push((Expr::Bool(true, pat_span), first));
                        while self.peek().tok == Tok::Pipe {
                            self.bump();
                            let g = self.parse_expr(0, true)?;
                            self.eat(&Tok::FatArrow)?;
                            let b = self.parse_expr(0, true)?;
                            guards.push((g, b));
                        }
                    }
                    let arm_span = pat_span.merge(guards.last().unwrap().1.span());
                    arms.push(MatchArm {
                        pattern,
                        guards,
                        span: arm_span,
                    });
                    if self.peek().tok == Tok::Semi {
                        self.bump();
                    }
                }
                self.eat(&Tok::RBrace)?;
                // Span from the `match` keyword through the last arm (an empty
                // arm list falls back to the keyword alone).
                let span = arms.last().map(|a| t.span.merge(a.span)).unwrap_or(t.span);
                Ok(Expr::Match {
                    scrutinee: Box::new(scrutinee),
                    arms,
                    span,
                })
            }
            Tok::KwFn => {
                self.bump();
                let mut params = Vec::new();
                while let Tok::Ident(_) = self.peek().tok {
                    let (pname, pspan) = self.expect_ident()?;
                    params.push(Param {
                        name: pname,
                        span: pspan,
                    });
                }
                self.eat(&Tok::FatArrow)?;
                let body = self.parse_expr(0, true)?;
                let span = t.span.merge(body.span());
                Ok(Expr::Lambda {
                    params,
                    body: Box::new(body),
                    span,
                })
            }
            Tok::KwDo => self.parse_do_block(),
            Tok::Ident(name) => {
                let start = t.span.start;
                self.bump();
                if self.peek().tok == Tok::Dot {
                    let proj = self.parse_field(Expr::Ref(name, t.span), start)?;
                    if is_atom_start(&self.peek().tok) {
                        // `k.unKleisli p.first` — apply a field projection as
                        // the callee (field access binds tighter than
                        // application).
                        let mut args = Vec::new();
                        while is_atom_start(&self.peek().tok) {
                            args.push(self.parse_atom(true)?);
                        }
                        let span = self.span_from(start);
                        Ok(Expr::ApplyExpr {
                            callee: Box::new(proj),
                            args,
                            span,
                        })
                    } else {
                        Ok(proj)
                    }
                } else if is_atom_start(&self.peek().tok) {
                    let mut args = Vec::new();
                    while is_atom_start(&self.peek().tok) {
                        args.push(self.parse_atom(true)?);
                    }
                    let span = t.span.merge(args.last().unwrap().span());
                    Ok(Expr::Apply { name, args, span })
                } else {
                    Ok(Expr::Ref(name, t.span))
                }
            }
            _ => self.parse_atom(false),
        }
    }

    /// `do { stmt; stmt; expr }` — monadic sequencing, desugared here to nested
    /// `bind e (fn x -> rest)` (Haskell `<-`), `let` statements to `Expr::Let`,
    /// and bare statement expressions to `bind e (fn _ -> rest)`.
    fn parse_do_block(&mut self) -> Result<Expr, CompileError> {
        let start = self.bump().span.start; // consume `do`
        self.eat(&Tok::LBrace)?;
        let mut stmts: Vec<DoStmt> = Vec::new();
        let mut result: Option<Expr> = None;
        while self.peek().tok != Tok::RBrace {
            if matches!(self.peek().tok, Tok::Ident(_)) {
                // `x <- e` (bind) or `let x = e` (let) or a bare expression.
                let save = self.pos();
                if let Ok((n, _)) = self.expect_ident() {
                    if self.peek().tok == Tok::LArrow {
                        self.bump();
                        let e = self.parse_expr(0, true)?;
                        stmts.push(DoStmt::Bind(n, e));
                        self.eat(&Tok::Semi)?;
                        continue;
                    }
                }
                self.seek(save);
            }
            if matches!(self.peek().tok, Tok::KwLet) {
                self.bump();
                let (n, _) = self.expect_ident()?;
                self.eat(&Tok::Eq)?;
                let e = self.parse_expr(0, true)?;
                stmts.push(DoStmt::Let(n, e));
                self.eat(&Tok::Semi)?;
                continue;
            }
            let e = self.parse_expr(0, true)?;
            if self.peek().tok == Tok::Semi {
                self.bump();
                // A trailing `;` before `}`: the statement is the block's
                // result expression (`do { x <- mx; pure x; }`).
                if self.peek().tok == Tok::RBrace {
                    result = Some(e);
                    break;
                }
                stmts.push(DoStmt::Stmt(e));
            } else {
                result = Some(e);
                break;
            }
        }
        self.eat(&Tok::RBrace)?;
        let mut rest = result.ok_or_else(|| self.error("do block must end with an expression"))?;
        for s in stmts.iter().rev() {
            match s {
                DoStmt::Bind(x, e) => {
                    let span = self.span_from(start);
                    let lam = Expr::Lambda {
                        params: vec![Param {
                            name: x.clone(),
                            span,
                        }],
                        body: Box::new(rest),
                        span,
                    };
                    rest = Expr::Apply {
                        name: "bind".to_string(),
                        args: vec![e.clone(), lam],
                        span,
                    };
                }
                DoStmt::Let(x, e) => {
                    let span = self.span_from(start);
                    rest = Expr::Let {
                        defs: vec![Def::Local {
                            name: x.clone(),
                            body: e.clone(),
                            where_defs: vec![],
                            span,
                        }],
                        body: Box::new(rest),
                        span,
                    };
                }
                DoStmt::Stmt(e) => {
                    let span = self.span_from(start);
                    let lam = Expr::Lambda {
                        params: vec![Param {
                            name: "_".to_string(),
                            span,
                        }],
                        body: Box::new(rest),
                        span,
                    };
                    rest = Expr::Apply {
                        name: "bind".to_string(),
                        args: vec![e.clone(), lam],
                        span,
                    };
                }
            }
        }
        Ok(rest)
    }

    /// Parse a `.field` or `.field := value` postfix after a leading record
    /// expression. `self.peek()` must be the `.` token.
    fn parse_field(&mut self, record: Expr, start: usize) -> Result<Expr, CompileError> {
        self.bump();
        let (field, _) = self.expect_ident()?;
        if self.peek().tok == Tok::ColonEq {
            self.bump();
            let value = self.parse_expr(0, false)?;
            Ok(Expr::FieldUpdate {
                record: Box::new(record),
                field,
                value: Box::new(value),
                span: self.span_from(start),
            })
        } else {
            Ok(Expr::FieldProject {
                record: Box::new(record),
                field,
                span: self.span_from(start),
            })
        }
    }

    fn parse_record_or_map(&mut self) -> Result<Expr, CompileError> {
        let start = self.eat(&Tok::LBrace)?.span.start;
        if self.peek().tok == Tok::RBrace {
            self.bump();
            return Ok(Expr::Record(Vec::new(), self.span_from(start)));
        }
        // A string literal key makes this a Map literal.
        let is_map = matches!(self.peek().tok, Tok::Str(_));
        let mut fields = Vec::new();
        let mut entries = Vec::new();
        loop {
            if is_map {
                let k = match self.bump().tok {
                    Tok::Str(s) => s,
                    other => {
                        return Err(self.error(&format!("expected string map key, found {other:?}")))
                    }
                };
                self.eat(&Tok::Colon)?;
                let v = self.parse_expr(0, true)?;
                entries.push((k, v));
            } else {
                let (key, _) = self.expect_ident()?;
                self.eat(&Tok::Colon)?;
                let val = self.parse_expr(0, true)?;
                fields.push((key, val));
            }
            if self.peek().tok == Tok::Comma {
                self.bump();
                if self.peek().tok == Tok::RBrace {
                    break;
                }
            } else if self.peek().tok == Tok::RBrace {
                break;
            } else {
                return Err(self.error("expected ',' or '}' in literal"));
            }
        }
        self.eat(&Tok::RBrace)?;
        if is_map {
            Ok(Expr::MapLit(entries, self.span_from(start)))
        } else {
            Ok(Expr::Record(fields, self.span_from(start)))
        }
    }

    /// Parse a single atom. `arg_list` is true when the atom is a juxtaposed
    /// argument of an enclosing application: a parenthesized expression in that
    /// position is a plain argument and must not absorb the following atoms as
    /// its own application (`fmap (fn x -> x) [1.0]` — the lambda is fmap's
    /// first argument). A bare parenthesized expression (`arg_list == false`)
    /// followed by atom-starting tokens is an application of the parenthesized
    /// callee: `(k.unKleisli) p.first`.
    fn parse_atom(&mut self, arg_list: bool) -> Result<Expr, CompileError> {
        let t = self.bump();
        match t.tok {
            Tok::Int(v) => Ok(Expr::Int(v, t.span)),
            Tok::Float(v) => Ok(Expr::Float(v, t.span)),
            Tok::Imag(v) => Ok(Expr::Imag(v, t.span)),
            Tok::Wire => Ok(Expr::Wire(t.span)),
            Tok::Cut => Ok(Expr::Cut(t.span)),
            Tok::Str(s) => Ok(Expr::Str(s, t.span)),
            Tok::Question => {
                let start = t.span.start;
                let (name, _) = self.expect_ident()?;
                let default = if self.peek().tok == Tok::Eq {
                    self.bump();
                    Some(Box::new(self.parse_expr(0, false)?))
                } else {
                    None
                };
                Ok(Expr::ActorParam {
                    name,
                    default,
                    span: self.span_from(start),
                })
            }
            Tok::Plus => Ok(Expr::Ref("+".into(), t.span)),
            Tok::Minus => Ok(Expr::Ref("-".into(), t.span)),
            Tok::Star => Ok(Expr::Ref("*".into(), t.span)),
            Tok::Slash => Ok(Expr::Ref("/".into(), t.span)),
            Tok::Percent => Ok(Expr::Ref("%".into(), t.span)),
            Tok::Ident(name) => {
                if self.peek().tok == Tok::Dot {
                    let proj = self.parse_field(Expr::Ref(name, t.span), t.span.start)?;
                    if !arg_list && is_atom_start(&self.peek().tok) {
                        // `(k.unKleisli) p.first` — apply a field projection
                        // as the callee.
                        let mut args = Vec::new();
                        while is_atom_start(&self.peek().tok) {
                            args.push(self.parse_atom(true)?);
                        }
                        let span = self.span_from(t.span.start);
                        Ok(Expr::ApplyExpr {
                            callee: Box::new(proj),
                            args,
                            span,
                        })
                    } else {
                        Ok(proj)
                    }
                } else {
                    Ok(Expr::Ref(name, t.span))
                }
            }
            Tok::LParen => {
                let start = t.span.start;
                let inner = self.parse_expr(0, false)?;
                self.eat(&Tok::RParen)?;
                if self.peek().tok == Tok::Dot {
                    self.parse_field(inner, start)
                } else if !arg_list && is_atom_start(&self.peek().tok) {
                    // `(expr) arg1 arg2` — apply a parenthesized expression.
                    let mut args = Vec::new();
                    while is_atom_start(&self.peek().tok) {
                        args.push(self.parse_atom(true)?);
                    }
                    let span = self.span_from(start);
                    Ok(Expr::ApplyExpr {
                        callee: Box::new(inner),
                        args,
                        span,
                    })
                } else {
                    Ok(inner)
                }
            }
            Tok::LBrace => {
                // rewind — parse_record_or_map handles the opening brace
                self.pos -= 1;
                self.parse_record_or_map()
            }
            Tok::LBracket => {
                let start = t.span.start;
                if self.peek().tok == Tok::RBracket {
                    self.bump();
                    return Ok(Expr::ListLit(Vec::new(), self.span_from(start)));
                }
                let mut elems = Vec::new();
                loop {
                    elems.push(self.parse_expr(0, true)?);
                    if self.peek().tok == Tok::Comma {
                        self.bump();
                        if self.peek().tok == Tok::RBracket {
                            break;
                        }
                    } else if self.peek().tok == Tok::RBracket {
                        break;
                    } else {
                        return Err(self.error("expected ',' or ']' in list literal"));
                    }
                }
                self.eat(&Tok::RBracket)?;
                Ok(Expr::ListLit(elems, self.span_from(start)))
            }
            Tok::KwTrue => Ok(Expr::Bool(true, t.span)),
            Tok::KwFalse => Ok(Expr::Bool(false, t.span)),
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
            Expr::Par(..) => {}
            other => panic!("expected Par, got {other:?}"),
        }
    }

    #[test]
    fn feedback_binds_loosest() {
        match body("main = + ~ _") {
            Expr::Loop(..) => {}
            other => panic!("expected Loop, got {other:?}"),
        }
    }

    #[test]
    fn seq_is_left_associative() {
        match body("main = _ : _ : _") {
            Expr::Seq(..) => {}
            other => panic!("expected Seq, got {other:?}"),
        }
    }

    #[test]
    fn application_uses_juxtaposition() {
        let p = prog("main = gain _ 2");
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
            Expr::Merge(..) => {}
            other => panic!("expected Merge, got {other:?}"),
        }
    }

    #[test]
    fn application_arg_may_be_a_composed_expression() {
        let p = prog("main = let g = _ : _ in f g 2");
        match p.main_def().unwrap().body() {
            Expr::Let { body, .. } => match body.as_ref() {
                Expr::Apply { name, args, .. } => {
                    assert_eq!(name, "f");
                    assert_eq!(args.len(), 2);
                    assert!(matches!(&args[0], Expr::Ref(g, _) if g == "g"));
                    assert!(matches!(&args[1], Expr::Int(2, _)));
                }
                other => panic!("expected Apply, got {other:?}"),
            },
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn juxtaposition_parse() {
        let p = prog("main regs = ay38910 1750000.0 regs : lofi 8 44100 0.75 1.0 1 0 1");
        match p.main_def().unwrap().body() {
            Expr::Seq(..) => {}
            other => panic!("expected Seq, got {other:?}"),
        }
    }

    #[test]
    fn parses_string_arg() {
        let p = parse(
            &tokenize(r#"main = f "x""#).unwrap(),
            r#"main = f "x""#.as_bytes(),
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

    #[test]
    fn parse_actor_param_no_default() {
        let p = prog("main = _ * ?gain");
        let main = p.main_def().unwrap();
        match main.body() {
            Expr::Arith {
                op: ArithOp::Mul,
                rhs,
                ..
            } => match rhs.as_ref() {
                Expr::ActorParam { name, default, .. } => {
                    assert_eq!(name, "gain");
                    assert!(default.is_none());
                }
                other => panic!("expected ActorParam, got {other:?}"),
            },
            other => panic!("expected Arith(Mul), got {other:?}"),
        }
    }

    #[test]
    fn parse_actor_param_with_default() {
        let p = prog("main = _ * ?gain=0.5");
        let main = p.main_def().unwrap();
        match main.body() {
            Expr::Arith {
                op: ArithOp::Mul,
                rhs,
                ..
            } => match rhs.as_ref() {
                Expr::ActorParam { name, default, .. } => {
                    assert_eq!(name, "gain");
                    assert!(default.is_some());
                    if let Some(d) = default {
                        assert!(matches!(d.as_ref(), Expr::Float(v, _) if (*v - 0.5).abs() < 1e-9));
                    }
                }
                other => panic!("expected ActorParam, got {other:?}"),
            },
            other => panic!("expected Arith(Mul), got {other:?}"),
        }
    }

    #[test]
    fn parse_multiple_actor_params() {
        let p = prog("main = lofi ?bitdepth=8 ?sr=44100 0.5 1.0");
        let main = p.main_def().unwrap();
        match main.body() {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "lofi");
                assert_eq!(args.len(), 4);
                match &args[0] {
                    Expr::ActorParam { name, default, .. } => {
                        assert_eq!(name, "bitdepth");
                        assert!(default.is_some());
                    }
                    other => panic!("expected ActorParam(bitdepth), got {other:?}"),
                }
                match &args[1] {
                    Expr::ActorParam { name, default, .. } => {
                        assert_eq!(name, "sr");
                        assert!(default.is_some());
                    }
                    other => panic!("expected ActorParam(sr), got {other:?}"),
                }
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn parse_complex_literal() {
        fn is_complex(e: &Expr, re: f64, im: f64) {
            if let Expr::Apply { name, args, .. } = e {
                assert_eq!(name, "complex");
                match (&args[0], &args[1]) {
                    (Expr::Float(a, _), Expr::Float(b, _)) => {
                        assert!((a - re).abs() < 1e-9, "re={a}, expected {re}");
                        assert!((b - im).abs() < 1e-9, "im={b}, expected {im}");
                    }
                    o => panic!("expected Float args, got {o:?}"),
                }
            } else {
                panic!("expected Apply(complex), got {e:?}");
            }
        }
        is_complex(&body("main = 3.0 + 4.0i"), 3.0, 4.0);
        is_complex(&body("main = 1.0 - 2.0i"), 1.0, -2.0);
        is_complex(&body("main = 0.5 + 1.5e1i"), 0.5, 15.0);
        is_complex(&body("main = -3.0 + 4.0i"), -3.0, 4.0);
        is_complex(&body("main = -1.0 - 2.0i"), -1.0, -2.0);
        is_complex(&body("main = -5 + 7i"), -5.0, 7.0);
    }

    #[test]
    fn parses_data_record_declaration() {
        let p = prog("data Point = { x: Float, y: Float }; main = Point { x: 1.0, y: 2.0 }");
        assert!(p.defs.iter().any(|d| matches!(d, Def::Data { .. })));
    }

    #[test]
    fn parses_data_sum_declaration() {
        let p = prog("data Shape = Circle Float | Rect Float Float; main = Circle 1.0");
        assert!(p.defs.iter().any(|d| matches!(d, Def::Sum { .. })));
    }

    #[test]
    fn parses_type_and_newtype() {
        let p = prog("type Angles = Float; newtype Hz = Float; main = Hz 440.0");
        assert!(p.defs.iter().any(|d| matches!(d, Def::TypeAlias { .. })));
        assert!(p.defs.iter().any(|d| matches!(d, Def::Newtype { .. })));
    }

    #[test]
    fn parses_typeclass_declaration() {
        let p = prog("typeclass Envelope a where { slope: a; }; main = _");
        assert!(p.defs.iter().any(|d| matches!(d, Def::Typeclass { .. })));
    }

    #[test]
    fn parses_parameterized_data_and_typeclass() {
        let p = prog("data Box a = { value: a }; typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }; main = _");
        match &p.defs[0] {
            Def::Data {
                name,
                tyvars,
                fields,
                ..
            } => {
                assert_eq!(name, "Box");
                assert_eq!(tyvars, &vec!["a".to_string()]);
                assert_eq!(
                    fields[0],
                    ("value".to_string(), TypeExpr::TName("a".into()))
                );
            }
            other => panic!("expected Data, got {other:?}"),
        }
        match &p.defs[1] {
            Def::Typeclass {
                name, var, methods, ..
            } => {
                assert_eq!(name, "Functor");
                assert_eq!(var, "f");
                assert_eq!(methods.len(), 1);
                assert_eq!(methods[0].0, "fmap");
                // Pin the flat-curried signature shape: `(a -> b) -> f a -> f b`.
                assert_eq!(
                    methods[0].1,
                    TypeExpr::TFunc(
                        vec![
                            TypeExpr::TFunc(
                                vec![TypeExpr::TName("a".into())],
                                Box::new(TypeExpr::TName("b".into()))
                            ),
                            TypeExpr::TApp("f".into(), vec![TypeExpr::TName("a".into())]),
                        ],
                        Box::new(TypeExpr::TApp(
                            "f".into(),
                            vec![TypeExpr::TName("b".into())]
                        ))
                    )
                );
            }
            other => panic!("expected Typeclass, got {other:?}"),
        }
    }

    #[test]
    fn tuple_type_desugars_to_pair() {
        // `(Float, Float)` in type position must desugar to `TApp("Pair", …)` —
        // typeclass defs store the raw TypeExpr (no resolution), so compilation
        // alone cannot catch a wrong desugar target.
        let p = prog("typeclass T a where { m: a (Float, Float) -> Float; }; main = _");
        match &p.defs[0] {
            Def::Typeclass { methods, .. } => {
                assert_eq!(
                    methods[0].1,
                    TypeExpr::TFunc(
                        vec![TypeExpr::TApp(
                            "a".into(),
                            vec![TypeExpr::TApp(
                                "Pair".into(),
                                vec![
                                    TypeExpr::TName("Float".into()),
                                    TypeExpr::TName("Float".into())
                                ]
                            )]
                        )],
                        Box::new(TypeExpr::TName("Float".into()))
                    )
                );
            }
            other => panic!("expected Typeclass, got {other:?}"),
        }
    }

    #[test]
    fn parses_type_application() {
        let p = prog("data V = { xs: List Float }; main = _");
        match &p.defs[0] {
            Def::Data { fields, .. } => {
                assert_eq!(
                    fields[0].1,
                    TypeExpr::TApp("List".into(), vec![TypeExpr::TName("Float".into())])
                );
            }
            other => panic!("expected Data, got {other:?}"),
        }
    }

    #[test]
    fn parses_instance_declaration() {
        let p = prog("instance Envelope Linear where { slope = 0.5; }; main = _");
        assert!(p.defs.iter().any(|d| matches!(d, Def::Instance { .. })));
    }

    #[test]
    fn parses_match_expression() {
        let p = prog("area x = match x of { Circle r => r; Rect w h => w; }; main = area");
        let area = p.defs.iter().find(|d| d.name() == "area").unwrap();
        assert!(matches!(area.body(), Expr::Match { .. }));
    }

    #[test]
    fn parse_if_expression() {
        let p = prog("main = if true then 1.0 else 2.0;");
        let main = p.main_def().unwrap();
        assert!(matches!(main.body(), Expr::If { .. }));
    }

    #[test]
    fn parse_match_patterns_and_guards() {
        let p = prog(
            "data Shape = Circle Float | Rect Float Float; \
             main = match s of { Circle r => r; 0 => 0.0; _ => 1.0; n | n > 0 => n; };",
        );
        let Expr::Match { arms, .. } = p.main_def().unwrap().body() else {
            panic!("expected a match expression");
        };
        assert_eq!(arms.len(), 4, "mixed ctor/literal/wildcard/var arms");
        assert!(matches!(
            &arms[0].pattern,
            Pattern::Ctor(c, args)
                if c == "Circle"
                    && args.len() == 1
                    && matches!(&args[0], Pattern::Var(v) if v == "r")
        ));
        assert!(
            matches!(arms[0].guards[0].0, Expr::Bool(true, _)),
            "a bare `Circle r => r` arm's first guard is `true`"
        );
        assert!(matches!(arms[1].pattern, Pattern::LitInt(0)));
        assert!(matches!(arms[2].pattern, Pattern::Wild));
        assert!(matches!(&arms[3].pattern, Pattern::Var(v) if v == "n"));
        assert!(
            matches!(arms[3].guards[0].0, Expr::Cmp { .. }),
            "a guarded arm's first alternative is the written guard (`n > 0`)"
        );
    }

    #[test]
    fn parses_field_projection() {
        let p = prog("main = p.x");
        assert!(matches!(
            p.main_def().unwrap().body(),
            Expr::FieldProject { .. }
        ));
    }

    #[test]
    fn parses_field_update() {
        let p = prog("main = p.x := 1.0");
        assert!(matches!(
            p.main_def().unwrap().body(),
            Expr::FieldUpdate { .. }
        ));
    }

    #[test]
    fn parses_field_access_as_argument() {
        let p = prog("main = f p.x");
        match p.main_def().unwrap().body() {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "f");
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0], Expr::FieldProject { .. }));
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn parses_lambda_literal() {
        let p = prog("double = fn x -> x * 2.0; main = double");
        let d = p.defs.iter().find(|d| d.name() == "double").unwrap();
        assert!(matches!(d.body(), Expr::Lambda { .. }));
    }

    #[test]
    fn parses_nested_lambda() {
        let p = prog("adder = fn n -> fn x -> x + n; main = adder 2.0");
        let d = p.defs.iter().find(|d| d.name() == "adder").unwrap();
        assert!(matches!(d.body(), Expr::Lambda { .. }));
    }

    #[test]
    fn parses_list_map_bool_cmp_logic() {
        match body("main = [1.0, 2.0]") {
            Expr::ListLit(elems, _) => assert_eq!(elems.len(), 2),
            other => panic!("expected ListLit, got {other:?}"),
        }
        match body("main = { \"a\": 1.0 }") {
            Expr::MapLit(entries, _) => assert_eq!(entries.len(), 1),
            other => panic!("expected MapLit, got {other:?}"),
        }
        match body("main = true") {
            Expr::Bool(true, _) => {}
            other => panic!("expected Bool, got {other:?}"),
        }
        match body("main = 1.0 < 2.0 && 3.0 > 1.0") {
            Expr::Logic { .. } => {}
            other => panic!("expected Logic, got {other:?}"),
        }
    }

    #[test]
    fn comparison_binds_tighter_than_logic() {
        match body("main = a < b || c > d") {
            Expr::Logic { lhs, rhs, .. } => {
                assert!(matches!(lhs.as_ref(), Expr::Cmp { .. }));
                assert!(matches!(rhs.as_ref(), Expr::Cmp { .. }));
            }
            other => panic!("expected Logic, got {other:?}"),
        }
    }

    #[test]
    fn arithmetic_binds_tighter_than_comparison() {
        match body("main = 1.0 + 2.0 < 4.0") {
            Expr::Cmp { lhs, rhs, .. } => {
                assert!(matches!(lhs.as_ref(), Expr::Arith { .. }));
                assert!(matches!(rhs.as_ref(), Expr::Float(v, _) if *v == 4.0));
            }
            other => panic!("expected Cmp, got {other:?}"),
        }
    }
}
