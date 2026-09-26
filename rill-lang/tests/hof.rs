//! Higher-order function combinators: functions applied to functions,
//! static call-depth rejection of recursion, and fixed-shape container mapping.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn twice_applies_twice() {
    // twice = fn f x -> f (f x);  double = fn x -> x * 2.0;  main = twice double 3.0 -> 12.0
    let mut prog = compile::<f32>(
        "twice = fn f x -> f (f x); double = fn x -> x * 2.0; main = twice double 3.0",
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
fn recursion_is_rejected() {
    // f = fn x -> f x  -> compile error (acyclic contract)
    let res = compile::<f32>("f = fn x -> f x; main = f 1.0");
    assert!(res.is_err());
}

#[test]
fn mutual_recursion_is_rejected() {
    // g calls h and h calls g: a static cycle in the fragment call graph.
    let res = compile::<f32>("g = fn n -> h n; h = fn m -> g m; main = g 1.0");
    assert!(res.is_err());
}

#[test]
fn omega_combinator_is_compile_error() {
    // make = fn f -> f f  — the omega combinator: transitively self-recursive
    // through its parameter (no NAMED recursion, not even a call). The acyclic
    // contract must reject it with a clean compile error, never a compiler
    // crash (regression: self-application cycled the value unification and
    // overflowed the stack).
    let res = compile::<f32>("make = fn f -> f f; main = 1.0");
    assert!(res.is_err());
}

#[test]
fn map_combinator_over_fixed_shape() {
    // map over a 2-field record: apply f to each element -> a 2-field result.
    // pair_map = fn f p -> Pair (f p.x) (f p.y)
    let mut prog = compile::<f32>(
        "data Pair = { x: Float, y: Float };
         pair_map = fn f p -> Pair { x: f p.x, y: f p.y };
         double = fn x -> x * 2.0;
         main = pair_map double (Pair { x: 1.0, y: 2.0 })",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Record(fields) => {
            assert_eq!(fields.len(), 2);
            let x = prog.arena().get(fields[0]).unwrap();
            let y = prog.arena().get(fields[1]).unwrap();
            assert_eq!(x, &rill_lang::arena::Value::Float(2.0));
            assert_eq!(y, &rill_lang::arena::Value::Float(4.0));
        }
        other => panic!("expected Record, got {other:?}"),
    }
}

#[test]
fn compose_three_times_does_not_exhaust_arena() {
    // Deep HOF nesting across ticks must stay within the static capacity bound.
    let mut prog = compile::<f32>(
        "apply3 = fn f x -> f (f (f x)); inc = fn x -> x + 1.0; main = apply3 inc 0.0",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..10 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    }
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(3.0)
    );
}
