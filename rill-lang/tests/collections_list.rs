//! End-to-end list-collection tests: literals, cons/head/tail/length/map/fold/filter.
pub mod common;

use common::*;
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;

/// Compile `src` against an empty foreign registry and RUN it once (the
/// single-run value surface), returning the already-run program.
fn run(src: &str) -> WorldState<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    eval_ffi(defs, expr, ffi).unwrap()
}

#[test]
fn list_literal_length_and_head() {
    let prog = run("main = length [1.0, 2.0, 3.0];");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(3)
    );
}

#[test]
fn cons_builds_list_and_head_is_maybe() {
    let prog = run(r"main = case (head (cons 1.0 (list))) of { Nothing -> 0.0; Just x -> x; };");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(1.0)
    );
}

#[test]
fn map_fold_filter_over_list() {
    let prog = run(r"main = fold (\a -> \b -> a + b) 0.0 (map (\x -> x * 2.0) [1.0, 2.0, 3.0]);");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(12.0)
    );
}

#[test]
fn list_grows_past_literal_capacity() {
    // Open collections: a `cons` past the former strict capacity is no longer
    // a runtime error — the list grows.
    let prog = run("xs = [1.0, 2.0]; main = length (cons 3.0 (cons 4.0 xs));");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(4)
    );
}

#[test]
fn cons_list_fold_sum() {
    // End-to-end: `list` gives an empty List; cons builds
    // 9 : 1 : [] and `fold (+)` reduces it to 10.0.
    let prog = run(r"main = fold (\a -> \b -> a + b) 0.0 (cons 9.0 (cons 1.0 (list)));");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(10.0)
    );
}

#[test]
fn cons_prepends_new_head() {
    // `cons x xs = x : xs` — the new element becomes the head. `head` of
    // `cons 9.0 (cons 1.0 (list))` must be `Just 9.0`; append semantics would
    // leave the old head 1.0 first.
    let prog = run("main = head (cons 9.0 (cons 1.0 (list)));");
    let v = prog.result().unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Sum(0, payload) => {
            assert_eq!(
                prog.arena().get(payload[0]).unwrap(),
                &rill_lang::arena::Value::Double(9.0)
            );
        }
        other => panic!("expected Just 9.0, got {other:?}"),
    }
}

#[test]
fn colon_is_cons_for_list_operands() {
    // `x : xs` — Haskell-style cons via the `:` overload. `:` is right-
    // associative for a List rhs: `1.0 : 2.0 : list` ≡ `1.0 : (2.0 : list)`.
    let prog = run("main = length (1.0 : 2.0 : list);");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(2),
        "1.0 : 2.0 : list must build a 2-element list"
    );
}

#[test]
fn colon_cons_chains_right_associatively() {
    // The parser builds `:` LEFT-associatively; a List-terminated chain is
    // re-associated right: `1.0 : 2.0 : 3.0 : list` ≡ 1.0 : (2.0 : (3.0 :
    // list)). Head must be the first element.
    let prog = run("main = head (1.0 : 2.0 : 3.0 : list);");
    let v = prog.result().unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Sum(0, payload) => {
            assert_eq!(
                prog.arena().get(payload[0]).unwrap(),
                &rill_lang::arena::Value::Double(1.0),
                "head of a cons chain must be the first element"
            );
        }
        other => panic!("expected Just 1.0, got {other:?}"),
    }
}

#[test]
fn colon_seq_still_composes_buffers() {
    // `:` on Buffer operands stays Faust sequencing: a partial arrow
    // `onepole 200.0 0.7` applied to a constant +1.0 buffer (a zero-frequency
    // square wave).
    use rill_lang::register;

    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_filters(&mut ffi);
    register::register_foreign_generators(&mut ffi);
    let (defs, expr) = "main = onepole 200.0 0.7 (square 0.0 1.0 0.0);"
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let mut out = [0.0f32; 64];
    let r = prog.result().unwrap();
    assert!(prog.copy_buffer(r, &mut out));
    assert!(
        out.iter().all(|v| v.is_finite()),
        "onepole seq output must be finite"
    );
    assert!(
        out[0] > 0.0,
        "onepole must pass a DC step, got out[0]={}",
        out[0]
    );
}

