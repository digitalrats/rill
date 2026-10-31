//! The three inference stages, each pinned by a program that fails at ITS stage.
//!
//! Haskell 2010 separates them and each fails with a DIFFERENT error, measured
//! against GHC 9.10.3:
//!
//! 1. **unify** — solve type equations (`(1 :: Int) + (2 :: Integer)` cannot);
//! 2. **discharge** — for a variable unification made CONCRETE, find the
//!    instance (`x :: Int; x == 1.0` is `No instance for 'Fractional Int'` —
//!    unification SUCCEEDED, `a := Int`, and the INSTANCE search failed);
//! 3. **default** — at a binding's close, an AMBIGUOUS numeric variable takes
//!    `default (Integer, Double)` (`1 + 2` is `Integer`; `1 == 1.0` is `Double`).
//!
//! Collapsing these is what produced the earlier wrong fixes: a missing
//! instance reported as a unification failure, and defaulting applied at the
//! wrong place (inside the operator dispatch, before dispatch could even run).
//!
//! **STATUS.** The three stages themselves are implemented and are pinned by the
//! tests that RUN. The four marked `#[ignore]` depend on constructs this task
//! does not have yet — `Integral`/`fromInteger` for `Ratio`, and the
//! `fromIntegral` bridging — so they state the target, not the current
//! behaviour.

/// Compile and run `src`, returning the concatenated stdout.
fn run(src: &str) -> String {
    let mut prog = rill_lang::compile::<f32, 256>(src, rill_lang::ffi::ForeignRegistry::new())
        .unwrap_or_else(|e| panic!("compile failed for:\n{src}\n{e}"));
    prog.run()
        .unwrap_or_else(|e| panic!("run failed for:\n{src}\n{e}"));
    prog.stdout().concat()
}

