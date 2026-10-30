//! `Ratio a` / `Rational` — Haskell 2010 §9.5's `Data.Ratio`, as far as the
//! classes rill has allow. See design A2/A4/A6.

use rill_lang::{compile, ffi::ForeignRegistry};

/// Compile and run `src`, returning the concatenated stdout.
fn run(src: &str) -> String {
    let mut prog = compile::<f32, 256>(src, ForeignRegistry::new())
        .unwrap_or_else(|e| panic!("compile failed for:\n{src}\n{e}"));
    prog.run()
        .unwrap_or_else(|e| panic!("run failed for:\n{src}\n{e}"));
    prog.stdout().concat()
}

/// Run `src` expecting a FAILURE, and return its detail.
///
/// Both channels are accepted, because either is a loud, correct answer where
/// GHC raises `Ratio has zero denominator` at run time: a compile error, or a
/// program that compiles and then faults while running (rill's clean fault
/// channel — the array surface documents the same policy instead of GHC's
/// `error`/bottom). What is NOT accepted is a program that runs to completion:
/// that is the silent `den = 0` wrong value this helper exists to catch.
fn run_faults(src: &str) -> String {
    let mut prog = match compile::<f32, 256>(src, ForeignRegistry::new()) {
        Ok(p) => p,
        Err(e) => return e.to_string(),
    };
    match prog.run() {
        Ok(()) => panic!(
            "must fault, but the program ran:\n{src}\ngot: {:?}",
            prog.stdout()
        ),
        Err(e) => e.to_string(),
    }
}

/// Assert `src` is rejected and that the error text names `reason`.
///
/// `is_err()` alone is not evidence: an unknown name is also an error. The
/// reason is what pins WHY the program is rejected — the omitted `fromInteger`
/// must fail at the call site, not through a stale name.
fn rejects_with(src: &str, reason: &str) {
    match compile::<f32, 256>(src, ForeignRegistry::new()) {
        Ok(_) => panic!("must be a compile error:\n{src}"),
        Err(e) => assert!(
            e.to_string().contains(reason),
            "must fail for the stated reason (`{reason}`), not another:\n{src}\ngot: {e}"
        ),
    }
}

/// The two rationals the equality/ordering tests use, at `Integer` fields.
const HALF: &str = "r = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\n";
const THIRD: &str = "s = Ratio { num: (1 :: Integer), den: (3 :: Integer) };\n";
const OVER_HALF: &str = "t = Ratio { num: (2 :: Integer), den: (4 :: Integer) };\n";

#[test]
fn ratio_equality_is_a_value_comparison() {
    // The two values are equal but not structurally equal: a derived instance
    // answers False. GHC: True.
    assert_eq!(
        run(&format!(
            "{HALF}{OVER_HALF}main = do {{ write (Just (show_bool (r == t))) stdout; return () }};"
        )),
        "True"
    );
}

#[test]
fn ratio_equality_control_is_false_for_unequal_values() {
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_bool (r == s))) stdout; return () }};"
        )),
        "False"
    );
}

#[test]
fn ratio_ordering_is_numeric_not_structural() {
    // Structural order compares `num` then `den`: 1/2 vs 1/3 gives num equal,
    // den 2 > 3 false — the wrong answer. GHC: 1/2 > 1/3.
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_bool (r > s))) stdout; return () }};"
        )),
        "True"
    );
}

// --- The instances the task supplies ---------------------------------------
//
// Every method below is spelled OUT in the instance rather than inherited from
// a class default. MEASURED: a class default body does not resolve in rill —
// its stored parameter list is empty, so a default that calls a sibling method
// reports "cannot resolve method `compare` of `Ord`: the argument type is not
// concrete". `(/=)`, `<`, `<=`, `>`, `>=`, `min` and `max` ARE such defaults, so
// an instance supplying only `(==)`/`compare` would leave them unusable. That
// is a pre-existing defect of the default mechanism, not something `Ratio`
// introduces: the same program over a MONOMORPHIC user record fails
// identically, and `min 1.0 2.0` on the primitive `Ord Double` reports
// "primitive instance `Ord Double` has no kernel for method `min`".

#[test]
fn ratio_inequality_is_the_value_comparison_too() {
    // `1/2 /= 2/4` is False (the values are equal); GHC agrees.
    assert_eq!(
        run(&format!(
            "{HALF}{OVER_HALF}main = do {{ write (Just (show_bool (r /= t))) stdout; return () }};"
        )),
        "False"
    );
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_bool (r /= s))) stdout; return () }};"
        )),
        "True"
    );
}

#[test]
fn every_ordering_operator_is_numeric_and_agreeing() {
    // All six come from the instance and each answers the VALUE relation:
    // 1/2 is greater than 1/3, so `< <=` are False and `> >=` are True. A
    // structural order would flip all four (num equal, den 2 < 3).
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_bool (r < s))) stdout; return () }};"
        )),
        "False"
    );
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_bool (r <= s))) stdout; return () }};"
        )),
        "False"
    );
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_bool (r >= s))) stdout; return () }};"
        )),
        "True"
    );
    // `compare` is the class's own method: the ORDER ENCODING is 1, -1, 0.
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_int (compare r s))) stdout; return () }};"
        )),
        "1"
    );
    assert_eq!(
        run(&format!(
            "{THIRD}{HALF}main = do {{ write (Just (show_int (compare s r))) stdout; return () }};"
        )),
        "-1"
    );
    assert_eq!(
        run(&format!(
            "{HALF}{OVER_HALF}main = do {{ write (Just (show_int (compare r t))) stdout; return () }};"
        )),
        "0"
    );
}

#[test]
fn min_and_max_compare_by_value() {
    // `min 1/2 1/3 = 1/3` (the smaller VALUE), `max` the larger. A structural
    // `min` would answer by `num` then `den` and pick 1/2 for both.
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_integer ((min r s).den))) stdout; return () }};"
        )),
        "3"
    );
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_integer ((max r s).den))) stdout; return () }};"
        )),
        "2"
    );
}

