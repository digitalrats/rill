//! AST → DSL source renderer.
//!
//! Converts a parsed [`crate::ast::Program`] back into a rill-lang source
//! string. The renderer is used for round-trip tests that verify
//! isomorphism between JSON and DSL representations. Rendering is total:
//! `if` and `match` render every arm, including guarded alternatives.

use crate::ast::{Def, Expr, Pattern, Program, TypeExpr};
use crate::error::CompileError;
use crate::mode::Mode;
use std::fmt::Write;

/// Render a program as a rill-lang source string.
pub fn render(program: &Program) -> Result<String, CompileError> {
    let mut buf = String::new();
    if program.mode != Mode::Strict {
        writeln!(buf, "{{-# MODE {} #-}}", mode_name(program.mode)).ok();
    }
    for (i, def) in program.defs.iter().enumerate() {
        if i > 0 {
            buf.push('\n');
        }
        render_def(def, &mut buf, 0)?;
    }
    Ok(buf)
}

/// The `{-# MODE ... #-}` spelling of an evaluation mode.
fn mode_name(mode: Mode) -> String {
    match mode {
        Mode::Strict => "strict".into(),
        Mode::Complete => "complete".into(),
    }
}

/// Render a definition NAME. An operator name (`++`, `+`, `==`) must be
/// parenthesized to be a valid definition head, exactly as it is written in
/// source (`(++) a b = …`).
fn render_def_name(name: &str, buf: &mut String) {
    if is_plain_ident(name) {
        write!(buf, "{name}").ok();
    } else {
        write!(buf, "({name})").ok();
    }
}

/// Whether `name` is a plain identifier, i.e. renderable without parentheses.
fn is_plain_ident(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Render a name in expression position. An operator reference needs the
/// parenthesized spelling (`(++)`, not `++`, which would re-parse as the infix
/// operator).
fn render_ref_name(name: &str, buf: &mut String) {
    if is_plain_ident(name) {
        write!(buf, "{name}").ok();
    } else {
        write!(buf, "({name})").ok();
    }
}

/// Render a CALLEE name. An operator callee must be parenthesized as a whole
/// atom: `(++) xs ys`, otherwise `++ xs ys` re-parses as an infix expression.
fn render_callee_name(name: &str, buf: &mut String) {
    if is_plain_ident(name) {
        write!(buf, "{name}").ok();
    } else {
        write!(buf, "({name})").ok();
    }
}

fn render_def(def: &Def, buf: &mut String, indent: usize) -> Result<(), CompileError> {
    let pad = " ".repeat(indent);
    match def {
        Def::Eval {
            name,
            params,
            body,
            where_defs,
            sig,
            ..
        } => {
            // A bang binding renders its marker before the bound name: `!x = e`.
            write!(buf, "{pad}{}", if def.is_bang() { "!" } else { "" }).ok();
            render_def_name(name, buf);
            for p in params {
                write!(buf, " {}{}", if p.bang { "!" } else { "" }, p.name).ok();
            }
            // Haskell 2010 signature annotation: `name args :: sig = body`.
            if let Some(sig) = sig {
                write!(buf, " :: ").ok();
                render_type_expr(sig, buf);
            }
            write!(buf, " = ").ok();
            render_expr(body, buf, 3)?;
            render_where(where_defs, buf, indent, &pad)?;
            Ok(())
        }
        Def::Data {
            name,
            tyvars,
            fields,
            ..
        } => {
            write!(buf, "{pad}data {name}").ok();
            for tv in tyvars {
                write!(buf, " {tv}").ok();
            }
            write!(buf, " = {{ ").ok();
            for (i, (fname, tname, strict)) in fields.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ").ok();
                }
                write!(buf, "{fname}: ").ok();
                if *strict {
                    write!(buf, "!").ok();
                }
                render_type_expr(tname, buf);
            }
            write!(buf, " }}").ok();
            Ok(())
        }
        Def::Sum {
            name,
            tyvars,
            ctors,
            ..
        } => {
            write!(buf, "{pad}data {name}").ok();
            for tv in tyvars {
                write!(buf, " {tv}").ok();
            }
            write!(buf, " = ").ok();
            for (i, (cname, payload)) in ctors.iter().enumerate() {
                if i > 0 {
                    write!(buf, " | ").ok();
                }
                write!(buf, "{cname}").ok();
                for (t, strict) in payload {
                    write!(buf, " ").ok();
                    if *strict {
                        write!(buf, "!").ok();
                    }
                    render_type_expr(t, buf);
                }
            }
            Ok(())
        }
        Def::Abstract { name, tyvars, .. } => {
            write!(buf, "{pad}data {name}").ok();
            for tv in tyvars {
                write!(buf, " {tv}").ok();
            }
            write!(buf, ";").ok();
            Ok(())
        }
        Def::TypeAlias { name, target, .. } => {
            write!(buf, "{pad}type {name} = ").ok();
            render_type_expr(target, buf);
            Ok(())
        }
        Def::Newtype { name, target, .. } => {
            write!(buf, "{pad}newtype {name} = {target}").ok();
            Ok(())
        }
        Def::Typeclass {
            name,
            var,
            supers,
            methods,
            ..
        } => {
            write!(buf, "{pad}typeclass ").ok();
            // A superclass context renders in the same shape the parser reads
            // back: `(C1 a, C2 a) =>`.
            if !supers.is_empty() {
                write!(buf, "(").ok();
                for (i, (c, v)) in supers.iter().enumerate() {
                    if i > 0 {
                        write!(buf, ", ").ok();
                    }
                    write!(buf, "{c} {v}").ok();
                }
                write!(buf, ") => ").ok();
            }
            write!(buf, "{name} {var} where {{ ").ok();
            for (mname, sig) in methods {
                render_def_name(mname, buf);
                write!(buf, ": ").ok();
                render_type_expr(sig, buf);
                write!(buf, "; ").ok();
            }
            write!(buf, "}}").ok();
            Ok(())
        }
        Def::Instance {
            class,
            head,
            method_bodies,
            ..
        } => {
            write!(buf, "{pad}instance {class} ").ok();
            // An applied head (`Kleisli m`, `Pair Int Int`) must be
            // parenthesized to re-parse; a bare head is written as-is.
            if matches!(head, TypeExpr::TApp(..)) {
                write!(buf, "(").ok();
                render_type_key_into(head, buf);
                write!(buf, ")").ok();
            } else {
                render_type_key_into(head, buf);
            }
            write!(buf, " where {{ ").ok();
            for (mname, params, body) in method_bodies {
                render_def_name(mname, buf);
                for p in params {
                    write!(buf, " {}", p.name).ok();
                }
                write!(buf, " = ").ok();
                render_expr(body, buf, 0)?;
                write!(buf, "; ").ok();
            }
            write!(buf, "}}").ok();
            Ok(())
        }
        Def::Foreign { name, sig, .. } => {
            write!(buf, r"{pad}foreign {name}: ").ok();
            render_type_expr(sig, buf);
            write!(buf, ";").ok();
            Ok(())
        }
        // The parser's intermediate equation forms never survive
        // [`crate::desugar`], which runs before this renderer in every
        // pipeline. Rendering them anyway keeps the error local to the
        // renderer rather than panicking on an unreachable arm.
        Def::Equations { .. } | Def::Signature { .. } => Err(CompileError::Unsupported(
            "a program reached the renderer before its equation surface was desugared".into(),
        )),
    }
}

