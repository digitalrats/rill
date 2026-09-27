//! End-to-end sum-value tests: `data ... = Ctor ... | ...` declarations flow
//! through the whole pipeline (infer → reduce → lower → executor) and a
//! `match` on a sum value selects its arm statically, exposing the arm's
//! payload through `RillProgram::value_outputs`.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn sum_match_runs() {
    // `s` is a value binding (CAF) whose body is a `Circle` sum construction;
    // `main` matches on it. The match selects the `Circle` arm; its output is
    // the payload value 1.5.
    let mut prog = compile::<f32>(
        "data Shape = Circle Float | Rect Float Float; s = Circle 1.5; main = match s of { Circle r => r; Rect w h => w; }",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(1.5));
}

#[test]
fn sum_match_selects_second_arm() {
    // The matching arm is NOT first: static dispatch must select the `Rect`
    // arm and route its `w` payload to the output (regression for the Task 8
    // fix, which previously hard-wired the first arm's register).
    let mut prog = compile::<f32>(
        "data Shape = Circle Float | Rect Float Float; s = Rect 2.0 3.0; main = match s of { Circle r => r; Rect w h => w; }",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(2.0));
}

#[test]
fn parameterized_user_sum_still_constructs_and_matches() {
    // Parameterized user sums are NOT registered in `data_arities` (only
    // parameterized RECORDS are) — they cannot be typeclass instances in v1,
    // but they must still work as ordinary data types: construction + match.
    let mut prog = compile::<f32>(
        "data Opt a = Some a | None; s = Some 1.5; main = match s of { Some x => x; None => 0.0; }",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(1.5));
}
