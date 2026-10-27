//! Haskell 2010 `Integer` (Report §6.4, Table 6.1): arbitrary-precision
//! integers. Removes OD-27's "Integer is absent" half.

use rill_lang::{compile, ffi::ForeignRegistry};

/// Compile and run `src`, returning the concatenated stdout.
fn run(src: &str) -> String {
    let mut prog = compile::<f32, 256>(src, ForeignRegistry::new())
        .unwrap_or_else(|e| panic!("compile failed for:\n{src}\n{e}"));
    prog.run()
        .unwrap_or_else(|e| panic!("run failed for:\n{src}\n{e}"));
    prog.stdout().concat()
}

/// Compile and run `src` in STRICT mode. Strict is the DEFAULT mode and prints
/// each `write` twice (pre-existing quirk, unrelated to literals), so callers
/// assert the doubled text.
fn run_strict(src: &str) -> String {
    let src = format!("{{-# MODE strict #-}}\n{src}");
    let mut prog = compile::<f32, 256>(&src, ForeignRegistry::new())
        .unwrap_or_else(|e| panic!("strict compile failed for:\n{src}\n{e}"));
    prog.run()
        .unwrap_or_else(|e| panic!("strict run failed for:\n{src}\n{e}"));
    prog.stdout().concat()
}

/// A program that prints one `show_integer` result.
fn shows_int(expr: &str) -> String {
    format!("main = do {{ write (Just (show_integer ({expr}))) stdout; return () }};")
}

/// A program that prints one `show_int` result.
fn shows_i64(expr: &str) -> String {
    format!("main = do {{ write (Just (show_int ({expr}))) stdout; return () }};")
}

/// A program that prints one `show_bool` result.
fn shows_bool(expr: &str) -> String {
    format!("main = do {{ write (Just (show_bool ({expr}))) stdout; return () }};")
}

/// Assert `src` is rejected BECAUSE the operand types cannot unify.
///
/// `is_err()` alone would stay green if `Integer` or `+` became unresolvable —
/// an unknown name is also an error. The reason is what makes the test evidence
/// for the mixed-`Int`/`Integer` rule.
fn rejects(src: &str) {
    rejects_with(src, "cannot unify");
}

/// Assert `src` is rejected and that the error text names `reason`.
fn rejects_with(src: &str, reason: &str) {
    match compile::<f32, 256>(src, ForeignRegistry::new()) {
        Ok(_) => panic!("must be a type error:\n{src}"),
        Err(e) => assert!(
            e.to_string().contains(reason),
            "must fail for the stated reason (`{reason}`), not another:\n{src}\ngot: {e}"
        ),
    }
}

#[test]
fn integer_is_a_distinct_type_name() {
    assert_eq!(run(&shows_int("2 :: Integer")), "2");
}

#[test]
fn integer_arithmetic_is_exact_and_does_not_overflow() {
    // The PRODUCT is outside i64's range while both operands fit an i64.
    // `Int` would wrap here; `Integer` must not. (An out-of-range integer
    // literal is itself writable — see `integer_literals_are_arbitrary_precision`.)
    assert_eq!(
        run(&shows_int(
            "(4000000000 :: Integer) * (4000000000 :: Integer)"
        )),
        "16000000000000000000"
    );
    assert_eq!(run(&shows_int("(2 :: Integer) + (3 :: Integer)")), "5");
    assert_eq!(run(&shows_int("(2 :: Integer) - (5 :: Integer)")), "-3");
}

#[test]
fn integer_comparison_works_and_is_numeric() {
    assert_eq!(
        run("main = do { write (Just (show_bool ((2 :: Integer) < (10 :: Integer)))) stdout; return () };"),
        "True"
    );
    assert_eq!(
        run("main = do { write (Just (show_bool ((2 :: Integer) == (2 :: Integer)))) stdout; return () };"),
        "True"
    );
}

#[test]
fn integer_and_int_do_not_mix() {
    // `(+) :: Num a => a -> a -> a` takes ONE `a`. GHC:
    // "Couldn't match expected type 'Int' with actual type 'Integer'".
    rejects("main = (1 :: Int) + (2 :: Integer);");
    rejects("main = (1 :: Integer) + (2 :: Int);");
    rejects("main = (1 :: Int) == (2 :: Integer);");
}

