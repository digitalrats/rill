//! Maybe/Pair/Either as builtin data types; nullary constructor support.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn nothing_and_just_via_match() {
    let mut prog =
        compile::<f32>("main = match (Nothing) of { Nothing => 42.0; Just x => x; };").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(42.0)
    );
}

#[test]
fn user_nullary_constructors_work() {
    let mut prog = compile::<f32>(
        "data Color = Red | Green Float; main = match (Red) of { Red => 1.0; Green g => g; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(1.0)
    );
}

#[test]
fn pair_projection_and_either() {
    let mut prog = compile::<f32>("p = Pair { first: 2.0, second: 3.0 }; main = p.first;").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(2.0)
    );
}

#[test]
fn pair_heterogeneous_fields() {
    let mut prog = compile::<f32>("p = Pair { first: 1.0, second: 2 }; main = p.second;").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    // p.second : Int(2) — heterogeneous Pair fields must typecheck.
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(2)
    );
}

#[test]
fn either_ctors_and_match() {
    let mut prog =
        compile::<f32>("main = match (Left 5.0) of { Left x => x; Right y => 0.0; };").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(5.0)
    );
}

#[test]
fn right_ctor_places_payload_in_second_slot() {
    // `Right b` binds the payload to the second type-var slot of `Either a b`.
    let mut prog =
        compile::<f32>("main = match (Right 7.0) of { Left x => 0.0; Right y => y; };").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(7.0)
    );
}
