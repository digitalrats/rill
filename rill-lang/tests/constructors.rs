//! A constructor is an ordinary curried function (Haskell 2010 §4.2.1):
//! `Just :: a -> Maybe a`, so it is usable as a VALUE and partially applicable.
//!
//! `GHC 9.10.3` accepts every program asserted here:
//!
//! ```haskell
//! g = Just            -- 5
//! map Just [1,2,3]    -- [Just 1, Just 2, Just 3]
//! x = Just 1 2        -- type error: `Just 1` is not a function
//! ```
//!
//! A NULLARY constructor is a plain value (`Nothing :: Maybe a`), and a
//! user-declared sum constructor behaves exactly like a builtin one.

pub mod common;

use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;

/// Compile `src` and run its `main` `IO` action once, returning the
/// concatenated stdout.
fn run(src: &str) -> String {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    let mut prog =
        rill_lang::compile::<f32, 256>(src, ffi).unwrap_or_else(|e| panic!("compile failed: {e}"));
    prog.run().unwrap_or_else(|e| panic!("run failed: {e}"));
    prog.stdout().concat()
}

/// Compile `src`, expecting the type checker to reject it, and return the
/// diagnostic.
fn compile_err(src: &str) -> String {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    match rill_lang::compile::<f32, 256>(src, ffi) {
        Ok(_) => panic!("the program must be rejected: {src}"),
        Err(e) => format!("{e}"),
    }
}

#[test]
fn a_constructor_is_usable_as_a_value() {
    // GHC: `map Just [1,2,3]` is `[Just 1, Just 2, Just 3]`. The assertion is
    // the list LENGTH, so a broken `Just` (a nullary constructor, or an arity
    // error) cannot satisfy it.
    assert_eq!(
        run("main = do { write (Just (show_int (length (map Just [1, 2, 3])))) stdout; return () };"),
        "3"
    );
    // The same constructor bound to a name is the same curried value.
    assert_eq!(
        run("g = Just;\nmain = do { write (Just (show_int (length (map g [1, 2, 3])))) stdout; return () };"),
        "3"
    );
}

#[test]
fn a_nullary_constructor_is_usable_as_a_value() {
    // GHC: `Nothing` is a value of `Maybe a`, not a function.
    assert_eq!(
        run(
            "xs = [Nothing];\nmain = do { write (Just (show_int (length xs))) stdout; return () };"
        ),
        "1"
    );
}

#[test]
fn a_user_sum_constructor_is_usable_as_a_value() {
    // A declared sum behaves exactly like a builtin one: `Some :: a -> Opt a`,
    // so `map Some` is a list of `Opt a`.
    assert_eq!(
        run("data Opt a = Some a | None;\n\
             f = Some;\n\
             main = do { write (Just (show_int (length (map f [1.0, 2.0])))) stdout; return () };"),
        "2"
    );
}

#[test]
fn a_constructor_checks_its_arity() {
    // GHC: `Just 1 2` is a type error — `Just 1` is not a function.
    let msg = compile_err("x = Just 1 2;\nmain = 0.0;");
    assert!(
        msg.contains("expects") && msg.contains("constructor `Just`"),
        "the error must name the constructor's arity: {msg}"
    );
    // And a partial application still yields a usable value: `Just 1.0` is
    // `Maybe Double`.
    assert!(rill_lang::compile_eval::<f32, 256>(
        "main = Just 1.0;",
        "main",
        rill_lang::mode::Mode::Strict
    )
    .is_ok());
}

#[test]
fn a_builtin_sig_is_a_curried_value() {
    // GHC accepts `w = write (Just "x")` — a builtin's signature is a curried
    // function type, so supplying one of its two arguments is a partial
    // application, not an arity error.
    assert_eq!(
        run("w = write (Just \"x\");\nmain = do { w stdout; return () };"),
        "x"
    );
}
