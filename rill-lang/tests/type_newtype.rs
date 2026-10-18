//! End-to-end `type` / `newtype` tests: type synonyms substitute in field
//! types, and newtype wrappers construct and flow through the value track to
//! `WorldState::result` as `Value::Newtype`.
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;

/// Compile `src` (which must bind `main` to the evaluated expression) and RUN
/// it once on the single-run value surface, returning the already-run program.
fn run(src: &str) -> WorldState<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    eval_ffi(defs, expr, ffi).unwrap()
}

#[test]
fn newtype_construct_runs() {
    // `h = Hz 440.0` is a newtype constructor; the value output holds a
    // `Newtype` wrapping the inner `Float(440.0)`.
    let prog = run("newtype Hz = Float; h = Hz 440.0; main = h");
    let v = prog.result().unwrap();
    let val = prog.arena().get(v).unwrap();
    let inner = match val {
        rill_lang::arena::Value::Newtype(r) => *r,
        other => {
            panic!("expected a Newtype value, got {:?}", other);
        }
    };
    assert_eq!(
        prog.arena().get(inner).unwrap(),
        &rill_lang::arena::Value::Float(440.0)
    );
}

#[test]
fn type_synonym_substitutes() {
    // `type Angles = Float` substitutes into the record field type: `p.x`
    // projects a Float, proving the synonym resolved to Float at compile time.
    let prog = run(
        "type Angles = Float; data Point = { x: Angles, y: Angles }; p = Point { x: 1.0, y: 2.0 }; main = p.x",
    );
    let v = prog.result().unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(1.0));
}

#[test]
fn type_declaration_order_does_not_matter() {
    // Regression: the synonym and newtype are declared AFTER the data type
    // that uses them. Registration must be order-independent.
    let prog =
        run("data Volt = { v: Scale }; type Scale = Float; v = Volt { v: 12.0 }; main = v.v");
    let v = prog.result().unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(12.0));
}
