//! A fractional literal carries the EXACT decimal fraction its digits spell.
//!
//! Haskell 2010 §6.4.1 makes a fractional literal `fromRational` applied to the
//! decimal fraction written in the source. Measured in GHC 9.10.3:
//!
//! ```text
//! ghci> 0.1 :: Rational
//! 1 % 10
//! ghci> toRational (0.1 :: Double)
//! 3602879701896397 % 36028797018963968
//! ```
//!
//! The two are different numbers, so a payload derived from an `f64` would
//! diverge. These tests pin the AST payload (the only place the exact value is
//! observable at this stage — `fromRational` arrives with Task 7) against the
//! GHC values, and pin the source spelling through `render` and
//! `program_json`.

use num_bigint::BigInt;
use num_rational::BigRational;
use rill_lang::ast::{Expr, Pattern, Program};
use rill_lang::lexer::tokenize;
use rill_lang::parser::parse;
use rill_lang::{compile, ffi::ForeignRegistry};

/// `n/d` as an exact rational, reduced the way `Data.Ratio`'s `%` reduces.
fn ratio(n: i64, d: i64) -> BigRational {
    BigRational::new(BigInt::from(n), BigInt::from(d))
}

/// `n/d` from decimal TEXTS, for values beyond `i64` (the ratio still
/// reduces, so `100/100` is `1`).
fn ratio_text(n: &str, d: &str) -> BigRational {
    BigRational::new(n.parse().unwrap(), d.parse().unwrap())
}

fn program_of(src: &str) -> Program {
    parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap()
}

/// The exact value of the fractional literal in `main`'s body.
fn main_literal_value(src: &str) -> BigRational {
    let program = program_of(src);
    match program.main_def().unwrap().body() {
        Expr::FractionalLit(v, _, _) => (**v).clone(),
        other => panic!("expected a fractional literal in `{src}`, got {other:?}"),
    }
}

/// Decimal digits of the AST payload's numerator (sign excluded).
fn numerator_digit_count(literal: &str) -> usize {
    let value = main_literal_value(&format!("main = {literal}"));
    value.numer().to_string().trim_start_matches('-').len()
}

/// Decimal digits of the AST payload's denominator.
fn denominator_digit_count(literal: &str) -> usize {
    main_literal_value(&format!("main = {literal}"))
        .denom()
        .to_string()
        .len()
}

/// Compile and run `src`, returning the concatenated stdout.
fn run(src: &str) -> String {
    let mut prog = compile::<f32, 256>(src, ForeignRegistry::new())
        .unwrap_or_else(|e| panic!("compile failed for:\n{src}\n{e}"));
    prog.run()
        .unwrap_or_else(|e| panic!("run failed for:\n{src}\n{e}"));
    prog.stdout().concat()
}

#[test]
fn a_decimal_literal_is_the_exact_fraction_of_its_digits() {
    // The four conversions the plan names, each measured at GHC as
    // `fromRational` of the same text.
    for (literal, exact) in [
        ("0.1", ratio(1, 10)),
        ("3.14", ratio(157, 50)),
        ("1e3", ratio(1000, 1)),
        ("1.5e-2", ratio(3, 200)),
    ] {
        assert_eq!(
            main_literal_value(&format!("main = {literal}")),
            exact,
            "`{literal}` must be the exact decimal fraction"
        );
    }
}

#[test]
fn the_payload_is_not_the_nearest_double() {
    // The f64 payload this replaced would answer `toRational (0.1 :: Double)`.
    // Pinning the INEQUALITY is what makes the test fail if the value is ever
    // built by converting through an f64.
    let as_f64 = 0.1f64.to_string();
    let from_f64 = BigRational::from_float(0.1).unwrap();
    assert_ne!(
        main_literal_value("main = 0.1"),
        from_f64,
        "`0.1` must not be the double's value ({as_f64})"
    );
    assert_eq!(
        from_f64,
        ratio(3602879701896397, 36028797018963968),
        "the GHC measurement of `toRational (0.1 :: Double)`"
    );
}

