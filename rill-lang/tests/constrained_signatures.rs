//! Haskell 2010 class contexts (`=>`) on `Def::Eval` signatures:
//! `listArray : Ix i => …`, `ixmap : (Ix i, Ix j) => …`.
//!
//! The context is carried on the signature; it is not yet enforced by the
//! typechecker (constrained-def dispatch is a later task), so an unconstrained
//! body must keep compiling.

use rill_lang::compile;
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
    let src = r#"ixmap a b :: (Ix i, Ix j) => Pair i i -> Pair j j -> Float = a;
main = 1.0;"#;
    let first = render(&program(src)).unwrap();
    assert!(
        first.contains("ixmap :: (Ix i, Ix j) => (Pair i i -> Pair j j -> Float) = "),
        "two constraints render parenthesized and comma-separated: {first}"
    );
    assert_eq!(render(&program(&first)).unwrap(), first);
}

#[test]
fn constrained_signature_does_not_break_typechecking() {
    // The constraint is recorded but not enforced yet; the body is an ordinary
    // unconstrained function and must compile and run.
    let mut prog = compile::<f32>("f x :: Ix i => Float -> Float = x; main = f 1.0;")
        .expect("a constrained signature must not break compilation");
    prog.run_value().unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(1.0)
    );
}
