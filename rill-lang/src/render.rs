//! AST → DSL source renderer.
//!
//! Converts a parsed [`crate::ast::Program`] back into a rill-lang source
//! string. The renderer is used for round-trip tests that verify
//! isomorphism between JSON and DSL representations. Rendering is total:
//! `if` and `match` render every arm, including guarded alternatives.

use crate::ast::{ArithOp, CmpOp, Def, Expr, LogicOp, Pattern, Program, TypeExpr};
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
            write!(buf, "{pad}{}{name}", if def.is_bang() { "!" } else { "" }).ok();
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
            for (i, (fname, tname)) in fields.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ").ok();
                }
                write!(buf, "{fname}: ").ok();
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
                for t in payload {
                    write!(buf, " ").ok();
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
            write!(buf, "{pad}type {name} = {target}").ok();
            Ok(())
        }
        Def::Newtype { name, target, .. } => {
            write!(buf, "{pad}newtype {name} = {target}").ok();
            Ok(())
        }
        Def::Typeclass {
            name, var, methods, ..
        } => {
            write!(buf, "{pad}typeclass {name} {var} where {{ ").ok();
            for (mname, sig) in methods {
                write!(buf, "{mname}: ").ok();
                render_type_expr(sig, buf);
                write!(buf, "; ").ok();
            }
            write!(buf, "}}").ok();
            Ok(())
        }
        Def::Instance {
            class,
            ty,
            method_bodies,
            ..
        } => {
            write!(buf, "{pad}instance {class} {ty} where {{ ").ok();
            for (mname, params, body) in method_bodies {
                write!(buf, "{mname}").ok();
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
    }
}

