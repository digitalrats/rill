//! Maybe/Pair/Either as builtin data types; nullary constructor support.
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;

/// Compile `src` against an empty foreign registry and RUN it once (the
/// single-run value surface), returning the already-run program.
fn run(src: &str) -> WorldState<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    eval_ffi(defs, expr, ffi).unwrap()
}

#[test]
fn nothing_and_just_via_match() {
    let prog = run(r"main = case (Nothing) of { Nothing -> 42.0; Just x -> x; };");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(42.0)
    );
}

#[test]
fn user_nullary_constructors_work() {
    let prog = run(
        r"data Color = Red | Green Double; main = case (Red) of { Red -> 1.0; Green g -> g; };",
    );
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(1.0)
    );
}

#[test]
fn pair_projection_and_either() {
    let prog = run("p = Pair { first: 2.0, second: 3.0 }; main = p.first;");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(2.0)
    );
}

#[test]
fn pair_heterogeneous_fields() {
    let prog = run("p = Pair { first: 1.0, second: 2 }; main = p.second;");
    let v = prog.result().unwrap();
    // `p.second : Integer` — heterogeneous Pair fields must typecheck, and an
    // unconstrained literal defaults along `default (Integer, Double)` to
    // `Integer` (GHC 9.10.3 measures `p :: (Double, Integer)`).
    assert_eq!(integral(prog.arena().get(v).unwrap()), 2);
}

#[test]
fn either_ctors_and_match() {
    let prog = run(r"main = case (Left 5.0) of { Left x -> x; Right y -> 0.0; };");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(5.0)
    );
}

#[test]
fn right_ctor_places_payload_in_second_slot() {
    // `Right b` binds the payload to the second type-var slot of `Either a b`.
    let prog = run(r"main = case (Right 7.0) of { Left x -> 0.0; Right y -> y; };");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(7.0)
    );
}

#[test]
fn nullary_match_is_arm_order_independent() {
    // Nothing matched with the Just arm FIRST must still select Nothing.
    let prog = run(r"main = case (Nothing) of { Just x -> x; Nothing -> 42.0; };");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(42.0)
    );
}

#[test]
fn lookup_key_via_let_binding_is_not_statically_misjudged() {
    // The static lookup analysis must NOT claim the key is absent when it
    // comes through a let-bound variable (would silently pick Nothing).
    let prog = run(
        r#"k = "a"; m = insert k 5.0 (empty_map); main = case (lookup "a" m) of { Nothing -> 0.0; Just x -> x; };"#,
    );
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(5.0)
    );
}

#[test]
fn match_builtin_sum_with_compound_payload_projects() {
    let prog = run(
        r"main = case (Just (Pair { first: 1.0, second: 2.0 })) of { Just p -> p.first; Nothing -> 0.0; };",
    );
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(1.0)
    );
}
