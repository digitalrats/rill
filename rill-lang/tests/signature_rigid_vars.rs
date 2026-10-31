//! A `::` signature declares a type per NAME, and a repeated letter is the only
//! relation between positions (Haskell 2010 §4.4.1.2).
//!
//! GHC 9.10.3, compiled module (GHCi's `ExtendedDefaultRules` default the
//! display and hide the difference), for `f x y = x`:
//!
//! | signature | GHC |
//! |---|---|
//! | `a -> b -> a` | accepts — the result IS the first parameter |
//! | `a -> a -> a` | accepts — one variable in all three positions |
//! | `a -> a -> b` | REJECTS — `b` is not what the body returns |
//! | `a -> b -> c` | REJECTS — `c` is a third, unrelated variable |
//!
//! and for `f x y = y`: `a -> a -> a` accepts, `a -> b -> a` REJECTS.
//!
//! The distinction is why a signature variable must be RIGID while its body is
//! checked: union-find alone unifies any two variables, so `c := a` made every
//! one of those programs compile.

mod common;

use common::*;

fn accepts(src: &str) -> bool {
    rill_lang::compile::<f32, 256>(src, rill_lang::ffi::ForeignRegistry::new()).is_ok()
}

fn prog(sig: &str, body: &str) -> String {
    format!(
        "f :: {sig}; f x y = {body};\n\
         main = do {{ write (Just (show_double (f 1.0 2.0))) stdout; return () }};"
    )
}

#[test]
fn a_distinct_result_variable_is_rejected() {
    assert!(
        !accepts(&prog("a -> b -> c", "x")),
        "`c` is a third variable the body does not produce"
    );
    assert!(
        !accepts(&prog("a -> a -> b", "x")),
        "`b` is not what the body returns"
    );
}

#[test]
fn a_repeated_variable_is_the_same_variable() {
    assert!(accepts(&prog("a -> a -> a", "x")));
    assert!(accepts(&prog("a -> a -> a", "y")));
    assert!(accepts(&prog("a -> b -> a", "x")));
}

#[test]
fn the_second_parameter_is_its_own_variable() {
    assert!(
        !accepts(&prog("a -> b -> a", "y")),
        "`y` has type `b`, and the signature declares the result is `a`"
    );
}
