//! `Integral` and `fromIntegral` (Haskell 2010 §6.4.1, §9.3).
//!
//! `fromIntegral :: (Integral a, Num b) => a -> b` is the LEGAL door between two
//! numeric types. It is not a relaxation of `(+) :: Num a => a -> a -> a`: the
//! sum still needs ONE type, and `fromIntegral` is what makes both operands have
//! it. That is the difference between the two programs below, which is the whole
//! point of this file:
//!
//! * `(1 :: Int) + (2.0 :: Double)` — rejected, two different concrete types;
//! * `fromIntegral (1 :: Int) + (2.0 :: Double)` — `3.0`, measured with GHC
//!   9.10.3. `fromIntegral (1 :: Int) :: Num b => b`, so the `Double` operand
//!   solves `b`.
//!
//! The Report's equation is `fromIntegral = fromInteger . toInteger`, so this
//! file also pins `toInteger`, the `Integral` method the composition needs.
//!
//! **STATUS: IGNORED.** `Integral`/`fromIntegral`/`toInteger` do not exist yet —
//! this file is the specification of the task that adds them. Every test here
//! fails with `unknown identifier`, which is the honest state, so they are
//! marked `#[ignore]` rather than left red (a permanently red suite hides a real
//! failure among them). Un-ignore each one as its construct lands.

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
#[ignore = "Integral/fromIntegral/fromInteger: not implemented — see the module note"]
fn from_integral_bridges_two_numeric_types() {
    // GHC: `fromIntegral (1 :: Int) + (2.0 :: Double)` is `3.0`.
    assert_eq!(
        run("main = do { write (Just (show_double (fromIntegral (1 :: Int) + (2.0 :: Double)))) stdout; return () };"),
        "3.0"
    );
    // The order does not matter: the LEFT operand may be the float.
    assert_eq!(
        run("main = do { write (Just (show_double ((2.0 :: Double) + fromIntegral (1 :: Int)))) stdout; return () };"),
        "3.0"
    );
    // And the same with `Integer` on the integral side.
    assert_eq!(
        run("main = do { write (Just (show_double (fromIntegral (1 :: Integer) + (2.0 :: Double)))) stdout; return () };"),
        "3.0"
    );
}

#[test]
#[ignore = "Integral/fromIntegral/fromInteger: not implemented — see the module note"]
fn a_direct_int_and_double_sum_is_still_rejected() {
    // The contrast the file exists for: no implicit coercion, so `fromIntegral`
    // is the ONLY way. A language that accepted this would make the test above
    // prove nothing.
    let e = err("main = (1 :: Int) + (2.0 :: Double);");
    assert!(
        e.contains("cannot unify"),
        "two concrete numeric types must fail to unify: {e}"
    );
}

#[test]
#[ignore = "Integral/fromIntegral/fromInteger: not implemented — see the module note"]
fn from_integral_converts_between_the_integral_types() {
    // GHC: `fromIntegral (3 :: Int) :: Integer` widens exactly.
    assert_eq!(
        run("main = do { write (Just (show_integer (fromIntegral (3 :: Int) :: Integer))) stdout; return () };"),
        "3"
    );
    // And narrows back, truncating like `fromInteger` (the one truncation
    // boundary).
    assert_eq!(
        run("main = do { write (Just (show_int (fromIntegral (3 :: Integer) :: Int))) stdout; return () };"),
        "3"
    );
    // GHC: `fromIntegral 1 + 2` defaults to `Integer` and answers 3.
    assert_eq!(
        run("main = do { write (Just (show_integer (fromIntegral 1 + 2))) stdout; return () };"),
        "3"
    );
}

#[test]
#[ignore = "Integral/fromIntegral/fromInteger: not implemented — see the module note"]
fn to_integer_is_the_integral_method() {
    // GHC: `toInteger (3 :: Int)` is `3 :: Integer`; on `Integer` it is the
    // identity, which is what makes `fromIntegral = fromInteger . toInteger`
    // total over both instances.
    assert_eq!(
        run("main = do { write (Just (show_integer (toInteger (3 :: Int)))) stdout; return () };"),
        "3"
    );
    assert_eq!(
        run("main = do { write (Just (show_integer (toInteger (3 :: Integer)))) stdout; return () };"),
        "3"
    );
    // A negative value keeps its sign: widening is exact, not magnitude-only.
    assert_eq!(
        run("main = do { write (Just (show_integer (toInteger (-3 :: Int)))) stdout; return () };"),
        "-3"
    );
}

#[test]
#[ignore = "Integral/fromIntegral/fromInteger: not implemented — see the module note"]
fn from_integral_rejects_a_non_integral_argument() {
    // `Integral String` has no instance, so the constraint is unsatisfiable —
    // the error must be about the missing instance, not a silent conversion.
    let e = err(r#"main = fromIntegral "s";"#);
    assert!(
        e.contains("no instance") || e.contains("Integral"),
        "a non-integral argument must fail on the missing instance: {e}"
    );
}
