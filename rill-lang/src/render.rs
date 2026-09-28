//! AST → DSL source renderer.
//!
//! Converts a parsed [`crate::ast::Program`] back into a rill-lang source
//! string. The renderer is used for round-trip tests that verify
//! isomorphism between JSON and DSL representations. Rendering is total:
//! `if` and `match` render every arm, including guarded alternatives.

use crate::ast::{ArithOp, CmpOp, Def, Expr, LogicOp, Pattern, Program, TypeExpr};
use crate::error::CompileError;
use std::fmt::Write;

/// Render a program as a rill-lang source string.
pub fn render(program: &Program) -> Result<String, CompileError> {
    let mut buf = String::new();
    for (i, def) in program.defs.iter().enumerate() {
        if i > 0 {
            buf.push('\n');
        }
        render_def(def, &mut buf, 0)?;
    }
    Ok(buf)
}

fn render_def(def: &Def, buf: &mut String, indent: usize) -> Result<(), CompileError> {
    let pad = " ".repeat(indent);
    match def {
        Def::Anchor {
            name,
            params,
            body,
            where_defs,
            ..
        } => {
            write!(buf, "{pad}{name}").ok();
            for p in params {
                write!(buf, " {}", p.name).ok();
            }
            write!(buf, " = ").ok();
            render_expr(body, buf, 3)?;
            if !where_defs.is_empty() {
                writeln!(buf, " where {{").ok();
                for wd in where_defs {
                    render_def(wd, buf, indent + 4)?;
                    writeln!(buf, ";").ok();
                }
                write!(buf, "{pad}}}").ok();
            }
            Ok(())
        }
        Def::Local { name, body, .. } => {
            write!(buf, "{pad}{name} = ").ok();
            render_expr(body, buf, 0)?;
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
    }
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
                render_type_expr(a, buf);
            }
        }
        TypeExpr::TFunc(args, ret) => {
            write!(buf, "(").ok();
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    write!(buf, " -> ").ok();
                }
                render_type_expr(a, buf);
            }
            write!(buf, " -> ").ok();
            render_type_expr(ret, buf);
            write!(buf, ")").ok();
        }
        TypeExpr::TCap(n) => {
            write!(buf, "{n}").ok();
        }
    }
}

