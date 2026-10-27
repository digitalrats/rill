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
    // The PRODUCT is outside i64's range while both operands fit an i64
    // literal (literal widening to arbitrary precision is a later stage, so a
    // literal beyond 2^63 cannot be written yet). `Int` would wrap here;
    // `Integer` must not.
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
