//! B3-1: the unary value model — a process type carries a single `ValueTy`,
//! with no `Rate`/`Channel`/`Scalar`. Signals are the ordinary
//! `ValueTy::Buffer`. These are the source-of-truth tests for the type system.

use rill_lang::lexer::tokenize;
use rill_lang::parser::parse;
use rill_lang::types::infer::infer_program;
use rill_lang::types::ty::ValueTy;

fn arrowty(src: &str) -> rill_lang::types::ty::ArrowTy {
    let p = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
    infer_program(&p, rill_lang::mode::Mode::default())
        .unwrap()
        .def_schemes["main"]
        .ty
        .clone()
}

#[test]
fn arrowty_is_plain_value_types() {
    // The process type is a single value type — no Channel/Rate/Scalar vector.
    let t = arrowty("main = 1.0");
    assert!(matches!(t.vty, ValueTy::Double));
}

#[test]
fn list_and_float_are_equal_data() {
    // All data is equal: a List-typed arrow and a Double-typed arrow are the
    // same ArrowTy shape, differing only in the ValueTy element.
    let t = arrowty(r"main = case (head [1.0, 2.0]) of { Nothing -> 0.0; Just x -> x }");
    assert!(matches!(t.vty, ValueTy::Double));
}

#[test]
fn record_is_an_ordinary_value_type() {
    let t = arrowty("data Point = { x: Double, y: Double }; main = Point { x: 1.0, y: 2.0 }");
    assert!(matches!(&t.vty, ValueTy::Data(name, _) if name == "Point"));
}
