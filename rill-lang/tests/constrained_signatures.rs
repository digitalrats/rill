//! Haskell 2010 class contexts (`=>`) on `Def::Eval` signatures:
//! `listArray : Ix i => …`, `ixmap : (Ix i, Ix j) => …`.
//!
//! The context is carried on the signature; it is not yet enforced by the
//! typechecker (constrained-def dispatch is a later task), so an unconstrained
//! body must keep compiling.
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::lexer::tokenize;
use rill_lang::parser::parse;
use rill_lang::render::render;

fn program(src: &str) -> rill_lang::ast::Program {
    let toks = tokenize(src).unwrap();
    parse(&toks, src.as_bytes()).unwrap()
}

#[test]
fn single_constraint_signature_roundtrips() {
    let src = r#"listArray lo hi :: Ix i => Pair i i -> List e -> Array i e = hi;
main = 1.0;"#;
    let first = render(&program(src)).unwrap();
    assert!(
        first.contains("listArray :: Ix i => (Pair i i -> List e -> Array i e) = "),
        "a single-constraint context renders bare: {first}"
    );
    // Render is a fixed point: the rendered text re-parses and re-renders
    // identically.
    assert_eq!(render(&program(&first)).unwrap(), first);
}

#[test]
fn multi_constraint_signature_roundtrips() {
    let src = r#"ixmap a b :: (Ix i, Ix j) => Pair i i -> Pair j j -> Double = a;
main = 1.0;"#;
    let first = render(&program(src)).unwrap();
    assert!(
        first.contains("ixmap :: (Ix i, Ix j) => (Pair i i -> Pair j j -> Double) = "),
        "two constraints render parenthesized and comma-separated: {first}"
    );
    assert_eq!(render(&program(&first)).unwrap(), first);
}

#[test]
fn constrained_signature_does_not_break_typechecking() {
    // The constraint is recorded but not enforced yet; the body is an ordinary
    // unconstrained function and must compile and run.
    let src = "f x :: Ix i => Double -> Double = x; main = f 1.0;";
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog =
        eval_ffi(defs, expr, ffi).expect("a constrained signature must not break compilation");
    let v = prog.result().expect("the run must produce a single result");
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(1.0)
    );
}

#[test]
fn class_name_is_not_hardcoded() {
    // Nothing may assume the context names `Ix`: any class name round-trips.
    let src = "same x y :: Eq a => a -> a -> Bool = true;\nmain = 1.0;";
    let first = render(&program(src)).unwrap();
    assert!(
        first.contains("same :: Eq a => (a -> a -> Bool) = "),
        "a non-`Ix` class context renders the same way: {first}"
    );
    assert_eq!(render(&program(&first)).unwrap(), first);
}

#[test]
fn constrained_signature_survives_typing() {
    // Pins the T5 contract: typing must NOT strip the context from
    // `Def::Eval.sig` — constrained-def dispatch reads it from there.
    let src = "f x :: Ix i => Double -> Double = x;\nmain = f 1.0;";
    let prog = program(src);
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode)
        .expect("a constrained signature must typecheck");
    let def = typed
        .program
        .defs
        .iter()
        .find(|d| d.name() == "f")
        .expect("the typed program keeps def `f`");
    match def {
        rill_lang::ast::Def::Eval {
            sig: Some(rill_lang::ast::TypeExpr::TConstrained(constraints, body)),
            ..
        } => {
            assert_eq!(
                constraints,
                &vec![("Ix".to_string(), "i".to_string())],
                "typing must preserve the constraint list verbatim"
            );
            assert_eq!(body.arg_count(), 1, "the body is still `Double -> Double`");
        }
        other => panic!("the typed program lost the constrained signature: {other:?}"),
    }
}

#[test]
fn bare_named_fn_into_function_shaped_param_names_the_annotation() {
    // PARK M9: a bare named function reference (`f`) passed to `ixmap`'s
    // function-shaped parameter (`i -> j`) types as the empty `Thunk([], [])` —
    // a bare ref's arity resolves only when it is applied — and the
    // constrained-apply argument unify reports an opaque `Thunk … with Thunk …`
    // mismatch. The diagnostic must name the callee and point at annotating
    // the parameter's type (or wrapping the ref in a lambda).
    let src = r#"f p = p;
main = ixmap (Pair { first: 0, second: 2 }) f (listArray (Pair { first: 0, second: 2 }) [1.0, 2.0, 3.0]);"#;
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let msg = match eval_ffi(defs, expr, ffi) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("the bare named-function argument must fail to compile"),
    };
    assert!(
        msg.contains("annotate the parameter's type"),
        "the diagnostic must suggest the missing annotation: {msg}"
    );
    assert!(
        msg.contains("ixmap"),
        "the diagnostic must name the callee: {msg}"
    );

    // The lambda-literal workaround the message recommends still compiles and
    // runs: the diagnostic must not fire on a real function value.
    let ok = r#"f p = p;
main = ixmap (Pair { first: 0, second: 2 }) (\x -> x) (listArray (Pair { first: 0, second: 2 }) [1.0, 2.0, 3.0]);"#;
    let (ok_defs, ok_expr) = ok
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let ok_expr = ok_expr.trim().trim_end_matches(';');
    eval_ffi(ok_defs, ok_expr, ForeignRegistry::<f32>::new())
        .expect("a lambda literal must keep the constrained call typechecking");
}
