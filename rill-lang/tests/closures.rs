//! Closure env capture and runtime dispatch: a lambda literal lowers to a
//! `FragmentIr`, free variables (including parent λ-parameters) are captured
//! by value into an env `Record`, and `ValueCallFunc` dispatches a call with a
//! temporary cell frame.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn lambda_captures_parent_param_by_value() {
    // adder = fn n -> fn x -> x + n;  add2 = adder 2.0;  main = add2 3.0 -> 5.0
    let mut prog =
        compile::<f32>("adder = fn n -> fn x -> x + n; add2 = adder 2.0; main = add2 3.0").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    let v = vo[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(5.0)
    );
}

#[test]
fn lambda_captures_top_level_def() {
    // k = 2.0;  double = fn x -> x * k;  main = double 21.0 -> 42.0
    let mut prog = compile::<f32>("k = 2.0; double = fn x -> x * k; main = double 21.0").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(42.0)
    );
}

#[test]
fn closure_output_survives_ticks() {
    // A closure value output pins its env record across ticks: re-processing
    // must not exhaust the fixed arena (the previous tick's env is released at
    // the start of the next tick).
    let mut prog = compile::<f32>("k = 2.0; double = fn x -> x * k; main = double").unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        match prog.arena().get(prog.value_outputs()[0].unwrap()).unwrap() {
            &rill_lang::arena::Value::Closure(_, _) => {}
            other => panic!("expected Closure value, got {other:?}"),
        }
    }
}

#[test]
fn nested_capture_produces_constant_output() {
    // addn = fn n -> fn x -> x + n;  main = addn 3.0 4.0
    // `addn 3.0 4.0` is a single Apply with TWO arguments, but `addn` takes one
    // λ-parameter — this must be a compile error (partial application is a
    // later task), not a silent wrong value.
    assert!(compile::<f32>("addn = fn n -> fn x -> x + n; main = addn 3.0 4.0").is_err());
}
