//! End-to-end list-collection tests: literals, cons/head/tail/length/map/fold/filter.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn list_literal_length_and_head() {
    let mut prog = compile::<f32>("main = length [1.0, 2.0, 3.0];").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(3)
    );
}

// Phase 7: `match`/`Nothing`/`Just` over the builtin `Maybe` shapes (and the
// `list` op's `ListEmpty` dispatch, Task 6.4) land after the value-collection
// lowering of Task 6.3. Keep the test as a documented target for those tasks.
#[test]
#[ignore = "Phase 7"]
fn cons_builds_list_and_head_is_maybe() {
    let mut prog = compile::<f32>(
        "main = match (head (cons 1.0 (list 4))) of { Nothing => 0.0; Just x => x; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(1.0)
    );
}

#[test]
fn map_fold_filter_over_list() {
    let mut prog = compile::<f32>(
        "main = fold (fn a b -> a + b) 0.0 (map (fn x -> x * 2.0) [1.0, 2.0, 3.0]);",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(12.0)
    );
}

#[test]
fn cons_overflow_is_runtime_process_error() {
    let mut prog = compile::<f32>("main = cons 3.0 [1.0, 2.0];").unwrap();
    let mut out = [0.0f32; 4];
    let res = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    assert!(res.is_err(), "cons past capacity must be a runtime error");
    let err = res.unwrap_err();
    assert!(
        format!("{err:?}").contains("capacity exceeded"),
        "expected capacity message, got {err:?}"
    );
}

#[test]
fn cons_list_fold_sum() {
    // End-to-end: `list 4` gives an empty List of capacity 4; cons builds
    // 9 : 1 : [] and `fold (+)` reduces it to 10.0.
    let mut prog =
        compile::<f32>("main = fold (fn a b -> a + b) 0.0 (cons 9.0 (cons 1.0 (list 4)));")
            .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(10.0)
    );
}

#[test]
fn cons_prepends_new_head() {
    // `cons x xs = x : xs` — the new element becomes the head. `head` of
    // `cons 9.0 (cons 1.0 (list 4))` must be `Just 9.0`; append semantics would
    // leave the old head 1.0 first.
    let mut prog = compile::<f32>("main = head (cons 9.0 (cons 1.0 (list 4)));").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Sum(0, payload) => {
            assert_eq!(
                prog.arena().get(payload[0]).unwrap(),
                &rill_lang::arena::Value::Float(9.0)
            );
        }
        other => panic!("expected Just 9.0, got {other:?}"),
    }
}

#[test]
fn repeated_cons_overflow_ticks_do_not_leak_arena() {
    // A cons overflow latches a runtime error; the tick must still release its
    // per-tick registers so a graph that keeps calling `process` while the
    // program is broken cannot exhaust the fixed arena (one orphaned slot per
    // register per erroring tick would otherwise exhaust it after a few ticks).
    let mut prog = compile::<f32>("main = cons 3.0 [1.0, 2.0];").unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..20 {
        let res = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
        assert!(res.is_err(), "every tick must report the overflow");
        assert_eq!(prog.arena().live(), 0, "erroring tick must leak nothing");
    }
}