#[test]
fn a_trailing_zero_fraction_keeps_every_digit() {
    // `1.00` is exactly 1 — the digits carry a denominator of 10^2 that reduces
    // to 1. The SPELLING is what pins it apart from `1.0`, and that survives.
    assert_eq!(main_literal_value("main = 1.00"), ratio(1, 1));
    let program = program_of("main = 1.00");
    match program.main_def().unwrap().body() {
        Expr::FractionalLit(_, text, _) => assert_eq!(text, "1.00"),
        other => panic!("expected a fractional literal, got {other:?}"),
    }
}

#[test]
fn a_fractional_pattern_carries_the_same_exact_value() {
    // Argument-pattern equations are desugared into a `Match` over the
    // parameters, so the pattern is observed AFTER the desugar pass.
    let mut program = program_of("f 0.5 = 1.0; f x = x; main = f 0.5");
    rill_lang::desugar::desugar_program(&mut program).unwrap();
    let Expr::Match { arms, .. } = program
        .defs
        .iter()
        .find(|d| d.name() == "f")
        .expect("`f` is defined")
        .body()
    else {
        panic!("expected a match for the equation set");
    };
    match &arms[0].pattern {
        Pattern::FractionalLit(v, text) => {
            assert_eq!(**v, ratio(1, 2));
            assert_eq!(text, "0.5");
        }
        other => panic!("expected a fractional pattern, got {other:?}"),
    }
}

#[test]
fn render_reproduces_the_source_spelling() {
    // `render` prints the literal's SOURCE TEXT, so `1e3` does not become
    // `1000.0` and `1.00` does not fold into `1.0`.
    for (src, expected) in [
        ("main = 0.1", "main = 0.1"),
        ("main = 3.14", "main = 3.14"),
        ("main = 1e3", "main = 1e3"),
        ("main = 1.5e-2", "main = 1.5e-2"),
        ("main = 1.00", "main = 1.00"),
    ] {
        let rendered = rill_lang::render::render(&program_of(src)).unwrap();
        assert_eq!(rendered, expected, "render must keep the source spelling");
        // ...and the rendering must re-parse to the same AST, value included.
        assert_eq!(
            program_of(&rendered),
            program_of(src),
            "render -> parse must be identity for `{src}`"
        );
    }
}

#[test]
fn a_rendered_exponent_literal_still_has_its_exact_value() {
    // The round-trip must preserve the PAYLOAD, not only the text: a rendering
    // that produced `1000.0` would parse back to the same number here but lose
    // the distinction for a literal like `1.2345678901234567890123`.
    let src = "main = 1.2345678901234567890123";
    let rendered = rill_lang::render::render(&program_of(src)).unwrap();
    assert_eq!(rendered, src);
    assert_eq!(
        main_literal_value(&rendered),
        main_literal_value(src),
        "the exact value must survive the render round-trip"
    );
    // The exact value keeps all 23 fractional digits; an f64 has ~17.
    assert_eq!(
        main_literal_value(src),
        ratio_text("12345678901234567890123", "10000000000000000000000")
    );
}

#[test]
fn render_reproduces_a_fractional_pattern_spelling() {
    // The pattern twin keeps the same two fields, so `render` emits the source
    // spelling there too (`0.50` does not fold into `0.5`).
    let src = "main = case 1.0 of { 0.50 -> 2.0; _ -> 3.0 }";
    let rendered = rill_lang::render::render(&program_of(src)).unwrap();
    assert!(
        rendered.contains("0.50 ->"),
        "the pattern's source spelling must survive render: {rendered}"
    );
}

#[test]
fn the_lowered_double_is_the_nearest_f64_to_the_exact_value() {
    // The IR still needs a `Double`, and the single conversion boundary rounds
    // the exact ratio correctly. GHC: `0.1 :: Double` prints `0.1`, and
    // `1.2345678901234567890123 :: Double` rounds to the same double Rust's
    // correctly-rounded parser gives.
    assert_eq!(
        run("main = do { write (Just (show_double 0.1)) stdout; return () };"),
        "0.1"
    );
    assert_eq!(
        run("main = do { write (Just (show_double 1.2345678901234567890123)) stdout; return () };"),
        "1.2345678901234567"
    );
}

