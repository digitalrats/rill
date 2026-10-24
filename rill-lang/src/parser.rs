//! Recursive-descent + Pratt (operator-precedence) parser.

use crate::ast::{ArithOp, CmpOp, Def, Expr, LogicOp, MatchArm, Param, Pattern, Program, TypeExpr};
use crate::error::{CompileError, Span};
use crate::lexer::{Tok, Token};
use crate::mode::Mode;

/// An infix operator: cons (`:`), tuple (`,`), or an arithmetic/logic/comparison
/// operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfixOp {
    /// `:` — list cons (right-associative).
    Cons,
    /// `,` — value tuple (Pair).
    Par,
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
    /// `!` — infix indexing (`a ! i` ≡ `index a i`).
    Index,
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
        Tok::AndAnd => (InfixOp::LogicAnd, 1, 2),
        Tok::OrOr => (InfixOp::LogicOr, 1, 2),
        // `:` — list cons, right-associative (`a : b : c` ≡ `a : (b : c)`).
        Tok::Colon => (InfixOp::Cons, 3, 3),
        Tok::EqEq => (InfixOp::CmpEq, 3, 4),
        Tok::NotEq => (InfixOp::CmpNe, 3, 4),
        Tok::Lt => (InfixOp::CmpLt, 3, 4),
        Tok::Gt => (InfixOp::CmpGt, 3, 4),
        Tok::Le => (InfixOp::CmpLe, 3, 4),
        Tok::Ge => (InfixOp::CmpGe, 3, 4),
        Tok::Comma => (InfixOp::Par, 9, 10),
        Tok::Plus => (InfixOp::Add, 11, 12),
        Tok::Minus => (InfixOp::Sub, 11, 12),
        Tok::Star => (InfixOp::Mul, 13, 14),
        Tok::Slash => (InfixOp::Div, 13, 14),
        Tok::Percent => (InfixOp::Rem, 13, 14),
        // `!` — infix indexing. Tight (just below juxtaposed application, above
        // every arithmetic operator) and left-associative: `a ! i + 1` ≡
        // `(a ! i) + 1`, `a ! b ! c` ≡ `(a ! b) ! c`. `!` is the index operator
        // in expression position regardless of whitespace (`a!1` ≡ `a ! 1`);
        // bang markers (`!x`, `!pat`) exist only in pattern positions, parsed
        // through separate paths.
        Tok::Bang => (InfixOp::Index, 15, 16),
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

/// Whether the token at `pos + 1` is a number directly attached to the
/// current `-` (a negative literal: `-1.0`, no whitespace).
fn peek_is_negative_literal(toks: &[Token], pos: usize) -> bool {
    if !matches!(toks[pos].tok, Tok::Minus) {
        return false;
    }
    let Some(next) = toks.get(pos + 1) else {
        return false;
    };
    matches!(next.tok, Tok::Int(_) | Tok::Float(_)) && next.span.start == toks[pos].span.end
}

/// Whether the `!` at `pos` is a bang marker rather than the infix indexing
/// operator. Mirroring [`peek_is_negative_literal`]'s adjacency model, a bang
/// is the marker `!pat` only when the following token is directly attached (no
/// whitespace) AND can begin a pattern; a separated `!` is infix indexing
/// (`a ! i`).
fn is_attached_bang(toks: &[Token], pos: usize) -> bool {
    if !matches!(toks[pos].tok, Tok::Bang) {
        return false;
    }
    let Some(next) = toks.get(pos + 1) else {
        return false;
    };
    is_pattern_start(&next.tok) && next.span.start == toks[pos].span.end
}

/// Whether a juxtaposed application argument may start here. `-` is included
/// ONLY when it begins a negative literal (`-1.0`); a standalone `-` is infix
/// subtraction and must reach the operator loop instead of being consumed as a
/// juxtaposed argument.
fn is_juxtaposition_arg_start(toks: &[Token], pos: usize) -> bool {
    if matches!(toks[pos].tok, Tok::Minus) {
        return peek_is_negative_literal(toks, pos);
    }
    is_atom_start(&toks[pos].tok)
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
            | Tok::Bang
    )
}

/// Whether a type expression is a bare type VARIABLE (Haskell 2010: a lowercase
/// name). Used to tell a partial instance head (`(Kleisli m)`) from a fully
/// concrete one (`(Pair Int Int)`).
fn is_type_var(te: &TypeExpr) -> bool {
    matches!(te, TypeExpr::TName(n) if n.chars().next().is_some_and(char::is_lowercase))
}