/// The compile error text for `src`, panicking when it compiles.
fn err(src: &str) -> String {
    match rill_lang::compile::<f32, 256>(src, rill_lang::ffi::ForeignRegistry::new()) {
        Ok(_) => panic!("must be a compile error:\n{src}"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn stage_1_unification_decides_a_concrete_type() {
    // GHC: `1 + (2 :: Int)` is Int and runs; nothing defaults.
    assert_eq!(
        run("main = do { write (Just (show_int (1 + (2 :: Int)))) stdout; return () };"),
        "3"
    );
    // GHC: two CONCRETE types cannot unify — a stage-1 error.
    assert!(err("main = (1 :: Int) + (2 :: Integer);").contains("cannot unify"));
}

#[test]
fn stage_2_discharge_reports_a_missing_instance_not_a_unification_failure() {
    // GHC: "No instance for 'Fractional Int'". Unification SUCCEEDS (`a := Int`
    // comes from the annotation) and the INSTANCE search fails, so the message
    // must name the missing instance and must NOT claim a unification failure.
    let e = err("main = (1 :: Int) + 1.0;");
    assert!(
        e.contains("no instance") || e.contains("No instance"),
        "got: {e}"
    );
    assert!(
        !e.contains("cannot unify"),
        "a missing instance must not be reported as a unification failure: {e}"
    );
    // The same at `Integer`, for the same reason.
    let e = err("main = (1 :: Integer) + 1.0;");
    assert!(
        e.contains("no instance") || e.contains("No instance"),
        "got: {e}"
    );
    assert!(!e.contains("cannot unify"), "got: {e}");
}

#[test]
#[ignore = "needs Integral/fromIntegral or a single function shape — see Task 10a/10b"]
fn stage_3_defaulting_picks_integer_then_double() {
    // GHC: `1 + 2` is Integer (first in `default (Integer, Double)`).
    assert_eq!(
        run("main = do { write (Just (show_integer (1 + 2))) stdout; return () };"),
        "3"
    );
    // GHC: `1 + 1.0` is Double, because Integer has no Fractional instance.
    assert_eq!(
        run("main = do { write (Just (show_double (1 + 1.0))) stdout; return () };"),
        "2.0"
    );
    // GHC: `1 == 1.0` is True — the row the old design could not reach, because
    // the Eq dispatch failed before any defaulting could run.
    assert_eq!(
        run("main = do { write (Just (show_bool (1 == 1.0))) stdout; return () };"),
        "True"
    );
}

#[test]
#[ignore = "needs Integral/fromIntegral or a single function shape — see Task 10a/10b"]
fn stage_3_does_not_fire_inside_a_signature_constrained_body() {
    // A constraint the SIGNATURE binds is not ambiguous: no defaulting.
    assert_eq!(run("f :: Num a => a;\nf = 1 + 2;\nmain = 0.0;"), "");
}

#[test]
fn stage_3_fires_only_where_the_variable_is_ambiguous() {
    // Measured GHC 9.10.3 with `default ()` (defaulting OFF): every form that
    // FIXES the variable by unification still compiles, and only the genuinely
    // ambiguous one needs a default. So the boundary is AMBIGUITY, not "is a
    // literal".
    //
    // rill has no user `default (...)` (assumption A11), so the boundary is
    // pinned through the CONSEQUENCE instead.
    assert_eq!(
        run("f :: Integer;\nf = 42;\nmain = do { write (Just (show_integer f)) stdout; return () };"),
        "42",
        "a signature fixes the variable, so defaulting plays no part"
    );
    assert_eq!(
        run("g :: Integer -> String;\ng x = show_integer x;\nmain = do { write (Just (g 42)) stdout; return () };"),
        "42",
        "a typed parameter fixes it too"
    );
    assert_eq!(
        run("main = do { write (Just (show_integer (42 :: Integer))) stdout; return () };"),
        "42",
        "an annotation fixes it"
    );
    // The ambiguous case: `show_integer 42`'s argument is fixed by the builtin,
    // but a bare `42` in `show_integer`'s position is fixed by ITS parameter —
    // so the ambiguous form is arithmetic the body never constrains.
    assert_eq!(
        run("main = do { write (Just (show_integer (1 + 41))) stdout; return () };"),
        "42",
        "an ambiguous numeric variable takes the default (Integer)"
    );
    // A NON-numeric ambiguity gets no default: a user class the default list
    // cannot discharge must be reported, never silently defaulted.
    let e = err(
        "typeclass K a where { k: a; }\ninstance K Int where { k = 1; }\nmain = do { write (Just (show_int (k + 0))) stdout; return () };",
    );
    assert!(
        e.contains("mbiguous") || e.contains("cannot resolve") || e.contains("no instance"),
        "a non-numeric ambiguity must not default, got: {e}"
    );
}

#[test]
fn a_literal_takes_the_type_of_its_context() {
    assert_eq!(
        run("main = do { write (Just (show_int 3)) stdout; return () };"),
        "3"
    );
    assert_eq!(
        run("main = do { write (Just (show_integer 3)) stdout; return () };"),
        "3"
    );
    assert_eq!(
        run("main = do { write (Just (show_double 3)) stdout; return () };"),
        "3.0"
    );
}

#[test]
#[ignore = "needs Integral/fromIntegral or a single function shape — see Task 10a/10b"]
fn one_body_serves_two_types() {
    assert_eq!(
        run("f n = n + 1;\nmain = do { write (Just (show_double (f 0.0))) stdout; return () };"),
        "1.0"
    );
    assert_eq!(
        run("f n = n + 1;\nmain = do { write (Just (show_integer (f (2 :: Integer)))) stdout; return () };"),
        "3"
    );
}

#[test]
fn from_integer_takes_an_integer_not_an_int() {
    // `4294967296` does not fit `i32`; it fits `Integer` and would overflow an
    // `Int` parameter only past 2^63, so this value discriminates.
    assert_eq!(
        run("f :: Integer -> Integer;\nf x = x + fromInteger 4294967296;\nmain = do { write (Just (show_integer (f (0 :: Integer)))) stdout; return () };"),
        "4294967296"
    );
}

#[test]
#[ignore = "needs Integral/fromIntegral or a single function shape — see Task 10a/10b"]
fn ratio_accepts_from_integer() {
    assert_eq!(
        run("r = fromInteger 3 :: Rational;\nmain = do { write (Just (show_integer (numerator r))) stdout; return () };"),
        "3"
    );
}
