//! Closure env capture and runtime dispatch: a lambda literal lowers to a
//! `FragmentIr`, free variables (including parent λ-parameters) are captured
//! by value into an env `Record`, and `ValueCallFunc` dispatches a call with a
//! temporary cell frame.
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;

/// Compile `src` against an empty foreign registry and RUN it once (the
/// single-run value surface), returning the already-run program.
fn run(src: &str) -> WorldState<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    eval_ffi(defs, expr, ffi).unwrap()
}

#[test]
fn lambda_captures_parent_param_by_value() {
    // adder = fn n -> fn x -> x + n;  add2 = adder 2.0;  main = add2 3.0 -> 5.0
    let prog = run(r"adder = \n -> \x -> x + n; add2 = adder 2.0; main = add2 3.0");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(5.0)
    );
}

#[test]
fn lambda_captures_top_level_def() {
    // k = 2.0;  double = fn x -> x * k;  main = double 21.0 -> 42.0
    let prog = run(r"k = 2.0; double = \x -> x * k; main = double 21.0");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(42.0)
    );
}

#[test]
fn nested_capture_produces_constant_output() {
    // addn = \n -> \x -> x + n;  main = addn 3.0 4.0
    // `addn 3.0 4.0` fully applies the curried definition: unary β-reduction
    // substitutes both λ-parameters and folds to the constant `4.0 + 3.0`.
    let prog = run(r"addn = \n -> \x -> x + n; main = addn 3.0 4.0");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(7.0)
    );
}
