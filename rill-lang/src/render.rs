//! AST → DSL source renderer.
//!
//! Converts a parsed [`crate::ast::Program`] back into a rill-lang source
//! string. The renderer is used for round-trip tests that verify
//! isomorphism between JSON and DSL representations.

use crate::ast::{ArithOp, Def, Expr, Program, TypeExpr};
use std::fmt::Write;

/// Render a program as a rill-lang source string.
pub fn render(program: &Program) -> String {
    let mut buf = String::new();
    for (i, def) in program.defs.iter().enumerate() {
        if i > 0 {
            buf.push('\n');
        }
        render_def(def, &mut buf, 0);
    }
    buf
}

fn render_def(def: &Def, buf: &mut String, indent: usize) {
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
            render_expr(body, buf, 3);
            if !where_defs.is_empty() {
                writeln!(buf, " where {{").ok();
                for wd in where_defs {
                    render_def(wd, buf, indent + 4);
                    writeln!(buf, ";").ok();
                }
                write!(buf, "{pad}}}").ok();
            }
        }
        Def::Local { name, body, .. } => {
            write!(buf, "{pad}{name} = ").ok();
            render_expr(body, buf, 0);
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
        }
        Def::TypeAlias { name, target, .. } => {
            write!(buf, "{pad}type {name} = {target}").ok();
        }
        Def::Newtype { name, target, .. } => {
            write!(buf, "{pad}newtype {name} = {target}").ok();
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
        }
        Def::Instance {
            class,
            ty,
            method_bodies,
            ..
        } => {
            write!(buf, "{pad}instance {class} {ty} where {{ ").ok();
            for (mname, param, body) in method_bodies {
                write!(buf, "{mname}").ok();
                if let Some(p) = param {
                    write!(buf, " {}", p.name).ok();
                }
                write!(buf, " = ").ok();
                render_expr(body, buf, 0);
                write!(buf, "; ").ok();
            }
            write!(buf, "}}").ok();
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

fn render_expr(expr: &Expr, buf: &mut String, outer_bp: u8) {
    match expr {
        Expr::Int(v, _) => {
            write!(buf, "{v}").ok();
        }
        Expr::Float(v, _) => {
            if *v == *v as i64 as f64 && v.is_finite() {
                write!(buf, "{:.1}", v).ok();
            } else {
                write!(buf, "{}", v).ok();
            }
        }
        Expr::Imag(v, _) => {
            write!(buf, "{v}i").ok();
        }
        Expr::Wire(_) => {
            write!(buf, "_").ok();
        }
        Expr::Cut(_) => {
            write!(buf, "!").ok();
        }
        Expr::Str(s, _) => {
            write!(buf, "\"{s}\"").ok();
        }
        Expr::Ref(name, _) => {
            write!(buf, "{name}").ok();
        }
        Expr::Apply { name, args, .. } => {
            write!(buf, "{name}").ok();
            for a in args {
                write!(buf, " ").ok();
                render_expr(a, buf, 20); // application args are tight
            }
        }
        Expr::Neg(inner, _) => {
            write!(buf, "-").ok();
            render_expr(inner, buf, 15);
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
                    render_def(d, buf, 0);
                    write!(buf, "; ").ok();
                }
                write!(buf, "}} in ").ok();
            } else {
                for d in defs {
                    render_def(d, buf, 0);
                }
                write!(buf, " in ").ok();
            }
            render_expr(body, buf, 0);
        }
        Expr::Record(fields, _) => {
            write!(buf, "{{ ").ok();
            for (i, (name, val)) in fields.iter().enumerate() {
                if i > 0 {
                    write!(buf, ", ").ok();
                }
                write!(buf, "{name}: ").ok();
                render_expr(val, buf, 0);
            }
            write!(buf, " }}").ok();
        }
        Expr::ActorParam { name, default, .. } => {
            write!(buf, "?{name}").ok();
            if let Some(d) = default {
                write!(buf, "=").ok();
                render_expr(d, buf, 0);
            }
        }
        Expr::FieldProject { record, field, .. } => {
            render_expr(record, buf, 20);
            write!(buf, ".{field}").ok();
        }
        Expr::FieldUpdate {
            record,
            field,
            value,
            ..
        } => {
            render_expr(record, buf, 20);
            write!(buf, ".{field} := ").ok();
            render_expr(value, buf, 0);
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            write!(buf, "match ").ok();
            render_expr(scrutinee, buf, 0);
            write!(buf, " of {{ ").ok();
            for (ctor, params, body) in arms {
                write!(buf, "{ctor}").ok();
                for p in params {
                    write!(buf, " {}", p.name).ok();
                }
                write!(buf, " => ").ok();
                render_expr(body, buf, 0);
                write!(buf, "; ").ok();
            }
            write!(buf, "}}").ok();
        }
        Expr::Lambda { params, body, .. } => {
            write!(buf, "fn").ok();
            for p in params {
                write!(buf, " {}", p.name).ok();
            }
            write!(buf, " -> ").ok();
            render_expr(body, buf, 0);
        }
    }
}

/// Render a binary node with parenthesization based on its binding power.
fn render_bin(
    lhs: &Expr,
    rhs: &Expr,
    buf: &mut String,
    outer_bp: u8,
    (prec, l_bp, r_bp, sym): (u8, u8, u8, &'static str),
) {
    if outer_bp > prec {
        write!(buf, "(").ok();
    }
    render_expr(lhs, buf, l_bp);
    write!(buf, " {sym} ").ok();
    render_expr(rhs, buf, r_bp);
    if outer_bp > prec {
        write!(buf, ")").ok();
    }
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
        let dsl = render(&prog);
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
        let dsl = render(&prog);
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
        let dsl = render(&prog);
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
        let dsl = render(&prog);
        assert_eq!(dsl, "double = fn x -> x * 2.0");
    }
}
