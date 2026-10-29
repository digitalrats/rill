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

// --- Fix round 3: the representation comes ONLY from the expected type --------
//
// Rounds 1 and 2 fixed one SITE at a time, and each fix left the next site
// broken (round 2's own fix turned `f x = x; f 9223372036854775808` back into
// the sign-flipped `-9.223372036854776e18`). Round 3 states the invariant
// instead:
//
//   The representation of an integer literal — `Int`, `Integer` or `Double` —
//   is decided ONLY by its expected type. Every path that lowers a literal
//   carries that expected type or explicitly establishes that it has none.
//
// Every expected value below was measured on this machine with `runghc`
// (GHC 9.10.3) before it was pinned; the numbers are GHC's, not hand-derived.

#[test]
fn an_annotated_literal_binding_takes_the_annotated_type_in_both_modes() {
    // `Expr::Annot` honoured only `:: Integer`, so a `:: Double` annotation was
    // INERT: the literal lowered on the `Int` track and printed the truncated,
    // sign-flipped `-9.223372036854776e18` (`-4.362896299872286e18` for the
    // 30-digit literal). GHC prints the `fromInteger` value in both cases.
    assert_both_modes(
        "x = (9223372036854775808 :: Double);\nmain = do { write (Just (show_double x)) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    assert_both_modes(
        "x = (123456789012345678901234567890 :: Double);\nmain = do { write (Just (show_double x)) stdout; return () };",
        "1.2345678901234568e29",
        "1.2345678901234568e291.2345678901234568e29",
    );
    assert_both_modes(
        "x = (123456789012345678901234567890 :: Integer);\nmain = do { write (Just (show_integer x)) stdout; return () };",
        "123456789012345678901234567890",
        "123456789012345678901234567890123456789012345678901234567890",
    );
}

#[test]
fn an_annotation_survives_the_negation_fold_in_both_modes() {
    // The annotation's expected type must reach a literal under `Expr::Neg`
    // too: `-N` is one literal (Haskell 2010 §3.8), not `negate N`.
    assert_both_modes(
        "x = ((-123456789012345678901234567890) :: Double);\nmain = do { write (Just (show_double x)) stdout; return () };",
        "-1.2345678901234568e29",
        "-1.2345678901234568e29-1.2345678901234568e29",
    );
}

#[test]
fn a_negated_literal_argument_widens_like_a_positive_one_in_both_modes() {
    // `reduce::is_integer_literal` counted `Expr::Neg` while
    // `infer::int_literal_widens_to` did not, so `f 2` was accepted at
    // `Double -> Double` and `f (-2)` was rejected. GHC accepts both. The two
    // now share ONE predicate (`Expr::is_int_literal`).
    assert_both_modes(
        "f :: Double -> Double;\nf p = p;\nmain = do { write (Just (show_double (f (-2)))) stdout; return () };",
        "-2.0",
        "-2.0-2.0",
    );
    assert_both_modes(
        "f :: Integer -> Integer;\nf p = p;\nmain = do { write (Just (show_integer (f (-2)))) stdout; return () };",
        "-2",
        "-2-2",
    );
}

