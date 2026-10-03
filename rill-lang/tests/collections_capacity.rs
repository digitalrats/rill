//! Arena capacity accounting for container-typed value subexpressions.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn container_outputs_pin_subtrees_across_ticks() {
    // A List output pins its full subtree across ticks; the arena bound must
    // include cap * elem slots or tick 2 exhausts the arena.
    let mut prog = compile::<f32>("main = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];").unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert!(matches!(
            prog.arena().get(v).unwrap(),
            &rill_lang::arena::Value::List { .. }
        ));
    }
}

#[test]
fn map_result_capacity_is_accounted() {
    // `map f xs` allocates cap(xs) element slots + the result container. The
    // result type must carry the SOURCE cap (not Cap(0)) so the arena bound
    // is exact. A 20-element map must not exhaust the arena.
    let mut prog = compile::<f32>(
        "xs = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0, 20.0]; main = length (map (fn x -> x * 2.0) xs);",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..2 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert_eq!(
            prog.arena().get(v).unwrap(),
            &rill_lang::arena::Value::Int(20)
        );
    }
}

#[test]
fn map_build_and_pin_across_ticks() {
    // Two inserts COW-build a map from an empty_map of spare capacity; the map
    // is rebuilt each tick, so its key/value slots must survive the tick-end
    // clear and stay addressable on the next tick.
    let mut prog = compile::<f32>(
        "m = insert \"a\" 1.0 (insert \"x\" 2.0 (empty_map)); main = member \"x\" m;",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert_eq!(
            prog.arena().get(v).unwrap(),
            &rill_lang::arena::Value::Bool(true)
        );
    }
}