#[test]
fn integer_is_not_fractional() {
    // GHC: `No instance for 'Fractional Integer'`. This is NOT a unification
    // failure — `Integer` IS a `Num`, it simply has no `Fractional` instance.
    rejects_with(
        "main = (1 :: Integer) / (2 :: Integer);",
        "no instance of `Fractional` for type `Integer`",
    );
}

#[test]
fn integer_unary_minus_stays_integer() {
    // `negate` on the `Integer` track: the runtime path is `zero - x`, and the
    // zero must be an `Integer` zero, not an `Int` one.
    assert_eq!(run(&shows_int("-(3 :: Integer)")), "-3");
    assert_eq!(run(&shows_int("(0 :: Integer) - (3 :: Integer)")), "-3");
}

#[test]
fn a_negative_integer_literal_stays_integer() {
    // GHC: `-3 :: Integer` is `-3`. The fold must not emit an Int constant:
    // the annotation types the node `Integer`, so an `Int` payload would be the
    // typed-`Integer`-slot-holding-an-`Int` defect. At 5e2b082 this was a type
    // error (`cannot unify value type Int with Integer`).
    assert_eq!(run(&shows_int("-3 :: Integer")), "-3");
}

#[test]
fn an_unannotated_literal_in_an_integer_position_is_an_integer() {
    // The literal is in range and needs no arbitrary precision: only its
    // REPRESENTATION was wrong. Each of these silently returned False or
    // faulted at 5e2b082, because the primitive's arguments were lowered
    // without the solved instance's parameter type.
    assert_eq!(run(&shows_bool("(2 :: Integer) == 2")), "True");
    assert_eq!(run(&shows_bool("(1 :: Integer) < 2")), "True");
    assert_eq!(run(&shows_int("(2 :: Integer) + 1")), "3");
    assert_eq!(
        run("f :: Integer -> Integer;\nf x = x + 1;\nmain = do { write (Just (show_integer (f (2 :: Integer)))) stdout; return () };"),
        "3"
    );
    assert_eq!(
        run("f :: Integer -> Bool;\nf x = x == 0;\nmain = do { write (Just (show_bool (f (0 :: Integer)))) stdout; return () };"),
        "True"
    );
}

#[test]
fn a_literal_on_the_selector_side_is_also_an_integer() {
    // `(==)`/`(<)` select their instance from the FIRST class-var argument, so
    // `0 == x` puts the literal in the SELECTOR position, not the tail. Every
    // case here silently returned the wrong Bool (or was rejected) before this
    // fix, while the mirror shapes with the literal on the right worked.
    assert_eq!(
        run("isZero :: Integer -> Bool;\nisZero x = 0 == x;\nmain = do { write (Just (show_bool (isZero (0 :: Integer)))) stdout; return () };"),
        "True"
    );
    assert_eq!(
        run("isZero :: Integer -> Bool;\nisZero x = x == 0;\nmain = do { write (Just (show_bool (isZero (0 :: Integer)))) stdout; return () };"),
        "True"
    );
    assert_eq!(
        run("isPos :: Integer -> Bool;\nisPos x = 0 < x;\nmain = do { write (Just (show_bool (isPos (0 :: Integer)))) stdout; return () };"),
        "False"
    );
    assert_eq!(
        run("isPos :: Integer -> Bool;\nisPos x = x < 0;\nmain = do { write (Just (show_bool (isPos (0 :: Integer)))) stdout; return () };"),
        "False"
    );
    assert_eq!(
        run("inc :: Integer -> Integer;\ninc x = 1 + x;\nmain = do { write (Just (show_integer (inc (2 :: Integer)))) stdout; return () };"),
        "3"
    );
    assert_eq!(
        run("inc :: Integer -> Integer;\ninc x = x + 1;\nmain = do { write (Just (show_integer (inc (2 :: Integer)))) stdout; return () };"),
        "3"
    );
}

#[test]
fn the_selector_fix_does_not_widen_int_and_double_calls() {
    // The solved parameter type is authoritative, not permissive: a mixed pair
    // still has to be a type error.
    rejects("main = (1 :: Int) + (2 :: Integer);");
    rejects("main = (1 :: Integer) == (2 :: Int);");
}