#[test]
fn ratio_arithmetic_combines_the_fields() {
    // 1/2 + 1/3 = 5/6, 1/2 - 1/3 = 1/6, 1/2 * 1/3 = 1/6 — each already in
    // lowest terms, so GHC's answer and the unreduced cross-product agree here
    // (the reduction is pinned where it DIFFERS, in
    // `arithmetic_results_are_in_lowest_terms`).
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_integer ((r + s).num))) stdout; write (Just \"/\") stdout; write (Just (show_integer ((r + s).den))) stdout; return () }};"
        )),
        "5/6"
    );
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_integer ((r - s).num))) stdout; write (Just \"/\") stdout; write (Just (show_integer ((r - s).den))) stdout; return () }};"
        )),
        "1/6"
    );
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_integer ((r * s).num))) stdout; write (Just \"/\") stdout; write (Just (show_integer ((r * s).den))) stdout; return () }};"
        )),
        "1/6"
    );
}

#[test]
fn ratio_division_and_recip_are_fractional() {
    // 1/2 / 1/3 = 3/2 (GHC: `3 % 2`), recip 1/2 = 2/1.
    assert_eq!(
        run(&format!(
            "{HALF}{THIRD}main = do {{ write (Just (show_integer ((r / s).num))) stdout; write (Just \"/\") stdout; write (Just (show_integer ((r / s).den))) stdout; return () }};"
        )),
        "3/2"
    );
    assert_eq!(
        run(&format!(
            "{HALF}main = do {{ write (Just (show_integer ((recip r).num))) stdout; write (Just \"/\") stdout; write (Just (show_integer ((recip r).den))) stdout; return () }};"
        )),
        "2/1"
    );
}

#[test]
fn every_arithmetic_method_keeps_the_denominator_positive() {
    // Sign normalisation is required of `+ - * /` (and `recip`), not just `/`:
    // each can produce a negative denominator when an INPUT is negative, and a
    // negative denominator is exactly what makes `Ord`/`signum` wrong. Each
    // body now also REDUCES through the `ratio_percent` kernel, so these are
    // GHC's own answers, measured: `1/2 + (-1)/2` is `0%1`;
    // `1/2 - 3/(-2)` is `2%1`; `1/2 * ((-1)/2)` is `(-1)%4`.
    assert_eq!(
        run("a = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\nb = Ratio { num: (-1 :: Integer), den: (2 :: Integer) };\nmain = do { write (Just (show_integer ((a + b).num))) stdout; write (Just \"/\") stdout; write (Just (show_integer ((a + b).den))) stdout; return () };"),
        "0/1"
    );
    assert_eq!(
        run("a = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\nb = Ratio { num: (3 :: Integer), den: (-2 :: Integer) };\nmain = do { write (Just (show_integer ((a - b).den))) stdout; return () };"),
        "1"
    );
    // `1/2 * ((-1)/2)` = `(-1) % 4`: without normalisation the product's
    // denominator is `-4` and `q > 0` answers `True` where the value is
    // negative. (This case happens to be irreducible, so it pins the SIGN half
    // on its own.)
    assert_eq!(
        run("a = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\nb = Ratio { num: (-1 :: Integer), den: (2 :: Integer) };\nq = a * b;\nzero = Ratio { num: (0 :: Integer), den: (1 :: Integer) };\nmain = do { write (Just (show_integer q.num)) stdout; write (Just \"/\") stdout; write (Just (show_integer q.den)) stdout; write (Just (show_bool (q > zero))) stdout; return () };"),
        "-1/4False"
    );
}

#[test]
fn ratio_negate_and_signum_avoid_the_missing_kernels() {
    // The `negate`/`signum` KERNELS have no IR instruction yet ("kernel
    // `Signum`, which has no IR instruction yet", design §3.9) and a bare `abs`
    // call is routed to the buffer unary path ("`abs` signal argument must be a
    // Buffer"). The instance's own equations therefore spell the results with
    // `-`, `+` and `<`, which Integer has.
    assert_eq!(
        run("r = Ratio { num: (3 :: Integer), den: (4 :: Integer) };\nmain = do { write (Just (show_integer ((negate r).num))) stdout; write (Just \"/\") stdout; write (Just (show_integer ((negate r).den))) stdout; return () };"),
        "-3/4"
    );
    assert_eq!(
        run("r = Ratio { num: (-3 :: Integer), den: (4 :: Integer) };\nmain = do { write (Just (show_integer ((signum r).num))) stdout; write (Just \"/\") stdout; write (Just (show_integer ((signum r).den))) stdout; return () };"),
        "-1/1"
    );
    assert_eq!(
        run("r = Ratio { num: (0 :: Integer), den: (4 :: Integer) };\nmain = do { write (Just (show_integer ((signum r).num))) stdout; return () };"),
        "0"
    );
    assert_eq!(
        run("r = Ratio { num: (3 :: Integer), den: (4 :: Integer) };\nmain = do { write (Just (show_integer ((signum r).num))) stdout; return () };"),
        "1"
    );
    // `negate`'s zero is built FROM THE FIELDS (`x.num - x.num`), so the method
    // is not pinned to an `Integer` field: the same instance serves `Ratio Int`.
    // A `(0 :: Integer)` literal in the body would fail this with "cannot unify
    // value type Int with Integer".
    assert_eq!(
        run("r :: Ratio Int;\nr = Ratio { num: 3, den: 4 };\nmain = do { write (Just (show_int ((negate r).num))) stdout; return () };"),
        "-3"
    );
    // `signum` DOES spell a `(1 :: Integer)` (no `fromInteger` exists yet to
    // build a field-typed one), so at `Ratio Int` it is a loud type error — the
    // deliberate limit recorded in the PRELUDE. Pinned so stage 7b removes it
    // by adding `fromInteger`, not by weakening the test. (The direction in the
    // message is `Integer with Int`: the method's own `Integer` literal is the
    // first side.)
    rejects_with(
        "r :: Ratio Int;\nr = Ratio { num: 3, den: 4 };\nmain = do { write (Just (show_int ((signum r).num))) stdout; return () };",
        "cannot unify value type Integer with Int",
    );
}

#[test]
fn rational_is_a_synonym_for_ratio_integer() {
    // `type Rational = Ratio Integer` (Report §9.5). The synonym is
    // transparent: a `Rational`-annotated binding is an ordinary `Ratio Integer`
    // record and its fields project.
    assert_eq!(
        run("a :: Rational;\na = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\nmain = do { write (Just (show_integer a.num)) stdout; write (Just \"/\") stdout; write (Just (show_integer a.den)) stdout; return () };"),
        "1/2"
    );
}

