//! `if` as a pure expression: static selection and result binding.
//!
//! The former SetParameter-driven per-tick switching tests
//! (`if_switches_on_setparameter_between_ticks`, `if_and_runtime_match_combined`,
//! `if_nested_in_expression_at_runtime`) were deleted with the re-run surface:
//! the single-run value model cannot inject a named parameter (`?g`) into its
//! one run, so the cross-tick branch switching they asserted is no longer
//! testable.
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;

fn float_output(src: &str) -> f64 {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let v = prog.result().expect("the run must produce a single result");
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(f) => *f,
        other => panic!("unexpected output {other:?}"),
    }
}

#[test]
fn if_static_branches() {
    assert_eq!(float_output("main = if true then 1.0 else 2.0;"), 1.0);
    assert_eq!(float_output("main = if false then 1.0 else 2.0;"), 2.0);
}

#[test]
fn if_binds_result_and_nests() {
    assert_eq!(
        float_output("x = if false then 1.0 else 2.0; main = 0.5 * x;"),
        1.0
    );
}

// NOTE: a value-track feedback accumulator (`acc = ~ (acc + 1.0)`) is not
// testable in v1 — `~`/`@` over values is not wired in lowering (lower.rs:
// "Value-state slots are not emitted by lowering in v1"), so no value-track
// program can switch across ticks except via a SetParameter-driven main cell,
// which the deleted `if_switches_on_setparameter_between_ticks` covered.