#[test]
fn int_still_wraps_where_integer_does_not() {
    // The CONTROL for the exactness test: the same shape on the Int track is
    // i64 arithmetic, and the report leaves overflow of fixed-width types
    // undefined (§6.4) — rill truncates. Pinned so nobody "fixes" Int by
    // routing it through Integer.
    let wrapped = run(&shows_i64("9223372036854775807 + 1"));
    assert_eq!(wrapped, "-9223372036854775808");
}

#[test]
fn integer_literals_are_arbitrary_precision() {
    // A literal far outside i64 must LEX and keep every digit.
    assert_eq!(
        run(&shows_int("123456789012345678901234567890 :: Integer")),
        "123456789012345678901234567890"
    );
}

#[test]
fn an_out_of_range_literal_on_the_int_track_truncates_like_ghc() {
    // GHC warns "Literal … is out of the Int range" and truncates. rill has no
    // warning channel; the value must still be the same i64 GHC produces.
    assert_eq!(
        run(&shows_i64("123456789012345678901234567890")),
        "-4362896299872285998"
    );
}

#[test]
fn an_out_of_range_literal_in_a_double_position_is_the_right_double() {
    // `def_arg_widens_to` lets an integer literal inhabit a `Double` parameter
    // (Haskell 2010 §6.4.1 `fromInteger`). Before this fix the literal was
    // truncated to i64 FIRST and then widened, flipping the sign:
    // `f 9223372036854775808` printed `-9.223372036854776e18` where GHC prints
    // `9.223372036854776e18`.
    assert_eq!(
        run("f :: Double -> Double;\nf x = x;\nmain = do { write (Just (show_double (f 9223372036854775808))) stdout; return () };"),
        "9.223372036854776e18"
    );
    assert_eq!(
        run("f :: Double -> Double;\nf x = x;\nmain = do { write (Just (show_double (f 123456789012345678901234567890))) stdout; return () };"),
        "1.2345678901234568e29"
    );
    // In-range controls: unchanged behaviour.
    assert_eq!(
        run("f :: Double -> Double;\nf x = x;\nmain = do { write (Just (show_double (f 9223372036854775807))) stdout; return () };"),
        "9.223372036854776e18"
    );
    assert_eq!(
        run("f :: Double -> Double;\nf x = x;\nmain = do { write (Just (show_double (f 2))) stdout; return () };"),
        "2.0"
    );
    // The same widening under the `Expr::Neg` constant fold: the literal is
    // negated in arbitrary precision BEFORE it becomes a double. GHC 9.10.3:
    // `f x = x * 2.0; f (-9223372036854775809)` = `-1.8446744073709552e19`
    // (at 67f44b7 rill printed the sign-flipped `1.8446744073709552e19`).
    //
    // A signature-less definition is used because `def_arg_widens_to` accepts a
    // literal against a `Double` formal only for `Expr::Int`, not for `Expr::Neg`
    // — `f :: Double -> Double; f (-2)` is a type error, which is the stage 7b
    // literal-polymorphism gap and is NOT changed here.
    assert_eq!(
        run("f x = x * 2.0;\nmain = do { write (Just (show_double (f (-9223372036854775809)))) stdout; return () };"),
        "-1.8446744073709552e19"
    );
    assert_eq!(
        run("f x = x * 2.0;\nmain = do { write (Just (show_double (f (-123456789012345678901234567890)))) stdout; return () };"),
        "-2.4691357802469136e29"
    );
    assert_eq!(
        run("f x = x * 2.0;\nmain = do { write (Just (show_double (f (-9223372036854775808)))) stdout; return () };"),
        "-1.8446744073709552e19"
    );
}

// --- Strict mode -----------------------------------------------------------------
//
// Strict is rill's DEFAULT mode (`mode.rs`: `#[derive(Default)] Strict`), and it
// takes a DIFFERENT lowering path from Complete: beta-reduction substitutes a
// call's arguments into the definition body BEFORE lowering, so the formula
// parameter's declared type is not attached to the substituted literal any more.
// Round 1 fixed only the Complete path (which keeps the call on the closure
// route, where `fn_param_tys` supplies the parameter type), so every case below
// was still wrong in Strict. Each new test therefore asserts BOTH modes.
//
// Strict prints every `write` twice (pre-existing, unrelated quirk — `2.0`
// prints as `2.02.0`), so the strict expectation is the doubled text.