#[test]
fn the_polymorphic_head_beats_the_derived_structural_marker() {
    // The load-bearing measurement (spec §1.3b). `derive_eq_ord` gives every
    // COMPARABLE declared type a structural `Eq`/`Ord` marker, and `Ratio` is
    // comparable, so the marker exists. A POLYMORPHIC explicit instance
    // (`Ratio a`) replaces it; a CONCRETE one (`Ratio Integer`) keys a
    // different registry entry and the marker keeps answering `False`.
    //
    // `Ratio Int` proves the head is genuinely polymorphic: the SAME instance
    // serves a field type that is not `Integer`, and still compares by value.
    assert_eq!(
        run("r :: Ratio Int;\nr = Ratio { num: 1, den: 2 };\ns :: Ratio Int;\ns = Ratio { num: 2, den: 4 };\nmain = do { write (Just (show_bool (r == s))) stdout; return () };"),
        "True"
    );
    // The structural answer really is different, so the assertion above cannot
    // pass by accident: the two records differ in BOTH fields.
    assert_eq!(
        run("r = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\ns = Ratio { num: (2 :: Integer), den: (4 :: Integer) };\nmain = do { write (Just (show_bool (r.num == s.num))) stdout; return () };"),
        "False"
    );
}

#[test]
fn division_normalises_the_sign() {
    // GHC: `(1%2) / ((-3)%4)` is `(-2) % 3` — the sign lives on the numerator
    // AND the pair is in lowest terms. Both halves come from the same
    // `ratio_percent` kernel the bodies call, so this is GHC's own answer.
    assert_eq!(
        run("a = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\nb = Ratio { num: (-3 :: Integer), den: (4 :: Integer) };\nq = a / b;\nmain = do { write (Just (show_integer q.num)) stdout; write (Just (show_integer q.den)) stdout; return () };"),
        "-23"
    );
    assert_eq!(
        run("a = Ratio { num: (-1 :: Integer), den: (2 :: Integer) };\nq = recip a;\nmain = do { write (Just (show_integer q.num)) stdout; write (Just (show_integer q.den)) stdout; return () };"),
        "-21"
    );
    // `(/)` and `recip` reduce through an `Integer`-only kernel, so they are
    // available at `Ratio Integer` alone. A `Ratio Int` division is REJECTED
    // LOUDLY rather than answering an unreduced pair — GHC accepts it, so this
    // is the recorded open defect A6/OD-24 (no `Integral` class), pinned here
    // so that adding `Integral` is what removes it, not a silent behaviour
    // change.
    rejects_with(
        "a :: Ratio Int;\na = Ratio { num: 1, den: 2 };\nq = recip a;\nmain = do { write (Just (show_int q.num)) stdout; return () };",
        "cannot unify value type Int with Integer",
    );
    // `negate` does NOT go through the kernel (H2010's `negate` does not
    // re-normalise), so it stays payload-generic and answers at `Ratio Int`.
    assert_eq!(
        run("a :: Ratio Int;\na = Ratio { num: 1, den: 2 };\nq = negate a;\nmain = do { write (Just (show_int q.num)) stdout; write (Just \"/\") stdout; write (Just (show_int q.den)) stdout; return () };"),
        "-1/2"
    );
}

#[test]
fn ordering_is_correct_for_a_negative_denominator_input() {
    // The value is 1/2 / (-3/4) = -2/3, so it is BELOW zero. A cross-multiplying
    // `compare` on an unnormalised 4/-6 answers True — the silent wrong answer
    // that sign normalisation removes.
    assert_eq!(
        run("a = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\nb = Ratio { num: (-3 :: Integer), den: (4 :: Integer) };\nq = a / b;\nzero = Ratio { num: (0 :: Integer), den: (1 :: Integer) };\nmain = do { write (Just (show_bool (q > zero))) stdout; return () };"),
        "False"
    );
}

#[test]
fn a_zero_denominator_faults() {
    // GHC: "Ratio has zero denominator". Both accepted channels are loud (a
    // compile error, or a run-time fault on rill's clean fault channel); what
    // is rejected is a program that RUNS, which is the old `den = 0` answer.
    // The message is asserted, not any failure: an unrelated fault would also
    // stop the program.
    let msg = run_faults("a = Ratio { num: (0 :: Integer), den: (1 :: Integer) };\nq = recip a;\nmain = do { write (Just (show_integer q.den)) stdout; return () };");
    assert!(
        msg.contains("Ratio has zero denominator"),
        "expected the zero-denominator fault, got: {msg}"
    );
    // `(/)` reaches the same fault when the divisor's numerator is zero —
    // `1/2 / 0/1` is GHC's `Ratio has zero denominator` too, not `4/0`.
    let msg = run_faults("a = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\nb = Ratio { num: (0 :: Integer), den: (1 :: Integer) };\nq = a / b;\nmain = do { write (Just (show_integer q.den)) stdout; return () };");
    assert!(
        msg.contains("Ratio has zero denominator"),
        "expected the zero-denominator fault from `/`, got: {msg}"
    );
}

#[test]
fn a_user_error_definition_shadows_the_prelude_error() {
    // `error` is Prelude §9.1's ordinary, SHADOWABLE binding — a user program
    // that defines its own `error` must get its own function, not a fault.
    // This is the control that keeps the builtin from behaving like a keyword.
    //
    // Note the Ratio fault does NOT depend on this name any more: the instance
    // bodies spell their bottom `__fault` (the prelude-internal reserved
    // spelling, resolved before any definition-site arm), so shadowing `error`
    // is safe — see `a_user_error_does_not_hijack_the_ratio_fault`.
    assert_eq!(
        run("error x = x + 1.0;\nmain = do { write (Just (show_double (error 1.0))) stdout; return () };"),
        "2.0"
    );
    // A non-String message is a type error, not an ignored argument.
    rejects_with("main = error 1.0;", "`error` expects a String message");
}

#[test]
fn a_user_error_does_not_hijack_the_ratio_fault() {
    // The Ratio instance's bottom must not be reachable through a shadowable
    // name: `recip (0/1)` has to fault even when the program defines `error`.
    // MEASURED before the fix: `q` was `7/1` — the instance body's `error` was
    // inlined BY NAME and the user's binding answered it. `run_faults` accepts
    // either loud channel but rejects "the program ran", so a `7/1` answer
    // panics; the message check additionally rules out an unrelated failure.
    let msg = run_faults("error x = Ratio { num: (7 :: Integer), den: (1 :: Integer) };\na = Ratio { num: (0 :: Integer), den: (1 :: Integer) };\nq = recip a;\nmain = do { write (Just (show_integer q.num)) stdout; return () };");
    assert!(
        msg.contains("Ratio has zero denominator"),
        "the Ratio zero-denominator fault must survive a user `error`, got: {msg}"
    );
}