fn render_expr(expr: &Expr, buf: &mut String, outer_bp: u8) -> Result<(), CompileError> {
    match expr {
        Expr::Int(v, _) => {
            write!(buf, "{v}").ok();
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
        Expr::Imag(v, _) => {
            write!(buf, "{v}i").ok();
            Ok(())
        }
        Expr::Wire(_) => {
            write!(buf, "_").ok();
            Ok(())
        }
        Expr::Cut(_) => {
            write!(buf, "!").ok();
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
        Expr::Neg(inner, _) => {
            write!(buf, "-").ok();
            render_expr(inner, buf, 15)?;
            Ok(())
        }
        Expr::Seq(lhs, rhs, _) => render_bin(lhs, rhs, buf, outer_bp, (3, 3, 4, ":")),
        Expr::Split(lhs, rhs, _) => render_bin(lhs, rhs, buf, outer_bp, (5, 5, 6, "<:")),
        Expr::Merge(lhs, rhs, _) => render_bin(lhs, rhs, buf, outer_bp, (7, 7, 8, ":>")),
        Expr::Par(lhs, rhs, _) => render_bin(lhs, rhs, buf, outer_bp, (9, 9, 10, ",")),
        Expr::Arith { op, lhs, rhs, .. } => render_bin(lhs, rhs, buf, outer_bp, arith_info(op)),
        Expr::Delay(lhs, rhs, _) => render_bin(lhs, rhs, buf, outer_bp, (15, 15, 16, "@")),
        Expr::Loop(lhs, rhs, _) => render_bin(lhs, rhs, buf, outer_bp, (1, 1, 2, "~")),
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
            write!(buf, "match ").ok();
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
                        write!(buf, " => ").ok();
                    } else {
                        write!(buf, " | ").ok();
                        render_expr(g, buf, 0)?;
                        write!(buf, " => ").ok();
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
            write!(buf, "fn").ok();
            for p in params {
                write!(buf, " {}", p.name).ok();
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
    }
}

/// Render a subpattern in constructor-argument position. A nested `Ctor` with
/// arguments is parenthesized (`Just (Left y) 1`) so the inner constructor
/// cannot swallow its sibling arguments on re-parse: the unparenthesized
/// `Just Left y 1` would re-parse as `Just (Left y 1)`, silently changing the
/// match.
fn render_pattern_nested(p: &Pattern, buf: &mut String) {
    if let Pattern::Ctor(_, args) = p {
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
            defs: vec![Def::Anchor {
                name: "main".into(),
                params: vec![],
                body: Expr::Apply {
                    name: "sine".into(),
                    args: vec![Expr::Float(440.0, span()), Expr::Float(0.5, span())],
                    span: span(),
                },
                span: span(),
                where_defs: vec![],
            }],
        };
        let dsl = render(&prog).unwrap();
        assert_eq!(dsl, "main = sine 440.0 0.5");
    }

    #[test]
    fn render_pipeline() {
        let prog = Program {
            defs: vec![Def::Anchor {
                name: "main".into(),
                params: vec![],
                body: Expr::Seq(
                    Box::new(Expr::Wire(span())),
                    Box::new(Expr::Apply {
                        name: "lowpass".into(),
                        args: vec![Expr::Float(1000.0, span()), Expr::Float(0.7, span())],
                        span: span(),
                    }),
                    span(),
                ),
                span: span(),
                where_defs: vec![],
            }],
        };
        let dsl = render(&prog).unwrap();
        assert_eq!(dsl, "main = _ : lowpass 1000.0 0.7");
    }

    #[test]
    fn render_with_param() {
        let prog = Program {
            defs: vec![Def::Anchor {
                name: "main".into(),
                params: vec![crate::ast::Param {
                    name: "gain".into(),
                    span: span(),
                }],
                body: Expr::Arith {
                    op: ArithOp::Mul,
                    lhs: Box::new(Expr::Wire(span())),
                    rhs: Box::new(Expr::Ref("gain".into(), span())),
                    span: span(),
                },
                span: span(),
                where_defs: vec![],
            }],
        };
        let dsl = render(&prog).unwrap();
        assert_eq!(dsl, "main gain = _ * gain");
    }

    #[test]
    fn render_lambda() {
        let prog = Program {
            defs: vec![Def::Local {
                name: "double".into(),
                body: Expr::Lambda {
                    params: vec![crate::ast::Param {
                        name: "x".into(),
                        span: span(),
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
            }],
        };
        let dsl = render(&prog).unwrap();
        assert_eq!(dsl, "double = fn x -> x * 2.0");
    }

    /// Render `main` from `src`, re-parse the rendered text, render again, and
    /// assert the two renderings are identical. Comparing render→parse→render
    /// strings sidesteps span differences that a direct AST comparison would
    /// flag after re-parsing.
    fn roundtrip_main(src: &str) -> String {
        let tokens = crate::lexer::tokenize(src).unwrap();
        let program = crate::parser::parse(&tokens, src.as_bytes()).unwrap();
        let main_program = Program {
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
    fn render_roundtrip_match_bare_ctor_arms() {
        let dsl = roundtrip_main(
            "data Shape = Circle Float | Rect Float Float; \
             main = match s of { Circle r => r; Rect w h => w; };",
        );
        assert_eq!(dsl, "main = match s of { Circle r => r; Rect w h => w; }");
    }

    #[test]
    fn render_roundtrip_match_guarded_arm() {
        let dsl = roundtrip_main("main = match n of { n | n > 0 => 1.0; _ => 0.0; };");
        assert_eq!(dsl, "main = match n of { n | n > 0 => 1.0; _ => 0.0; }");
    }

    #[test]
    fn render_roundtrip_match_nested_pattern_literal_wildcard() {
        let dsl = roundtrip_main("main = match x of { Just (Left y) => y; 0 => 0.0; _ => 1.0; };");
        assert_eq!(
            dsl,
            "main = match x of { Just (Left y) => y; 0 => 0.0; _ => 1.0; }"
        );
    }

    #[test]
    fn render_roundtrip_match_multi_alternative_guarded_arm() {
        // Guarded first alternative followed by a second guarded alternative:
        // both guards must be rendered (regression for the first alternative
        // being dropped).
        let dsl =
            roundtrip_main("main = match n of { n | n > 0 => 1.0 | n == 1.0 => 2.0; _ => 0.0; };");
        assert_eq!(
            dsl,
            "main = match n of { n | n > 0 => 1.0 | n == 1.0 => 2.0; _ => 0.0; }"
        );
    }

    #[test]
    fn render_roundtrip_match_bare_then_guarded_alternative() {
        // Bare first alternative followed by a guarded alternative.
        let dsl = roundtrip_main("main = match n of { n => 1.0 | n > 0 => 2.0; _ => 0.0; };");
        assert_eq!(
            dsl,
            "main = match n of { n => 1.0 | n > 0 => 2.0; _ => 0.0; }"
        );
    }

    #[test]
    fn render_roundtrip_match_written_true_guard() {
        // A written `true` guard in a non-first position must not be conflated
        // with the bare arm's synthetic first-alternative `true` sentinel
        // (regression: it previously rendered as `n => 1.0 => 2.0`).
        let dsl = roundtrip_main("main = match n of { n => 1.0 | true => 2.0; _ => 0.0; };");
        assert_eq!(
            dsl,
            "main = match n of { n => 1.0 | true => 2.0; _ => 0.0; }"
        );
    }

    #[test]
    fn render_roundtrip_match_nested_ctor_with_sibling_arg() {
        // A nested ctor with a sibling argument must be parenthesized so the
        // inner ctor cannot swallow the sibling on re-parse (regression: it
        // previously rendered as `Just Left y 1` and re-parsed as
        // `Just (Left y 1)`).
        let dsl = roundtrip_main("main = match x of { Just (Left y) 1 => y; _ => 0.0; };");
        assert_eq!(dsl, "main = match x of { Just (Left y) 1 => y; _ => 0.0; }");
    }
}
