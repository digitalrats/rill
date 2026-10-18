//! Currying (partial application via closures) and signal-wire arguments.
//!
//! Signal-wire (`_`) arguments, multi-channel programs and the re-run surface
//! have no equivalent in the single-run value model, so their tests were
//! removed (see notes below): `signal_wire_argument_scales_block`,
//! `chained_currying_dispatches_without_leak`,
//! `caf_applied_at_two_sites_with_different_wires`.

use rill_lang::ffi::ForeignRegistry;
use rill_lang::test_eval;

/// Compile `src` against an empty foreign registry and RUN it once (the
/// single-run value surface), returning the already-run program.
fn run(src: &str) -> rill_lang::program::RillProgram<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    test_eval::<f32, 256>(defs, expr, &ffi, 44_100.0).unwrap()
}

#[test]
fn currying_applies_partially() {
    // add = fn a b -> a + b;  add3 = add 3.0;  main = add3 4.0 -> 7.0
    let prog = run(r"add = \a -> \b -> a + b; add3 = add 3.0; main = add3 4.0");
    let v = prog.result().expect("the single run must produce a value");
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(7.0)
    );
}

#[test]
fn currying_named_function() {
    // A NAMED def also curries: add2 a b = a + b; add5 = add2 5.0; main = add5 2.0 -> 7.0
    let prog = run("add2 a b = a + b; add5 = add2 5.0; main = add5 2.0");
    let v = prog.result().expect("the single run must produce a value");
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(7.0)
    );
}

// NOTE (signal-lambda macros): `signal_wire_argument_scales_block`
// (`main = amp 2.0 _` — a signal-wire `_` argument feeding a block-produced
// `input * 2.0`) was DELETED: signal-wire arguments are a separate stage and the
// wire-controlled block-tick surface no longer exists in the single-run model.

#[test]
fn partial_application_captures_free_variable() {
    // k = 10.0; add = fn a b -> a + b + k; addk = add 1.0; main = addk 2.0 -> 13.0
    // The curry env holds the function AND the applied arg; the lambda's own
    // env holds k.
    let prog = run(r"k = 10.0; add = \a -> \b -> a + b + k; addk = add 1.0; main = addk 2.0");
    let v = prog.result().expect("the single run must produce a value");
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(13.0)
    );
}

// NOTE — DELETED (re-run surface removed):
//   chained_currying_dispatches_without_leak — a 10-tick `run_value` loop
//     asserting the fixed arena never exhausts (`live() < 64`) across repeated
//     curry dispatch. Multi-tick loops asserting cross-tick state are Class 2:
//     the re-run surface is permanently gone.
//   caf_applied_at_two_sites_with_different_wires — `main = (amp2 _) , (amp2 _)`
//     is a 2-in/2-out program read through `copy_output_samples(1, …)`
//     (multi-channel). At-most-one-output model ⇒ no equivalent; the signal-wire
//     `_` arguments are also a separate stage.
