//! `Expr::Apply` always carries at least one argument, and `Expr::Ref` is the
//! handle for a bare name.
//!
//! Haskell 2010 has no zero-argument application: `f` and `f ()` differ by an
//! ARGUMENT (and `()` is an ordinary value, the unit constructor). GHC Core draws
//! the same line, measured with `-ddump-ds`:
//!
//! ```haskell
//! g = \y -> y     -- definition: a lambda
//! h = g (IS 1#)   -- application: App(Var g, arg)
//! k = g           -- reference: Var g, with NO App at all
//! ```
//!
//! A CONSTANT is not an application either: `x = 42` is a CAF (`x = I# 42#` in
//! Core, `IS! [42#]` in STG), computed lazily once.
//!
//! So the AST has three distinct shapes and this test pins the line between the
//! first two: a bare name is `Ref`, an application is `Apply { args: 1.. }`.

use rill_lang::ast::{Def, Expr};
use rill_lang::{lexer::tokenize, parser::parse};

fn walk(e: &Expr, bad: &mut Vec<String>) {
    match e {
        Expr::Apply { name, args, .. } => {
            if args.is_empty() {
                bad.push(name.clone());
            }
            for a in args {
                walk(a, bad);
            }
        }
        Expr::Lambda { body, .. } => walk(body, bad),
        Expr::ApplyExpr { callee, args, .. } => {
            walk(callee, bad);
            for a in args {
                walk(a, bad);
            }
        }
        Expr::Let { defs, body, .. } => {
            for d in defs {
                if let Def::Eval { body, .. } = d {
                    walk(body, bad);
                }
            }
            walk(body, bad);
        }
        Expr::ListLit(xs, _) => {
            for x in xs {
                walk(x, bad);
            }
        }
        Expr::MapLit(es, _) => {
            for (_, x) in es {
                walk(x, bad);
            }
        }
        Expr::Record(fs, _) => {
            for (_, x) in fs {
                walk(x, bad);
            }
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            walk(scrutinee, bad);
            for a in arms {
                for (_, g) in &a.guards {
                    walk(g, bad);
                }
            }
        }
        Expr::If {
            cond, then, els, ..
        } => {
            walk(cond, bad);
            walk(then, bad);
            walk(els, bad);
        }
        Expr::Neg(x, _) => walk(x, bad),
        Expr::FieldProject { record, .. } => walk(record, bad),
        Expr::FieldUpdate { record, value, .. } => {
            walk(record, bad);
            walk(value, bad);
        }
        Expr::Annot { expr, .. } => walk(expr, bad),
        _ => {}
    }
}

#[test]
fn a_bare_name_is_a_reference_not_a_zero_argument_application() {
    let toks = tokenize("c = 42.0; main = c;").unwrap();
    let p = parse(&toks, b"c = 42.0; main = c;").unwrap();
    let main = p.defs.iter().find(|d| d.name() == "main").unwrap();
    assert!(
        matches!(main.body(), Expr::Ref(n, _) if n == "c"),
        "`main = c` must be a REFERENCE (a handle), got {:?}",
        main.body()
    );
}

#[test]
fn an_application_always_carries_an_argument() {
    let srcs = [
        "main = gain 1.0 2;",
        "main = sine 440.0 0.5 0.0;",
        "f x = x; main = f 1.0;",
        "c = 42.0; main = c;",
        "main = do { write (Just (show_double 1.0)) stdout; return () };",
        "f = \\x -> x + h; main = f 2.5;",
        "main = case x of { Just v -> v; Nothing -> 0.0 };",
    ];
    let mut bad = Vec::new();
    for src in srcs {
        let toks = tokenize(src).unwrap();
        let p = parse(&toks, src.as_bytes()).unwrap();
        for d in &p.defs {
            if let Def::Eval { body, .. } = d {
                walk(body, &mut bad);
            }
        }
    }
    assert!(
        bad.is_empty(),
        "`Apply` with no arguments is the malformed state the Ref/Apply split exists to \
         prevent; found: {bad:?}"
    );
}
