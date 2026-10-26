//! Argument checking at a user-definition call.
//!
//! Haskell 2010 §4.1.3 requires an application's argument type to match the
//! parameter type, so `f x = x + 1.0; f "str"` is a type error. Builtin calls
//! already enforced this; user definitions were the one hole, accepting a
//! `String` for a `Double` parameter and only failing later (or not at all).
//!
//! Each test asserts a computed value or a rejection, never a bare `is_ok()`:
//! a check that rejects everything would satisfy a bare `is_err()`, and one
//! that accepts everything would satisfy a bare `is_ok()`.

pub mod common;

use common::*;
use rill_lang::arena::Value;
use rill_lang::error::CompileError;

/// Evaluate the `main` expression of `src` and return its value.
///
/// `src` must bind `main` to the expression under test; the definitions before
/// it are handed to [`eval_ffi`] as the definition group.
fn run_value(src: &str) -> Value {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = rill_lang::ffi::ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).expect("the program must compile and run");
    let v = prog.result().expect("the run must produce a single result");
    prog.arena().get(v).unwrap().clone()
}

/// Compile `src` and return the rejection message, failing the test if the
/// program is accepted.
fn rejection(src: &str) -> String {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = rill_lang::ffi::ForeignRegistry::<f32>::new();
    match eval_ffi(defs, expr, ffi) {
        Err(e) => match e {
            CompileError::Type { msg, .. } => msg,
            other => panic!("expected a type error, got {other:?}"),
        },
        Ok(_) => panic!("the program was accepted but must be rejected: {src}"),
    }
}

/// `main`'s value as a number: value arithmetic preserves the `Int` track
/// (`1 + 2` is `Int(3)`), so the assertion compares through `f64`.
fn run_num(src: &str) -> f64 {
    match run_value(src) {
        Value::Double(x) => x,
        Value::Int(i) => i as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

#[test]
fn a_string_argument_to_a_float_parameter_is_rejected() {
    // The defect from the brief, reduced: `f`'s parameter is pinned to `Double`
    // by its body (`x + 1.0`), and a `String` literal is passed instead.
    let msg = rejection(r#"f x = x + 1.0; main = f "str";"#);
    assert!(
        msg.contains("`f`"),
        "the error must name the invoked definition: {msg}"
    );
    assert!(
        msg.contains("argument 1"),
        "the error must name the argument position: {msg}"
    );
    assert!(
        msg.contains("String") && msg.contains("Double"),
        "the error must name both types: {msg}"
    );
}

#[test]
fn a_float_argument_to_an_int_parameter_is_rejected() {
    // An explicit `Int -> Int` signature is the call-site contract: a `Double`
    // literal does not inhabit `Int` (there is no `fromRational` narrowing).
    let msg = rejection(r#"f x :: Int -> Int = x; main = f 1.5;"#);
    assert!(
        msg.contains("`f`") && msg.contains("argument 1"),
        "the error must name the definition and the position: {msg}"
    );
    assert!(
        msg.contains("Double") && msg.contains("Int"),
        "the error must name both types: {msg}"
    );
}

#[test]
fn a_matching_argument_is_accepted() {
    // The same definition applied at its own type still computes: the check
    // rejects the mismatch without disturbing a well-typed call.
    assert_eq!(
        run_num(r#"f x = x + 1.0; main = f 1.0;"#),
        2.0,
        "a matching Double argument must run to 2.0"
    );
    // An explicit signature, an Int-literal argument (`fromInteger`-
    // polymorphic) and a repeated application all stay accepted.
    assert_eq!(
        run_num(r#"f x :: Double -> Double = x + 1.0; main = f (f 1.0);"#),
        3.0,
        "a nested matching call must run to 3.0"
    );
    assert_eq!(
        run_num(r#"f n :: Int -> Int = n + 1; main = f 1;"#),
        2.0,
        "an Int literal argument must be accepted by an Int parameter"
    );
}

#[test]
fn an_unresolved_argument_defers() {
    // `id2 x = x` is `a -> a` (Haskell 2010 §4.1.2), so the formal parameter is
    // an unresolved type variable and the check must DEFER: `id2 "str"` is
    // valid Haskell and must keep compiling and evaluating to the string.
    assert_eq!(
        run_value(r#"id2 x = x; main = id2 "str";"#),
        Value::String("str".into()),
        "a polymorphic definition must accept a String argument"
    );
    // The same holds for a body-constrained-but-still-polymorphic definition
    // (`length xs` leaves the element type open) and for a constrained
    // signature (`a -> a`).
    assert_eq!(
        run_num(r#"len xs = length xs; main = len ["a", "b"];"#),
        2.0,
        "a List-parameter definition must accept a List of Strings"
    );
    assert_eq!(
        run_value(r#"id3 x :: a -> a = x; main = id3 "str";"#),
        Value::String("str".into()),
        "a forall-constrained `a -> a` definition must stay polymorphic"
    );
    // Polymorphism survives instantiation at DIFFERENT types in ONE program:
    // the check must not pin the definition to the first call site's type.
    // (`if id2 true …` cannot express this — a pre-existing limitation of
    // `if`-condition typing, unrelated to argument checking.)
    assert_eq!(
        run_value(r#"id2 x = x; main = concat_string (id2 "a") (show_double (id2 1.0));"#),
        Value::String("a1.0".into()),
        "the same definition must instantiate at String and Double in one program"
    );
}

#[test]
fn higher_order_argument_is_checked() {
    // A function-typed parameter is checked structurally: `g : Double -> Double`
    // cannot be handed `\s -> show_double s` (a `Double -> String`). Both sides
    // of the arrow are compared, not just the fact that both are functions.
    let msg = rejection(
        r#"f :: (Double -> Double) -> Double; f = \g -> g 1.0; main = f (\s -> show_double s);"#,
    );
    assert!(
        msg.contains("`f`") && msg.contains("argument 1"),
        "the error must name the definition and the position: {msg}"
    );
    assert!(
        msg.contains("Double -> String") && msg.contains("Double -> Double"),
        "the error must spell out both function types: {msg}"
    );
    // The mirror image: a `Double -> String` parameter handed a
    // `Double -> Double` closure.
    let msg = rejection(
        r#"f :: (Double -> String) -> String; f = \g -> g 1.0; main = f (\s -> s + 1.0);"#,
    );
    assert!(
        msg.contains("Double -> Double") && msg.contains("Double -> String"),
        "the error must spell out both function types: {msg}"
    );
    // A MATCHING higher-order argument still runs: the same wrapper applied to
    // a `Double -> Double` closure computes 4.0.
    assert_eq!(
        run_num(r#"f :: (Double -> Double) -> Double; f = \g -> g 1.0; main = f (\x -> x * 4.0);"#),
        4.0,
        "a matching higher-order argument must run to 4.0"
    );
    // And the function-typed parameter's own RESULT is part of the contract.
    assert_eq!(
        run_value(
            r#"f :: (Double -> String) -> String; f = \g -> g 2.0;
               main = f (\x -> concat_string (show_double x) "!");"#
        ),
        Value::String("2.0!".into()),
        "a matching String-returning closure must run"
    );
}