/// Render an optional `where { … }` clause. The braces keep the clause on one
/// visual block and round-trip through the parser's brace form of
/// `parse_where_block`.
fn render_where(
    where_defs: &[Def],
    buf: &mut String,
    indent: usize,
    pad: &str,
) -> Result<(), CompileError> {
    if where_defs.is_empty() {
        return Ok(());
    }
    writeln!(buf, " where {{").ok();
    for wd in where_defs {
        render_def(wd, buf, indent + 4)?;
        writeln!(buf, ";").ok();
    }
    write!(buf, "{pad}}}").ok();
    Ok(())
}

/// Render a type expression in a declaration (name, application, function
/// type, or capacity literal).
fn render_type_expr(t: &TypeExpr, buf: &mut String) {
    match t {
        TypeExpr::TName(n) => {
            write!(buf, "{n}").ok();
        }
        TypeExpr::TApp(head, args) => {
            if render_builtin_type_sugar(head, args, buf, &mut render_type_arg) {
                return;
            }
            write!(buf, "{head}").ok();
            for a in args {
                write!(buf, " ").ok();
                render_type_arg(a, buf);
            }
        }
        TypeExpr::TFunc(args, ret) => {
            write!(buf, "(").ok();
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    write!(buf, " -> ").ok();
                }
                render_type_arg(a, buf);
            }
            write!(buf, " -> ").ok();
            render_type_expr(ret, buf);
            write!(buf, ")").ok();
        }
        TypeExpr::TForall(vars, inner) => {
            write!(buf, "forall").ok();
            for v in vars {
                write!(buf, " {v}").ok();
            }
            write!(buf, ". ").ok();
            render_type_expr(inner, buf);
        }
        TypeExpr::TConstrained(constraints, inner) => {
            // Haskell 2010: a single constraint stands bare, two or more must be
            // parenthesized and comma-separated to re-parse.
            if constraints.len() == 1 {
                let (class, var) = &constraints[0];
                write!(buf, "{class} {var}").ok();
            } else {
                write!(buf, "(").ok();
                for (i, (class, var)) in constraints.iter().enumerate() {
                    if i > 0 {
                        write!(buf, ", ").ok();
                    }
                    write!(buf, "{class} {var}").ok();
                }
                write!(buf, ")").ok();
            }
            write!(buf, " => ").ok();
            render_type_expr(inner, buf);
        }
    }
}

/// Write the built-in list or tuple constructor in its SOURCE spelling —
/// `[Int]` for `List Int`, `(a, b)` for `Pair a b` — and report whether it did.
///
/// Haskell 2010 §3.7 spells these two constructors with bracket/comma syntax;
/// the parser desugars that syntax to the nominal application
/// (`TApp("List", [t])`, `TApp("Pair", [a, b])`), so printing the nominal form
/// back would show a spelling nobody writes. A PARTIALLY applied constructor
/// (`Pair a`, `List`) has no source spelling and returns `false`, keeping the
/// nominal form the diag must still be able to name.
fn render_builtin_type_sugar(
    head: &str,
    args: &[TypeExpr],
    buf: &mut String,
    render_arg: &mut dyn FnMut(&TypeExpr, &mut String),
) -> bool {
    match (head, args) {
        ("List", [elem]) => {
            buf.push('[');
            render_arg(elem, buf);
            buf.push(']');
            true
        }
        ("Pair", [first, second]) => {
            buf.push('(');
            render_arg(first, buf);
            buf.push_str(", ");
            render_arg(second, buf);
            buf.push(')');
            true
        }
        _ => false,
    }
}

/// Render a type expression in argument position: a `forall` binder must be
/// parenthesized so it does not absorb a following arrow chain on re-parse
/// (`(forall b. a) -> a` must not re-read as `forall b. (a -> a)`).
///
/// A `TConstrained` context is unreachable here today: the parser only builds
/// one at the ROOT of a `::` signature, never inside an argument. Should a later
/// task admit one, it needs its own parenthesization here (a bare `C v => T`
/// would otherwise absorb the enclosing arrow chain and not re-parse).
fn render_type_arg(t: &TypeExpr, buf: &mut String) {
    if matches!(t, TypeExpr::TForall(..)) {
        write!(buf, "(").ok();
        render_type_expr(t, buf);
        write!(buf, ")").ok();
    } else {
        render_type_expr(t, buf);
    }
}

