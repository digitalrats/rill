//! End-to-end sum-value tests: `data ... = Ctor ... | ...` declarations flow
//! through the whole pipeline (infer → reduce → lower → executor) and a
//! `match` on a sum value selects its arm statically, exposing the arm's
//! payload through the single run result (`WorldState::result`).
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;

#[test]
fn sum_match_runs() {
    // `s` is a value binding (CAF) whose body is a `Circle` sum construction;
    // `main` matches on it. The match selects the `Circle` arm; its result is
    // the payload value 1.5.
    let src = r"data Shape = Circle Float | Rect Float Float; s = Circle 1.5; main = case s of { Circle r -> r; Rect w h -> w; }";
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let v = prog.result().expect("the run must produce a single result");
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(1.5));
}

#[test]
fn sum_match_selects_second_arm() {
    // The matching arm is NOT first: static dispatch must select the `Rect`
    // arm and route its `w` payload to the result (regression for the Task 8
    // fix, which previously hard-wired the first arm's register).
    let src = r"data Shape = Circle Float | Rect Float Float; s = Rect 2.0 3.0; main = case s of { Circle r -> r; Rect w h -> w; }";
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let v = prog.result().expect("the run must produce a single result");
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(2.0));
}

#[test]
fn parameterized_user_sum_still_constructs_and_matches() {
    // Parameterized user sums are NOT registered in `data_arities` (only
    // parameterized RECORDS are) — they cannot be typeclass instances in v1,
    // but they must still work as ordinary data types: construction + match.
    let src =
        r"data Opt a = Some a | None; s = Some 1.5; main = case s of { Some x -> x; None -> 0.0; }";
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let v = prog.result().expect("the run must produce a single result");
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(1.5));
}
