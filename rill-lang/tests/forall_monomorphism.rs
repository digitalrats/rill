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

// ---------------------------------------------------------------------------
// §4.4.3.1 versus §4.5.5: what the SPELLING decides
// ---------------------------------------------------------------------------

/// Infer `src` and return the typed program, for scheme-level assertions.
fn scheme_of(src: &str) -> rill_lang::types::infer::TypedProgram {
    let toks = rill_lang::lexer::tokenize(src).expect("the source must lex");
    let mut prog = rill_lang::parser::parse(&toks, src.as_bytes()).expect("the source must parse");
    rill_lang::desugar::desugar_program(&mut prog).expect("the source must desugar");
    rill_lang::types::infer::infer_program(&prog, prog.mode).expect("the source must infer")
}

/// Locate a compile error without failing on a successful compile.
fn compile_error(src: &str) -> Option<String> {
    rill_lang::compile::<f32, 256>(src, rill_lang::ffi::ForeignRegistry::new())
        .err()
        .map(|e| e.to_string())
}

#[test]
fn a_lambda_bound_binding_is_a_pattern_binding_and_stays_monomorphic() {
    // Haskell 2010 §4.4.3.1 makes `double = \x -> x * 2.0` and
    // `double x = x * 2.0` the same VALUE, but §4.5.5 applies to a SIMPLE
    // PATTERN BINDING — no parameter list — and the λ on the right-hand side
    // does not turn it into a function binding. GHC 9.10.3, compiled module
    // (GHCi's `ExtendedDefaultRules` default the DISPLAY and hide this):
    //
    //   double = \x -> x * 2.0
    //   main = print (double (1.5 :: Double), double (1 :: Integer))
    //     → Couldn't match expected type 'Double' with actual type 'Integer'
    //
    // So ONE shared type variable, decided by the first use. The program is
    // rejected at compile time, never at run time.
    let both = "double = \\x -> x * 2.0;\n\
                main = do { write (Just (show_double (double (1.5 :: Double)))) stdout; \
                            write (Just (show_int (double (1 :: Integer)))) stdout; return () };";
    assert!(
        compile_error(both).is_some(),
        "a pattern-bound `double = \\x -> x * 2.0` is monomorphic: two incompatible \
         call-site types must be a COMPILE error"
    );
    // The same binding at ONE type is fine.
    assert_eq!(
        run("double = \\x -> x * 2.0;\n\
             main = do { write (Just (show_double (double 1.5))) stdout; return () };"),
        "3.0"
    );
}

#[test]
fn a_function_binding_with_the_same_body_is_generalised() {
    // The SAME body with a written parameter list IS a function binding, so
    // §4.5.5 does not apply and the scheme generalises. GHC accepts the same
    // two-type program only in the `Num` case; with `2.0` it rejects for a
    // different reason (`No instance for 'Fractional Integer'`), which is the
    // instance failure rather than the restriction.
    //
    // What must NOT happen is the pattern-binding behaviour: the function form
    // is not one shared variable, so the definition's own scheme is quantified.
    let t = scheme_of("double x = x * 2.0;");
    let double = t.def_schemes.get("double").expect("double has a scheme");
    assert!(
        !double.vars.is_empty(),
        "`double x = x * 2.0` is a function binding, so §4.5.5 does not apply and \
         its type variable must be QUANTIFIED, got {:?}",
        double.ty.vty
    );
}

#[test]
fn the_pattern_binding_keeps_one_shared_variable() {
    // The mirror image: the pattern form must NOT quantify, because the one
    // shared variable is what makes every use agree on a single type.
    let t = scheme_of("double = \\x -> x * 2.0;");
    let double = t.def_schemes.get("double").expect("double has a scheme");
    assert!(
        double.vars.is_empty(),
        "`double = \\x -> x * 2.0` is a SIMPLE PATTERN BINDING (§4.5.5), so its \
         variable must stay UNQUANTIFIED, got vars={:?} ty={:?}",
        double.vars,
        double.ty.vty
    );
}