/// Canonical single-line spelling of a type, used as an instance-registry key:
/// `Pair Int (Pair Int Int)`. Unlike [`render_type_expr`], a nested
/// application is parenthesized so the spelling is unambiguous (a flat
/// `Pair Int Pair Int Int` would not re-parse to the same tree).
///
/// This is deliberately NOT the source spelling: it must agree byte for byte
/// with [`crate::types::ty::value_type_key`] (dispatch keys a selector's value
/// type that way), so it keeps the nominal `Pair`/`List` names. Diagnostics
/// that quote what a user wrote go through [`render_type_spelling`].
pub(crate) fn render_type_key(t: &TypeExpr) -> String {
    let mut buf = String::new();
    render_type_key_into(t, &mut buf);
    buf
}

/// The SOURCE spelling of a type expression for diagnostics: the built-in list
/// and tuple applications read back as `[t]` / `(a, b)`, exactly as a program
/// writes them (Haskell 2010 §3.7). A partially applied constructor keeps its
/// nominal name (`Pair Int`, `List`) — those have no sugar spelling.
pub(crate) fn render_type_spelling(t: &TypeExpr) -> String {
    let mut buf = String::new();
    render_type_spelling_into(t, &mut buf);
    buf
}

fn render_type_spelling_into(t: &TypeExpr, buf: &mut String) {
    render_type_into(t, buf, Spelling::Source);
}

/// Whether a `TApp` is written with the built-in list/tuple syntax.
fn is_builtin_type_sugar(t: &TypeExpr) -> bool {
    matches!(
        t,
        TypeExpr::TApp(head, args)
            if matches!((head.as_str(), args.len()), ("List", 1) | ("Pair", 2))
    )
}

fn render_type_key_into(t: &TypeExpr, buf: &mut String) {
    render_type_into(t, buf, Spelling::Nominal);
}

/// Which spelling a single-line type renderer produces.
#[derive(Clone, Copy, PartialEq)]
enum Spelling {
    /// Nominal constructor application: `Pair Int (List Int)`, `(a, b)`-free.
    /// The instance-registry key; must agree with
    /// [`crate::types::ty::value_type_key`].
    Nominal,
    /// Haskell 2010 source syntax: `(Int, [Int])`.
    Source,
}

/// Render a type on ONE line, parenthesizing a nested application so the
/// spelling is unambiguous and re-parses to the same tree.
fn render_type_into(t: &TypeExpr, buf: &mut String, spelling: Spelling) {
    match t {
        TypeExpr::TName(n) => buf.push_str(n),
        TypeExpr::TApp(head, args) => {
            if spelling == Spelling::Source
                && render_builtin_type_sugar(head, args, buf, &mut |a, b| {
                    render_type_into(a, b, spelling)
                })
            {
                return;
            }
            buf.push_str(head);
            for a in args {
                buf.push(' ');
                // A nested application must be parenthesized to stay
                // unambiguous — except a sugar form, which already carries its
                // own brackets/parens.
                let nested = match spelling {
                    // The registry key parenthesizes every non-name argument,
                    // so a key is unambiguous whatever the argument's shape.
                    Spelling::Nominal => !matches!(a, TypeExpr::TName(_)),
                    Spelling::Source => {
                        matches!(a, TypeExpr::TApp(..)) && !is_builtin_type_sugar(a)
                    }
                };
                if nested {
                    buf.push('(');
                }
                render_type_into(a, buf, spelling);
                if nested {
                    buf.push(')');
                }
            }
        }
        // Function types and binders render identically in both spellings;
        // [`render_type_expr`] already parenthesizes them for re-parsing.
        other => render_type_expr(other, buf),
    }
}