#[test]
fn the_internal_bottom_cannot_be_shadowed_by_a_user_binding() {
    // `__fault` is the PRELUDE's internal bottom, compared by exact spelling at
    // the FIRST arm of application lowering. A user definition of that name must
    // not intercept it: if the arm ever moves after the definition-site arms,
    // this program stops faulting and reports a type error instead.
    //
    // The user binding carries a CONFLICTING signature on purpose — that is the
    // shape a lookup-based (rather than ordering-based) guard would honour.
    let msg = run_faults("__fault :: Int -> Int;\n__fault x = x;\na = Ratio { num: (0 :: Integer), den: (1 :: Integer) };\nq = recip a;\nmain = do { write (Just (show_integer q.den)) stdout; return () };");
    assert!(
        msg.contains("Ratio has zero denominator"),
        "a user `__fault` must not intercept the Ratio zero-denominator fault, got: {msg}"
    );
}

#[test]
fn a_lambda_parameter_named_error_is_captured() {
    // H2010: a parameter shadows a Prelude name, including inside a nested
    // lambda. This compiled before `error` became a special form and must keep
    // compiling. MEASURED before the fix: "`error` expects a String message,
    // got Double" — the special form ran before the capture set was consulted.
    assert_eq!(
        run("f error = \\y -> error y;\ninc x = x + 1.0;\nmain = do { write (Just (show_double ((f inc) 1.0))) stdout; return () };"),
        "2.0"
    );
}

// --- `gcd`, `%`, `numerator` and `denominator` (Data.Ratio's surface) ------
//
// Task 4. `%` is a real operator now (`infixl 7`, the Report's precedence for
// `Data.Ratio.%`), `gcd` is a builtin at `Integer`, and `numerator`/
// `denominator` are ordinary PRELUDE field projections — NOT builtins, because
// a builtin beside `.num` would be a second mechanism for reading a field.

#[test]
fn gcd_is_an_arbitrary_precision_integer_gcd() {
    // GHC: `gcd 12 18` is 6, and `gcd` of a negative pair is NON-NEGATIVE
    // (`gcd (-4) 6` is 2) — the sign is a property of the algorithm, not of
    // the inputs.
    assert_eq!(
        run("main = do { write (Just (show_integer (gcd (12 :: Integer) (18 :: Integer)))) stdout; return () };"),
        "6"
    );
    assert_eq!(
        run("main = do { write (Just (show_integer (gcd ((0 :: Integer) - 4) (6 :: Integer)))) stdout; return () };"),
        "2"
    );
    assert_eq!(
        run("main = do { write (Just (show_integer (gcd (0 :: Integer) (0 :: Integer)))) stdout; return () };"),
        "0"
    );
    // Both-negative and mixed-sign pairs are non-negative too, and the parity
    // cases are where a hand-rolled binary GCD usually breaks: `gcd 48 18` (both
    // even, common factor of two), `gcd 17 5` (both odd), `gcd 0 7` / `gcd 7 0`
    // (one zero), `gcd (-12) (-18)` (both negative).
    assert_eq!(
        run("main = do { write (Just (show_integer (gcd (48 :: Integer) (18 :: Integer)))) stdout; write (Just (show_integer (gcd (17 :: Integer) (5 :: Integer)))) stdout; write (Just (show_integer (gcd (0 :: Integer) (7 :: Integer)))) stdout; write (Just (show_integer (gcd (7 :: Integer) (0 :: Integer)))) stdout; write (Just (show_integer (gcd ((0 :: Integer) - 12) ((0 :: Integer) - 18)))) stdout; return () };"),
        "61776"
    );
    // The kernel works on `BigInt`, so a gcd far outside `i64` is exact.
    // GHC: `gcd 123456789012345678901234567890 98765432109876543210987654321`
    // is `900000000090000000009`.
    assert_eq!(
        run("main = do { write (Just (show_integer (gcd (123456789012345678901234567890 :: Integer) (98765432109876543210987654321 :: Integer)))) stdout; return () };"),
        "900000000090000000009"
    );
}

#[test]
fn gcd_is_an_ordinary_shadowable_prelude_binding() {
    // H2010 Prelude §6.4.2: `gcd` is a function, not a keyword, so a program
    // may define its own — exactly like `error`. The public `gcd` is therefore
    // a PRELUDE DEFINITION over the reserved `__gcd` kernel, not a builtin (a
    // builtin would have reserved the spelling, which H2010 does not allow).
    assert_eq!(
        run("gcd a b = (99 :: Integer);\nmain = do { write (Just (show_integer (gcd (4 :: Integer) (6 :: Integer)))) stdout; return () };"),
        "99"
    );
    assert_eq!(
        run("gcd a b = (99 :: Integer);\nf x = gcd x (6 :: Integer);\nmain = do { write (Just (show_integer (f (4 :: Integer)))) stdout; return () };"),
        "99"
    );
    // The kernel is NOT shadowable: it is spelled `__gcd` and resolved by
    // reserved spelling ahead of every definition-site arm, so the Prelude's
    // own `gcd` definition still answers 2. (Without that ordering the kernel
    // was a plain name and MEASURED `gcd 4 6` answered 99.)
    assert_eq!(
        run("__gcd a b = (99 :: Integer);\nmain = do { write (Just (show_integer (gcd (4 :: Integer) (6 :: Integer)))) stdout; return () };"),
        "2"
    );
}

#[test]
fn percent_normalizes() {
    // GHC: numerator (3 % 6) is 1, denominator is 2.
    assert_eq!(
        run("r = 3 % 6;\nmain = do { write (Just (show_integer (numerator r))) stdout; return () };"),
        "1"
    );
    assert_eq!(
        run("r = 3 % 6;\nmain = do { write (Just (show_integer (denominator r))) stdout; return () };"),
        "2"
    );
}

#[test]
fn the_sign_lives_on_the_numerator() {
    // GHC: `1 % (-2)` prints as `(-1) % 2`.
    assert_eq!(
        run("r = (1 :: Integer) % (-2);\nmain = do { write (Just (show_integer (numerator r))) stdout; write (Just (show_integer (denominator r))) stdout; return () };"),
        "-12"
    );
}

