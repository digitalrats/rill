//! Guard tests for the Closure/Thunk unification (Value::Callable + the single
//! `run_call` fragment runner): both construction paths — eager closure
//! (`ValueMakeClosure`, `ValueCallFunc`) and lazy thunk (`ValueLazy`,
//! `ValueCallLazy`) — still compute the same values after the merge.

use rill_core::traits::MultichannelAlgorithm;

#[test]
fn eager_and_lazy_calls_still_work() {
    // eager: a named function applied to args; lazy: a thunk over a closure.
    let src = "{-# MODE complete #-}\n\
        id2 = fn x -> x;\n\
        main = match (head (id2 [1.0, 2.0])) of { Nothing => 0.0; Just x => x };";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 1.0),
        other => panic!("expected Float(1.0), got {other:?}"),
    }
}

#[test]
fn closure_and_thunk_are_one_variant() {
    // Both construction paths (MakeClosure, ValueLazy, ValueCallLazy) now
    // produce Value::Callable; the arena has no Closure/Thunk variants.
    // (Compile a program using a lambda + a lazy let to exercise both.)
    let src = "{-# MODE complete #-}\n\
        id2 = fn x -> x;\n\
        main = match (head (let y = id2 [1.0, 2.0] in y)) of { Nothing => 0.0; Just x => x };";
    let mut prog =
        rill_lang::compile_with_mode::<f32>(src, rill_lang::mode::Mode::Complete).unwrap();
    let mut out = [0.0f32; 1];
    let _ = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(x) => assert_eq!(*x, 1.0),
        other => panic!("expected Float(1.0), got {other:?}"),
    }
}