fn render_expr(expr: &Expr, buf: &mut String, outer_bp: u8) -> Result<(), CompileError> {
    match expr {
        Expr::Int(v, _) => {
            write!(buf, "{v}").ok();
            Ok(())
        }
        Expr::Void(_) => {
            write!(buf, "()").ok();
            Ok(())
        }
        // A fractional literal re-emits its SOURCE spelling, so `1e3` stays
        // `1e3` and `1.00` stays `1.00` instead of folding to a canonical
        // `f64` print. The exact value is not consulted: the text is part of
        // the node precisely so the round-trip is faithful.
        Expr::FractionalLit(_, text, _) => {
            write!(buf, "{text}").ok();
            Ok(())
        }
        Expr::Str(s, _) => {
            write!(buf, "\"{s}\"").ok();
            Ok(())
        }
        Expr::Ref(name, _) => {
            render_ref_name(name, buf);
            Ok(())
        }
        Expr::Apply { name, args, .. } => {
            // An `Apply` whose NAME is an infix operator is what the parser
            // produces for the infix spelling (`a + b` and `(+) a b` are the
            // same AST — Haskell 2010 §3.4, System FC has no operator node).
            // Render it infix so the source round-trips readably; the binding
            // powers come from the same §4.4.2 table the parser uses.
            if let (Some(info), 2) = (infix_spelling(name), args.len()) {
                return render_bin(&args[0], &args[1], buf, outer_bp, info);
            }
            render_callee_name(name, buf);
            for a in args {
                write!(buf, " ").ok();
                render_expr(a, buf, 20)?; // application args are tight
            }
            Ok(())
        }
        Expr::ApplyExpr { callee, args, .. } => {
            // The currying pass rewrites a two-argument application into a
            // chain of unary ones, so the operator application the parser
            // produced for `a + b` arrives here as `((+) a) b`. Recognise that
            // chain and render it infix, exactly like a direct `Apply`.
            if let Some((lhs, rhs, info)) = infix_application(callee, args) {
                return render_bin(lhs, rhs, buf, outer_bp, info);
            }
            write!(buf, "(").ok();
            render_expr(callee, buf, 0)?;
            write!(buf, ")").ok();
            for a in args {
                write!(buf, " ").ok();
                render_expr(a, buf, 20)?; // application args are tight
            }
            Ok(())
        }
        Expr::Neg(inner, _) => {
            write!(buf, "-").ok();
            render_expr(inner, buf, 15)?;
            Ok(())
        }
        // A kernel renders back as its reserved spacing — the spelling is
        // internal, and the renderer's contract is a SOURCE round-trip (the
        // re-parsed program must be the same AST, not necessarily a program a
        // user wrote). The name is the one `Kernel::from_reserved_spelling`
        // recognises, so the round-trip re-binds the same node.
        Expr::Kernel { kernel, args, .. } => {
            write!(buf, "{}", kernel.spelling()).ok();
            for a in args {
                write!(buf, " ").ok();
                render_expr(a, buf, 20)?; // application args are tight
            }
            Ok(())
        }
        Expr::Annot { expr, ty, .. } => {
            // `e :: t` (Haskell 2010 §3.12). Two parenthesization concerns:
            //
            // 1. The annotated side of `exp :: type` must be an `infixexp`, so
            //    a full-`exp` form (a lambda, `let`, `if`, `case`, or another
            //    annotation) needs parentheses or the annotation would bind
            //    into that form's body instead (`\x -> x :: Int` is
            //    `\x -> (x :: Int)`).
            // 2. The annotation itself lives at `exp` level, looser than every
            //    operator, so an operator context must parenthesize it — an
            //    unparenthesized `x + y :: Int` re-parses as `(x + y) :: Int`.
            let inner_parens = annot_operand_needs_parens(expr);
            let outer_parens = outer_bp > 0;
            if outer_parens {
                write!(buf, "(").ok();
            }
            if inner_parens {
                write!(buf, "(").ok();
            }
            render_expr(expr, buf, 0)?;
            if inner_parens {
                write!(buf, ")").ok();
            }
            write!(buf, " :: ").ok();
            render_type_expr(ty, buf);
            if outer_parens {
                write!(buf, ")").ok();
            }
            Ok(())
        }
        Expr::Let { defs, body, .. } => {
            write!(buf, "let ").ok();
            if defs.len() > 1 {
                write!(buf, "{{ ").ok();
                for d in defs {
                    render_def(d, buf, 0)?;
                    write!(buf, "; ").ok();
                }
                write!(buf, "}} in ").ok();
            } else {
                for d in defs {
                    render_def(d, buf, 0)?;
                }
                write!(buf, " in ").ok();
            }
            render_expr(body, buf, 0)?;
            Ok(())
        }
        Expr::Record(fields, _) => {
            write!(buf, "{{ ").ok();
            for (i, (name, val)) in fields.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ").ok();
                }
                write!(buf, "{name}: ").ok();
                render_expr(val, buf, 0)?;
            }
            write!(buf, " }}").ok();
            Ok(())
        }
        Expr::ActorParam { name, default, .. } => {
            write!(buf, "?{name}").ok();
            if let Some(d) = default {
                write!(buf, "=").ok();
                render_expr(d, buf, 0)?;
            }
            Ok(())
        }
        Expr::FieldProject { record, field, .. } => {
            render_expr(record, buf, 20)?;
            write!(buf, ".{field}").ok();
            Ok(())
        }
        Expr::FieldUpdate {
            record,
            field,
            value,
            ..
        } => {
            render_expr(record, buf, 20)?;
            write!(buf, ".{field} := ").ok();
            render_expr(value, buf, 0)?;
            Ok(())
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            write!(buf, "case ").ok();
            render_expr(scrutinee, buf, 0)?;
            write!(buf, " of {{ ").ok();
            for arm in arms {
                render_pattern(&arm.pattern, buf);
                for (g, body) in &arm.guards {
                    // An alternative with NO guard (`pattern -> body`) renders
                    // without a `| guard` prefix; a written guard — even one
                    // that happens to be `True` — renders explicitly.
                    match g {
                        None => {
                            write!(buf, " -> ").ok();
                        }
                        Some(g) => {
                            write!(buf, " | ").ok();
                            render_expr(g, buf, 0)?;
                            write!(buf, " -> ").ok();
                        }
                    }
                    render_expr(body, buf, 0)?;
                }
                write!(buf, "; ").ok();
            }
            write!(buf, "}}").ok();
            Ok(())
        }
        Expr::If {
            cond, then, els, ..
        } => {
            write!(buf, "if ").ok();
            render_expr(cond, buf, 0)?;
            write!(buf, " then ").ok();
            render_expr(then, buf, 0)?;
            write!(buf, " else ").ok();
            render_expr(els, buf, 0)?;
            Ok(())
        }
        Expr::Lambda { params, body, .. } => {
            write!(buf, "\\").ok();
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    write!(buf, " ").ok();
                }
                write!(buf, "{}{}", if p.bang { "!" } else { "" }, p.name).ok();
            }
            write!(buf, " -> ").ok();
            render_expr(body, buf, 0)?;
            Ok(())
        }
        Expr::ListLit(elems, _) => {
            write!(buf, "[").ok();
            for (i, e) in elems.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ").ok();
                }
                render_expr(e, buf, 0)?;
            }
            write!(buf, "]").ok();
            Ok(())
        }
        Expr::MapLit(entries, _) => {
            write!(buf, "{{ ").ok();
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ").ok();
                }
                write!(buf, "\"{k}\": ").ok();
                render_expr(v, buf, 0)?;
            }
            write!(buf, " }}").ok();
            Ok(())
        }
    }
}

/// A two-operand operator application, ready to render infix: the operands and
/// the operator's binding powers.
type InfixOperands<'a> = (&'a Expr, &'a Expr, (u8, u8, u8, &'static str));

