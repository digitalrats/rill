//! `forall` and the monomorphism restriction (Haskell 2010 §4.5.5).
//!
//! The Report's restriction applies to a binding WITHOUT an explicit signature.
//! An annotation is the programmer SAYING what the type is, so generalising it
//! needs no permission — and a signed binding therefore stays polymorphic until
//! a POSITION instantiates it.
//!
//! Measured with GHC 9.10.3 (`-ddump-types`), which is what each case below
//! pins:
//!
//! | binding | GHC |
//! |---|---|
//! | `bare = 1` | `Integer` — the restriction applies |
//! | `bare = 1` used at `bare + 1.5` | `Double` — ONE shared variable, and `Fractional` rules out `Integer` |
//! | `h :: Num b => b; h = 1` | `forall b. Num b => b` |
//! | `h :: Double; h = 1` | `Double` — the signature is monomorphic |
//!
//! The `h` row is the one that regressed: the restriction was applied to a
//! SIGNED binding, so `h` became a shared monomorphic variable and defaulted to
//! `Integer`, making `(h :: Integer, h :: Double)` — legal in one GHC program —
//! a rejection here.

mod common;

use common::*;

/// Compile and run `src`, returning the concatenated stdout.
fn run(src: &str) -> String {
    let mut prog = rill_lang::compile::<f32, 256>(src, rill_lang::ffi::ForeignRegistry::new())
        .unwrap_or_else(|e| panic!("compile failed for:\n{src}\n{e}"));
    prog.run()
        .unwrap_or_else(|e| panic!("run failed for:\n{src}\n{e}"));
    prog.stdout().concat()
}

#[test]
fn a_signed_nullary_binding_stays_polymorphic() {
    // GHC: `h :: forall b. Num b => b`, and BOTH instantiations work in ONE
    // program. Defaulting `h` to `Integer` would reject the second.
    assert_eq!(
        run("h :: Num b => b;\nh = 1;\nmain = do { write (Just (show_integer (h :: Integer))) stdout; write (Just (show_double (h :: Double))) stdout; return () };"),
        "11.0"
    );
    // The same with an EXPLICIT `forall`: the binder spelling must not matter.
    assert_eq!(
        run("h :: forall b. Num b => b;\nh = 1;\nmain = do { write (Just (show_double (h :: Double))) stdout; return () };"),
        "1.0"
    );
}

#[test]
fn a_signed_function_stays_polymorphic() {
    // GHC: `f :: forall b. Num b => b -> b`; applied at two types.
    assert_eq!(
        run("f :: Num b => b -> b;\nf x = x;\nmain = do { write (Just (show_integer (f 1))) stdout; write (Just (show_double (f 1.5))) stdout; return () };"),
        "11.5"
    );
}

#[test]
fn the_restriction_still_applies_to_an_UNSIGNED_binding() {
    // GHC: `bare :: Integer` — no signature, so the constrained variable is NOT
    // generalised and stage 3's default decides it.
    assert_eq!(
        run("bare = 1;\nmain = do { write (Just (show_integer (bare + 1))) stdout; return () };"),
        "2"
    );
    // ONE shared variable, and `Fractional` from the literal rules `Integer`
    // out, so the SAME binding is `Double` here. GHC measures `bare :: Double`
    // and answers `2.5`: this is not a widening, it is the restriction keeping
    // one variable the whole group agrees on.
    assert_eq!(
        run("bare = 1;\nmain = do { write (Just (show_double (bare + 1.5))) stdout; return () };"),
        "2.5"
    );
}

#[test]
fn a_monomorphic_signature_stays_monomorphic() {
    // GHC: `h :: Double`. The signature is not a licence to generalise: it
    // fixes the type, and the literal takes it through `fromInteger`.
    assert_eq!(
        run("h :: Double;\nh = 1;\nmain = do { write (Just (show_double h)) stdout; return () };"),
        "1.0"
    );
}