/// Render a type expression in argument position: a `forall` binder must be
/// parenthesized so it does not absorb a following arrow chain on re-parse
/// (`(forall b. a) -> a` must not re-read as `forall b. (a -> a)`).
fn render_type_arg(t: &TypeExpr, buf: &mut String) {
    if matches!(t, TypeExpr::TForall(..)) {
        write!(buf, "(").ok();
        render_type_expr(t, buf);
        write!(buf, ")").ok();
    } else {
        render_type_expr(t, buf);
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
        Expr::Float(v, _) => {
            if *v == *v as i64 as f64 && v.is_finite() {
                write!(buf, "{:.1}", v).ok();
            } else {
                write!(buf, "{}", v).ok();
            }
            Ok(())
        }
        Expr::Str(s, _) => {
            write!(buf, "\"{s}\"").ok();
            Ok(())
        }
        Expr::Ref(name, _) => {
            write!(buf, "{name}").ok();
            Ok(())
        }
        Expr::Apply { name, args, .. } => {
            write!(buf, "{name}").ok();
            for a in args {
                write!(buf, " ").ok();
                render_expr(a, buf, 20)?; // application args are tight
            }
            Ok(())
        }
        Expr::ApplyExpr { callee, args, .. } => {
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
        Expr::Arith { op, lhs, rhs, .. } => render_bin(lhs, rhs, buf, outer_bp, arith_info(op)),
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
                for (i, (g, body)) in arm.guards.iter().enumerate() {
                    // A bare arm's FIRST alternative carries the synthetic
                    // `true` guard and renders without a `| guard` prefix;
                    // every other alternative — including a written `true`
                    // guard — renders its guard explicitly.
                    if i == 0 && matches!(g, Expr::Bool(true, _)) {
                        write!(buf, " -> ").ok();
                    } else {
                        write!(buf, " | ").ok();
                        render_expr(g, buf, 0)?;
                        write!(buf, " -> ").ok();
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
        Expr::Bool(v, _) => {
            write!(buf, "{}", if *v { "true" } else { "false" }).ok();
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
        Expr::Cmp { op, lhs, rhs, .. } => render_bin(lhs, rhs, buf, outer_bp, cmp_info(op)),
        Expr::Logic { op, lhs, rhs, .. } => render_bin(lhs, rhs, buf, outer_bp, logic_info(op)),
    }
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

fn arith_info(op: &ArithOp) -> (u8, u8, u8, &'static str) {
    match op {
        ArithOp::Add => (11, 11, 12, "+"),
        ArithOp::Sub => (11, 11, 12, "-"),
        ArithOp::Mul => (13, 13, 14, "*"),
        ArithOp::Div => (13, 13, 14, "/"),
        ArithOp::Rem => (13, 13, 14, "%"),
    }
}

fn cmp_info(op: &CmpOp) -> (u8, u8, u8, &'static str) {
    match op {
        CmpOp::Eq => (3, 3, 4, "=="),
        CmpOp::Ne => (3, 3, 4, "!="),
        CmpOp::Lt => (3, 3, 4, "<"),
        CmpOp::Gt => (3, 3, 4, ">"),
        CmpOp::Le => (3, 3, 4, "<="),
        CmpOp::Ge => (3, 3, 4, ">="),
    }
}

fn logic_info(op: &LogicOp) -> (u8, u8, u8, &'static str) {
    match op {
        LogicOp::And => (1, 1, 2, "&&"),
        LogicOp::Or => (1, 1, 2, "||"),
    }
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
        Pattern::LitFloat(v) => {
            write!(buf, "{v}").ok();
        }
        Pattern::LitBool(v) => {
            write!(buf, "{v}").ok();
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
/// it (`Just (!Left y 1)`), since `Just !Left y 1` would attach the bang to
/// the whole constructor application.
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
                    args: vec![Expr::Float(440.0, span()), Expr::Float(0.5, span())],
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
                    args: vec![Expr::Float(1000.0, span()), Expr::Float(0.7, span())],
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
                body: Expr::Arith {
                    op: ArithOp::Mul,
                    lhs: Box::new(Expr::Float(2.0, span())),
                    rhs: Box::new(Expr::Ref("gain".into(), span())),
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
                    body: Box::new(Expr::Arith {
                        op: ArithOp::Mul,
                        lhs: Box::new(Expr::Ref("x".into(), span())),
                        rhs: Box::new(Expr::Float(2.0, span())),
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
        let dsl = roundtrip_main("main = if true then 1.0 else 2.0;");
        assert_eq!(dsl, "main = if true then 1.0 else 2.0");
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
                body: Expr::Float(1.0, span()),
                where_defs: vec![],
                sig: Some(TypeExpr::TName("Float".into())),
                bang: false,
                span: span(),
            }],
        };
        assert_eq!(render(&prog).unwrap(), "main :: Float = 1.0");
    }

    #[test]
    fn render_roundtrip_def_signature() {
        let dsl = roundtrip_main("main :: Float = 1.0;");
        assert_eq!(dsl, "main :: Float = 1.0");
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
        let dsl = roundtrip_main("main :: Float = g where { g = 1.0; };");
        assert_eq!(dsl, "main :: Float = g where {\n    g = 1.0;\n}");
    }

    #[test]
    fn render_roundtrip_foreign_decl() {
        // A `foreign` declaration renders a trailing `;` (render.rs
        // Def::Foreign arm) that the top-level parse loop — not the def parser
        // — consumes. Full-program round-trip must stay idempotent.
        let src = r"foreign biquad : FixedBuffer f32 -> Float -> Float -> Float -> FixedBuffer f32;
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
            r"data Shape = Circle Float | Rect Float Float; main = case s of { Circle r -> r; Rect w h -> w; };",
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
        // with the bare arm's synthetic first-alternative `true` sentinel
        // (regression: it previously rendered as `n -> 1.0 -> 2.0`).
        let dsl = roundtrip_main(r"main = case n of { n -> 1.0 | true -> 2.0; _ -> 0.0; };");
        assert_eq!(
            dsl,
            r"main = case n of { n -> 1.0 | true -> 2.0; _ -> 0.0; }"
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