/// The registry key and bound type arguments of an instance head.
///
/// A head that binds type variables is a PARTIAL instance: it keys by its
/// constructor (`(Kleisli m)` → `Kleisli`) and lists the bound slots in
/// `head_args`. A head whose arguments are all concrete types is keyed by the
/// FULL applied type (`(Pair Int Int)` → `Pair Int Int`), which is what lets
/// `Ix Int`, `Ix (Pair Int Int)` and `Ix (Pair Int (Pair Int Int))` coexist
/// without colliding on the `Pair` constructor.
fn instance_head_key(head: &TypeExpr) -> (String, Vec<String>) {
    let ctor = match head {
        TypeExpr::TName(n) => n.clone(),
        TypeExpr::TApp(h, _) => h.clone(),
        _ => crate::render::render_type_key(head),
    };
    match head {
        TypeExpr::TApp(_, args) if args.iter().any(is_type_var) => (
            ctor,
            args.iter().map(crate::render::render_type_key).collect(),
        ),
        _ => (crate::render::render_type_key(head), Vec::new()),
    }
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
        // Skip layout newlines: the parser is whitespace-agnostic outside
        // layout blocks (offside rule in Task 11). Newline tokens are kept in
        // the stream for layout analysis but skipped here by default.
        let mut i = self.pos;
        while i < self.toks.len() && self.toks[i].tok == Tok::Newline {
            i += 1;
        }
        if i < self.toks.len() {
            &self.toks[i]
        } else {
            &self.toks[self.toks.len() - 1]
        }
    }
    fn bump(&mut self) -> Token {
        // Return the current non-newline token, then advance past it and any
        // following layout newlines.
        let mut i = self.pos;
        while i < self.toks.len() && self.toks[i].tok == Tok::Newline {
            i += 1;
        }
        let t = self.toks[i.min(self.toks.len() - 1)].clone();
        self.pos = (i + 1).min(self.toks.len() - 1);
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

    /// Index of the next non-newline token — the token [`Self::peek`] reports.
    /// Span-adjacency checks must use this index, not the raw `pos`, or a bang
    /// marker that begins a line (after a layout newline) looks separated.
    fn cur(&self) -> usize {
        self.skip_newlines(self.pos)
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
            Tok::Bang => {
                // A bang pattern is always written attached: `!x`, `!Just y`.
                if !is_attached_bang(self.toks, self.cur()) {
                    return Err(
                        self.error("a bang pattern must be written `!pat` (no space after `!`)")
                    );
                }
                self.bump();
                Ok(Pattern::Bang(Box::new(self.parse_pattern()?)))
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

    /// Whether a parameter may start here: a bare identifier, or a bang marker
    /// attached to one (`!x`).
    fn at_param_start(&self) -> bool {
        match self.peek().tok {
            Tok::Ident(_) => true,
            Tok::Bang => is_attached_bang(self.toks, self.cur()),
            _ => false,
        }
    }

    /// Parse one parameter: `x` or a bang parameter `!x`. The argument of a
    /// bang parameter is forced to WHNF on application entry.
    fn parse_param(&mut self) -> Result<Param, CompileError> {
        if self.peek().tok == Tok::Bang && !is_attached_bang(self.toks, self.cur()) {
            return Err(self.error("a bang parameter must be written `!name` (no space after `!`)"));
        }
        let bang_span = if self.peek().tok == Tok::Bang {
            Some(self.bump().span)
        } else {
            None
        };
        let (name, name_span) = self.expect_ident()?;
        let span = match bang_span {
            Some(s) => s.merge(name_span),
            None => name_span,
        };
        Ok(Param {
            name,
            span,
            bang: bang_span.is_some(),
        })
    }

    /// True when a `Newline` layout separator sits immediately after the last
    /// consumed token, i.e. the next non-newline token begins a new line. The
    /// offside rule compares that token's column against the enclosing layout
    /// context to decide whether it continues the block or closes it.
    fn at_line_start(&self) -> bool {
        self.pos < self.toks.len() && self.toks[self.pos].tok == Tok::Newline
    }

    /// True when the next token opens a fresh layout item in an offside block
    /// whose first item starts at `layout_col`: it begins a new line at the
    /// same column (or deeper).
    fn layout_continues(&self, layout_col: usize) -> bool {
        self.peek().tok != Tok::Eof && self.at_line_start() && self.cur_col() >= layout_col
    }

    /// True when an offside block's layout context closes before the next
    /// token: a line indented less than the block's column.
    fn layout_closes(&self, layout_col: usize) -> bool {
        self.at_line_start() && self.cur_col() < layout_col
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
        let mut mode = Mode::default();
        let mut mode_explicit = false;
        loop {
            if !matches!(self.peek().tok, Tok::Pragma { .. }) {
                break;
            }
            let token = self.bump();
            match token.tok {
                Tok::Pragma { name, value } => {
                    if name == "MODE" {
                        mode_explicit = true;
                        if value == "strict" {
                            mode = Mode::Strict;
                        } else if value == "complete" {
                            mode = Mode::Complete;
                        } else {
                            return Err(CompileError::Pragma {
                                msg: format!(
                                    "unknown MODE value `{value}` (expected `strict` or `complete`)"
                                ),
                                span: token.span,
                            });
                        }
                    } else {
                        return Err(CompileError::Pragma {
                            msg: format!("unknown pragma `{name}` (expected `MODE`)"),
                            span: token.span,
                        });
                    }
                }
                _ => unreachable!("pragma token expected"),
            }
        }
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
        Ok(Program {
            mode,
            mode_explicit,
            defs,
        })
    }

    fn parse_top_def(&mut self) -> Result<Def, CompileError> {
        let t = self.peek().clone();
        match t.tok {
            Tok::KwData => return self.parse_data_def(),
            Tok::KwType => return self.parse_type_alias_def(),
            Tok::KwNewtype => return self.parse_newtype_def(),
            Tok::KwTypeclass => return self.parse_typeclass_def(),
            Tok::KwInstance => return self.parse_instance_def(),
            Tok::KwForeign => return self.parse_foreign_def(),
            _ => {}
        }
        let start = self.peek().span.start;
        // A leading bang marker (`let !x = e`) marks the binding itself: the
        // right-hand side is forced to WHNF at the binding point.
        let bang = if self.peek().tok == Tok::Bang {
            if !is_attached_bang(self.toks, self.cur()) {
                return Err(
                    self.error("a bang binding must be written `!name` (no space after `!`)")
                );
            }
            self.bump();
            true
        } else {
            false
        };
        let name = match &self.peek().tok {
            Tok::Ident(n) => {
                let n = n.clone();
                self.bump();
                n
            }
            other => {
                return Err(CompileError::Parse {
                    msg: format!("expected definition name, found {other:?}"),
                    span: self.peek().span,
                })
            }
        };
        let mut params = Vec::new();
        while self.at_param_start() {
            params.push(self.parse_param()?);
        }
        // A separated `!` cannot appear in a definition head (the head is a
        // name plus parameters, never an operator expression), so it is always
        // a mistyped bang parameter.
        if self.peek().tok == Tok::Bang {
            return Err(self.error("a bang parameter must be written `!name` (no space after `!`)"));
        }
        // A bang binding marks a single variable, not a function: the force
        // applies to the bound VALUE, and a curried `\x -> …` RHS has none to
        // force at the binding point (GHC rejects `!f x = e` for the same
        // reason).
        if bang && !params.is_empty() {
            return Err(CompileError::Parse {
                msg: "a bang binding must bind a single variable (`!x = e`), not a function (`!f x = e`)"
                    .into(),
                span: Span::new(start, params.last().map(|p| p.span.end).unwrap_or(start)),
            });
        }
        // Optional `:: TypeExpr` signature annotation (Haskell 2010). Typing is
        // still inferred (HM); the signature is a hint/constraint checked by a
        // later task. Kept on the `Def::Eval` node so the renderer can round-trip
        // it.
        let mut sig = None;
        if self.peek().tok == Tok::DoubleColon {
            self.bump();
            sig = Some(self.parse_sig_type()?);
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

        // Unify Anchor/Local into the single `Def::Eval` form. Multi-parameter
        // definitions are curried later (Task 6); params are kept here.
        Ok(Def::Eval {
            name,
            params,
            body,
            where_defs,
            sig,
            bang,
            span,
        })
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
        // An abstract (opaque) type declaration has no `= …` body:
        // `data World;` / `data STRef s a;`.
        if self.peek().tok == Tok::Semi {
            return Ok(Def::Abstract {
                name,
                tyvars,
                span: self.span_from(start),
            });
        }
        self.eat(&Tok::Eq)?;
        if self.peek().tok == Tok::LBrace {
            // product: { f1: T1, f2: T2 }
            self.bump();
            let mut fields = Vec::new();
            while self.peek().tok != Tok::RBrace {
                let (fname, _) = self.expect_ident()?;
                self.eat(&Tok::Colon)?;
                let strict = if self.peek().tok == Tok::Bang {
                    self.bump();
                    true
                } else {
                    false
                };
                let t = self.parse_type_expr()?;
                fields.push((fname, t, strict));
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
            loop {
                let (cname, _) = self.expect_ident()?;
                let mut payload = Vec::new();
                // A payload type stops at a newline boundary: `peek` skips
                // layout newlines, so without this check the NEXT definition's
                // name (e.g. `main` on the following line) would be swallowed
                // as one more payload type. The renderer's def separator is the
                // bare newline — the same boundary a definition would hit.
                while !self.at_line_start()
                    && matches!(self.peek().tok, Tok::Ident(_) | Tok::Int(_) | Tok::Bang)
                {
                    let strict = if self.peek().tok == Tok::Bang {
                        self.bump();
                        true
                    } else {
                        false
                    };
                    let t = self.parse_type_single()?;
                    payload.push((t, strict));
                }
                ctors.push((cname, payload));
                // A sum declaration ends when no further constructor follows
                // (`|`): a bare newline (the renderer's def separator) or `;`
                // terminates it exactly like a definition boundary would.
                if self.peek().tok == Tok::Pipe {
                    self.bump();
                } else {
                    break;
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
        // `()` — the unit type. Haskell 2010 spells the unit type as the empty
        // tuple; `ValueTy::Void`'s type name is already `"()"` (see
        // `TypeEnv::vty_of_name`), which is what the builtin ST signatures use
        // internally, so the source-level spelling maps straight onto it.
        if self.peek().tok == Tok::RParen {
            self.bump();
            return Ok(TypeExpr::TName("()".into()));
        }
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
            Tok::KwForall => self.parse_forall_type(),
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
            Tok::KwForall => self.parse_forall_type(),
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

    /// `forall a b. T` — explicit universal quantification. Reads one or more
    /// type-variable names, then a `.`, then the inner type expression, which
    /// may carry a Haskell 2010 class context (`Ix i => T`) exactly like a `::`
    /// signature body.
    fn parse_forall_type(&mut self) -> Result<TypeExpr, CompileError> {
        self.bump(); // consume `forall`
        let mut names = Vec::new();
        while let Tok::Ident(name) = self.peek().tok.clone() {
            self.bump();
            names.push(name);
        }
        if names.is_empty() {
            return Err(self.error("expected at least one type variable after `forall`"));
        }
        self.eat(&Tok::Dot)?;
        let inner = self.parse_sig_type()?;
        Ok(TypeExpr::TForall(names, Box::new(inner)))
    }

    /// Parse a `::` signature type: an optional Haskell 2010 class context
    /// (`Ix i => T`, `(Ix i, Ix j) => T`) wrapping the body type.
    fn parse_sig_type(&mut self) -> Result<TypeExpr, CompileError> {
        match self.parse_sig_constraints()? {
            Some(constraints) => {
                let body = self.parse_type_expr()?;
                Ok(TypeExpr::TConstrained(constraints, Box::new(body)))
            }
            None => self.parse_type_expr(),
        }
    }

    /// Parse a signature's constraint context, if one is present. A constraint
    /// is a class name applied to a single type variable — `C v`, or a
    /// comma-separated parenthesized list `(C v, C w)`. The representation is
    /// generic over the class name (`Ix` is not special).
    fn parse_sig_constraints(&mut self) -> Result<Option<Vec<(String, String)>>, CompileError> {
        if !self.at_sig_constraints() {
            // `C v, C w => T` would otherwise parse `C v` as a type application
            // and fail later on the comma with an unrelated message.
            if self.at_unparenthesized_multi_constraints() {
                return Err(self
                    .error("a multi-constraint context must be parenthesized: `(C v, C w) => T`"));
            }
            return Ok(None);
        }
        let parenthesized = self.peek().tok == Tok::LParen;
        if parenthesized {
            self.bump();
        }
        let mut constraints = Vec::new();
        loop {
            let (class, _) = self.expect_ident()?;
            let (var, _) = self.expect_ident()?;
            constraints.push((class, var));
            if !parenthesized {
                break;
            }
            if self.peek().tok == Tok::Comma {
                self.bump();
            } else {
                self.eat(&Tok::RParen)?;
                break;
            }
        }
        self.eat(&Tok::FatArrow)?;
        Ok(Some(constraints))
    }

    /// Whether the next tokens spell a signature class context. Decided by
    /// lookahead because a parenthesized TYPE opens with the same `(`: only the
    /// shape `C v` / `(C v, …)` followed by `=>` is a context, so
    /// `(Pair i i) -> …` falls through to the ordinary type parser.
    fn at_sig_constraints(&self) -> bool {
        let mut i = self.skip_newlines(self.pos);
        match self.toks.get(i).map(|t| &t.tok) {
            Some(Tok::Ident(_)) => {
                i = self.skip_newlines(i + 1);
                matches!(self.toks.get(i).map(|t| &t.tok), Some(Tok::Ident(_)))
                    && matches!(
                        self.toks.get(self.skip_newlines(i + 1)).map(|t| &t.tok),
                        Some(Tok::FatArrow)
                    )
            }
            Some(Tok::LParen) => {
                i = self.skip_newlines(i + 1);
                loop {
                    if !matches!(self.toks.get(i).map(|t| &t.tok), Some(Tok::Ident(_))) {
                        return false;
                    }
                    i = self.skip_newlines(i + 1);
                    if !matches!(self.toks.get(i).map(|t| &t.tok), Some(Tok::Ident(_))) {
                        return false;
                    }
                    i = self.skip_newlines(i + 1);
                    match self.toks.get(i).map(|t| &t.tok) {
                        Some(Tok::Comma) => i = self.skip_newlines(i + 1),
                        Some(Tok::RParen) => {
                            return matches!(
                                self.toks.get(self.skip_newlines(i + 1)).map(|t| &t.tok),
                                Some(Tok::FatArrow)
                            );
                        }
                        _ => return false,
                    }
                }
            }
            _ => false,
        }
    }

    /// Whether the next tokens spell the INVALID unparenthesized form of a
    /// multi-constraint context: `C v, C w => T`. Used only to report a
    /// deliberate diagnostic; a valid context is always parenthesized.
    fn at_unparenthesized_multi_constraints(&self) -> bool {
        let mut i = self.skip_newlines(self.pos);
        if !matches!(self.toks.get(i).map(|t| &t.tok), Some(Tok::Ident(_))) {
            return false;
        }
        i = self.skip_newlines(i + 1);
        if !matches!(self.toks.get(i).map(|t| &t.tok), Some(Tok::Ident(_))) {
            return false;
        }
        matches!(
            self.toks.get(self.skip_newlines(i + 1)).map(|t| &t.tok),
            Some(Tok::Comma)
        )
    }

    /// The first non-newline token index at or after `i`, clamped to the last
    /// token. [`Self::peek`]/[`Self::bump`] skip layout newlines, so lookahead
    /// must skip them too. Also backs [`Self::cur`], so the newline-skipping
    /// rule lives in exactly one place.
    fn skip_newlines(&self, mut i: usize) -> usize {
        while i < self.toks.len() && self.toks[i].tok == Tok::Newline {
            i += 1;
        }
        i.min(self.toks.len().saturating_sub(1))
    }

    /// Parse a type expression: a chain of juxta-applied type names and type
    /// variables, `(T -> U -> V)` curried function types, and capacity ints.
    fn parse_type_expr(&mut self) -> Result<TypeExpr, CompileError> {
        let first = self.parse_type_atom()?;
        if self.peek().tok == Tok::ThinArrow {
            let mut args = vec![first];
            while self.peek().tok == Tok::ThinArrow {
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

    /// `typeclass C a where { m: sig; }` — method dictionary. A method may
    /// declare a class-level default body after its signature:
    /// `second: sig = k body;` — the default instances omit (precedence:
    /// instance body > default > error).
    fn parse_typeclass_def(&mut self) -> Result<Def, CompileError> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident()?;
        let (var, _) = self.expect_ident()?;
        self.eat(&Tok::KwWhere)?;
        self.eat(&Tok::LBrace)?;
        let mut methods = Vec::new();
        let mut defaults = Vec::new();
        while self.peek().tok != Tok::RBrace {
            let (mname, _) = self.expect_ident()?;
            self.eat(&Tok::Colon)?;
            let sig = self.parse_type_expr()?;
            let mut default = None;
            if self.peek().tok == Tok::Eq {
                self.bump();
                // Zero or more parameter bindings before the body:
                // `second k = …`, `both f g = …`.
                let mut dparams = Vec::new();
                while matches!(self.peek().tok, Tok::Ident(_)) {
                    let (p, ps) = self.expect_ident()?;
                    dparams.push(Param {
                        name: p,
                        span: ps,
                        bang: false,
                    });
                }
                let body = self.parse_expr(0, true)?;
                default = Some((dparams, body));
            }
            methods.push((mname.clone(), sig));
            if let Some((p, b)) = default {
                defaults.push((mname, p, b));
            }
            self.eat(&Tok::Semi)?;
        }
        self.eat(&Tok::RBrace)?;
        Ok(Def::Typeclass {
            name,
            var,
            methods,
            defaults,
            span: self.span_from(start),
        })
    }

    /// `instance C T where { m p1 p2 = body; }` — concrete instance. Each
    /// method optionally binds one or more parameters (`show f = f`,
    /// `fmap g xs = map g xs`), β-substituted at each call site.
    ///
    /// The head may be constraint-qualified and/or partially applied:
    /// `instance (Monad m) => Arrow (Kleisli m) where { … }`.
    fn parse_instance_def(&mut self) -> Result<Def, CompileError> {
        let start = self.bump().span.start;
        let mut constraints = Vec::new();
        if self.peek().tok == Tok::LParen {
            // `(Monad m) =>` (comma-separated constraints inside parens).
            self.bump();
            loop {
                let (class, _) = self.expect_ident()?;
                let (var, _) = self.expect_ident()?;
                constraints.push((class, var));
                if self.peek().tok == Tok::RParen {
                    self.bump();
                    break;
                }
                if self.peek().tok == Tok::Comma {
                    self.bump();
                    continue;
                }
                return Err(self.error("expected ')' or ',' in instance constraint list"));
            }
            self.eat(&Tok::FatArrow)?;
        }
        let (class, _) = self.expect_ident()?;
        // Head: either a bare type (`List`, `Int`) or a parenthesized applied
        // type (`(Kleisli m)`, `(Pair Int Int)`, `(Pair Int (Pair Int Int))`).
        let head = if self.peek().tok == Tok::LParen {
            self.bump();
            let h = self.parse_type_atom()?;
            self.eat(&Tok::RParen)?;
            h
        } else {
            let (t, _) = self.expect_ident()?;
            TypeExpr::TName(t)
        };
        let (ty, head_args) = instance_head_key(&head);
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
                    bang: false,
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
            head,
            ty,
            constraints,
            head_args,
            method_bodies,
            span: self.span_from(start),
        })
    }

    /// `foreign name : TypeExpr;` — a foreign builtin signature declared in the
    /// language. Parsed by the standard type parser; the carried arrow's `FixedBuffer`
    /// channels are the signal-track parameters. Like every sibling def parser
    /// (data/sum/typeclass/instance/alias/newtype), leaves the trailing `;` for the
    /// caller (`parse_program` / `parse_where_block`) to consume.
    fn parse_foreign_def(&mut self) -> Result<Def, CompileError> {
        let start = self.bump().span.start;
        let (name, _) = self.expect_ident()?;
        self.eat(&Tok::Colon)?;
        let sig = self.parse_type_expr()?;
        Ok(Def::Foreign {
            name,
            sig,
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
                    // `:` — list cons: `a : b` ≡ `cons a b`.
                    InfixOp::Cons => Expr::Apply {
                        name: "cons".to_string(),
                        args: vec![lhs, rhs],
                        span,
                    },
                    // `,` — value tuple: `a , b` ≡ `Pair { first: a, second: b }`.
                    InfixOp::Par => Expr::Apply {
                        name: "Pair".to_string(),
                        args: vec![Expr::Record(
                            vec![("first".to_string(), lhs), ("second".to_string(), rhs)],
                            span,
                        )],
                        span,
                    },
                    // `!` — infix indexing: `a ! i` ≡ `index a i`.
                    InfixOp::Index => Expr::Apply {
                        name: "index".to_string(),
                        args: vec![lhs, rhs],
                        span,
                    },
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
            Tok::KwCase => {
                self.bump();
                let scrutinee = self.parse_expr(0, false)?;
                self.eat(&Tok::KwOf)?;
                // Both spellings are accepted: `case x of { … }` with explicit
                // braces, or the offside form where the first alternative's
                // column opens the layout context.
                let braced = self.peek().tok == Tok::LBrace;
                if braced {
                    self.bump();
                }
                let layout_col = self.cur_col();
                let mut arms = Vec::new();
                loop {
                    if braced {
                        if self.peek().tok == Tok::RBrace {
                            break;
                        }
                    } else if self.peek().tok == Tok::Eof || self.layout_closes(layout_col) {
                        break;
                    }
                    let pat_span = self.peek().span;
                    let pattern = self.parse_pattern()?;
                    let mut guards = Vec::new();
                    if self.peek().tok == Tok::Pipe {
                        // Guarded arm: `pat | g1 -> b1 | g2 -> b2` — the first
                        // alternative's guard is written explicitly.
                        while self.peek().tok == Tok::Pipe {
                            self.bump();
                            let g = self.parse_expr(0, true)?;
                            self.eat(&Tok::ThinArrow)?;
                            let b = self.parse_expr(0, true)?;
                            guards.push((g, b));
                        }
                    } else {
                        // Bare arm: `pat -> body | g1 -> b1 | ...` — the first
                        // alternative is unconditional (guard `true`).
                        self.eat(&Tok::ThinArrow)?;
                        let first = self.parse_expr(0, true)?;
                        guards.push((Expr::Bool(true, pat_span), first));
                        while self.peek().tok == Tok::Pipe {
                            self.bump();
                            let g = self.parse_expr(0, true)?;
                            self.eat(&Tok::ThinArrow)?;
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
                if braced {
                    self.eat(&Tok::RBrace)?;
                }
                // Span from the `case` keyword through the last arm (an empty
                // arm list falls back to the keyword alone).
                let span = arms.last().map(|a| t.span.merge(a.span)).unwrap_or(t.span);
                Ok(Expr::Match {
                    scrutinee: Box::new(scrutinee),
                    arms,
                    span,
                })
            }
            Tok::Backslash => {
                self.bump();
                // A lambda is strictly unary: `\x -> body`. Multi-parameter
                // currying lives in definitions (`foo x y = body` ≡
                // `foo = \x -> \y -> body`) and applications (`f a b` ≡
                // `((f a) b)`), never in a single lambda literal.
                let param = self.parse_param()?;
                self.eat(&Tok::ThinArrow)?;
                let body = self.parse_expr(0, true)?;
                let span = t.span.merge(body.span());
                Ok(Expr::Lambda {
                    params: vec![param],
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
                    if is_juxtaposition_arg_start(self.toks, self.pos) {
                        // `k.unKleisli p.first` — apply a field projection as
                        // the callee (field access binds tighter than
                        // application).
                        let mut args = Vec::new();
                        while is_juxtaposition_arg_start(self.toks, self.pos) {
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
                } else if is_juxtaposition_arg_start(self.toks, self.pos) {
                    let mut args = Vec::new();
                    while is_juxtaposition_arg_start(self.toks, self.pos) {
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
        let braced = self.peek().tok == Tok::LBrace;
        if braced {
            self.bump();
        }
        // Offside context: the column of the block's first statement (unused
        // in the brace form, where `;` delimits statements).
        let layout_col = self.cur_col();
        let mut stmts: Vec<DoStmt> = Vec::new();
        let mut result: Option<Expr> = None;
        loop {
            // Block end: `}` (brace form) or a line indented less than the
            // layout column / end of input (offside form).
            if braced {
                if self.peek().tok == Tok::RBrace {
                    break;
                }
            } else if self.peek().tok == Tok::Eof || self.layout_closes(layout_col) {
                break;
            }
            if matches!(self.peek().tok, Tok::Ident(_)) {
                // `x <- e` (bind) or a bare expression.
                let save = self.pos();
                if let Ok((n, _)) = self.expect_ident() {
                    if self.peek().tok == Tok::LArrow {
                        self.bump();
                        let e = self.parse_expr(0, true)?;
                        stmts.push(DoStmt::Bind(n, e));
                        if braced {
                            self.eat(&Tok::Semi)?;
                        }
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
                if braced {
                    self.eat(&Tok::Semi)?;
                }
                continue;
            }
            let e = self.parse_expr(0, true)?;
            if braced {
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
            } else if self.layout_continues(layout_col) {
                // Another statement follows on a line at the layout column; this
                // one is a `bind`-once statement (`Stmt`).
                stmts.push(DoStmt::Stmt(e));
            } else {
                // The block's final expression.
                result = Some(e);
                break;
            }
        }
        if braced {
            self.eat(&Tok::RBrace)?;
        }
        let mut rest = result.ok_or_else(|| self.error("do block must end with an expression"))?;
        for s in stmts.iter().rev() {
            match s {
                DoStmt::Bind(x, e) => {
                    let span = self.span_from(start);
                    let lam = Expr::Lambda {
                        params: vec![Param {
                            name: x.clone(),
                            span,
                            bang: false,
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
                        defs: vec![Def::Eval {
                            name: x.clone(),
                            params: vec![],
                            body: e.clone(),
                            where_defs: vec![],
                            sig: None,
                            bang: false,
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
                            bang: false,
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
            Tok::Minus => {
                // A `-` directly attached to a number is a negative literal
                // (applying unary minus to the literal); a separated `-` is the
                // unary minus function reference (infix subtraction is handled
                // by the operator loop).
                // `t` is the `-` we just bumped; `self.pos` now points at the
                // next token. A number directly attached (no whitespace) is a
                // negative literal.
                let Some(next) = self.toks.get(self.pos) else {
                    return Ok(Expr::Ref("-".into(), t.span));
                };
                if matches!(next.tok, Tok::Int(_) | Tok::Float(_)) && next.span.start == t.span.end
                {
                    self.bump(); // the number
                    match next.tok {
                        Tok::Float(v) => Ok(Expr::Neg(Box::new(Expr::Float(v, next.span)), t.span)),
                        Tok::Int(v) => Ok(Expr::Neg(Box::new(Expr::Int(v, next.span)), t.span)),
                        _ => Ok(Expr::Ref("-".into(), t.span)),
                    }
                } else {
                    Ok(Expr::Ref("-".into(), t.span))
                }
            }
            Tok::Star => Ok(Expr::Ref("*".into(), t.span)),
            Tok::Slash => Ok(Expr::Ref("/".into(), t.span)),
            Tok::Percent => Ok(Expr::Ref("%".into(), t.span)),
            Tok::Ident(name) => {
                if self.peek().tok == Tok::Dot {
                    let proj = self.parse_field(Expr::Ref(name, t.span), t.span.start)?;
                    Ok(proj)
                } else {
                    Ok(Expr::Ref(name, t.span))
                }
            }
            Tok::LParen => {
                // `()` — the unit literal. `t` is the already-consumed LParen;
                // the RParen is the next token.
                if self.peek().tok == Tok::RParen {
                    self.bump();
                    return Ok(Expr::Void(t.span));
                }
                let start = t.span.start;
                let inner = self.parse_expr(0, false)?;
                self.eat(&Tok::RParen)?;
                if self.peek().tok == Tok::Dot {
                    self.parse_field(inner, start)
                } else if !arg_list && is_juxtaposition_arg_start(self.toks, self.pos) {
                    // `(expr) arg1 arg2` — apply a parenthesized expression.
                    let mut args = Vec::new();
                    while is_juxtaposition_arg_start(self.toks, self.pos) {
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
    let mut program = Parser::new(tokens, src).parse_program()?;
    desugar_program(&mut program);
    Ok(program)
}

/// Currying desugar (Task 6): multi-parameter definitions and multi-argument
/// applications become nested unary lambdas / applications. This is the single
/// point where the surface syntax `step a b = body` / `f a b c` is reduced to
/// the unary core `step = \a -> (\b -> body)` / `((f a) b) c`. Runs once, at
/// the end of [`parse`], before inference or lowering see the AST.
pub(crate) fn desugar_program(p: &mut Program) {
    curry_defs_in(&mut p.defs);
}

/// Curry every `Def::Eval` with more than one parameter into nested unary
/// lambdas (`params: vec![]`), and walk every expression contained in the defs
/// (`where`/`let` blocks, instance/default method bodies) for application
/// currying.
fn curry_defs_in(defs: &mut Vec<Def>) {
    for d in defs {
        match d {
            Def::Eval {
                params,
                body,
                where_defs,
                ..
            } => {
                if params.len() > 1 {
                    let mut lam = body.clone();
                    for p in params.iter().rev() {
                        lam = Expr::Lambda {
                            params: vec![p.clone()],
                            body: Box::new(lam),
                            span: p.span,
                        };
                    }
                    *body = lam;
                    params.clear();
                }
                curry_defs_in(where_defs);
                curry_expr(body);
            }
            Def::Typeclass { defaults, .. } => {
                for (_, _, body) in defaults {
                    curry_expr(body);
                }
            }
            Def::Instance { method_bodies, .. } => {
                for (_, _, body) in method_bodies {
                    curry_expr(body);
                }
            }
            _ => {}
        }
    }
}

/// Post-order walk: curry every `Expr::Apply` with more than one argument into
/// a right-nested chain of single-argument `Expr::ApplyExpr` nodes.
///
/// `f a b c` ⇒ `ApplyExpr(ApplyExpr(ApplyExpr(Ref f, [a]), [b]), [c])`. A
/// single-argument `Apply` is already unary and is left in place. `ApplyExpr`
/// nodes are walked but not rewritten (the plan's application currying covers
/// name-based applications; expression callees already parse left-associatively
/// as juxtaposition).
fn curry_expr(e: &mut Expr) {
    match e {
        Expr::Apply { name, args, span } => {
            for a in args.iter_mut() {
                curry_expr(a);
            }
            if args.len() > 1 {
                let name = name.clone();
                let sp = *span;
                let mut callee = Expr::ApplyExpr {
                    callee: Box::new(Expr::Ref(name, sp)),
                    args: vec![args[0].clone()],
                    span: sp,
                };
                for a in args.iter().skip(1) {
                    callee = Expr::ApplyExpr {
                        callee: Box::new(callee),
                        args: vec![a.clone()],
                        span: a.span(),
                    };
                }
                *e = callee;
            }
        }
        Expr::ApplyExpr { callee, args, .. } => {
            curry_expr(callee);
            for a in args.iter_mut() {
                curry_expr(a);
            }
        }
        Expr::Neg(inner, _) => curry_expr(inner),
        Expr::Arith { lhs, rhs, .. } => {
            curry_expr(lhs);
            curry_expr(rhs);
        }
        Expr::Let { defs, body, .. } => {
            curry_defs_in(defs);
            curry_expr(body);
        }
        Expr::Record(fields, _) => {
            for (_, f) in fields.iter_mut() {
                curry_expr(f);
            }
        }
        Expr::ActorParam {
            default: Some(d), ..
        } => curry_expr(d),
        Expr::FieldProject { record, .. } => curry_expr(record),
        Expr::FieldUpdate { record, value, .. } => {
            curry_expr(record);
            curry_expr(value);
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            curry_expr(scrutinee);
            for arm in arms {
                for (g, b) in arm.guards.iter_mut() {
                    curry_expr(g);
                    curry_expr(b);
                }
            }
        }
        Expr::If {
            cond, then, els, ..
        } => {
            curry_expr(cond);
            curry_expr(then);
            curry_expr(els);
        }
        Expr::Lambda { body, .. } => curry_expr(body),
        Expr::ListLit(xs, _) => {
            for x in xs.iter_mut() {
                curry_expr(x);
            }
        }
        Expr::MapLit(fields, _) => {
            for (_, v) in fields.iter_mut() {
                curry_expr(v);
            }
        }
        Expr::Cmp { lhs, rhs, .. } => {
            curry_expr(lhs);
            curry_expr(rhs);
        }
        Expr::Logic { lhs, rhs, .. } => {
            curry_expr(lhs);
            curry_expr(rhs);
        }
        _ => {}
    }
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

    /// Descend a curried application chain (`((f a) b) c`) back to its named
    /// callee and the original argument list. Returns `None` for anything that
    /// does not bottom out in a name-based application.
    fn curry_head(e: &Expr) -> Option<(&str, Vec<&Expr>)> {
        match e {
            Expr::ApplyExpr { callee, args, .. } => match callee.as_ref() {
                // Base of a curried chain: `ApplyExpr(Ref f, [a])` is `f a`.
                Expr::Ref(name, _) => Some((name.as_str(), args.iter().collect())),
                _ => {
                    let (name, mut collected) = curry_head(callee)?;
                    collected.extend(args.iter());
                    Some((name, collected))
                }
            },
            Expr::Apply { name, args, .. } => Some((name.as_str(), args.iter().collect())),
            _ => None,
        }
    }

    #[test]
    fn pragma_complete_parses() {
        let src = "{-# MODE complete #-}\nmain = 1.0";
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        assert_eq!(p.mode, crate::mode::Mode::Complete);
        assert!(p.mode_explicit);
    }

    #[test]
    fn pragma_strict_is_default() {
        let src = "main = 1.0";
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        assert_eq!(p.mode, crate::mode::Mode::Strict);
        assert!(!p.mode_explicit);
    }

    #[test]
    fn pragma_overrides_previous() {
        let src = "{-# MODE complete #-}\n{-# MODE strict #-}\nmain = 1.0";
        let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
        assert_eq!(p.mode, crate::mode::Mode::Strict);
    }

    #[test]
    fn bad_pragma_errors() {
        let src = "{-# MODE nope #-}\nmain = 1.0";
        assert!(parse(&tokenize(src).unwrap(), src.as_bytes()).is_err());
    }

    #[test]
    fn bad_pragma_unknown_name_errors() {
        let src = "{-# FOO bar #-}\nmain = 1.0";
        assert!(parse(&tokenize(src).unwrap(), src.as_bytes()).is_err());
    }

    #[test]
    fn parses_main_without_params() {
        let p = prog("main = 1.0 * 0.5");
        let main = p.main_def().unwrap();
        assert_eq!(main.params().len(), 0);
    }

    #[test]
    fn parses_main_with_params() {
        let p = prog("main f x = 1.0 * 0.5");
        let main = p.main_def().unwrap();
        // Multi-parameter definitions are curried (Task 6): `main f x = body`
        // becomes `main = \f -> \x -> body`.
        assert_eq!(main.params().len(), 0);
        match main.body() {
            Expr::Lambda { params, body, .. } => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "f");
                match body.as_ref() {
                    Expr::Lambda { params, body, .. } => {
                        assert_eq!(params.len(), 1);
                        assert_eq!(params[0].name, "x");
                        assert!(matches!(body.as_ref(), Expr::Arith { .. }));
                    }
                    other => panic!("expected nested lambda, got {other:?}"),
                }
            }
            other => panic!("expected curried lambdas, got {other:?}"),
        }
    }

    #[test]
    fn parses_main_with_where_block() {
        let p =
            prog("main = osc where { osc freq = sin freq * 0.5; lpf cut = lowpass 1.0 cut 0.7; }");
        let main = p.main_def().unwrap();
        assert_eq!(main.where_defs().len(), 2);
        match &main.where_defs()[0] {
            Def::Eval { name, params, .. } => {
                assert_eq!(name, "osc");
                assert_eq!(params.len(), 1);
            }
            _ => panic!("expected Eval"),
        }
        match &main.where_defs()[1] {
            Def::Eval { name, params, .. } => {
                assert_eq!(name, "lpf");
                assert_eq!(params.len(), 1);
            }
            _ => panic!("expected Eval"),
        }
    }

    #[test]
    fn parses_where_local_binding() {
        let p = prog("main = osc where { freq = 440; }");
        let main = p.main_def().unwrap();
        assert_eq!(main.where_defs().len(), 1);
        matches!(&main.where_defs()[0], Def::Eval { name, .. } if name == "freq");
    }

    #[test]
    fn arithmetic_binds_tighter_than_comma() {
        // `1.0 * 2.0 , 3.0` — `*` binds tighter than the `,` tuple operator:
        // the outer form is `Pair { first: (1.0 * 2.0), second: 3.0 }`.
        match body("main = 1.0 * 2.0 , 3.0") {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "Pair");
                assert_eq!(args.len(), 1);
                if let Expr::Record(fields, _) = &args[0] {
                    assert_eq!(fields.len(), 2);
                    assert!(matches!(&fields[0].1, Expr::Arith { .. }));
                    assert!(matches!(&fields[1].1, Expr::Float(3.0, _)));
                } else {
                    panic!("expected Pair record, got {other:?}", other = args[0]);
                }
            }
            other => panic!("expected Pair, got {other:?}"),
        }
    }

    #[test]
    fn colon_is_right_associative_cons() {
        // `1.0 : 2.0 : list` ≡ `1.0 : (2.0 : list)` — cons nests to the right.
        // The name-based application curries to a chain, but `curry_head`
        // recovers the original argument list `cons 1.0 (cons 2.0 list)`.
        match curry_head(&body("main = 1.0 : 2.0 : list")) {
            Some(("cons", args)) => {
                assert_eq!(args.len(), 2);
                assert!(matches!(args[0], Expr::Float(1.0, _)));
                match curry_head(args[1]) {
                    Some(("cons", args)) => {
                        assert_eq!(args.len(), 2);
                        assert!(matches!(args[0], Expr::Float(2.0, _)));
                        assert!(matches!(args[1], Expr::Ref(r, _) if r == "list"));
                    }
                    other => panic!("expected nested cons, got {other:?}"),
                }
            }
            other => panic!("expected cons, got {other:?}"),
        }
    }

    #[test]
    fn application_uses_juxtaposition() {
        let p = prog("main = gain 1.0 2");
        let main = p.main_def().unwrap();
        match curry_head(main.body()) {
            Some(("gain", args)) => {
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn grouping_paren_comma_is_tuple() {
        // `(1.0 , 2.0)` — inside grouping parens the `,` is the tuple operator,
        // producing a `Pair { first, second }` construction.
        match body("main = (1.0 , 2.0)") {
            Expr::Apply { name, args, .. } => {
                assert_eq!(name, "Pair");
                assert_eq!(args.len(), 1);
                if let Expr::Record(fields, _) = &args[0] {
                    assert_eq!(fields.len(), 2);
                    assert_eq!(fields[0].0, "first");
                    assert_eq!(fields[1].0, "second");
                } else {
                    panic!("expected Pair record, got {other:?}", other = args[0]);
                }
            }
            other => panic!("expected Pair, got {other:?}"),
        }
    }

    #[test]
    fn application_arg_may_be_a_composed_expression() {
        let p = prog("main = let g = 1.0 : list in f g 2");
        match p.main_def().unwrap().body() {
            Expr::Let { body, .. } => match curry_head(body) {
                Some(("f", args)) => {
                    assert_eq!(args.len(), 2);
                    assert!(matches!(args[0], Expr::Ref(g, _) if g == "g"));
                    assert!(matches!(args[1], Expr::Int(2, _)));
                }
                other => panic!("expected Apply, got {other:?}"),
            },
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn juxtaposition_parse() {
        let p = prog("main regs = ay38910 1750000.0 regs : list");
        match curry_head(p.main_def().unwrap().body()) {
            Some(("cons", args)) => {
                assert_eq!(args.len(), 2);
                match curry_head(args[0]) {
                    Some(("ay38910", args)) => {
                        assert_eq!(args.len(), 2);
                        assert!(matches!(args[0], Expr::Float(1750000.0, _)));
                        assert!(matches!(args[1], Expr::Ref(r, _) if r == "regs"));
                    }
                    other => panic!("expected ay38910 apply, got {other:?}"),
                }
                assert!(matches!(args[1], Expr::Ref(r, _) if r == "list"));
            }
            other => panic!("expected cons, got {other:?}"),
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
    fn parses_program_without_main() {
        // `main` is not required by the parser anymore: the entry name is a
        // compile-time pipeline parameter (compile selects `main`; eval selects
        // its synthesized marker). A def list without `main` is a valid program.
        let p = prog("double x = x * 2.0");
        assert_eq!(p.defs.len(), 1);
        assert_eq!(p.defs[0].name(), "double");
    }

    #[test]
    fn rejects_empty_program() {
        // Def-less input is still rejected (the empty-program rule stays).
        assert!(parse(&tokenize("").unwrap(), "".as_bytes()).is_err());
        assert!(parse(&tokenize("_ * 0.5").unwrap(), "_ * 0.5".as_bytes()).is_err());
    }

    #[test]
    fn parses_top_level_multi_def() {
        let p = prog("sq x = x * x; main = sq 1.0");
        assert_eq!(p.defs.len(), 2);
        assert_eq!(p.defs[0].name(), "sq");
        assert_eq!(p.defs[1].name(), "main");
    }

    #[test]
    fn parses_top_level_multi_def_no_semicolon() {
        let p = prog("gain = 1.0 * 0.5; main = gain");
        assert_eq!(p.defs.len(), 2);
    }

    #[test]
    fn parses_let_expression() {
        let p = prog("main = let gain = 1.0 * 0.5 in gain");
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
        let p = prog("main = let { g = 1.0 * 0.5; } in g");
        let main = p.main_def().unwrap();
        assert!(matches!(main.body(), Expr::Let { .. }));
    }

    #[test]
    fn parses_do_block_with_layout() {
        // Offside rule: a `do` block without braces — statements delimited by
        // the layout context's indentation column.
        let b = body("main = do\n    write (Just 1.0) stdout\n    return ()\n");
        assert_eq!(
            curry_head(&b).map(|(name, _)| name),
            Some("bind"),
            "expected the trailing statement to desugar to a `bind`, got {b:?}"
        );
    }

    #[test]
    fn parses_do_block_layout_bind_statement() {
        // `x <- e` in layout form (no `;`): the block's first statement binds.
        let b = body("main = do\n    x <- fromList list\n    toList x\n");
        assert_eq!(
            curry_head(&b).map(|(name, _)| name),
            Some("bind"),
            "expected `bind`, got {b:?}"
        );
    }

    #[test]
    fn parses_case_with_layout() {
        // Offside rule: a `case` alternatives block without braces.
        let b = body("main = case n of\n    Just v -> v\n    Nothing -> 0.0\n");
        match b {
            Expr::Match { arms, .. } => assert_eq!(arms.len(), 2),
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn parses_case_layout_then_top_level_def() {
        // The layout block must close when the next line is indented LESS than
        // the alternatives, so the following top-level definition is separate.
        let p = prog("main = case n of\n    Just v -> v\n    Nothing -> 0.0\nother = 1.0\n");
        assert_eq!(p.defs.len(), 2);
        assert_eq!(p.defs[1].name(), "other");
    }

    #[test]
    fn parses_do_layout_then_top_level_def() {
        let p = prog("main = do\n    write (Just 1.0) stdout\n    return ()\nother = 1.0\n");
        assert_eq!(p.defs.len(), 2);
        assert_eq!(p.defs[1].name(), "other");
    }

    #[test]
    fn parses_where_block_layout_no_semicolons() {
        // Offside defs in a `where` clause, separated only by the layout
        // context's column (no `;`).
        let src = "main = myFilter where\n    myFilter = sin 440.0\n    gain = 1.0 * 0.5\n";
        let p = prog(src);
        let main = p.main_def().unwrap();
        let where_defs = main.where_defs();
        assert_eq!(where_defs.len(), 2);
        assert_eq!(where_defs[0].name(), "myFilter");
        assert_eq!(where_defs[1].name(), "gain");
        assert!(matches!(main.body(), Expr::Ref(name, _) if name == "myFilter"));
    }

    #[test]
    fn parses_do_inside_layout_where() {
        // Nested layout contexts: a `do` block indented inside a `where` def.
        let src =
            "main = run where\n    run = do\n        write (Just 1.0) stdout\n        return ()\n";
        let p = prog(src);
        let main = p.main_def().unwrap();
        let where_defs = main.where_defs();
        assert_eq!(where_defs.len(), 1);
        assert_eq!(where_defs[0].name(), "run");
        let body = match &where_defs[0] {
            Def::Eval { body, .. } => body,
            other => panic!("expected an eval def, got {other:?}"),
        };
        assert_eq!(
            curry_head(body).map(|(name, _)| name),
            Some("bind"),
            "expected the do body to desugar to a `bind`, got {body:?}"
        );
    }

    #[test]
    fn parses_do_block_layout_case_body() {
        // Layout `case` whose arms' bodies are layout `do` blocks.
        let src = "main = case x of\n    Just v -> do\n        write (Just v) stdout\n        return ()\n    Nothing -> do\n        return ()\n";
        let b = body(src);
        let Expr::Match { arms, .. } = b else {
            panic!("expected Match, got {b:?}");
        };
        assert_eq!(arms.len(), 2);
        // A multi-statement `do` desugars to a `bind` chain; a single-statement
        // block is just its result expression.
        assert_eq!(
            curry_head(&arms[0].guards[0].1).map(|(name, _)| name),
            Some("bind"),
            "expected the first arm body to desugar to a `bind`, got {:?}",
            arms[0].guards[0].1
        );
        assert!(matches!(
            &arms[1].guards[0].1,
            Expr::Apply { name, .. } if name == "return"
        ));
    }

    #[test]
    fn main_with_where_and_top_level() {
        let p = prog("gain = 1.0 * 0.5; main = gain where { x = 1; }");
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
        let p = prog("main = 1.0 * ?gain");
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
        let p = prog("main = 1.0 * ?gain=0.5");
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
        match curry_head(main.body()) {
            Some(("lofi", args)) => {
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
    fn parse_strict_product_field() {
        let p = prog("data P = { x: !Float, y: Float }; main = P { x: 1.0, y: 2.0 }");
        match &p.defs[0] {
            Def::Data { fields, .. } => {
                assert_eq!(
                    fields,
                    &vec![
                        ("x".to_string(), TypeExpr::TName("Float".into()), true),
                        ("y".to_string(), TypeExpr::TName("Float".into()), false),
                    ]
                );
            }
            other => panic!("expected Data, got {other:?}"),
        }
    }

    #[test]
    fn parse_strict_sum_payload() {
        let p = prog("data T = C !Int | D Float; main = C 1");
        match &p.defs[0] {
            Def::Sum { ctors, .. } => {
                assert_eq!(
                    ctors,
                    &vec![
                        ("C".to_string(), vec![(TypeExpr::TName("Int".into()), true)]),
                        (
                            "D".to_string(),
                            vec![(TypeExpr::TName("Float".into()), false)]
                        ),
                    ]
                );
            }
            other => panic!("expected Sum, got {other:?}"),
        }
    }

    #[test]
    fn newtype_strict_field_is_parse_error() {
        // Haskell 2010: newtypes cannot declare strict fields.
        let src = "newtype Hz = Hz !Float; main = Hz 1.0";
        assert!(parse(&tokenize(src).unwrap(), src.as_bytes()).is_err());
    }

    #[test]
    fn parses_type_and_newtype() {
        let p = prog("type Angles = Float; newtype Hz = Float; main = Hz 440.0");
        assert!(p.defs.iter().any(|d| matches!(d, Def::TypeAlias { .. })));
        assert!(p.defs.iter().any(|d| matches!(d, Def::Newtype { .. })));
    }

    #[test]
    fn parses_typeclass_declaration() {
        let p = prog("typeclass Envelope a where { slope: a; }; main = 1.0");
        assert!(p.defs.iter().any(|d| matches!(d, Def::Typeclass { .. })));
    }

    #[test]
    fn parses_parameterized_data_and_typeclass() {
        let p = prog("data Box a = { value: a }; typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }; main = 1.0");
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
                    ("value".to_string(), TypeExpr::TName("a".into()), false)
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
    fn parses_typeclass_default_bodies() {
        // `mname : sig = p1 p2 body;` — a class-level default body the
        // instances may omit. Params bind between `=` and the body.
        let p = prog("typeclass Arrow a where { arr: (b -> c) -> a b c; second: a b c -> a (Pair d b) (Pair d c) = k (compose (first k)); both: a b c -> a d e -> a (Pair b d) (Pair c e) = f g (compose (first f) (second g)); }; main = 1.0");
        match &p.defs[0] {
            Def::Typeclass { name, defaults, .. } => {
                assert_eq!(name, "Arrow");
                assert_eq!(defaults.len(), 2);
                // `second k = compose (first k)` — one param `k`.
                assert_eq!(defaults[0].0, "second");
                assert_eq!(defaults[0].1.len(), 1);
                assert_eq!(defaults[0].1[0].name, "k");
                // `both f g = …` — two params.
                assert_eq!(defaults[1].0, "both");
                assert_eq!(defaults[1].1.len(), 2);
                assert_eq!(defaults[1].1[0].name, "f");
                assert_eq!(defaults[1].1[1].name, "g");
            }
            other => panic!("expected Typeclass, got {other:?}"),
        }
    }

    #[test]
    fn tuple_type_desugars_to_pair() {
        // `(Float, Float)` in type position must desugar to `TApp("Pair", …)` —
        // typeclass defs store the raw TypeExpr (no resolution), so compilation
        // alone cannot catch a wrong desugar target.
        let p = prog("typeclass T a where { m: a (Float, Float) -> Float; }; main = 1.0");
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

    /// The `TypeExpr` stored as a `Def::Eval` signature.
    fn eval_sig(src: &str) -> TypeExpr {
        let p = prog(src);
        match &p.defs[0] {
            Def::Eval { sig: Some(sig), .. } => sig.clone(),
            other => panic!("expected Eval with a signature, got {other:?}"),
        }
    }

    #[test]
    fn parses_single_constraint_signature_context() {
        // Haskell 2010: `f : C v => T` — one constraint, no parentheses.
        let sig = eval_sig("f :: Ix i => Pair i i -> List e -> Array i e = f; main = 1.0");
        match &sig {
            TypeExpr::TConstrained(constraints, body) => {
                assert_eq!(constraints, &vec![("Ix".to_string(), "i".to_string())]);
                assert_eq!(body.arg_count(), 2);
                match body.as_ref() {
                    TypeExpr::TFunc(args, _) => {
                        assert_eq!(
                            args[0],
                            TypeExpr::TApp(
                                "Pair".into(),
                                vec![TypeExpr::TName("i".into()), TypeExpr::TName("i".into())]
                            )
                        );
                        assert_eq!(
                            args[1],
                            TypeExpr::TApp("List".into(), vec![TypeExpr::TName("e".into())])
                        );
                    }
                    other => panic!("expected TFunc body, got {other:?}"),
                }
            }
            other => panic!("expected a constrained signature, got {other:?}"),
        }
    }

    #[test]
    fn parses_parenthesized_constraint_list_signature_context() {
        // `(C v, C v) => T` — a comma-separated constraint list.
        let sig =
            eval_sig("ixmap :: (Ix i, Ix j) => Pair i i -> Pair j j -> Float = ixmap; main = 1.0");
        match &sig {
            TypeExpr::TConstrained(constraints, body) => {
                assert_eq!(
                    constraints,
                    &vec![
                        ("Ix".to_string(), "i".to_string()),
                        ("Ix".to_string(), "j".to_string()),
                    ]
                );
                assert_eq!(body.arg_count(), 2);
            }
            other => panic!("expected a constrained signature, got {other:?}"),
        }
    }

    #[test]
    fn parenthesized_type_is_not_a_constraint_context() {
        // `(Pair i i) -> Float` starts with `(` but holds ONE type, not a
        // `C v` constraint pair — the constraint lookahead must not swallow it.
        let sig = eval_sig("f :: (Pair i i) -> Float = f; main = 1.0");
        match &sig {
            TypeExpr::TFunc(args, _) => assert_eq!(args.len(), 1),
            other => panic!("expected an unconstrained TFunc, got {other:?}"),
        }
    }

    #[test]
    fn unconstrained_signature_stays_unwrapped() {
        // Backward-compatible shape: no `=>` means no `TConstrained` wrapper.
        let sig = eval_sig("f :: Float -> Float = f; main = 1.0");
        assert!(matches!(sig, TypeExpr::TFunc(ref args, _) if args.len() == 1));
    }

    #[test]
    fn constraint_class_name_is_not_hardcoded() {
        // The context is generic over the class: nothing may assume `Ix`.
        let sig = eval_sig("f :: Monad m => m a -> m a = f; main = 1.0");
        match &sig {
            TypeExpr::TConstrained(constraints, body) => {
                assert_eq!(constraints, &vec![("Monad".to_string(), "m".to_string())]);
                assert_eq!(body.arg_count(), 1);
            }
            other => panic!("expected a constrained signature, got {other:?}"),
        }
    }

    #[test]
    fn unparenthesized_multi_constraint_is_rejected_with_a_diagnostic() {
        // `C v, C w => T` is not valid Haskell 2010; without the guard it fails
        // later on the comma with an unrelated "expected Eq" message.
        let src = "f :: Ix i, Ix j => Float = f; main = 1.0";
        let err = parse(&tokenize(src).unwrap(), src.as_bytes())
            .expect_err("an unparenthesized multi-constraint context must fail");
        match err {
            CompileError::Parse { msg, .. } => assert!(
                msg.contains("multi-constraint context must be parenthesized"),
                "expected the parenthesization diagnostic, got: {msg}"
            ),
            other => panic!("expected a Parse error, got {other:?}"),
        }
    }

    #[test]
    fn parses_type_application() {
        let p = prog("data V = { xs: List Float }; main = 1.0");
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
        let p = prog("instance Envelope Linear where { slope = 0.5; }; main = 1.0");
        assert!(p.defs.iter().any(|d| matches!(d, Def::Instance { .. })));
    }

    #[test]
    fn parses_foreign_def_in_brace_where_block() {
        // `foreign` inside a brace-style where-block: the def parser must
        // leave the trailing `;` for `parse_where_block` to consume
        // (regression: it used to eat its own semi, double-eating and failing
        // on the next def with "expected Semi").
        let p = prog(
            r"main = x where { foreign biquad : FixedBuffer f32 -> FixedBuffer f32; x = 1.0; }",
        );
        let main = p.main_def().unwrap();
        assert_eq!(main.where_defs().len(), 2);
        match &main.where_defs()[0] {
            Def::Foreign { name, .. } => assert_eq!(name, "biquad"),
            other => panic!("expected Foreign, got {other:?}"),
        }
    }

    #[test]
    fn parses_constraint_instance_head() {
        let p = prog("instance (Monad m) => Arrow (Kleisli m) where { arr f = f; }; main = 1.0");
        let inst = p
            .defs
            .iter()
            .find(|d| matches!(d, Def::Instance { .. }))
            .unwrap();
        let Def::Instance {
            class,
            ty,
            constraints,
            head_args,
            method_bodies,
            ..
        } = inst
        else {
            unreachable!("expected an instance def")
        };
        assert_eq!(class, "Arrow");
        assert_eq!(ty, "Kleisli");
        assert_eq!(constraints, &[("Monad".to_string(), "m".to_string())]);
        assert_eq!(head_args, &["m".to_string()]);
        assert_eq!(method_bodies.len(), 1);
    }

    #[test]
    fn plain_instance_has_empty_constraints_and_head_args() {
        // Backward compatibility: `instance C T where { … }` (no constraint
        // list, no parenthesized head) must keep both new fields empty.
        let p = prog("instance Envelope Linear where { slope = 0.5; }; main = 1.0");
        let Def::Instance {
            constraints,
            head_args,
            ..
        } = p
            .defs
            .iter()
            .find(|d| matches!(d, Def::Instance { .. }))
            .unwrap()
        else {
            unreachable!("expected an instance def")
        };
        assert!(constraints.is_empty());
        assert!(head_args.is_empty());
    }

    #[test]
    fn parses_match_expression() {
        let p = prog(r"area x = case x of { Circle r -> r; Rect w h -> w; }; main = area");
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
            r"data Shape = Circle Float | Rect Float Float; main = case s of { Circle r -> r; 0 -> 0.0; _ -> 1.0; n | n > 0 -> n; };",
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
            "a bare `Circle r -> r` arm's first guard is `true`"
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
    fn rejects_fat_arrow_as_match_arm() {
        // Haskell 2010: `=>` is the class-constraint arrow, so a match arm
        // spelled with `=>` must be a parse error (the legacy spelling was
        // dropped — backward compatibility is not required).
        let src = "main = case x of { _ => 1.0 }";
        match parse(&tokenize(src).unwrap(), src.as_bytes()) {
            Err(CompileError::Parse { msg, .. }) => assert!(
                msg.contains("ThinArrow"),
                "expected the missing-arrow message to mention ThinArrow, got: {msg}"
            ),
            other => panic!("expected a Parse error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_thin_arrow_in_instance_constraint_head() {
        // The constraint list before an instance head is introduced by `=>`;
        // `->` is the function-type arrow and must be rejected there.
        let src = "instance (Monad m) -> Arrow (Kleisli m) where { arr f = f; }; main = 1.0";
        match parse(&tokenize(src).unwrap(), src.as_bytes()) {
            Err(CompileError::Parse { msg, .. }) => assert!(
                msg.contains("FatArrow"),
                "expected the missing-arrow message to mention FatArrow, got: {msg}"
            ),
            other => panic!("expected a Parse error, got {other:?}"),
        }
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
        let p = prog(r"double = \x -> x * 2.0; main = double");
        let d = p.defs.iter().find(|d| d.name() == "double").unwrap();
        assert!(matches!(d.body(), Expr::Lambda { .. }));
    }

    #[test]
    fn parses_nested_lambda() {
        let p = prog(r"adder = \n -> \x -> x + n; main = adder 2.0");
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

    #[test]
    fn backslash_lambda_is_unary() {
        match body("main = \\x -> x + 1.0") {
            Expr::Lambda { params, body, .. } => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "x");
                assert!(matches!(body.as_ref(), Expr::Arith { .. }));
            }
            other => panic!("expected Lambda, got {other:?}"),
        }
    }

    /// Parse `src`, expecting an error, and return its rendered message.
    fn parse_err(src: &str) -> String {
        parse(&tokenize(src).unwrap(), src.as_bytes())
            .expect_err("expected a parse error")
            .to_string()
    }

    #[test]
    fn bang_index_parses_as_index_application() {
        // A separated `!` is infix indexing: `a ! i` desugars to `index a i`.
        let e = body("main = a ! i");
        let Some(("index", args)) = curry_head(&e) else {
            panic!("expected an `index` application, got {e:?}");
        };
        assert_eq!(args.len(), 2);
        assert!(matches!(args[0], Expr::Ref(n, _) if n == "a"));
        assert!(matches!(args[1], Expr::Ref(n, _) if n == "i"));
    }

    #[test]
    fn bang_index_is_left_associative() {
        // `a ! b ! c` ≡ `(a ! b) ! c` ≡ `index (index a b) c`.
        let e = body("main = a ! b ! c");
        let Some(("index", args)) = curry_head(&e) else {
            panic!("expected an `index` application, got {e:?}");
        };
        assert_eq!(args.len(), 2);
        assert!(matches!(curry_head(args[0]), Some(("index", inner)) if inner.len() == 2));
        assert!(matches!(args[1], Expr::Ref(n, _) if n == "c"));
    }

    #[test]
    fn bang_index_binds_tighter_than_plus() {
        // `a ! i + 1` ≡ `(a ! i) + 1` — indexing sits just below application.
        match body("main = a ! i + 1") {
            Expr::Arith {
                op: ArithOp::Add,
                lhs,
                rhs,
                ..
            } => {
                assert!(matches!(curry_head(&lhs), Some(("index", args)) if args.len() == 2));
                assert!(matches!(rhs.as_ref(), Expr::Int(1, _)));
            }
            other => panic!("expected Arith, got {other:?}"),
        }
    }

    #[test]
    fn attached_bang_in_expression_position_is_indexing() {
        // A `!` in expression position is the infix index operator regardless
        // of whitespace: `a!i`, `a !i`, `a! 1`, `a ! 1` all desugar to a
        // two-arg `index` application — matching the other infix operators
        // (`a+1` ≡ `a + 1`). Bang markers only exist in pattern positions
        // (params, case patterns, bindings), which parse through separate
        // paths.
        for src in ["main = a!i", "main = a !i", "main = a! i", "main = a ! i"] {
            let e = body(src);
            let Some(("index", args)) = curry_head(&e) else {
                panic!("expected an `index` application for `{src}`, got {e:?}");
            };
            assert_eq!(args.len(), 2, "`{src}` must desugar to a two-arg `index`");
            assert!(matches!(args[0], Expr::Ref(n, _) if n == "a"));
        }
        // A numeric index parses the same way: `a!1` ≡ `index a 1`.
        let e = body("main = a!1");
        let Some(("index", args)) = curry_head(&e) else {
            panic!("expected an `index` application for `main = a!1`, got {e:?}");
        };
        assert_eq!(args.len(), 2);
        assert!(matches!(args[1], Expr::Int(1, _)));
    }

    #[test]
    fn bang_param_marks_definition_param() {
        // `f !x = …` — the bang marker rides on the parameter.
        let p = prog("main !x = 1.0");
        let main = p.main_def().unwrap();
        assert_eq!(main.params().len(), 1);
        assert_eq!(main.params()[0].name, "x");
        assert!(main.params()[0].bang, "`!x` must set Param.bang");
    }

    #[test]
    fn bang_param_survives_currying() {
        // `f a !x = …` curries into `f = \a -> \!x -> …`; the marker is kept.
        match body("main a !x = 1.0") {
            Expr::Lambda { params, body, .. } => {
                assert_eq!(params[0].name, "a");
                assert!(!params[0].bang);
                match body.as_ref() {
                    Expr::Lambda { params, .. } => {
                        assert_eq!(params[0].name, "x");
                        assert!(params[0].bang);
                    }
                    other => panic!("expected nested lambda, got {other:?}"),
                }
            }
            other => panic!("expected curried lambda, got {other:?}"),
        }
    }

    #[test]
    fn bang_lambda_param_parses() {
        match body("main = \\ !x -> x") {
            Expr::Lambda { params, .. } => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "x");
                assert!(params[0].bang);
            }
            other => panic!("expected Lambda, got {other:?}"),
        }
    }

    #[test]
    fn bang_let_binding_parses() {
        match body("main = let !x = 1.0 in x") {
            Expr::Let { defs, .. } => {
                assert_eq!(defs.len(), 1);
                assert_eq!(defs[0].name(), "x");
                assert!(defs[0].is_bang(), "`let !x = e` must set Def::Eval.bang");
            }
            other => panic!("expected Let, got {other:?}"),
        }
    }

    #[test]
    fn bang_case_pattern_parses() {
        match body("main = case 1.0 of { !x -> x }") {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 1);
                assert!(
                    matches!(&arms[0].pattern, Pattern::Bang(inner)
                        if matches!(inner.as_ref(), Pattern::Var(v) if v == "x")),
                    "expected Pattern::Bang(Var x), got {:?}",
                    arms[0].pattern
                );
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn bang_nested_in_constructor_pattern() {
        // `Just !x` — a bang sub-pattern inside a constructor pattern.
        match body("main = case Just 1.0 of { Just !x -> x; Nothing -> 0.0 }") {
            Expr::Match { arms, .. } => {
                assert!(
                    matches!(&arms[0].pattern, Pattern::Ctor(c, args)
                        if c == "Just"
                            && matches!(args.as_slice(), [Pattern::Bang(_)])),
                    "expected Ctor(Just, [Bang …]), got {:?}",
                    arms[0].pattern
                );
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn bang_on_function_binding_is_rejected() {
        // A bang binding marks a single variable. `!f x = e` has nothing to
        // force at the binding point (the RHS is a curried function), so it is
        // rejected instead of silently dropping the parameter binding.
        for src in ["main = let !f x = x in 1.0", "main = 1.0 where !f x = x"] {
            let msg = parse_err(src);
            assert!(
                msg.contains("single variable"),
                "expected a single-variable diagnostic for `{src}`, got: {msg}"
            );
        }
        // The variable-only forms still parse.
        assert!(matches!(body("main = let !x = 1.0 in x"), Expr::Let { .. }));
        assert!(matches!(body("main = \\ !x -> x"), Expr::Lambda { .. }));
    }

    #[test]
    fn spaced_bang_pattern_is_rejected() {
        // Bang is always attached: `! x` is not a bang pattern.
        let msg = parse_err("main = case 1.0 of { ! x -> x }");
        assert!(
            msg.contains("bang pattern must be written"),
            "expected a bang-attachment diagnostic, got: {msg}"
        );
        // Nor is a spaced bang a parameter marker: the head rejects it with the
        // bang-specific message, not a generic `expected Eq`.
        let msg = parse_err("main ! x = 1.0");
        assert!(
            msg.contains("bang parameter must be written"),
            "expected a bang-parameter diagnostic, got: {msg}"
        );
    }

    #[test]
    fn case_parses_like_match() {
        match body("main = case x of { Just v -> v; Nothing -> 0.0; }") {
            Expr::Match {
                scrutinee, arms, ..
            } => {
                assert!(matches!(scrutinee.as_ref(), Expr::Ref(n, _) if n == "x"));
                assert_eq!(arms.len(), 2);
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }
}