/// Assert `src` produces `complete` in Complete mode and `strict` in Strict mode.
fn assert_both_modes(src: &str, complete: &str, strict: &str) {
    assert_eq!(run(src), complete, "Complete mode:\n{src}");
    assert_eq!(run_strict(src), strict, "Strict mode:\n{src}");
}

#[test]
fn an_integer_literal_argument_reaches_its_formal_type_in_both_modes() {
    // `def_arg_widens_to` accepts an integer literal at a `Double`/`Integer`
    // formal (Haskell 2010 §6.4.1 `fromInteger`). The literal must then CONVERT
    // from the arbitrary-precision value, not from its low 64 bits: at round 1
    // Strict printed the sign-flipped `-9.223372036854776e18`.
    assert_both_modes(
        "f :: Double -> Double;\nf x = x;\nmain = do { write (Just (show_double (f 9223372036854775808))) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    // The parameter type survives a body that forwards the value to a builtin
    // whose OWN declared parameter is `Double`.
    assert_both_modes(
        "g :: Double -> String;\ng x = show_double x;\nmain = do { write (Just (g 9223372036854775808)) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    assert_both_modes(
        "g :: Double -> String;\ng x = show_double x;\nmain = do { write (Just (g 123456789012345678901234567890)) stdout; return () };",
        "1.2345678901234568e29",
        "1.2345678901234568e291.2345678901234568e29",
    );
    // Controls: in range, unchanged in both modes.
    assert_both_modes(
        "f :: Double -> Double;\nf x = x;\nmain = do { write (Just (show_double (f 9223372036854775807))) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    assert_both_modes(
        "f :: Double -> Double;\nf x = x;\nmain = do { write (Just (show_double (f 2))) stdout; return () };",
        "2.0",
        "2.02.0",
    );
}

#[test]
fn a_builtin_argument_gets_its_declared_parameter_type_in_both_modes() {
    // `show_double :: Double -> String` lives in `env.builtin_sigs`; before
    // round 2 only its RESULT type was used, so a literal argument was lowered
    // with no expected type. It is now accepted where GHC accepts it and
    // converts through `fromInteger`.
    assert_both_modes(
        "main = do { write (Just (show_double 123456789012345678901234567890)) stdout; return () };",
        "1.2345678901234568e29",
        "1.2345678901234568e291.2345678901234568e29",
    );
    assert_both_modes(
        "main = do { write (Just (show_double 2)) stdout; return () };",
        "2.0",
        "2.02.0",
    );
    // Control: `show_int :: Int -> String` stays on the truncating Int track.
    assert_both_modes(
        "main = do { write (Just (show_int 123456789012345678901234567890)) stdout; return () };",
        "-4362896299872285998",
        "-4362896299872285998-4362896299872285998",
    );
}

#[test]
fn an_annotated_literal_at_double_is_the_double_in_both_modes() {
    // `123456789012345678901234567890 :: Double` is `fromInteger` at `Double`
    // (Report §6.4.1). It was a type error at `cea759c` and in fix round 1; it
    // works now because a literal with a `Double` expected type types as a
    // `Double` instead of falling through to the `Int` track. Asserted so the
    // annotated spelling cannot silently regress when stage 7b reworks the
    // literal rule.
    assert_both_modes(
        "main = do { write (Just (show_double (123456789012345678901234567890 :: Double))) stdout; return () };",
        "1.2345678901234568e29",
        "1.2345678901234568e291.2345678901234568e29",
    );
    assert_both_modes(
        "main = do { write (Just (show_double (2 :: Double))) stdout; return () };",
        "2.0",
        "2.02.0",
    );
}

#[test]
fn an_integer_literal_argument_reaches_an_integer_formal_in_both_modes() {
    // `def_arg_widens_to` accepted a literal at `Double` but not at `Integer` —
    // an unstateable asymmetry, since both are the same `fromInteger` rule.
    // GHC accepts both.
    assert_both_modes(
        "f :: Integer -> Integer;\nf x = x;\nmain = do { write (Just (show_integer (f 2))) stdout; return () };",
        "2",
        "22",
    );
    assert_both_modes(
        "f :: Integer -> Integer;\nf x = x;\nmain = do { write (Just (show_integer (f 123456789012345678901234567890))) stdout; return () };",
        "123456789012345678901234567890",
        "123456789012345678901234567890123456789012345678901234567890",
    );
}