#[test]
fn a_large_exponent_stays_exact_within_the_budget() {
    // GHC 9.10.3: `numerator (1e2000 :: Rational)` has 2001 decimal digits.
    // A magnitude-banded clamp (`|value| <= 10^1000`) silently truncated this
    // to 1001, so the AST payload was `10^1000`, not `1e2000`.
    let digits = numerator_digit_count("1e2000");
    assert_eq!(digits, 2001, "`1e2000` must be exactly 10^2000");
    // In-band exactness must not be band-dependent: `1e500` was already exact
    // under the band, and `1e2000` must agree with it digit for digit.
    let digits500 = numerator_digit_count("1e500");
    assert_eq!(
        digits500, 501,
        "in-band exactness must not be band-dependent"
    );
    // The same value, checked as a value rather than a count.
    assert_eq!(
        main_literal_value("main = 1e2000"),
        BigRational::from_integer(BigInt::from(10).pow(2000)),
        "the payload is `10^2000`, not its clamped representative"
    );
}

#[test]
fn a_negative_exponent_stays_exact_within_the_budget() {
    // GHC 9.10.3: `numerator (1e-500 :: Rational)` is 1 and
    // `denominator (1e-500 :: Rational)` is 10^500 — 501 decimal digits.
    let digits = denominator_digit_count("1e-500");
    assert_eq!(digits, 501, "`1e-500` must be exactly 10^-500");
    assert_eq!(
        numerator_digit_count("1e-500"),
        1,
        "`1e-500` reduces to `1 / 10^500`, so the numerator is `1`"
    );
    // Far outside the old band, still exactly representable within budget.
    assert_eq!(denominator_digit_count("1e-2000"), 2001);
}

#[test]
fn an_out_of_range_exponent_becomes_infinity_like_ghc() {
    // GHC 9.10.3: `print (1e4294967296 :: Double)` is `Infinity`; the baseline
    // accepted it, so rejecting it is a regression. rill's `show_double`
    // spells infinity `inf`, NOT `Infinity` — a separately recorded defect
    // (open defect OD-F4 in the task report), so this asserts rill's spelling
    // and does not widen the scope here.
    assert_eq!(
        run("main = do { write (Just (show_double 1e4294967296)) stdout; return () };"),
        "inf"
    );
}

#[test]
fn an_underflowing_exponent_becomes_zero_not_a_panic() {
    // The scale computation must not overflow `i64`: `0.01e-9223372036854775807`
    // panicked "attempt to subtract with overflow" through the public API.
    assert_eq!(
        run(
            "main = do { write (Just (show_double 0.01e-9223372036854775807)) stdout; return () };"
        ),
        "0.0"
    );
}

#[test]
fn a_fractional_pattern_matches_at_run_time() {
    // The exact value has to reach the match TEST, not only the AST: the first
    // arm compares the scrutinee against `3.5` and must be taken, while a
    // different scrutinee falls through to the wildcard.
    let first = "main = do { write (Just (show_double (case 3.5 of { 3.5 -> 111.0; _ -> 222.0 }))) stdout; return () };";
    assert_eq!(run(first), "111.0");
    let other = "main = do { write (Just (show_double (case 3.25 of { 3.5 -> 111.0; _ -> 222.0 }))) stdout; return () };";
    assert_eq!(run(other), "222.0");
    // The literal is exact end to end: `0.1` is `1/10`, whose nearest double
    // is the same one the pattern compares against, so the arm is taken even
    // though `0.1 + 0.2` (not a literal) is not exactly `0.3`.
    let tenth = "main = do { write (Just (show_double (case 0.1 of { 0.1 -> 111.0; _ -> 222.0 }))) stdout; return () };";
    assert_eq!(run(tenth), "111.0");
}
