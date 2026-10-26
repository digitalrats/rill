//! `?name` control parameters are runtime cells: a value-context read
//! materialises the persistent cell through the value track and reads its
//! default (`0.0`) when unset. Under the single-run value surface there is no
//! way to inject a `SetParameter` into the run, so the cell-persistence tests
//! were removed (see notes below).
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;

fn value_output(prog: &WorldState<f32, 256>) -> rill_lang::arena::Value {
    prog.arena()
        .get(prog.result().expect("the single run must produce a value"))
        .unwrap()
        .clone()
}

#[test]
fn actor_param_defaults_to_zero() {
    // `?gain` is a runtime-stack cell, not intern_param; its default is 0.
    let (defs, expr) = "main = ?gain"
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    assert_eq!(value_output(&prog), rill_lang::arena::Value::Double(0.0));
}

// NOTE — DELETED (re-run surface removed; `test_eval` cannot inject params):
//   set_param_writes_into_the_cell — `prog.set_param(idx, …)` before `run_value`,
//     asserting the cell holds the set value across two ticks (persistence).
//   param_reads_as_buffer_in_signal_context — a `?name` read by a signal-track
//     builtin (`square 440.0 ?level 0.0`), `set_param` between two runs.
//   main_param_reads_as_value_in_value_context — `set_param` before the run,
//     asserting the value read returns the set value across two ticks.
// All three assert value-from-set-param / cross-tick persistence, which the
// single internal run of `test_eval` cannot feed or observe.