#[test]
fn numerator_and_denominator_are_field_projections() {
    // They read the fields, they do not normalise: a hand-written unreduced
    // record reports its fields as written. This is what makes them
    // projections rather than a second construction mechanism, and it is why
    // the ARITHMETIC bodies must reduce for their answers to match GHC.
    assert_eq!(
        run("r = Ratio { num: (6 :: Integer), den: (8 :: Integer) };\nmain = do { write (Just (show_integer (numerator r))) stdout; write (Just \"/\") stdout; write (Just (show_integer (denominator r))) stdout; return () };"),
        "6/8"
    );
}

#[test]
fn percent_is_a_fixity_table_operator() {
    // `infixl 7` (Data.Ratio). Precedence is pinned against the COMPARISON
    // level (4): `%` binds tighter, so this parses as `(1%2) == (2%4)` and
    // answers True. Were `%` looser, the parse would be `1 % (2 == 2) % 4` — a
    // type error, not a True.
    assert_eq!(
        run("main = do { write (Just (show_bool ((1 :: Integer) % 2 == 2 % 4))) stdout; return () };"),
        "True"
    );
    // ... and against the ADDITIVE level (6): `1 % 2 + 1 % 2` is
    // `(1%2) + (1%2)` = `1 % 1`. A looser `%` would parse
    // `1 % (2 + 1) % 2`, a type error — so the value changes with the level.
    assert_eq!(
        run("main = do { write (Just (show_integer (numerator ((1 :: Integer) % 2 + 1 % 2)))) stdout; write (Just \"/\") stdout; write (Just (show_integer (denominator ((1 :: Integer) % 2 + 1 % 2)))) stdout; return () };"),
        "1/1"
    );
    // ... and it is NOT looser than `*` either: `2 * 1 % 4` is `(2*1) % 4` =
    // `2 % 4` = `1 % 2`. Under a `%` TIGHTER than `*` that would be
    // `2 * (1 % 4)`, a type error.
    assert_eq!(
        run("main = do { write (Just (show_integer (numerator (2 * (1 :: Integer) % 4)))) stdout; write (Just \"/\") stdout; write (Just (show_integer (denominator (2 * (1 :: Integer) % 4)))) stdout; return () };"),
        "1/2"
    );
    // Associativity at `%`'s OWN level is not observable: both readings of
    // `a % b % c` are type errors, because `%`'s operands are `Integral` and a
    // `Ratio` is not. `infixl 7` is therefore recorded in the fixity table (and
    // here) rather than pinned by a program.
    rejects_with(
        "q :: Ratio Integer;\nq = (12 :: Integer) % 8 % 2;\nmain = do { write (Just (show_integer (numerator q))) stdout; return () };",
        "cannot unify value type",
    );
}

#[test]
fn a_zero_denominator_percent_faults() {
    // GHC: `1 % 0` is "Ratio has zero denominator". rill's own partial
    // operations spell that bottom `__fault`, which a user `error` cannot
    // hijack — so the message must survive a shadowing `error` as well.
    let msg = run_faults("q = (1 :: Integer) % 0;\nmain = do { write (Just (show_integer (numerator q))) stdout; return () };");
    assert!(
        msg.contains("Ratio has zero denominator"),
        "expected the zero-denominator fault from `%`, got: {msg}"
    );
    let msg = run_faults("error x = (7 :: Integer);\nq = (1 :: Integer) % 0;\nmain = do { write (Just (show_integer (numerator q))) stdout; return () };");
    assert!(
        msg.contains("Ratio has zero denominator"),
        "the `%` zero-denominator fault must survive a user `error`, got: {msg}"
    );
}

#[test]
fn arithmetic_results_are_in_lowest_terms() {
    // GHC: `(1%2) / ((-3)%4)` is `(-2) % 3`.
    assert_eq!(
        run("a = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\nb = Ratio { num: (-3 :: Integer), den: (4 :: Integer) };\nq = a / b;\nmain = do { write (Just (show_integer (numerator q))) stdout; write (Just (show_integer (denominator q))) stdout; return () };"),
        "-23"
    );
    // GHC: `numerator (1%2 + 1%2)` is 1, denominator 1.
    assert_eq!(
        run("a = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\nq = a + a;\nmain = do { write (Just (show_integer (numerator q))) stdout; write (Just (show_integer (denominator q))) stdout; return () };"),
        "11"
    );
    // `recip` reduces too: `recip (6%8)` is `4 % 3` (GHC), not `8 % 6`.
    assert_eq!(
        run("q = recip (Ratio { num: (6 :: Integer), den: (8 :: Integer) });\nmain = do { write (Just (show_integer (numerator q))) stdout; write (Just \"/\") stdout; write (Just (show_integer (denominator q))) stdout; return () };"),
        "4/3"
    );
    // ... and so do `-` and `*`: `3/6 - 1/6` is `1 % 3` (GHC), `2/3 * 3/4` is
    // `1 % 2`.
    assert_eq!(
        run("a = Ratio { num: (3 :: Integer), den: (6 :: Integer) };\nb = Ratio { num: (1 :: Integer), den: (6 :: Integer) };\nq = a - b;\nmain = do { write (Just (show_integer (numerator q))) stdout; write (Just \"/\") stdout; write (Just (show_integer (denominator q))) stdout; return () };"),
        "1/3"
    );
    assert_eq!(
        run("a = Ratio { num: (2 :: Integer), den: (3 :: Integer) };\nb = Ratio { num: (3 :: Integer), den: (4 :: Integer) };\nq = a * b;\nmain = do { write (Just (show_integer (numerator q))) stdout; write (Just \"/\") stdout; write (Just (show_integer (denominator q))) stdout; return () };"),
        "1/2"
    );
}

