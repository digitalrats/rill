//! Currying (partial application via closures) and signal-wire arguments.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn currying_applies_partially() {
    // add = fn a b -> a + b;  add3 = add 3.0;  main = add3 4.0 -> 7.0
    let mut prog =
        compile::<f32>("add = fn a b -> a + b; add3 = add 3.0; main = add3 4.0").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(7.0)
    );
}

#[test]
fn currying_named_function() {
    // A NAMED def also curries: add2 a b = a + b; add5 = add2 5.0; main = add5 2.0 -> 7.0
    let mut prog = compile::<f32>("add2 a b = a + b; add5 = add2 5.0; main = add5 2.0").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(7.0)
    );
}

#[test]
fn signal_wire_argument_scales_block() {
    // amp = fn g x -> x * g;  main = amp 2.0 _  ->  input * 2.0  (block output)
    let mut prog = compile::<f32>("amp = fn g x -> x * g; main = amp 2.0 _").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0, 2.0, 3.0, 4.0]], &mut [&mut out]).unwrap();
    assert_eq!(out, [2.0, 4.0, 6.0, 8.0]);
}

#[test]
fn chained_currying_dispatches_without_leak() {
    // add = fn a b c -> a + b + c; a1 = add 1.0; a2 = a1 2.0; main = a2 3.0 -> 6.0
    // Repeated dispatch through nested curry closures must not exhaust the
    // fixed arena (each curry call allocates cells + scratch; all released on
    // return).
    let mut prog =
        compile::<f32>("add = fn a b c -> a + b + c; a1 = add 1.0; a2 = a1 2.0; main = a2 3.0")
            .unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..10 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    }
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(6.0)
    );
    assert!(prog.arena().live() < 64);
}

#[test]
fn partial_application_captures_free_variable() {
    // k = 10.0; add = fn a b -> a + b + k; addk = add 1.0; main = addk 2.0 -> 13.0
    // The curry env holds the function AND the applied arg; the lambda's own
    // env holds k.
    let mut prog =
        compile::<f32>("k = 10.0; add = fn a b -> a + b + k; addk = add 1.0; main = addk 2.0")
            .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(13.0)
    );
}

#[test]
fn caf_applied_at_two_sites_with_different_wires() {
    // The value-CAF `amp2 = amp 2.0` applied to two DIFFERENT signal wires
    // (`main = (amp2 _) , (amp2 _)` is a 2-in/2-out program): each call site
    // must lower with ITS OWN wire, not reuse the first site's lowered body
    // (regression: the CAF cache was keyed by name only).
    let mut prog =
        compile::<f32>("amp = fn g x -> x * g; amp2 = amp 2.0; main = (amp2 _) , (amp2 _)")
            .unwrap();
    let mut l = [0.0f32; 4];
    let mut r = [0.0f32; 4];
    MultichannelAlgorithm::process(
        &mut prog,
        &[&[1.0, 2.0, 3.0, 4.0], &[3.0, 4.0, 5.0, 6.0]],
        &mut [&mut l, &mut r],
    )
    .unwrap();
    assert_eq!(l, [2.0, 4.0, 6.0, 8.0], "left  = input[0] * 2.0");
    assert_eq!(r, [6.0, 8.0, 10.0, 12.0], "right = input[1] * 2.0");
}
