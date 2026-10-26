//! Higher-order function combinators: functions applied to functions,
//! static call-depth rejection of recursion, and fixed-shape container mapping.
pub mod common;

use common::*;
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;

/// Compile `src` (which must bind `main` to the evaluated expression) and RUN
/// it once on the single-run value surface, returning the already-run program.
fn run(src: &str) -> WorldState<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    eval_ffi(defs, expr, ffi).unwrap()
}

#[test]
fn twice_applies_twice() {
    // twice = fn f x -> f (f x);  double = fn x -> x * 2.0;  main = twice double 3.0 -> 12.0
    let prog = run(r"twice = \f -> \x -> f (f x); double = \x -> x * 2.0; main = twice double 3.0");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(12.0)
    );
}

#[test]
fn recursion_is_rejected() {
    // f = fn x -> f x  -> compile error (acyclic contract)
    let res = compile_eval::<f32, 256>(r"f = \x -> f x; main = f 1.0", "main", Mode::Strict);
    assert!(res.is_err());
}

#[test]
fn mutual_recursion_is_rejected() {
    // g calls h and h calls g: a static cycle in the fragment call graph.
    let res = compile_eval::<f32, 256>(
        r"g = \n -> h n; h = \m -> g m; main = g 1.0",
        "main",
        Mode::Strict,
    );
    assert!(res.is_err());
}

#[test]
fn omega_combinator_is_compile_error() {
    // make = fn f -> f f  — the omega combinator: transitively self-recursive
    // through its parameter (no NAMED recursion, not even a call). The acyclic
    // contract must reject it with a clean compile error, never a compiler
    // crash (regression: self-application cycled the value unification and
    // overflowed the stack).
    let res = compile_eval::<f32, 256>(r"make = \f -> f f; main = 1.0", "main", Mode::Strict);
    assert!(res.is_err());
}

#[test]
fn map_combinator_over_fixed_shape() {
    // map over a 2-field record: apply f to each element -> a 2-field result.
    // pair_map = fn f p -> Pair (f p.x) (f p.y)
    let prog = run(r"data Pair = { x: Double, y: Double };
         pair_map = \f -> \p -> Pair { x: f p.x, y: f p.y };
         double = \x -> x * 2.0;
         main = pair_map double (Pair { x: 1.0, y: 2.0 })");
    let v = prog.result().unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Record(fields) => {
            assert_eq!(fields.len(), 2);
            let x = prog.arena().get(fields[0]).unwrap();
            let y = prog.arena().get(fields[1]).unwrap();
            assert_eq!(x, &rill_lang::arena::Value::Double(2.0));
            assert_eq!(y, &rill_lang::arena::Value::Double(4.0));
        }
        other => panic!("expected Record, got {other:?}"),
    }
}

#[test]
fn compose_three_times_does_not_exhaust_arena() {
    // Deep HOF nesting must stay within the static capacity bound. The former
    // version re-ran the program across 10 ticks to probe cross-tick arena
    // accounting; the re-run surface is gone, so the idempotent re-assert loop
    // collapses to the single run (the value computation is unchanged).
    let prog = run(r"apply3 = \f -> \x -> f (f (f x)); inc = \x -> x + 1.0; main = apply3 inc 0.0");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(3.0)
    );
}