/// Recognise the curried form of a two-argument operator application:
/// `((+) a) b` (the AST the currying pass builds for both `a + b` and
/// `(+) a b`). Returns the two operands and the operator's binding powers so
/// the caller can render it infix.
fn infix_application<'a>(callee: &'a Expr, args: &'a [Expr]) -> Option<InfixOperands<'a>> {
    // `(+) a b` written as a parenthesized operator head: the parser keeps the
    // whole application as one `ApplyExpr` over `Ref(op)`.
    if let (Expr::Ref(name, _), [lhs, rhs]) = (callee, args) {
        return Some((lhs, rhs, infix_spelling(name)?));
    }
    // `((+) a) b` — the curried chain the parser builds for the written infix
    // spelling and for `(+) a b` inside an application argument.
    let [rhs] = args else {
        return None;
    };
    let Expr::ApplyExpr {
        callee: inner,
        args: first_args,
        ..
    } = callee
    else {
        return None;
    };
    let [lhs] = first_args.as_slice() else {
        return None;
    };
    let Expr::Ref(name, _) = inner.as_ref() else {
        return None;
    };
    Some((lhs, rhs, infix_spelling(name)?))
}

/// Render a binary node with parenthesization based on its binding power.
fn render_bin(
    lhs: &Expr,
    rhs: &Expr,
    buf: &mut String,
    outer_bp: u8,
    (prec, l_bp, r_bp, sym): (u8, u8, u8, &'static str),
) -> Result<(), CompileError> {
    if outer_bp > prec {
        write!(buf, "(").ok();
    }
    render_expr(lhs, buf, l_bp)?;
    write!(buf, " {sym} ").ok();
    render_expr(rhs, buf, r_bp)?;
    if outer_bp > prec {
        write!(buf, ")").ok();
    }
    Ok(())
}

/// The infix spelling and binding powers `(prec, l_bp, r_bp, symbol)` of an
/// application NAME that denotes an infix operator, or `None` for every other
/// function. `prec` is the node's own level; a child is parenthesized when the
/// level it inherits (`l_bp` on the left, `r_bp` on the right) is above its own.
///
/// The numbers are the parser's (§4.4.2, scaled): a left-associative operator
/// yields the same type on its left (`r_bp = prec + 1` parenthesizes it on the
/// right), a right-associative one the mirror image, and the non-associative
/// comparisons parenthesize a comparison on EITHER side (`l_bp = r_bp = prec + 1`)
/// so `(a == b) == c` — legal only parenthesized — round-trips as written.
fn infix_spelling(name: &str) -> Option<(u8, u8, u8, &'static str)> {
    Some(match name {
        // `infix 4`, non-associative.
        "==" => (5, 6, 6, "=="),
        "/=" => (5, 6, 6, "/="),
        "<" => (5, 6, 6, "<"),
        ">" => (5, 6, 6, ">"),
        "<=" => (5, 6, 6, "<="),
        ">=" => (5, 6, 6, ">="),
        // `infixr 2` / `infixr 3`: `&&` and `||` are ordinary Prelude
        // functions, so they render exactly like every other operator.
        "||" => (1, 2, 1, "||"),
        "&&" => (3, 4, 3, "&&"),
        // `infixr 5` / `infixr 6`.
        "++" => (7, 8, 7, "++"),
        "<>" => (9, 10, 9, "<>"),
        // `infixl 6` / `infixl 7`.
        "+" => (11, 11, 12, "+"),
        "-" => (11, 11, 12, "-"),
        "*" => (13, 13, 14, "*"),
        "/" => (13, 13, 14, "/"),
        "%" => (13, 13, 14, "%"),
        _ => return None,
    })
}

/// Whether the expression on the LEFT of `::` needs parentheses to stay a
/// single `infixexp` (Haskell 2010 §3.12: `exp → infixexp :: [context =>] type`).
///
/// A lambda, `let`, `if`, `case`, or nested annotation is a full `exp`, not an
/// `infixexp`; rendered bare it would swallow the annotation into its own body
/// or tail (`\x -> x :: Int` annotates the lambda's BODY, not the lambda).
/// Every other form — literals, references, applications, operators — is an
/// `infixexp` already.
fn annot_operand_needs_parens(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Lambda { .. }
            | Expr::Let { .. }
            | Expr::If { .. }
            | Expr::Match { .. }
            | Expr::Annot { .. }
    )
}

fn render_pattern(p: &Pattern, buf: &mut String) {
    match p {
        Pattern::Wild => {
            write!(buf, "_").ok();
        }
        Pattern::Var(n) => {
            write!(buf, "{n}").ok();
        }
        Pattern::LitInt(v) => {
            write!(buf, "{v}").ok();
        }
        Pattern::FractionalLit(_, text) => {
            write!(buf, "{text}").ok();
        }
        Pattern::LitStr(s) => {
            write!(buf, "\"{s}\"").ok();
        }
        Pattern::Ctor(n, args) => {
            write!(buf, "{n}").ok();
            for a in args {
                write!(buf, " ").ok();
                render_pattern_nested(a, buf);
            }
        }
        Pattern::Bang(inner) => {
            write!(buf, "!").ok();
            render_pattern_nested(inner, buf);
        }
    }
}

