//! Maybe/Pair/Either as builtin data types; nullary constructor support.

use rill_lang::compile;

#[test]
fn nothing_and_just_via_match() {
    let mut prog =
        compile::<f32>(r"main = case (Nothing) of { Nothing -> 42.0; Just x -> x; };").unwrap();
    prog.run_value().unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(42.0)
    );
}

#[test]
fn user_nullary_constructors_work() {
    let mut prog = compile::<f32>(
        r"data Color = Red | Green Float; main = case (Red) of { Red -> 1.0; Green g -> g; };",
    )
    .unwrap();
    prog.run_value().unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(1.0)
    );
}

#[test]
fn pair_projection_and_either() {
    let mut prog = compile::<f32>("p = Pair { first: 2.0, second: 3.0 }; main = p.first;").unwrap();
    prog.run_value().unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(2.0)
    );
}

#[test]
fn pair_heterogeneous_fields() {
    let mut prog = compile::<f32>("p = Pair { first: 1.0, second: 2 }; main = p.second;").unwrap();
    prog.run_value().unwrap();
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
        compile::<f32>(r"main = case (Left 5.0) of { Left x -> x; Right y -> 0.0; };").unwrap();
    prog.run_value().unwrap();
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
        compile::<f32>(r"main = case (Right 7.0) of { Left x -> 0.0; Right y -> y; };").unwrap();
    prog.run_value().unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(7.0)
    );
}

#[test]
fn nullary_match_is_arm_order_independent() {
    // Nothing matched with the Just arm FIRST must still select Nothing.
    let mut prog =
        compile::<f32>(r"main = case (Nothing) of { Just x -> x; Nothing -> 42.0; };").unwrap();
    prog.run_value().unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(42.0)
    );
}

#[test]
fn lookup_key_via_let_binding_is_not_statically_misjudged() {
    // The static lookup analysis must NOT claim the key is absent when it
    // comes through a let-bound variable (would silently pick Nothing).
    let mut prog = compile::<f32>(
        r#"k = "a"; m = insert k 5.0 (empty_map); main = case (lookup "a" m) of { Nothing -> 0.0; Just x -> x; };"#,
    )
    .unwrap();
    prog.run_value().unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(5.0)
    );
}

#[test]
fn match_builtin_sum_with_compound_payload_projects() {
    let mut prog = compile::<f32>(
        r"main = case (Just (Pair { first: 1.0, second: 2.0 })) of { Just p -> p.first; Nothing -> 0.0; };",
    )
    .unwrap();
    prog.run_value().unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(1.0)
    );
}