#[test]
fn a_user_percent_cannot_hijack_the_arithmetic_normalisation() {
    // The instance bodies reduce through the PRELUDE-INTERNAL `__ratio_percent`
    // kernel, not through the surface `%`. That is load-bearing for the same
    // reason Task 3's `__fault` is: an instance body is inlined BY NAME, so if
    // the bodies called `%`, a program's own definition would answer them.
    // MEASURED with the surface spelling: `a / b` answered `7/1`.
    assert_eq!(
        run("(%) a b = Ratio { num: (7 :: Integer), den: (1 :: Integer) };\na = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\nb = Ratio { num: (3 :: Integer), den: (4 :: Integer) };\nq = a / b;\nmain = do { write (Just (show_integer (numerator q))) stdout; write (Just \"/\") stdout; write (Just (show_integer (denominator q))) stdout; return () };"),
        "2/3"
    );
    // ... and the zero-denominator fault still faults under that shadow, so the
    // bottom is not reachable through the operator either.
    let msg = run_faults("(%) a b = Ratio { num: (7 :: Integer), den: (1 :: Integer) };\na = Ratio { num: (0 :: Integer), den: (1 :: Integer) };\nq = recip a;\nmain = do { write (Just (show_integer (numerator q))) stdout; return () };");
    assert!(
        msg.contains("Ratio has zero denominator"),
        "the reduction kernel must not be reachable through a user `%`, got: {msg}"
    );
    // The kernel's OWN spelling is reserved by construction, not by a
    // `builtin_sigs` reservation (that path resolves AFTER the definition-site
    // arms, and MEASURED a user `__ratio_percent` answered `a / b` with `7/1`
    // when it was one). It is resolved first, like `__fault`.
    assert_eq!(
        run("__ratio_percent a b = Ratio { num: (7 :: Integer), den: (1 :: Integer) };\nf x y = __ratio_percent x y;\nj = f (1 :: Integer) (2 :: Integer);\nmain = do { write (Just (show_integer (numerator j))) stdout; return () };"),
        "1"
    );
    assert_eq!(
        run("__ratio_percent a b = Ratio { num: (7 :: Integer), den: (1 :: Integer) };\na = Ratio { num: (1 :: Integer), den: (2 :: Integer) };\nb = Ratio { num: (3 :: Integer), den: (4 :: Integer) };\nq = a / b;\nmain = do { write (Just (show_integer (numerator q))) stdout; write (Just \"/\") stdout; write (Just (show_integer (denominator q))) stdout; return () };"),
        "2/3"
    );
    // The SURFACE `%` is a shadowable Prelude binding too (H2010: `(%)` is a
    // function, not a keyword), so a program's own `(%)` DOES replace what
    // `3 % 6` means — while still not reaching the arithmetic above.
    assert_eq!(
        run("(%) a b = Ratio { num: (7 :: Integer), den: (1 :: Integer) };\nq = (3 :: Integer) % (6 :: Integer);\nmain = do { write (Just (show_integer (numerator q))) stdout; return () };"),
        "7"
    );
}

#[test]
fn percent_and_gcd_reject_a_non_integer_payload() {
    // `%` is `Integer -> Integer -> Ratio Integer` and `gcd` is
    // `Integer -> Integer -> Integer`, because rill has no `Integral` class
    // (design A6 / A4, OD-24). The narrowing is a RECORDED OPEN DEFECT, not a
    // settled behaviour — so a call at another payload is a loud rejection,
    // never a silent `f64` computation. Both names are ordinary Prelude
    // DEFINITIONS, so the diagnostic is the shared call-checking one, naming
    // the offending argument and both types.
    rejects_with(
        "main = do { write (Just (show_integer (gcd 1.0 2.0))) stdout; return () };",
        "argument 1 of `gcd` has type Double, but the parameter has type Integer",
    );
    rejects_with(
        "main = do { write (Just (show_integer (numerator (1.0 % 2.0)))) stdout; return () };",
        "argument 1 of `%` has type Double, but the parameter has type Integer",
    );
    // GHC accepts `Ratio Int` arithmetic (`Integral Int` holds there) and
    // rejects `Ratio Double` (`No instance for 'Integral Double'`). rill
    // rejects BOTH, because the kernel is `Integer`-only — that is the A6 cost,
    // and it is loud in both directions rather than silently unreduced.
    rejects_with(
        "r :: Ratio Int;\nr = 1 % 2;\nmain = do { write (Just (show_int (numerator r))) stdout; return () };",
        "cannot unify value type Integer with Int",
    );
}

#[test]
fn from_integer_is_deliberately_omitted_pending_the_integer_signature() {
    // `Num (Ratio a)` supplies no `fromInteger`, because the Prelude still
    // declares it `fromInteger : Int -> a`: a body would have to unify the `Int`
    // argument with the `Integer` field and fails with "cannot unify value type
    // Int with Integer". An omitted method fails LOUDLY at the call site
    // instead of silently building a wrong value; stage 7b corrects the
    // signature to `Integer -> a` and adds the body.
    //
    // The position is SIGNED, not inferred: `f :: Ratio Integer -> Ratio
    // Integer; f x = fromInteger 2` fixes the method's result type from the
    // signature, so the rejection is the missing instance itself and not a
    // numeric-defaulting coincidence. (MEASURED: this belongs to the same
    // diagnostic; the defaulting path reaches it too, which is exactly why the
    // pin must not depend on defaulting.)
    rejects_with(
        "f :: Ratio Integer -> Ratio Integer;\nf x = fromInteger 2;\nmain = f (Ratio { num: (1 :: Integer), den: (2 :: Integer) });",
        "no instance of `Num` for type `Ratio` and no default for `fromInteger`",
    );
    // The original inferred-position form, kept as the second measurement: it
    // reaches the same diagnostic through the expected `Ratio` field type.
    rejects_with(
        "main = do { write (Just (show_integer (fromInteger 2).num)) stdout; return () };",
        "no instance of `Num` for type `Ratio` and no default for `fromInteger`",
    );
    // `fromRational` is not a `Fractional` method yet, so it is not a name at
    // all — the same two-step, one level up.
    rejects_with(
        "main = do { write (Just (show_integer (fromRational 2.0).num)) stdout; return () };",
        "unknown identifier `fromRational`",
    );
}

