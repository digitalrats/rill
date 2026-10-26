//! End-to-end data-program tests: `data` declarations flow through the whole
//! pipeline (infer → reduce → lower → executor) and the program's single run
//! result is exposed via `WorldState::result`.
//!
//! The multi-tick persistence tests moved off this file when the value surface
//! became single-run: `test_eval` executes the entry fragment exactly once, so the cross-tick arena-accounting observables ("the value
//! output keeps one slot alive across ticks", "tick 2 must not exhaust the
//! fixed arena") no longer exist.
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;

#[test]
fn record_flow_through_process() {
    // `p` is a value binding (CAF) whose body is a record constructor; `main`
    // projects a field. The program must compile and run, and the single run
    // result must hold Double(2.0) — the projected `p.x`.
    let src = "data Point = { x: Double, y: Double }; p = Point { x: 2.0, y: 3.0 }; main = p.x";
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let v = prog.result().expect("the run must produce a single result");
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(2.0)
    );
}
