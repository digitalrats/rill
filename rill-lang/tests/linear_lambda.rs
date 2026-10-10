//! B3-1: the linear lambda model — `ArrowTy = Vec<ValueTy>`, no
//! `Rate`/`Channel`/`Scalar`. Signals are the ordinary `ValueTy::Buffer`.
//! These are the source-of-truth tests for the new type system.

use rill_lang::lexer::tokenize;
use rill_lang::parser::parse;
use rill_lang::types::infer::infer_program;
use rill_lang::types::ty::ValueTy;

fn arrowty(src: &str) -> rill_lang::types::ty::ArrowTy {
    let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
    infer_program(&p, rill_lang::mode::Mode::default())
        .unwrap()
        .process_ty
}

#[test]
fn arrowty_is_plain_value_types() {
    // ArrowTy carries ValueTy vectors directly — no Channel/Rate/Scalar.
    let t = arrowty("main = 1.0");
    assert_eq!(t.ins.len(), 0);
    assert_eq!(t.outs.len(), 1);
    assert!(matches!(t.outs[0], ValueTy::Float));
}

#[test]
fn list_and_float_are_equal_data() {
    // All data is equal: a List-typed arrow and a Float-typed arrow are the
    // same ArrowTy shape, differing only in the ValueTy element.
    let t = arrowty("main = match (head [1.0, 2.0]) of { Nothing => 0.0; Just x => x }");
    assert!(matches!(t.outs[0], ValueTy::Float));
}

#[test]
fn record_is_an_ordinary_value_type() {
    let t = arrowty("data Point = { x: Float, y: Float }; main = Point { x: 1.0, y: 2.0 }");
    assert!(matches!(&t.outs[0], ValueTy::Data(name, _) if name == "Point"));
}