#[test]
fn fold_rejects_wrong_arity_closure_at_compile_time() {
    // `fold` calls its closure with (acc, elem), so a unary closure is a type
    // error. It used to compile and then panic the runtime path with an
    // index-out-of-bounds in `call_closure_args`.
    let err = match compile_eval::<f32, 256>(
        r"main = fold (\a -> a) 0.0 [1.0, 2.0, 3.0];",
        "main",
        Mode::Strict,
    ) {
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
    let err = match compile_eval::<f32, 256>(
        r"main = map (\a -> \b -> b) [1.0, 2.0];",
        "main",
        Mode::Strict,
    ) {
        Ok(_) => panic!("wrong-arity map closure must not compile"),
        Err(e) => e,
    };
    assert!(
        format!("{err:?}").contains("unary function"),
        "expected a closure-arity message, got {err:?}"
    );
}

#[test]
fn list_empty_has_zero_length() {
    // `list` (no capacity arg) is an empty list with length 0.
    let prog = run("main = length (list);");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(0)
    );
}

#[test]
fn map_result_element_type_comes_from_closure_return() {
    // A map over an INT list whose closure RETURNS a Double must be typed
    // `List Double` (the closure's return type), not `List Int` (the source
    // element type), so a later `cons 4.0` typechecks.
    //
    // The closure must be `Int -> Double` in its own right: rill's integer
    // literals are monomorphic `Int` (no `fromInteger` elaboration yet —
    // design §3.6, stage 7), so `\x -> x + 0.5` over an `Int` list is the
    // `Fractional Int` error GHC reports for the same program with the list
    // pinned: `map (\x -> x + 0.5) ([1,2,3] :: [Int])` →
    // "No instance for 'Fractional Int' arising from the literal '0.5'". The
    // `if` closure below has type `Int -> Double`, which GHC accepts
    // (`4.0 : map (\x -> if x > 0 then 1.0 else 0.0) ([1,2,3] :: [Int])`
    // evaluates to `[4.0,1.0,1.0,1.0]`).
    assert!(
        compile_eval::<f32, 256>(
            r"main = cons 4.0 (map (\x -> if x > 0 then 1.0 else 0.0) [1, 2, 3]);",
            "main",
            Mode::Strict,
        )
        .is_ok(),
        "a type-changing map followed by a Double cons must compile"
    );
    // And the `Fractional Int` program really is rejected, not silently
    // retyped: the same expression with a fractional literal beside an `Int`
    // list element.
    assert!(
        compile_eval::<f32, 256>(
            r"main = cons 4.0 (map (\x -> x + 0.5) [1, 2, 3]);",
            "main",
            Mode::Strict,
        )
        .is_err(),
        "`x + 0.5` over an `Int` element is `Fractional Int` (GHC: type error)"
    );
    // End-to-end length 4: the source needs spare capacity for the cons (map
    // preserves the source cap).
    let prog = run(
        r"main = length (cons 4.0 (map (\x -> x + 0.5) (cons 3.0 (cons 2.0 (cons 1.0 (list))))));",
    );
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(4)
    );
}

#[test]
fn map_preserves_type_when_closure_returns_same_type() {
    // A same-type closure (`x * 2.0` : Double -> Double) keeps the map result a
    // `List Double` — the common case must keep typechecking.
    let prog = run(r"main = length (map (\x -> x * 2.0) [1.0, 2.0, 3.0]);");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(3)
    );
}

#[test]
fn match_over_non_analyzable_scrutinee_dispatches_at_runtime() {
    // `head (filter ...)` is not statically analyzable — the runtime dispatch
    // must select `Just` and expose its payload (regression for the old
    // static-only behavior which rejected it).
    let prog = run(
        r"main = case (head (filter (\x -> x > 1.0) [1.0, 2.0, 3.0])) of { Nothing -> 0.0; Just x -> x; };",
    );
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(2.0)
    );
}
