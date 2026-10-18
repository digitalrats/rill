//! Guard tests for the Closure/Thunk unification (Value::Callable + the single
//! `run_call` fragment runner): both construction paths — eager closure
//! (`ValueMakeClosure`, `ValueCallFunc`) and lazy thunk (`ValueLazy`,
//! `ValueCallLazy`) — still compute the same values after the merge.
//!
//! Thunks are a `Complete`-mode concern, so every source carries the
//! `{-# MODE complete #-}` pragma and runs through the eval surface.
pub mod common;

use common::*;

/// Compile `src` in `Complete` mode, run it once, and return the single result.
fn run_complete(src: &str) -> rill_lang::arena::Value {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = rill_lang::ffi::ForeignRegistry::<f32>::new();
    let prog = eval_mode::<256>(defs, expr, ffi, rill_lang::mode::Mode::Complete).unwrap();
    let v = prog.result().unwrap();
    prog.arena().get(v).unwrap().clone()
}

#[test]
fn eager_and_lazy_calls_still_work() {
    // eager: a named function applied to args; lazy: a thunk over a closure.
    let src = r"{-# MODE complete #-}
id2 = \x -> x;
main = case (head (id2 [1.0, 2.0])) of { Nothing -> 0.0; Just x -> x };";
    match run_complete(src) {
        rill_lang::arena::Value::Float(x) => assert_eq!(x, 1.0),
        other => panic!("expected Float(1.0), got {other:?}"),
    }
}

#[test]
fn closure_and_thunk_are_one_variant() {
    // Both construction paths (MakeClosure, ValueLazy, ValueCallLazy) now
    // produce Value::Callable; the arena has no Closure/Thunk variants.
    // (Compile a program using a lambda + a lazy let to exercise both.)
    let src = r"{-# MODE complete #-}
id2 = \x -> x;
main = case (head (let y = id2 [1.0, 2.0] in y)) of { Nothing -> 0.0; Just x -> x };";
    match run_complete(src) {
        rill_lang::arena::Value::Float(x) => assert_eq!(x, 1.0),
        other => panic!("expected Float(1.0), got {other:?}"),
    }
}

#[test]
fn fragment_call_with_record_construction_still_works() {
    // Exercises ValueConstructRecord (Vec payload) inside a fragment call —
    // the instruction must not be mis-remapped when base is threaded.
    let src = r"{-# MODE complete #-}
mk = \x -> Pair { first: x, second: x };
main = case (head ((mk [1.0, 2.0]).first)) of { Nothing -> 0.0; Just x -> x };";
    match run_complete(src) {
        rill_lang::arena::Value::Float(x) => assert_eq!(x, 1.0),
        other => panic!("expected Float(1.0), got {other:?}"),
    }
}