#[test]
fn a_signature_less_identity_does_not_default_its_parameter_to_int() {
    // REGRESSION from round 2: `compile_anchor_closure` used inference's
    // DEFAULTED parameter list, so `f x = x` (schemed `Int -> Int`) advertised
    // `Int -> Int` and every call site truncated the literal. A default is not
    // a declaration. GHC prints `9.223372036854776e18` for both spellings.
    assert_both_modes(
        "f x = x;\nmain = do { write (Just (show_double (f 9223372036854775808))) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    assert_both_modes(
        "f :: a -> a;\nf y = y;\nmain = do { write (Just (show_double (f 9223372036854775808))) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    // The polymorphic definition still works at non-numeric types — the
    // placeholder only replaces the SLOT's advertised scalar, never the
    // argument's own typing.
    assert_both_modes(
        "f x = x;\nmain = do { write (Just (f \"hi\")) stdout; return () };",
        "hi",
        "hihi",
    );
}

#[test]
fn a_declared_binding_and_a_declared_result_reach_the_literal_in_both_modes() {
    // In Strict mode a call's reduced literal lands in a binding whose type
    // nobody supplies. A binding's OWN `::` is that declaration, and it must
    // reach both the literal and the closure body. A BARE binding stays
    // defaulted: GHC prints `-4362896299872285998`-style `Int` truncation only
    // when the use site is monomorphic `Int`; used at `Double` GHC prints
    // `9.223372036854776e18`, which the annotation-carrying call site below
    // also produces.
    assert_both_modes(
        "f :: Double -> Double;\nf p = p;\nx = f 9223372036854775808;\nmain = do { write (Just (show_double x)) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    assert_both_modes(
        "x :: Double;\nx = 9223372036854775808;\nmain = do { write (Just (show_double x)) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    assert_both_modes(
        "x :: Integer;\nx = 123456789012345678901234567890;\nmain = do { write (Just (show_integer x)) stdout; return () };",
        "123456789012345678901234567890",
        "123456789012345678901234567890123456789012345678901234567890",
    );
    // A declared RESULT type reaches a literal body: a closure body is its own
    // fragment, so no call-site expected type can get there.
    assert_both_modes(
        "f :: Double -> Double;\nf p = 123456789012345678901234567890;\nmain = do { write (Just (show_double (f 1.0))) stdout; return () };",
        "1.2345678901234568e29",
        "1.2345678901234568e291.2345678901234568e29",
    );
    assert_both_modes(
        "f :: Integer -> Integer;\nf p = 123456789012345678901234567890;\nmain = do { write (Just (show_integer (f 1))) stdout; return () };",
        "123456789012345678901234567890",
        "123456789012345678901234567890123456789012345678901234567890",
    );
}

#[test]
fn an_integer_pattern_literal_takes_the_scrutinee_type_in_both_modes() {
    // A `Pattern::LitInt` is an integer literal too, and its ONE expected type
    // is the scrutinee's. Pinning every such pattern to `Int` rejected
    // `f :: Integer -> Integer; f 2 = 5` (GHC accepts it) and made the lowered
    // constant a cross-kind comparison against an `Integer`/`Double`
    // scrutinee.
    assert_both_modes(
        "f :: Integer -> Integer;\nf 2 = 5;\nf p = p;\nmain = do { write (Just (show_integer (f 2))) stdout; return () };",
        "5",
        "55",
    );
    assert_both_modes(
        "f :: Double -> Double;\nf 2 = 5.0;\nf p = p;\nmain = do { write (Just (show_double (f 2))) stdout; return () };",
        "5.0",
        "5.05.0",
    );
    // The pattern still misses a different value (the constant is not the
    // always-true degenerate).
    assert_both_modes(
        "f :: Integer -> Integer;\nf 2 = 5;\nf p = p;\nmain = do { write (Just (show_integer (f 7))) stdout; return () };",
        "7",
        "77",
    );
}

// --- Fix round 4: a GUESSED type never converts an integer literal ------------
//
// Round 3 shipped a `Double` placeholder for a parameter whose type inference
// left polymorphic. That was the same mistake in the other direction from the
// `Int` default it replaced: `Double` happens to be right at 2^63 and lossy
// above 2^53. GHC's model has no guess at all — `42` is `fromInteger (42 ::
// Integer)` and its type comes from unification with the context — so a slot
// with NO declaration supplies NO expected type and the literal keeps its own
// conversion. A record literal's field, by contrast, HAS a declared type
// (the field's type in the record type the binding declares) and must receive
// it.
//
// Every expected value below was measured on this machine with `runghc`
// (GHC 9.10.3) before it was pinned. Strict prints each `write` twice (the
// pre-existing doubling quirk), so its expectation is the doubled text.

#[test]
fn a_record_field_literal_takes_the_declared_field_type_in_both_modes() {
    // `p :: Pair Double Double` declares `first: Double`, so `9223372036854775808`
    // is `fromInteger` at `Double`. Before this fix the field received no
    // expected type (the caller's `expected` only reached the constructor for a
    // `Con` head), so the literal stayed on the `Int` track and printed the
    // low-64-bit truncation `-9.223372036854776e18` — a WRONG VALUE, not an
    // error. `123456789012345678901234567890` printed `-4.362896299872286e18`.
    assert_both_modes(
        "p :: Pair Double Double;\np = Pair { first: 9223372036854775808, second: 1.0 };\nmain = do { write (Just (show_double p.first)) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    assert_both_modes(
        "p :: Pair Double Double;\np = Pair { first: 123456789012345678901234567890, second: 1.0 };\nmain = do { write (Just (show_double p.first)) stdout; return () };",
        "1.2345678901234568e29",
        "1.2345678901234568e291.2345678901234568e29",
    );
    // Control: an in-range field is unchanged in both modes.
    assert_both_modes(
        "p :: Pair Double Double;\np = Pair { first: 2, second: 1.0 };\nmain = do { write (Just (show_double p.first)) stdout; return () };",
        "2.0",
        "2.02.0",
    );
}

#[test]
fn a_user_record_field_literal_takes_the_declared_field_type_in_both_modes() {
    // The same rule for a declared `data` record: `x: Double` is the field's
    // declaration, so the field literal is `fromInteger` at `Double`.
    assert_both_modes(
        "data Point = { x: Double, y: Double };\np :: Point;\np = Point { x: 9223372036854775808, y: 1.0 };\nmain = do { write (Just (show_double p.x)) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    // A type SYNONYM of the record is the same declaration.
    assert_both_modes(
        "data Point = { x: Double, y: Double };\ntype P = Point;\np :: P;\np = Point { x: 9223372036854775808, y: 1.0 };\nmain = do { write (Just (show_double p.x)) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    // A NEWTYPE's declared target is the constructor argument's expected type,
    // so its literal converts as a declaration does.
    //
    // This assertion PROJECTS the value (a `Map` key comparison). A
    // `write (Just "ok")` body would prove nothing: it never forces the
    // binding, so the constructor is never lowered and neither
    // `lower_newtype_constructor` nor its target threading is exercised — the
    // reviewer proved exactly that. A `Map` probe DOES force it: a key built
    // from `9223372036854775808` matches the correctly converted
    // `9.223372036854776e18` and does NOT match the low-64-bit truncation
    // `-9.223372036854776e18`, so a regression to the `Int` track flips both
    // rows. Every expected value was measured with GHC 9.10.3
    // (`Data.Map.insert`/`member` on a `deriving (Eq, Ord)` newtype).
    assert_both_modes(
        "newtype Hz = Double;\nm = insert (Hz 9223372036854775808) 1.0 (empty_map);\nmain = do { write (Just (show_bool (member (Hz 9.223372036854776e18) m))) stdout; return () };",
        "True",
        "TrueTrue",
    );
    // The sign-flipped twin: the truncation WOULD match this key, so `False`
    // here is the falsifiable direction (the defect printed `True`).
    assert_both_modes(
        "newtype Hz = Double;\nm = insert (Hz 9223372036854775808) 1.0 (empty_map);\nmain = do { write (Just (show_bool (member (Hz (-9.223372036854776e18)) m))) stdout; return () };",
        "False",
        "FalseFalse",
    );
    // The 30-digit literal through the same target: the correct
    // `1.2345678901234568e29` matches.
    assert_both_modes(
        "newtype Hz = Double;\nm = insert (Hz 123456789012345678901234567890) 1.0 (empty_map);\nmain = do { write (Just (show_bool (member (Hz 1.2345678901234568e29) m))) stdout; return () };",
        "True",
        "TrueTrue",
    );
    // Control: an in-range literal is unchanged.
    assert_both_modes(
        "newtype Hz = Double;\nm = insert (Hz 2) 1.0 (empty_map);\nmain = do { write (Just (show_bool (member (Hz 2.0) m))) stdout; return () };",
        "True",
        "TrueTrue",
    );
}

#[test]
fn an_expected_type_flows_through_a_chain_of_polymorphic_calls() {
    // The expectation must unify the callee's SCHEME: `f x = x` schemes as
    // `a -> a`, so an expected `Double` at the call site pins `a` in BOTH the
    // parameter and the result. Matching only the result's residual var left
    // the parameter unpinned, so the literal kept its i64 truncation and the
    // sign flipped (-9.22…e18 where GHC prints 9.22…e18).
    //
    // Every expected value below was measured with `runghc` (GHC 9.10.3) on
    // this machine before it was pinned.
    assert_both_modes(
        "f x = x;\ng y = f y;\nmain = do { write (Just (show_double (g 9223372036854775808))) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    assert_both_modes(
        "f x = x;\ng y = f y;\nmain = do { write (Just (show_double (g 123456789012345678901234567890))) stdout; return () };",
        "1.2345678901234568e29",
        "1.2345678901234568e291.2345678901234568e29",
    );
    // A λ indirection: `g = \y -> f y` has the SAME `a -> a` scheme, so it is
    // the same unification, not a second mechanism.
    assert_both_modes(
        "f x = x;\ng = \\y -> f y;\nmain = do { write (Just (show_double (g 9223372036854775808))) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    // A chain of any length: each hop is the same `a -> a` unification, so no
    // hop count is special.
    //
    // GHC 9.10.3 prints `1.2345678901234568e29` here, NOT the `2.4691357802469136e29`
    // the brief's table gave: that value is exactly TWICE this one. Re-measured
    // with `runghc` (plain, and via `h 1234…890 + h 1234…890`, which prints the
    // doubled value) — the program has a single `h` call, so GHC's answer is the
    // undoubled one, and rill now agrees with it. The `2.469…e29` row is kept
    // below as a separate assertion reached through an explicit doubling, so
    // neither value is lost.
    assert_both_modes(
        "f x = x;\ng y = f y;\nh z = g z;\nmain = do { write (Just (show_double (h 123456789012345678901234567890))) stdout; return () };",
        "1.2345678901234568e29",
        "1.2345678901234568e291.2345678901234568e29",
    );
    // The doubled value, reachable only by doubling the call: GHC 9.10.3 prints
    // `2.4691357802469136e29` for `h n + h n` with the same 30-digit literal.
    assert_both_modes(
        "f x = x;\ng y = f y;\nh z = g z;\nmain = do { write (Just (show_double (h 123456789012345678901234567890 + h 123456789012345678901234567890))) stdout; return () };",
        "2.4691357802469136e29",
        "2.4691357802469136e292.4691357802469136e29",
    );
    // The same scheme at two DISTINCT types: each call site gets a FRESH
    // `Subst`, so the `Double` use and the `Int` use do not leak into each
    // other. `9007199254740993` (2^53+1) is exact as an `Int` and rounds as a
    // `Double`, so a leaked bind shows up as `9007199254740992`. GHC 9.10.3
    // prints exactly these two lines.
    assert_both_modes(
        "f x = x;\ng y = f y;\nh z = g z;\nmain = do { write (Just (show_double (h 9223372036854775808))) stdout; write (Just (show_int (h 9007199254740993))) stdout; return () };",
        "9.223372036854776e189007199254740993",
        "9.223372036854776e189.223372036854776e1890071992547409939007199254740993",
    );
    // The literal's type also reaches the OTHER operand of `==`, so `1` is a
    // `Double` here and the comparison is `True` (GHC 9.10.3).
    assert_both_modes(
        "f x = x;\ng y = f y;\nmain = do { write (Just (show_bool (g 1 == 1.0))) stdout; return () };",
        "True",
        "TrueTrue",
    );
    // Control: zero hops, unchanged.
    assert_both_modes(
        "f x = x;\nmain = do { write (Just (show_double (f 9223372036854775808))) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
}

#[test]
fn a_polymorphic_identity_converts_its_argument_by_the_call_sites_type_in_both_modes() {
    // REGRESSION from round 3: `closure_param_tys` advertised a `Double`
    // placeholder for `f x = x`'s polymorphic parameter, so every call site
    // lowered its argument as a `Double`. `9007199254740993` (2^53+1) is an
    // exact `Int` and a rounded `Double`, so the placeholder printed
    // `9007199254740992` — GHC prints `9007199254740993`.
    //
    // The parameter now advertises its residual VARIABLE (no information), so
    // the call site supplies no expected type and the literal keeps its exact
    // `Int` conversion. `show_double (f 9223372036854775808)` still gets
    // `Double` because `pin_ret_head_types` performs the `a -> a` unification
    // the signature describes.
    assert_both_modes(
        "f x = x;\nmain = do { write (Just (show_int (f 9007199254740993))) stdout; return () };",
        "9007199254740993",
        "90071992547409939007199254740993",
    );
    // Control: the literal with no call at all.
    assert_both_modes(
        "main = do { write (Just (show_int (9007199254740993))) stdout; return () };",
        "9007199254740993",
        "90071992547409939007199254740993",
    );
    // The 2^63 case the placeholder was added for must still work: `Double` is
    // right there, but only because the signature says so.
    assert_both_modes(
        "f :: Double -> Double;\nf x = x;\nmain = do { write (Just (show_double (f 9223372036854775808))) stdout; return () };",
        "9.223372036854776e18",
        "9.223372036854776e189.223372036854776e18",
    );
    assert_both_modes(
        "f :: Integer -> Integer;\nf x = x;\nmain = do { write (Just (show_integer (f 2))) stdout; return () };",
        "2",
        "22",
    );
}

#[test]
fn a_guessed_type_does_not_convert_a_negated_literal_either() {
    // `-9007199254740993` is one literal (Haskell 2010 §3.8). The same
    // placeholder reached it through the `Expr::Neg` fold.
    assert_both_modes(
        "f x = x;\nmain = do { write (Just (show_int (f (-9007199254740993)))) stdout; return () };",
        "-9007199254740993",
        "-9007199254740993-9007199254740993",
    );
}

#[test]
fn an_out_of_range_literal_through_a_polymorphic_slot_keeps_ghcs_int_truncation() {
    // The sharpest separation of the two guesses: at `Int` type GHC truncates
    // the 30-digit literal to `-4362896299872285998`, while the `Double`
    // placeholder rounded it to `9223372036854775807` (via 1.234…e29 saturated
    // and cast). The exact `Int` conversion is the `Int`-track answer, and it is
    // what a slot with no declaration must produce.
    assert_both_modes(
        "f x = x;\nmain = do { write (Just (show_int (f 123456789012345678901234567890))) stdout; return () };",
        "-4362896299872285998",
        "-4362896299872285998-4362896299872285998",
    );
}

#[test]
fn a_polymorphic_helper_accepts_a_computed_argument() {
    // These worked before this plan started and were briefly rejected: the
    // call-site unify committed the argument's own DEFAULTED `Int` to the
    // callee's formal before the expected `Double` could reach it. Only a bare
    // parameter reference (the `g y = f y` chain case) may be linked here.
    assert_eq!(
        run("f x = x;\nmain = do { write (Just (show_double (f (2 + 3)))) stdout; return () };"),
        "5.0"
    );
    assert_eq!(
        run("f x = x;\nd :: Double;\nd = 1 + 2;\nmain = do { write (Just (show_double (f d))) stdout; return () };"),
        "3.0"
    );
    assert_eq!(
        run("f x = x;\nmain = do { write (Just (show_double (f (if True then 1 else 2)))) stdout; return () };"),
        "1.0"
    );
}

#[test]
fn a_builtin_argument_is_inferred_at_its_declared_parameter_type() {
    // `show_int :: Int -> String` declares a concrete `Int` parameter, so its
    // argument must be inferred AT `Int`. Without the threading a
    // result-directed method inside the argument cannot select its instance:
    // `fromInteger : Int -> a` mentions the class variable only in its RESULT,
    // so `fromInteger 2` alone is "cannot resolve method `fromInteger` of `Num`:
    // expected type unknown". Threading the declared parameter type is the ONLY
    // thing that fixes it — `int_literal_widens_to` does not apply, because the
    // argument is an application, not a literal.
    use rill_lang::lexer::tokenize;
    use rill_lang::mode::Mode;
    use rill_lang::parser::parse;
    let src = "main = do { write (Just (show_int (fromInteger 2))) stdout; return () };";
    let toks = tokenize(src).expect("lex");
    let prog = parse(&toks, src.as_bytes()).expect("parse");
    // The assertion is that inference ACCEPTS the program at all: without the
    // declared-parameter-type threading `fromInteger 2` cannot select a `Num`
    // instance ("expected type unknown"), so the call above would fail.
    rill_lang::types::infer::infer_program(&prog, Mode::Complete)
        .unwrap_or_else(|e| panic!("`{src}` must typecheck: {e}"));
    // The control that already worked: a bare integer literal at `Int`.
    assert_eq!(
        run("main = do { write (Just (show_int 2)) stdout; return () };"),
        "2"
    );
    // And a `Double`-parameter builtin, where the threading decides the literal's
    // `fromInteger` target rather than merely satisfying a unification.
    assert_eq!(
        run("main = do { write (Just (show_double 2)) stdout; return () };"),
        "2.0"
    );
}
