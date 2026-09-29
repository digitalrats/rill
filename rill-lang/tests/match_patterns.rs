//! Runtime `match`: dispatch on non-static scrutinees, literal/wildcard/var
//! patterns, nested patterns, and guards.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

fn run_float(src: &str) -> f64 {
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(f) => *f,
        rill_lang::arena::Value::Int(i) => *i as f64,
        other => panic!("unexpected output {other:?}"),
    }
}

#[test]
fn runtime_match_on_head_of_filter() {
    // `head (filter ...)` is not statically analyzable; the runtime dispatch
    // must select `Just` and expose its payload.
    assert_eq!(
        run_float("main = match (head (filter (fn x -> x > 1.0) [1.0, 2.0, 3.0])) of { Nothing => 0.0; Just x => x; };"),
        2.0
    );
}

#[test]
fn literal_int_patterns() {
    assert_eq!(
        run_float("main = match 0 of { 0 => 1.0; 1 => 2.0; _ => 3.0; };"),
        1.0
    );
    assert_eq!(
        run_float("main = match 5 of { 0 => 1.0; 1 => 2.0; _ => 3.0; };"),
        3.0
    );
}

#[test]
fn literal_bool_patterns_without_wildcard() {
    assert_eq!(
        run_float("main = match true of { true => 1.0; false => 0.0; };"),
        1.0
    );
    assert_eq!(
        run_float("main = match false of { true => 1.0; false => 0.0; };"),
        0.0
    );
}

#[test]
fn nested_pattern() {
    assert_eq!(
        run_float("main = match Just (Left 2.0) of { Just (Left x) => x; _ => 0.0; };"),
        2.0
    );
}

#[test]
fn var_pattern_binds_whole_value() {
    assert_eq!(run_float("main = match 7 of { v => v; };"), 7.0);
}

#[test]
fn guards_evaluate_in_order_with_fallthrough() {
    // Guards are evaluated in order; a failing guard falls through to the next
    // alternative, then to the next arm.
    assert_eq!(
        run_float("main = match 3.0 of { n | n > 2.0 => 1.0; n | n > 1.0 => 2.0; _ => 3.0; };"),
        1.0
    );
    assert_eq!(
        run_float("main = match 1.5 of { n | n > 2.0 => 1.0; n | n > 1.0 => 2.0; _ => 3.0; };"),
        2.0
    );
    assert_eq!(
        run_float("main = match 0.5 of { n | n > 2.0 => 1.0; n | n > 1.0 => 2.0; _ => 3.0; };"),
        3.0
    );
}

#[test]
fn guarded_arm_without_unguarded_fallback_is_compile_error() {
    // A guarded arm does not satisfy compile-time totality (spec §4.3): the
    // `Just` arm is guarded, `Nothing` is unguarded, and there is no wildcard
    // — the match is non-exhaustive and must NOT compile.
    assert!(compile::<f32>(
        "main = match Just 5.0 of { Just x | x > 10.0 => 1.0; Nothing => 2.0; };",
    )
    .is_err());
}

#[test]
fn match_over_sum_param_switches_with_setparameter() {
    // The scrutinee is a runtime `if` (driven by SetParameter), and the outer
    // match dispatches on the runtime constructor each tick.
    let mut prog = compile::<f32>(
        "main g = match (if g > 0.5 then Just 1.0 else Nothing) of { Just x => x; Nothing => 0.0; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(0.0)
    );
    let idx = prog.param_index("g").unwrap();
    prog.set_param(idx, rill_core::traits::ParamValue::Float(1.0));
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(1.0)
    );
}
