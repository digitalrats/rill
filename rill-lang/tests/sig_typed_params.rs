//! Signature-driven typing for ORDINARY (unconstrained) definitions: a `::`
//! annotation types a definition's parameters, and the definition's scheme
//! carries those same types — so a call site and the lowered parameter
//! registers agree.
//!
//! The parser curries every multi-parameter definition into a λ-nest and clears
//! `Def::Eval::params`, so an annotated `f a b :: Int -> Int -> Int` recovers
//! its formal parameters from the signature's arity. Without that recovery the
//! parameters fell back to the scalar `Float` default: the `Int` call site
//! could not type-check, and a `Float` scheme contradicted an `Int`-typed body.
pub mod common;

use common::*;
use rill_lang::arena::Value;

/// Compile and evaluate the `main` expression of `src` in a single run,
/// returning its value.
fn run_value(src: &str) -> Value {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = rill_lang::ffi::ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let v = prog.result().expect("the run must produce a single result");
    prog.arena().get(v).unwrap().clone()
}

/// Compile and run `src`, returning `main`'s value as a number. Value
/// arithmetic preserves the `Int` track (`1 + 2` is `Int(3)`, not `Float(3.0)`),
/// so an assertion compares through `f64` rather than pinning one spelling.
fn run_num(src: &str) -> f64 {
    match run_value(src) {
        Value::Float(x) => x,
        Value::Int(i) => i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

#[test]
fn multi_parameter_signature_types_every_parameter() {
    // `a` and `b` come from the signature, not from the `Float` default: the
    // `Int` literals at the call site only unify if the scheme really is
    // `Int -> Int -> Int`.
    let src = "f a b :: Int -> Int -> Int = a + b;\nmain = f 1 2;";
    assert_eq!(run_num(src), 3.0);
}

#[test]
fn multi_parameter_signature_drives_the_body_too() {
    // A non-scalar parameter type proves the signature reaches the BODY's
    // typing, not just the scheme: `show_int n` and `concat_string` only
    // type-check when `s` is bound as a `String`.
    let src = "join3 s n :: String -> Int -> String = concat_string s (show_int n);\nmain = join3 \"x\" 7;";
    assert_eq!(run_value(src), Value::String("x7".into()));
}

#[test]
fn single_parameter_signature_scheme_matches_the_body() {
    // The scheme used to be `Float -> Int` while the body was checked with
    // `x : Int` — the call site was typed by `Float` and the lowered parameter
    // register by `Int`. `n` is a closed `Int` value (a `length` result);
    // unlike an integer literal sitting in an expected-`Float` position it has
    // no coercion to hide the mismatch.
    let src = "n = length [1.0, 2.0];\ninc x :: Int -> Int = x + 1;\nmain = inc n;";
    assert_eq!(run_num(src), 3.0);
}

#[test]
fn shared_type_variable_keeps_its_identity_across_the_scheme() {
    // `a` in the parameter position and `a` in the result are the SAME
    // signature variable: the scheme must not generalize them independently
    // (which would let `pick` return a type unrelated to its argument) while
    // still instantiating afresh at every call site.
    let src = "pick x :: a -> a = x;\nmain = pick 1 + pick 2;";
    assert_eq!(run_num(src), 3.0);
}

#[test]
fn signature_less_accessor_call_names_the_missing_annotation() {
    // PARK M10: `at a i = a ! i` is a signature-less CURRIED accessor (its body
    // is a λ-nest), so it types as a bare `Thunk([],[])`: the structured
    // parameter type `a : Array i e` is recovered from the constrained `index`
    // inside the λ-body but never reaches the scheme. A call with a bound
    // pair — `at (0, 9) 9` — reports an opaque `cannot unify Pair … with Array`
    // instead of pointing at the cause. The diagnostic must name `at` and
    // suggest annotating the parameter's type.
    let (defs, expr) = ("at a i = a ! i;", "at (0, 9) 9");
    let ffi = rill_lang::ffi::ForeignRegistry::<f32>::new();
    let msg = match eval_ffi(defs, expr, ffi) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("the signature-less accessor call must fail to compile"),
    };
    assert!(
        msg.contains("annotate the parameter's type"),
        "the diagnostic must suggest the missing annotation: {msg}"
    );
    assert!(
        msg.contains("`at`"),
        "the diagnostic must name the definition: {msg}"
    );

    // The advice is actionable: the same accessor with a signature compiles
    // and reads the array element.
    let src = "at a i :: Array Int Float -> Int -> Float = a ! i;\n\
               main = at (listArray (Pair { first: 0, second: 2 }) [1.0, 2.0, 3.0]) 1;";
    assert_eq!(run_value(src), Value::Float(2.0));
}