// --- Reserved kernel spellings survive EVERY pass (fix round 1) ------------
//
// The kernels behind `gcd`, `%` and the `Ratio` fault are not reachable through
// a name a program can redefine. Two previous attempts expressed that as a
// reserved SPELLING compared ahead of the definition-site arms, first in
// `lower` (Task 3), then in `infer` as well. Both were defeated by the same
// thing: a pass nobody had covered yet.
//
// `crate::compile_to_ir_entry` runs tokenize → parse → desugar → infer →
// **reduce** → lower. A reserved-spelling guard in `infer` and in `lower` still
// leaves `reduce` between them, and `reduce` β-reduces a user `__gcd` that
// carries a TYPE SIGNATURE into the Prelude's `gcd` body before lowering sees
// the name. MEASURED at HEAD: `gcd 12 18` answered `99`.
//
// The fix binds the operation to an `Expr::Kernel` node at PARSE time, so there
// is no name for any pass to look up. Every test below uses the SIGNED user
// definition — the shape that defeated the spelling guard.
//
// NAMING NOTE (fix round 2): these were called `<pass>_pass_...` and claimed to
// pin `infer`, `reduce` and `lower` SEPARATELY. They do not — measured under
// mutation, the three fail and pass TOGETHER. The β-reduction they originally
// targeted can no longer occur (`reduce` sees a node, not a name), and with one
// shared node every pass that used to compare a spelling is now bypassed by the
// same single mechanism, so no per-pass mutation can distinguish them. The names
// now say what they actually pin: that no signed user spelling of a kernel can
// change the answer, checked at three different operand pairs.

#[test]
fn a_signed_user_kernel_spelling_cannot_change_gcd() {
    // The signed user definition is the shape `reduce` β-reduced before the node
    // existed, and the shape that made the `infer`/`lower` spelling comparisons
    // resolve against the user's body. Three operand pairs, so an answer that
    // coincides with a small literal cannot pass by luck.
    //
    // GHC has no `__gcd`; the Prelude's `gcd` answers these.
    assert_eq!(
        run("__gcd :: Integer -> Integer -> Integer;\n__gcd a b = 99;\nmain = do { write (Just (show_integer (gcd (12 :: Integer) (18 :: Integer)))) stdout; return () };"),
        "6"
    );
    assert_eq!(
        run("__gcd :: Integer -> Integer -> Integer;\n__gcd a b = 99;\nmain = do { write (Just (show_integer (gcd (144 :: Integer) (60 :: Integer)))) stdout; return () };"),
        "12"
    );
    assert_eq!(
        run("__gcd :: Integer -> Integer -> Integer;\n__gcd a b = 99;\nmain = do { write (Just (show_integer (gcd (0 :: Integer) (7 :: Integer)))) stdout; return () };"),
        "7"
    );
    // The kernel reached from INSIDE a definition body must also be the kernel,
    // not the user's function — otherwise the Prelude's own `gcd` definition
    // would compute against the user's body.
    assert_eq!(
        run("__gcd :: Integer -> Integer -> Integer;\n__gcd a b = 99;\nf x y = gcd x y;\nmain = do { write (Just (show_integer (f (12 :: Integer) (18 :: Integer)))) stdout; return () };"),
        "6"
    );
}

#[test]
fn a_signed_user_ratio_percent_spelling_cannot_change_the_normalisation() {
    // The same attack against the OTHER integer kernel. `3 % 6` must still be
    // `1/2`: the signature makes the user `__ratio_percent` β-reducible, so a
    // name-based guard let `7/1` through. MEASURED before the node existed:
    // this program printed 7.
    assert_eq!(
        run("__ratio_percent :: Integer -> Integer -> Ratio Integer;\n__ratio_percent a b = Ratio { num: 7, den: 1 };\nmain = do { write (Just (show_integer (numerator ((3 :: Integer) % (6 :: Integer))))) stdout; write (Just \"/\") stdout; write (Just (show_integer (denominator ((3 :: Integer) % (6 :: Integer))))) stdout; return () };"),
        "1/2"
    );
    // ... and the ARITHMETIC bodies must not have been rewritten to the user's
    // kernel either: `1/6 + 1/6` is `1/3` in GHC.
    assert_eq!(
        run("__ratio_percent :: Integer -> Integer -> Ratio Integer;\n__ratio_percent a b = Ratio { num: 7, den: 1 };\na = Ratio { num: (1 :: Integer), den: (6 :: Integer) };\nq = a + a;\nmain = do { write (Just (show_integer (numerator q))) stdout; write (Just \"/\") stdout; write (Just (show_integer (denominator q))) stdout; return () };"),
        "1/3"
    );
}

#[test]
fn a_signed_user_fault_spelling_cannot_replace_the_language_bottom() {
    // `__fault` has the same shape and therefore the same hole, so it is pinned
    // under the same attack. The signed user binding must not replace the
    // language's own bottom: `recip (0/1)` still faults with GHC's message.
    //
    // SCOPE OF THIS TEST (fix round 2): it pins the NODE PATH only. The PRELUDE
    // no longer contains a `__fault` call SITE — the Ratio fault is emitted
    // inline by lowering — so nothing here exercises a kernel node reached from
    // prelude source. What it does pin is that the `__fault` NODE built by
    // `parser::curry_expr` from the program's own spelling is the language's
    // bottom, and that a signed user definition of that spelling cannot be
    // substituted into the `Ratio` instance body.
    //
    // MEASURED before the node existed: `recip (0/1)` compiled against the
    // user's `__fault`, whose body was β-reduced into the instance body.
    let msg = run_faults("__fault :: String -> Integer;\n__fault x = 99;\na = Ratio { num: (0 :: Integer), den: (1 :: Integer) };\nq = recip a;\nmain = do { write (Just (show_integer q.den)) stdout; return () };");
    assert!(
        msg.contains("Ratio has zero denominator"),
        "a SIGNED user `__fault` must not intercept the Ratio zero-denominator fault, got: {msg}"
    );
    // The spelling is INTERNAL (`ir.rs`: "not part of the language's
    // surface"), so a program that writes it reaches the KERNEL, not its own
    // definition — the same rule that makes the invocation un-interceptable.
    // Pinned rather than left implicit, because "a user may shadow it" and "a
    // user may not" are the two readings the earlier rounds oscillated between.
    let msg = run_faults("__fault :: String -> Integer;\n__fault x = 99;\nmain = do { write (Just (show_integer (__fault \"boom\"))) stdout; return () };");
    assert!(
        msg.contains("boom"),
        "a program's own `__fault` call reaches the kernel, whose message is the argument, got: {msg}"
    );
}

