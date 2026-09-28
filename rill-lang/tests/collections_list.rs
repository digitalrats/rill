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

#[test]
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

#[test]
fn fold_rejects_wrong_arity_closure_at_compile_time() {
    // `fold` calls its closure with (acc, elem), so a unary closure is a type
    // error. It used to compile and then panic the runtime path with an
    // index-out-of-bounds in `call_closure_args`.
    let err = match compile::<f32>("main = fold (fn a -> a) 0.0 [1.0, 2.0, 3.0];") {
        Ok(_) => panic!("wrong-arity fold closure must not compile"),
        Err(e) => e,
    };
    assert!(
        format!("{err:?}").contains("binary function"),
        "expected a closure-arity message, got {err:?}"
    );
}

#[test]
fn map_rejects_wrong_arity_closure_at_compile_time() {
    // `map` calls its closure with one element, so a binary closure is a type
    // error. It used to compile and silently produce an empty list.
    let err = match compile::<f32>("main = map (fn a b -> b) [1.0, 2.0];") {
        Ok(_) => panic!("wrong-arity map closure must not compile"),
        Err(e) => e,
    };
    assert!(
        format!("{err:?}").contains("unary function"),
        "expected a closure-arity message, got {err:?}"
    );
}

#[test]
fn consed_list_shares_source_elements_across_ticks() {
    // The CAF `l = cons 1.0 (cons 2.0 (list 4))` is [1.0, 2.0]; main's output
    // `cons 9.0 l` is [9.0, 1.0, 2.0], sharing l's elements 1.0/2.0. Each tick
    // rebuilds and then clears the source list at tick end, so the Cons arm
    // MUST recount (RC++) the shared source elements or the pinned output's
    // refs dangle and read None on the next tick.
    let mut prog = compile::<f32>("l = cons 1.0 (cons 2.0 (list 4)); main = cons 9.0 l;").unwrap();
    let mut out = [0.0f32; 4];
    for tick in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        match prog.arena().get(v).unwrap() {
            rill_lang::arena::Value::List { elems, .. } => {
                assert_eq!(
                    prog.arena().get(elems[1]).unwrap(),
                    &rill_lang::arena::Value::Float(1.0),
                    "tick {tick}: shared source element [1] must survive the tick-end clear"
                );
                assert_eq!(
                    prog.arena().get(elems[2]).unwrap(),
                    &rill_lang::arena::Value::Float(2.0),
                    "tick {tick}: shared source element [2] must survive the tick-end clear"
                );
            }
            other => panic!("tick {tick}: expected a List, got {other:?}"),
        }
    }
}

#[test]
fn list_negative_capacity_does_not_explode() {
    // A negative capacity input must not wrap into a huge `usize` (the interp
    // clamps Int capacities; a non-Int input falls back to 0). Regression guard
    // for the defensive `max(0)` clamp in the ListEmpty arm.
    let mut prog = compile::<f32>("main = list (0 - 3);").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::List { cap, .. } => assert_eq!(*cap, 0),
        other => panic!("expected a List, got {other:?}"),
    }
}

#[test]
fn map_result_element_type_comes_from_closure_return() {
    // `map (fn x -> x + 0.5) [1, 2, 3]` over an INT list must be typed
    // `List Float` (the closure's RETURN type), not `List Int` (the source
    // element type): a later `cons 4.0` must typecheck — it used to fail with a
    // spurious "cannot unify Int with Float".
    assert!(
        compile::<f32>("main = cons 4.0 (map (fn x -> x + 0.5) [1, 2, 3]);").is_ok(),
        "a type-changing map followed by a Float cons must compile"
    );
    // End-to-end length 4: the source needs spare capacity for the cons (map
    // preserves the source cap).
    let mut prog = compile::<f32>(
        "main = length (cons 4.0 (map (fn x -> x + 0.5) (cons 3.0 (cons 2.0 (cons 1.0 (list 4))))));",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(4)
    );
}

#[test]
fn map_preserves_type_when_closure_returns_same_type() {
    // A same-type closure (`x * 2.0` : Float -> Float) keeps the map result a
    // `List Float` — the common case must keep typechecking.
    let mut prog =
        compile::<f32>("main = length (map (fn x -> x * 2.0) [1.0, 2.0, 3.0]);").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(3)
    );
}

#[test]
fn list_capacity_arg_must_be_int() {
    // `list "foo"` — a non-Int capacity argument — must be a COMPILE error,
    // not a silent `Cap(0)`.
    assert!(compile::<f32>("main = list \"foo\";").is_err());
}

#[test]
fn match_over_non_analyzable_scrutinee_is_compile_error() {
    // `head (filter ...)` is not statically resolvable in v1: the old lowering
    // silently picked arm 0 (Nothing) even when the runtime head is Just — a
    // silent wrong value. It must now be a COMPILE error.
    let res = compile::<f32>(
        "main = match (head (filter (fn x -> x > 1.0) [1.0, 2.0, 3.0])) of { Nothing => 0.0; Just x => x; };",
    );
    assert!(
        res.is_err(),
        "a match over a non-statically-resolvable scrutinee must not compile"
    );
    if let Some(msg) = res.err().map(|e| format!("{e:?}")) {
        assert!(
            msg.contains("statically resolvable"),
            "expected a 'statically resolvable' message, got: {msg}"
        );
    }
}
