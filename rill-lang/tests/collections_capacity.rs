//! Arena capacity accounting for container-typed value subexpressions.

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
fn map_result_capacity_is_accounted() {
    // `map f xs` allocates cap(xs) element slots + the result container. The
    // result type must carry the SOURCE cap (not Cap(0)) so the arena bound
    // is exact: a single 20-element map run must not exhaust the arena.
    let prog = run(
        r"xs = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0, 20.0]; main = length (map (\x -> x * 2.0) xs);",
    );
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(20)
    );
}
