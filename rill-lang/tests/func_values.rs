//! First-class named function references (v1): `f = double` binds a value of
//! type `ValueTy::Func([], [])`. A bare func value as a program output is a
//! `Value::Closure`; calling it dispatches to the referenced definition.
//!
//! v1 scope: only NAMED references (no lambdas/closures). Calls are resolved
//! at compile time by β-reduction, so `ValueCallFunc` stays a documented
//! no-op reserved for a future runtime-dispatch task.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn function_reference_calls_signal_definition() {
    // `f = double` binds a func value; `main = f 21.0` β-reduces the reference
    // chain at compile time to the signal computation `21.0 * 2.0`, so the
    // program's BLOCK output is 42.0. A func value over a signal function
    // collapses to the block track — only the value-track form (below) yields
    // a `Value::Closure`/value-channel output.
    let mut prog = compile::<f32>("double x = x * 2.0; f = double; main = f 21.0").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out, [42.0, 42.0, 42.0, 42.0]);
    assert_eq!(
        prog.value_outputs().len(),
        0,
        "signal program has no value outputs"
    );
}

#[test]
fn function_reference_calls_value_definition() {
    // A func value over a VALUE function: `first p = p.x` (p is a value
    // λ-parameter) called through `f` yields a VALUE output Float(5.0) — the
    // value-track form of a named function-reference call.
    let mut prog = compile::<f32>(
        "data Point = { x: Float, y: Float }; first p = p.x; f = first; main = f (Point { x: 5.0, y: 1.0 })",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(5.0)
    );
}

#[test]
fn func_value_as_output() {
    // `main = f` where `f = double` produces a first-class function value
    // output: a `Value::Closure` referencing the named definition `double`.
    let mut prog = compile::<f32>("double x = x * 2.0; f = double; main = f").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    match prog.arena().get(v).unwrap() {
        &rill_lang::arena::Value::Closure(_, _) => {}
        other => panic!("expected Closure value, got {other:?}"),
    }
}

#[test]
fn func_value_as_output_survives_ticks() {
    // A Closure value output pins no subtree (a single slot), but the output must
    // still survive across ticks without exhausting the fixed arena.
    let mut prog = compile::<f32>("double x = x * 2.0; f = double; main = f").unwrap();
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
fn func_value_can_be_chained() {
    // `g = f` where `f = double` resolves through the reference chain: calling
    // `g` still dispatches to `double` (the scheme collapses the func-value
    // chain to the referenced definition at inference time).
    let mut prog = compile::<f32>("double x = x * 2.0; f = double; g = f; main = g 21.0").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out, [42.0, 42.0, 42.0, 42.0]);
}

#[test]
fn func_value_call_arity_mismatch_is_error() {
    // `double` takes one argument: calling its func value with the wrong arity
    // must be a compile error, not a runtime surprise.
    let r = compile::<f32>("double x = x * 2.0; f = double; main = f 21.0 22.0");
    assert!(r.is_err());
}

#[test]
fn direct_bare_ref_produces_func_value() {
    // `main = double` (a bare reference to a definition with λ-parameters)
    // is itself a func value output — the binding `f = double` is sugar for
    // the same reference.
    let mut prog = compile::<f32>("double x = x * 2.0; main = double").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    match prog.arena().get(prog.value_outputs()[0].unwrap()).unwrap() {
        &rill_lang::arena::Value::Closure(_, _) => {}
        other => panic!("expected Closure value, got {other:?}"),
    }
}

#[test]
fn value_function_with_match_body_calls_correct_arm() {
    // A value function whose body is a `match` on its value λ-parameter:
    // substitution must descend into the match arms (with arm bindings
    // shadowing the outer parameter), so `f (Circle 7.0)` selects Circle's
    // arm and yields Float(7.0).
    let mut prog = compile::<f32>(
        "data Shape = Circle Float | Rect Float Float; radius s = match s of { Circle r => r; Rect w h => w; }; f = radius; main = f (Circle 7.0)",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(7.0)
    );
}

#[test]
fn func_value_in_signal_combinator_is_error() {
    // A bare func value has no block representation: composing it through a
    // signal combinator must be a clean compile error, not a lowering panic.
    assert!(compile::<f32>("double x = x * 2.0; main = double : _").is_err());
    assert!(compile::<f32>("double x = x * 2.0; main = double + 1.0").is_err());
}