#[test]
fn percent_sections_parse_and_apply() {
    // Haskell 2010 §3.5: `(% 2)` is `\x -> x % 2` (a RIGHT section) and
    // `(2 %)` is `\x -> 2 % x` (a LEFT section). Both are valid surface
    // syntax, and GHC accepts both.
    //
    // MEASURED at HEAD: both PANICKED the compiler with
    // `panicked at parser.rs: "a bare operator name always maps to an InfixOp"`.
    // `bare_operator_name` had learned `Tok::Percent` (`(% 2)` and `(2 %)` were
    // therefore RECOGNISED as sections) while `infix_op_from_name` had not, so a
    // valid program reached an `expect` the table could not satisfy. At base
    // both were a clean parse error, so this was a NEW user-reachable panic.
    //
    // Expected values are GHC's, checked with `runghc`, not assumed:
    //     > numerator ((% 2) 3)      -- 3
    //     > denominator ((% 2) 3)    -- 2
    //     > numerator ((2 %) 3)      -- 2
    //     > denominator ((2 %) 3)    -- 3
    // i.e. `(% 2) 3` is `3 % 2` = 3/2 and `(2 %) 3` is `2 % 3` = 2/3.
    assert_eq!(
        run("main = do { write (Just (show_integer (numerator ((% 2) 3)))) stdout; return () };"),
        "3"
    );
    assert_eq!(
        run("main = do { write (Just (show_integer (denominator ((% 2) 3)))) stdout; return () };"),
        "2"
    );
    assert_eq!(
        run("main = do { write (Just (show_integer (numerator ((2 %) 3)))) stdout; return () };"),
        "2"
    );
    assert_eq!(
        run("main = do { write (Just (show_integer (denominator ((2 %) 3)))) stdout; return () };"),
        "3"
    );
    // The two sections are DIFFERENT functions: `(% 2) 3` is 3/2 and `(2 %) 3`
    // is 2/3, so a fix that made both sides of the section the same value would
    // fail here even though both single assertions above could pass.
    assert_eq!(
        run("main = do { write (Just (show_integer (numerator ((% 2) 3) - numerator ((2 %) 3)))) stdout; return () };"),
        "1"
    );
}

#[test]
fn every_bare_operator_name_a_section_can_name_has_an_infix_op() {
    // The panic in `percent_sections_parse_and_apply` was a TABLE DISAGREEMENT:
    // `bare_operator_name` (which decides what a section may name) and
    // `infix_op_from_name` (which maps that name to an operator) must accept
    // exactly the same set, or a recognised section reaches an `expect`.
    //
    // This pins the second operator the tables disagreed about so the class of
    // defect is covered, not just the instance: `/=` is what `bare_operator_name`
    // returns for `Tok::NotEq` while `infix_op_from_name` spells the same
    // operator `"!="`. Whichever spelling wins must not panic, so both section
    // forms are exercised through the public compile path.
    assert_eq!(
        run("f = (/= 2);\nmain = do { write (Just (show_bool (f 1))) stdout; return () };"),
        "True"
    );
    assert_eq!(
        run("f = (1 /=);\nmain = do { write (Just (show_bool (f 2))) stdout; return () };"),
        "True"
    );
}

// --- Every expression walker visits a kernel's ARGUMENTS (fix round 2) ------
//
// `Expr::Kernel` has no callee name — that is what makes it interception-proof.
// The cost is that a walker with a `_ => {}` catch-all silently stops at the
// node and never sees its children. Measured at `e16de37`: the `let`-bound
// helper below was dropped by `reduce`'s binding-pruning step ("unknown `g` in
// value lowering") while the `where` spelling of the same program survived.
//
// The walkers that visit expression children are `lower::collect_def_ref_names`
// — which ALSO backs `lower::compute_recursive_locals` and
// `lower::body_carries_class_demand` — and `infer::collect_ref_names` (the
// complete-mode recursion re-rate). Every test here uses a binding reachable
// ONLY from inside a kernel argument, so a walker that stops at the node
// answers wrong.
//
// Two of the four have NO public-surface discriminator, measured, and are
// pinned by a unit test beside the walker instead of a golden value:
//
// - `lower::compute_recursive_locals`: every program whose only path to a local
//   self-reference runs through a kernel argument fails EARLIER, in inference,
//   for unrelated `Int`/`Integer` reasons (a local `f n = case n == 0 of …`
//   gets a `Double` λ-parameter, so `gcd (f …)` is an OD-24 type error). The
//   cycle detection never runs on a shape that would expose it.
//   → `lower::tests::compute_recursive_locals_sees_a_kernel_argument`
// - `infer::collect_ref_names`: a mutual-recursion partner reachable only
//   through a kernel argument compiles and runs IDENTICALLY with and without
//   the arm (verified by mutation over the whole suite plus 20 hand-built
//   partner shapes) — the re-rate's effect is masked by later instantiation.
//   → `types::infer::tests::collect_ref_names_walks_a_kernel_arguments`

#[test]
fn a_let_bound_helper_used_only_inside_a_kernel_argument_survives() {
    // `gcd`'s argument is an `Expr::Kernel` node; a walker that ignores the
    // node's children thinks `g` is unreferenced and `reduce` drops the binding.
    // GHC: 6.
    assert_eq!(
        run("main = let { g x = (12 :: Integer); } in do { write (Just (show_integer (gcd (g 0) (18 :: Integer)))) stdout; return () };"),
        "6"
    );
}

#[test]
fn a_where_bound_helper_used_only_inside_a_kernel_argument_survives() {
    // The same program in `where` spelling. This one happened to survive at
    // `e16de37`, which is why it is pinned BESIDE the `let` form: the two
    // spellings must not disagree.
    assert_eq!(
        run("f x = gcd (g x) (18 :: Integer);\ng y = (12 :: Integer);\nmain = do { write (Just (show_integer (f 0))) stdout; return () };"),
        "6"
    );
}

#[test]
fn a_class_method_demand_inside_a_kernel_argument_is_seen() {
    // `body_carries_class_demand` decides whether a signature-less curried def
    // needs per-call-site specialization. The `Ix` demand below (`a ! i`)
    // lives ONLY in `gcd`'s argument. When the walker sees it, a bare
    // reference is rejected as a class-constrained definition that must be
    // applied; when it does not, the body is lowered as an ordinary value and
    // dies in the `Ix` dispatch with an opaque missing-default message.
    rejects_with(
        "at a i = gcd (a ! i) (16 :: Integer);\ng = at;\nmain = do { write (Just (show_integer (g (listArray (Pair { first: 0, second: 2 }) [(8 :: Integer), (8 :: Integer), (3 :: Integer)]) 1))) stdout; return () };",
        "cannot use class-constrained definition `at` without applying it",
    );
}
