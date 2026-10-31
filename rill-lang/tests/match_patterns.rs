//! Runtime `match`: dispatch on non-static scrutinees, literal/wildcard/var
//! patterns, nested patterns, and guards.
pub mod common;

use common::*;
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;

fn run_float(src: &str) -> f64 {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let v = prog.result().expect("the run must produce a single result");
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Double(f) => *f,
        rill_lang::arena::Value::Int(i) => *i as f64,
        // An unconstrained numeric literal defaults along
        // `default (Integer, Double)`, so `7` is `Integer` — GHC measures the
        // same type for it. The scalar view is the same number.
        rill_lang::arena::Value::Integer(i) => {
            f64::from(i.to_string().parse::<f64>().unwrap_or(0.0))
        }
        other => panic!("unexpected output {other:?}"),
    }
}

#[test]
fn runtime_match_on_head_of_filter() {
    // `head (filter ...)` is not statically analyzable; the runtime dispatch
    // must select `Just` and expose its payload.
    assert_eq!(
        run_float(
            r"main = case (head (filter (\x -> x > 1.0) [1.0, 2.0, 3.0])) of { Nothing -> 0.0; Just x -> x; };"
        ),
        2.0
    );
}

#[test]
fn literal_int_patterns() {
    assert_eq!(
        run_float(r"main = case 0 of { 0 -> 1.0; 1 -> 2.0; _ -> 3.0; };"),
        1.0
    );
    assert_eq!(
        run_float(r"main = case 5 of { 0 -> 1.0; 1 -> 2.0; _ -> 3.0; };"),
        3.0
    );
}

#[test]
fn literal_bool_patterns_without_wildcard() {
    assert_eq!(
        run_float(r"main = case true of { true -> 1.0; false -> 0.0; };"),
        1.0
    );
    assert_eq!(
        run_float(r"main = case false of { true -> 1.0; false -> 0.0; };"),
        0.0
    );
}

#[test]
fn nested_pattern() {
    assert_eq!(
        run_float(r"main = case Just (Left 2.0) of { Just (Left x) -> x; _ -> 0.0; };"),
        2.0
    );
}

#[test]
fn var_pattern_binds_whole_value() {
    assert_eq!(run_float(r"main = case 7 of { v -> v; };"), 7.0);
}

#[test]
fn guards_evaluate_in_order_with_fallthrough() {
    // Guards are evaluated in order; a failing guard falls through to the next
    // alternative, then to the next arm.
    assert_eq!(
        run_float(r"main = case 3.0 of { n | n > 2.0 -> 1.0; n | n > 1.0 -> 2.0; _ -> 3.0; };"),
        1.0
    );
    assert_eq!(
        run_float(r"main = case 1.5 of { n | n > 2.0 -> 1.0; n | n > 1.0 -> 2.0; _ -> 3.0; };"),
        2.0
    );
    assert_eq!(
        run_float(r"main = case 0.5 of { n | n > 2.0 -> 1.0; n | n > 1.0 -> 2.0; _ -> 3.0; };"),
        3.0
    );
}

#[test]
fn guarded_arm_without_unguarded_fallback_is_compile_error() {
    // A guarded arm does not satisfy compile-time totality (spec §4.3): the
    // `Just` arm is guarded, `Nothing` is unguarded, and there is no wildcard
    // — the match is non-exhaustive and must NOT compile.
    assert!(compile_eval::<f32, 256>(
        r"main = case Just 5.0 of { Just x | x > 10.0 -> 1.0; Nothing -> 2.0; };",
        "main",
        Mode::Strict
    )
    .is_err());
}

// Task 13b: removed with the signal surface syntax (unbound wire scrutinee).
// The `_` wire token no longer parses, and the unary value model has no runtime
// path that latches a `ProcessError` from an uninitialized (`None`/`Bottom`)
// match scrutinee: static `ValueMatch` on a Bottom writes Bottom into every arm
// register without latching an error, and the runtime dispatch fail block is
// only reachable from a non-exhaustive match, which inference rejects at
// compile time (totality). The runtime "match on an unbound cell errors" path
// depended on the wire cell, so it cannot be expressed anymore.

// Deleted with the re-run surface:
// `match_over_sum_param_switches_with_setparameter` asserted a runtime match
// whose scrutinee was switched by SetParameter (`?g`) between ticks — the
// single-run value model cannot inject a named parameter into its one run, so
// the cross-tick switching it exercised is no longer testable. The same match
// with a STATIC scrutinee is covered by `runtime_match_on_head_of_filter`.
