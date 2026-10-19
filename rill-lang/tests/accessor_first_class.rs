//! Signature-less curried accessors are first-class λ-parameters: a def like
//! `at a i = a ! i` must type its λ-parameters from the recovered body and
//! specialize per call site in lowering, exactly like a class-constrained def.
//!
//! Today the specialized lowering does not exist: the accessor body reaches
//! the `Ix` typeclass dispatch with an unsolved index var and dies in lowering
//! with `no instance of `Ix` for type `Pair` and no default for `inRange``.
//! The first three tests pin the target behavior and are RED; the remaining
//! three pin behavior that already holds.
pub mod common;

use common::*;
use rill_lang::arena::Value;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;

/// Compile and evaluate the `main` expression of `src` in a single run,
/// returning its value.
fn run_value(src: &str) -> Value {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let v = prog.result().expect("the run must produce a single result");
    prog.arena().get(v).unwrap().clone()
}

/// Compile and run `src`, returning `main`'s value as a number.
fn run_num(src: &str) -> f64 {
    match run_value(src) {
        Value::Float(x) => x,
        Value::Int(i) => i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

#[test]
fn signature_less_array_accessor_reads_a_cell_in_strict() {
    // park10: today this dies in lowering with
    // "no instance of `Ix` for type `Pair` and no default for `inRange`": the
    // λ-body's `a ! i` reaches the `index`/`Ix` dispatch with `i` unsolved.
    // Target: the accessor specializes per call site and reads the cell.
    let src = "at a i = a ! i;\n\
                main = at (listArray (Pair { first: 0, second: 2 }) [1.0, 2.0, 3.0]) 1;";
    assert_eq!(run_num(src), 2.0);
}

#[test]
fn signature_less_array_accessor_reads_a_cell_in_complete() {
    // Same accessor under `Mode::Complete` (the mode region/ST programs use).
    let (defs, expr) = (
        "at a i = a ! i;",
        "at (listArray (Pair { first: 0, second: 2 }) [1.0, 2.0, 3.0]) 1",
    );
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_mode::<256>(defs, expr, ffi, Mode::Complete).unwrap();
    let v = prog.result().expect("the run must produce a single result");
    match prog.arena().get(v).unwrap().clone() {
        Value::Float(x) => assert_eq!(x, 2.0),
        other => panic!("expected a Float, got {other:?}"),
    }
}

#[test]
fn signature_less_accessor_works_with_a_2d_index_pair() {
    // The specialized body must discharge `Ix (Pair Int Int)` like the prelude
    // accessor does for the annotated case (array_immutable's 2-D spell): the
    // accessor reads the center cell of a 3x3 row-major matrix through a
    // `Pair` index.
    let src = "at a i = a ! i;\n\
                main = at (listArray (Pair { first: Pair { first: 0, second: 0 }, \
                                                  second: Pair { first: 2, second: 2 } }) \
                           [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]) \
                           (Pair { first: 1, second: 1 });";
    assert_eq!(run_num(src), 5.0);
}

#[test]
fn genuinely_unbound_params_stay_float() {
    // A signature-less def whose parameters never bind a class demand keeps the
    // ordinary `Float` treatment — no per-call-site specialization, no change.
    let src = "add a b = a + b;\nmain = add 2.0 3.0;";
    assert_eq!(run_num(src), 5.0);
}

#[test]
fn signature_less_list_accessor_keeps_working() {
    // A signature-less accessor over list functions involves no class demand
    // and must keep working unchanged. `head (tail [10.0, 20.0])` is
    // `Just 20.0` — `head` returns a `Maybe`, so the invocation is deref'd
    // through the same `case` idiom the lazy-mode suites use.
    let src = "nth xs k = head (tail xs);\n\
                main = case nth [10.0, 20.0] 0 of { Nothing -> 0.0; Just x -> x };";
    assert_eq!(run_num(src), 20.0);
}

#[test]
fn mismatching_call_still_names_the_annotation() {
    // PARK M10 stays: a mismatching call to the signature-less accessor still
    // names `at` and points at the missing parameter annotation.
    let (defs, expr) = ("at a i = a ! i;", "at (0, 9) 9");
    let ffi = ForeignRegistry::<f32>::new();
    let msg = match eval_ffi(defs, expr, ffi) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("the mismatching call must fail to compile"),
    };
    assert!(
        msg.contains("annotate the parameter's type"),
        "the diagnostic must suggest the missing annotation: {msg}"
    );
    assert!(
        msg.contains("`at`"),
        "the diagnostic must name the definition: {msg}"
    );
}