/// Render a subpattern in constructor-argument position. A nested `Ctor` with
/// arguments is parenthesized (`Just (Left y) 1`) so the inner constructor
/// cannot swallow its sibling arguments on re-parse: the unparenthesized
/// `Just Left y 1` would re-parse as `Just (Left y 1)`, silently changing the
/// match. A bang marker in front of such a constructor is parenthesized with
/// the constructor application, marker inside the parens (`Just (!(Left y)) 1`),
/// because an unparenthesized `Just !Left y` would attach the bang to the whole
/// `Left` application at re-parse.
fn render_pattern_nested(p: &Pattern, buf: &mut String) {
    if let Pattern::Ctor(_, args) = p.strip_bang() {
        if !args.is_empty() {
            write!(buf, "(").ok();
            render_pattern(p, buf);
            write!(buf, ")").ok();
            return;
        }
    }
    render_pattern(p, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Def;
    use crate::error::Span;

    fn span() -> Span {
        Span::new(0, 0)
    }

    /// A fractional literal for `text`, carrying the exact value its digits
    /// spell and the source spelling the renderer reproduces.
    fn fl(text: &str) -> Expr {
        Expr::FractionalLit(
            Box::new(crate::lexer::decimal_rational(text).unwrap()),
            text.to_string(),
            span(),
        )
    }

    #[test]
    fn render_simple_apply() {
        let prog = Program {
            mode: Mode::Strict,
            mode_explicit: false,
            defs: vec![Def::Eval {
                name: "main".into(),
                params: vec![],
                body: Expr::Apply {
                    name: "sine".into(),
                    args: vec![fl("440.0"), fl("0.5")],
                    span: span(),
                },
                span: span(),
                where_defs: vec![],
                sig: None,
                bang: false,
            }],
        };
        let dsl = render(&prog).unwrap();
        assert_eq!(dsl, "main = sine 440.0 0.5");
    }

    #[test]
    fn render_apply() {
        let prog = Program {
            mode: Mode::Strict,
            mode_explicit: false,
            defs: vec![Def::Eval {
                name: "main".into(),
                params: vec![],
                body: Expr::Apply {
                    name: "lowpass".into(),
                    args: vec![fl("1000.0"), fl("0.7")],
                    span: span(),
                },
                span: span(),
                where_defs: vec![],
                sig: None,
                bang: false,
            }],
        };
        let dsl = render(&prog).unwrap();
        assert_eq!(dsl, "main = lowpass 1000.0 0.7");
    }

    #[test]
    fn render_with_param() {
        let prog = Program {
            mode: Mode::Strict,
            mode_explicit: false,
            defs: vec![Def::Eval {
                name: "main".into(),
                params: vec![crate::ast::Param {
                    name: "gain".into(),
                    span: span(),
                    bang: false,
                }],
                body: Expr::Apply {
                    name: "*".into(),
                    args: vec![fl("2.0"), Expr::Ref("gain".into(), span())],
                    span: span(),
                },
                span: span(),
                where_defs: vec![],
                sig: None,
                bang: false,
            }],
        };
        let dsl = render(&prog).unwrap();
        assert_eq!(dsl, "main gain = 2.0 * gain");
    }

    #[test]
    fn render_lambda() {
        let prog = Program {
            mode: Mode::Strict,
            mode_explicit: false,
            defs: vec![Def::Eval {
                name: "double".into(),
                params: vec![],
                body: Expr::Lambda {
                    params: vec![crate::ast::Param {
                        name: "x".into(),
                        span: span(),
                        bang: false,
                    }],
                    body: Box::new(Expr::Apply {
                        name: "*".into(),
                        args: vec![Expr::Ref("x".into(), span()), fl("2.0")],
                        span: span(),
                    }),
                    span: span(),
                },
                where_defs: vec![],
                span: span(),
                sig: None,
                bang: false,
            }],
        };
        let dsl = render(&prog).unwrap();
        assert_eq!(dsl, "double = \\x -> x * 2.0");
    }

    /// Render `main` from `src`, re-parse the rendered text, render again, and
    /// assert the two renderings are identical. Comparing render→parse→render
    /// strings sidesteps span differences that a direct AST comparison would
    /// flag after re-parsing.
    fn roundtrip_main(src: &str) -> String {
        let tokens = crate::lexer::tokenize(src).unwrap();
        let program = crate::parser::parse(&tokens, src.as_bytes()).unwrap();
        let main_program = Program {
            mode: Mode::Strict,
            mode_explicit: false,
            defs: vec![program.main_def().unwrap().clone()],
        };
        let first = render(&main_program).unwrap();
        let tokens2 = crate::lexer::tokenize(&first).unwrap();
        let reparsed = crate::parser::parse(&tokens2, first.as_bytes()).unwrap();
        let second = render(&reparsed).unwrap();
        assert_eq!(
            first, second,
            "render → parse → render is not idempotent for: {src}\nfirst: {first}\nsecond: {second}"
        );
        first
    }

    #[test]
    fn render_roundtrip_if() {
        // A condition renders with the CANONICAL constructor spelling: `true`
        // is an alias of `True` (`data Bool = False | True`), and the renderer
        // quotes the constructor it resolved to.
        let dsl = roundtrip_main("main = if true then 1.0 else 2.0;");
        assert_eq!(dsl, "main = if True then 1.0 else 2.0");
    }

    #[test]
    fn render_operator_definition_head_is_parenthesized() {
        // `(++) a b = …` — an operator definition head must keep its
        // parentheses, or the rendered source re-parses as an infix expression
        // (or not at all). The `++` prelude definition is the live case.
        let dsl = render_roundtrip_program("(++) a b = append_list a b;");
        assert!(dsl.starts_with("(++) "), "head lost its parentheses: {dsl}");

        // An operator APPLICATION renders back in INFIX position: `a ++ b` and
        // `(++) a b` are the same AST (`Expr::Apply { name: "++" }`), and the
        // infix spelling is what a reader wrote — a bare `++ a b` would not
        // re-parse at all.
        let infix = render_roundtrip_program("xs = [1.0] ++ [2.0];");
        assert_eq!(
            infix, "xs = [1.0] ++ [2.0]",
            "an operator application must render infix: {infix}"
        );
        // The prefix spelling produces the same AST and therefore the same
        // rendering — the two spellings are one binding (Haskell 2010 §3.4).
        let prefix = render_roundtrip_program("xs = (++) [1.0] [2.0];");
        assert_eq!(prefix, "xs = [1.0] ++ [2.0]", "prefix spelling: {prefix}");
    }

    /// Render the WHOLE program (data/type declarations included), re-parse the
    /// rendered text, render again, and assert idempotence plus that the strict
    /// flag survives.
    fn render_roundtrip_program(src: &str) -> String {
        let tokens = crate::lexer::tokenize(src).unwrap();
        let program = crate::parser::parse(&tokens, src.as_bytes()).unwrap();
        let first = render(&program).unwrap();
        let tokens2 = crate::lexer::tokenize(&first).unwrap();
        let reparsed = crate::parser::parse(&tokens2, first.as_bytes()).unwrap();
        let second = render(&reparsed).unwrap();
        assert_eq!(
            first, second,
            "render → parse → render is not idempotent for: {src}\nfirst: {first}\nsecond: {second}"
        );
        first
    }

    #[test]
    fn render_roundtrip_strict_product_data() {
        let dsl = render_roundtrip_program(
            "data P = { x: !Double, y: Double }; main = P { x: 1.0, y: 2.0 }",
        );
        assert!(
            dsl.contains("!Double"),
            "strict field flag was lost in render: {dsl}"
        );
    }

    #[test]
    fn render_roundtrip_strict_sum_data() {
        let dsl = render_roundtrip_program("data T = C !Int | D Double; main = C 1");
        assert!(
            dsl.contains("!Int"),
            "strict payload flag was lost in render: {dsl}"
        );
    }

    #[test]
    fn render_def_with_signature() {
        // `Def::Eval` with a `sig` renders the annotation between the head and
        // the `=`.
        let prog = Program {
            mode: Mode::Strict,
            mode_explicit: false,
            defs: vec![Def::Eval {
                name: "main".into(),
                params: vec![],
                body: fl("1.0"),
                where_defs: vec![],
                sig: Some(TypeExpr::TName("Double".into())),
                bang: false,
                span: span(),
            }],
        };
        assert_eq!(render(&prog).unwrap(), "main :: Double = 1.0");
    }

    #[test]
    fn type_key_spelling_matches_value_type_key() {
        // The instance registry spells an instance head with `render_type_key`
        // and dispatch spells the selector's value type with `value_type_key`.
        // The two must agree byte for byte, or an instance that registers fine
        // silently stops resolving.
        let env = crate::types::ty::TypeEnv::with_builtins();
        let nested_in_arg = TypeExpr::TApp(
            "Pair".into(),
            vec![
                TypeExpr::TApp("List".into(), vec![TypeExpr::TName("Int".into())]),
                TypeExpr::TName("Int".into()),
            ],
        );
        let nested_in_arg2 = TypeExpr::TApp(
            "Pair".into(),
            vec![
                TypeExpr::TName("Int".into()),
                TypeExpr::TApp(
                    "Pair".into(),
                    vec![TypeExpr::TName("Int".into()), TypeExpr::TName("Int".into())],
                ),
            ],
        );
        for te in [&nested_in_arg, &nested_in_arg2] {
            let vty = crate::types::infer::data_field_vty(&env, &[], te);
            assert_eq!(
                render_type_key(te),
                crate::types::ty::value_type_key(&env, &vty).expect("concrete type spells a key"),
                "registry and dispatch must spell {te:?} identically"
            );
        }
        assert_eq!(render_type_key(&nested_in_arg), "Pair (List Int) Int");
        assert_eq!(render_type_key(&nested_in_arg2), "Pair Int (Pair Int Int)");
    }

    #[test]
    fn render_roundtrip_def_signature() {
        let dsl = roundtrip_main("main :: Double = 1.0;");
        assert_eq!(dsl, "main :: Double = 1.0");
    }

    #[test]
    fn render_roundtrip_def_signature_forall() {
        // A `forall` signature survives render → parse → render (the `forall`
        // body is parenthesized in argument position so it cannot absorb the
        // arrow chain on re-parse).
        let dsl = roundtrip_main("main :: (forall b. b -> b) = \\x -> x;");
        assert_eq!(dsl, "main :: forall b. (b -> b) = \\x -> x");
    }

    #[test]
    fn render_roundtrip_def_signature_with_where() {
        let dsl = roundtrip_main("main :: Double = g where { g = 1.0; };");
        assert_eq!(dsl, "main :: Double = g where {\n    g = 1.0;\n}");
    }

    #[test]
    fn render_roundtrip_constrained_def_signature() {
        // A single-constraint context renders unparenthesized (`C v => T`) and
        // survives render → parse → render. The built-in list/tuple
        // constructors render in their SOURCE spelling (`[e]`, `(i, i)`), which
        // re-parses to the same `TApp("List"/"Pair")` tree.
        let dsl = roundtrip_main("main :: Ix i => Pair i i -> List e -> Array i e = g;");
        assert_eq!(dsl, "main :: Ix i => ((i, i) -> [e] -> Array i e) = g");
    }

    #[test]
    fn render_roundtrip_multi_constraint_def_signature() {
        // Two or more constraints render as a parenthesized, comma-separated
        // list — the only Haskell 2010 form that re-parses.
        let dsl = roundtrip_main("main :: (Ix i, Ix j) => Double = 1.0;");
        assert_eq!(dsl, "main :: (Ix i, Ix j) => Double = 1.0");
    }

    #[test]
    fn render_roundtrip_foreign_decl() {
        // A `foreign` declaration renders a trailing `;` (render.rs
        // Def::Foreign arm) that the top-level parse loop — not the def parser
        // — consumes. Full-program round-trip must stay idempotent.
        let src = r"foreign biquad : FixedBuffer Double -> Double -> Double -> Double -> FixedBuffer Double;
main = 1.0;";
        let tokens = crate::lexer::tokenize(src).unwrap();
        let program = crate::parser::parse(&tokens, src.as_bytes()).unwrap();
        let first = render(&program).unwrap();
        let tokens2 = crate::lexer::tokenize(&first).unwrap();
        let reparsed = crate::parser::parse(&tokens2, first.as_bytes()).unwrap();
        let second = render(&reparsed).unwrap();
        assert_eq!(
            first, second,
            "render → parse → render is not idempotent for: {src}\nfirst: {first}\nsecond: {second}"
        );
    }

    #[test]
    fn render_roundtrip_match_bare_ctor_arms() {
        let dsl = roundtrip_main(
            r"data Shape = Circle Double | Rect Double Double; main = case s of { Circle r -> r; Rect w h -> w; };",
        );
        assert_eq!(dsl, r"main = case s of { Circle r -> r; Rect w h -> w; }");
    }

    #[test]
    fn render_roundtrip_match_guarded_arm() {
        let dsl = roundtrip_main(r"main = case n of { n | n > 0 -> 1.0; _ -> 0.0; };");
        assert_eq!(dsl, r"main = case n of { n | n > 0 -> 1.0; _ -> 0.0; }");
    }

    #[test]
    fn render_roundtrip_match_nested_pattern_literal_wildcard() {
        let dsl = roundtrip_main(r"main = case x of { Just (Left y) -> y; 0 -> 0.0; _ -> 1.0; };");
        assert_eq!(
            dsl,
            r"main = case x of { Just (Left y) -> y; 0 -> 0.0; _ -> 1.0; }"
        );
    }

    #[test]
    fn render_roundtrip_match_multi_alternative_guarded_arm() {
        // Guarded first alternative followed by a second guarded alternative:
        // both guards must be rendered (regression for the first alternative
        // being dropped).
        let dsl =
            roundtrip_main(r"main = case n of { n | n > 0 -> 1.0 | n == 1.0 -> 2.0; _ -> 0.0; };");
        assert_eq!(
            dsl,
            r"main = case n of { n | n > 0 -> 1.0 | n == 1.0 -> 2.0; _ -> 0.0; }"
        );
    }

    #[test]
    fn render_roundtrip_match_bare_then_guarded_alternative() {
        // Bare first alternative followed by a guarded alternative.
        let dsl = roundtrip_main(r"main = case n of { n -> 1.0 | n > 0 -> 2.0; _ -> 0.0; };");
        assert_eq!(
            dsl,
            r"main = case n of { n -> 1.0 | n > 0 -> 2.0; _ -> 0.0; }"
        );
    }

    #[test]
    fn render_roundtrip_match_written_true_guard() {
        // A written `true` guard in a non-first position must not be conflated
        // with a bare arm's UNGUARDED alternative (regression: it previously
        // rendered as `n -> 1.0 -> 2.0`). It renders as the constructor it
        // resolved to, `True`.
        let dsl = roundtrip_main(r"main = case n of { n -> 1.0 | true -> 2.0; _ -> 0.0; };");
        assert_eq!(
            dsl,
            r"main = case n of { n -> 1.0 | True -> 2.0; _ -> 0.0; }"
        );
    }

    #[test]
    fn render_roundtrip_match_nested_ctor_with_sibling_arg() {
        // A nested ctor with a sibling argument must be parenthesized so the
        // inner ctor cannot swallow the sibling on re-parse (regression: it
        // previously rendered as `Just Left y 1` and re-parsed as
        // `Just (Left y 1)`).
        let dsl = roundtrip_main(r"main = case x of { Just (Left y) 1 -> y; _ -> 0.0; };");
        assert_eq!(dsl, r"main = case x of { Just (Left y) 1 -> y; _ -> 0.0; }");
    }

    #[test]
    fn render_roundtrip_bang_forms() {
        // Bang markers survive render → parse → render: on parameters, on
        // lambdas, on `let` bindings, and on `case` patterns (including a bang
        // in front of a nested constructor with a sibling argument).
        let dsl = roundtrip_main("main !x = x + 1.0;");
        assert_eq!(dsl, "main !x = x + 1.0");
        let dsl = roundtrip_main(r"main = \ !x -> x + 1.0;");
        assert_eq!(dsl, r"main = \!x -> x + 1.0");
        let dsl = roundtrip_main("main = let !x = 1.0 in x;");
        assert_eq!(dsl, "main = let !x = 1.0 in x");
        let dsl = roundtrip_main(r"main = case Just 1.0 of { Just !x -> x; Nothing -> 0.0; };");
        assert_eq!(
            dsl,
            r"main = case Just 1.0 of { Just !x -> x; Nothing -> 0.0; }"
        );
        let dsl = roundtrip_main(r"main = case x of { Just !(Left y) 1 -> y; _ -> 0.0; };");
        assert_eq!(
            dsl,
            r"main = case x of { Just (!(Left y)) 1 -> y; _ -> 0.0; }"
        );
    }

    #[test]
    fn render_strict_default_emits_no_pragma() {
        let src = "main = 2.0 * 0.5";
        let tokens = crate::lexer::tokenize(src).unwrap();
        let program = crate::parser::parse(&tokens, src.as_bytes()).unwrap();
        let dsl = render(&program).unwrap();
        assert_eq!(dsl, "main = 2.0 * 0.5");
    }

    #[test]
    fn render_roundtrip_complete_pragma() {
        // A `{-# MODE complete #-}` pragma renders first and survives a
        // parse → render → parse → render round-trip.
        let src = "{-# MODE complete #-}\nmain = 1.0";
        let tokens = crate::lexer::tokenize(src).unwrap();
        let program = crate::parser::parse(&tokens, src.as_bytes()).unwrap();
        let first = render(&program).unwrap();
        assert_eq!(first, "{-# MODE complete #-}\nmain = 1.0");
        let tokens2 = crate::lexer::tokenize(first.as_str()).unwrap();
        let reparsed = crate::parser::parse(&tokens2, first.as_bytes()).unwrap();
        assert_eq!(render(&reparsed).unwrap(), first);
    }
}
