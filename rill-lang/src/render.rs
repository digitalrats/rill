//! AST → DSL source renderer.
//!
//! Converts a parsed [`crate::ast::Program`] back into a rill-lang source
//! string. The renderer is used for round-trip tests that verify
//! isomorphism between JSON and DSL representations.

use crate::ast::{BinOp, Def, Expr, Program};
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
        Expr::Bin { op, lhs, rhs, .. } => {
            let (prec, l_bp, r_bp, sym) = bin_info(op);
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
    }
}

fn bin_info(op: &BinOp) -> (u8, u8, u8, &'static str) {
    match op {
        BinOp::Seq => (3, 3, 4, ":"),
        BinOp::Split => (5, 5, 6, "<:"),
        BinOp::Merge => (7, 7, 8, ":>"),
        BinOp::Par => (9, 9, 10, ","),
        BinOp::Add => (11, 11, 12, "+"),
        BinOp::Sub => (11, 11, 12, "-"),
        BinOp::Mul => (13, 13, 14, "*"),
        BinOp::Div => (13, 13, 14, "/"),
        BinOp::Rem => (13, 13, 14, "%"),
        BinOp::Delay => (15, 15, 16, "@"),
        BinOp::Feedback => (1, 1, 2, "~"),
        BinOp::FeedbackTap => (1, 1, 2, "<~"),
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
                body: Expr::Bin {
                    op: BinOp::Seq,
                    lhs: Box::new(Expr::Wire(span())),
                    rhs: Box::new(Expr::Apply {
                        name: "lowpass".into(),
                        args: vec![Expr::Float(1000.0, span()), Expr::Float(0.7, span())],
                        span: span(),
                    }),
                    span: span(),
                },
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
                body: Expr::Bin {
                    op: BinOp::Mul,
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
}
