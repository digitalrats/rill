//! First-class named function references (v1): `f = double` binds a value of
//! type `ValueTy::Func([], [])`. A bare func value as a program output is an
//! eager `Value::Callable` (no memo cell); calling it dispatches to the
//! referenced definition.
//!
//! Named references to λ-parameter definitions are β-reduced at compile time
//! (see `reduce.rs`), so `ValueCallFunc` runtime dispatch is exercised by
//! closure-valued calls (lambdas, partial application, HOFs — see
//! `tests/closures.rs`, `tests/currying_wires.rs`, `tests/hof.rs`).
pub mod common;

use common::*;
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;

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
fn function_reference_calls_signal_definition() {
    // `f = double` binds a func value; `main = f 21.0` β-reduces the reference
    // chain at compile time to the signal computation `21.0 * 2.0`, so the
    // program's BLOCK output is 42.0. A func value over a signal function
    // collapses to the block track — only the value-track form (below) yields
    // a `Value::Callable`/value-channel output.
    compile_eval::<f32, 256>(
        "double x = x * 2.0; f = double; main = f 21.0",
        "main",
        Mode::Strict,
    )
    .unwrap();
}

#[test]
fn function_reference_calls_value_definition() {
    // A func value over a VALUE function: `first p = p.x` (p is a value
    // λ-parameter) called through `f` yields a VALUE output Float(5.0) — the
    // value-track form of a named function-reference call.
    let prog = run(
        "data Point = { x: Float, y: Float }; first p = p.x; f = first; main = f (Point { x: 5.0, y: 1.0 })",
    );
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(5.0)
    );
}

#[test]
fn func_value_as_output() {
    // `main = f` where `f = double` produces a first-class function value
    // output: an eager `Value::Callable` referencing the named definition
    // `double`.
    let prog = run("double x = x * 2.0; f = double; main = f");
    let v = prog.result().unwrap();
    match prog.arena().get(v).unwrap() {
        &rill_lang::arena::Value::Callable { cell: None, .. } => {}
        other => panic!("expected eager Callable value, got {other:?}"),
    }
}

#[test]
fn func_value_can_be_chained() {
    // `g = f` where `f = double` resolves through the reference chain: calling
    // `g` still dispatches to `double` (the scheme collapses the func-value
    // chain to the referenced definition at inference time).
    compile_eval::<f32, 256>(
        "double x = x * 2.0; f = double; g = f; main = g 21.0",
        "main",
        Mode::Strict,
    )
    .unwrap();
}

#[test]
fn func_value_call_arity_mismatch_is_error() {
    // `double` takes one argument: calling its func value with the wrong arity
    // must be a compile error, not a runtime surprise.
    let r = compile_eval::<f32, 256>(
        "double x = x * 2.0; f = double; main = f 21.0 22.0",
        "main",
        Mode::Strict,
    );
    assert!(r.is_err());
}

#[test]
fn direct_bare_ref_produces_func_value() {
    // `main = double` (a bare reference to a definition with λ-parameters)
    // is itself a func value output — the binding `f = double` is sugar for
    // the same reference.
    let prog = run("double x = x * 2.0; main = double");
    let v = prog.result().unwrap();
    match prog.arena().get(v).unwrap() {
        &rill_lang::arena::Value::Callable { cell: None, .. } => {}
        other => panic!("expected eager Callable value, got {other:?}"),
    }
}

#[test]
fn named_ref_closure_references_real_fragment() {
    // A bare named reference (`f = double`) compiles `double`'s body into a
    // REAL fragment (Task 5: anchor bodies compile to fragments so named-def
    // currying works). The closure must NOT reference the old
    // `usize::MAX` sentinel no-op fragment.
    let prog = run("double x = x * 2.0; f = double; main = f");
    let v = prog.result().unwrap();
    match prog.arena().get(v).unwrap() {
        &rill_lang::arena::Value::Callable {
            fragment,
            cell: None,
            ..
        } => {
            assert_ne!(
                fragment,
                u32::MAX,
                "named-ref closure must reference a real fragment, not the sentinel no-op"
            );
        }
        other => panic!("expected eager Callable value, got {other:?}"),
    }
}

#[test]
fn value_function_with_match_body_calls_correct_arm() {
    // A value function whose body is a `match` on its value λ-parameter:
    // substitution must descend into the match arms (with arm bindings
    // shadowing the outer parameter), so `f (Circle 7.0)` selects Circle's
    // arm and yields Float(7.0).
    let prog = run(
        r"data Shape = Circle Float | Rect Float Float; radius s = case s of { Circle r -> r; Rect w h -> w; }; f = radius; main = f (Circle 7.0)",
    );
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(7.0)
    );
}

#[test]
fn func_value_in_signal_combinator_is_error() {
    // A bare func value has no block representation: composing it through a
    // signal combinator must be a clean compile error, not a lowering panic.
    assert!(compile_eval::<f32, 256>(
        "double x = x * 2.0; main = double : _",
        "main",
        Mode::Strict
    )
    .is_err());
    assert!(compile_eval::<f32, 256>(
        "double x = x * 2.0; main = double + 1.0",
        "main",
        Mode::Strict
    )
    .is_err());
}
