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
    // 1/2 + 1/3 = 5/6, 1/2 - 1/3 = 1/6, 1/2 * 1/3 = 1/6. Nothing normalises
    // yet (`%`/`gcd` are a later task), so the denominator is the product.
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
fn from_integer_is_deliberately_omitted_pending_the_integer_signature() {
    // `Num (Ratio a)` supplies no `fromInteger`, because the Prelude still
    // declares it `fromInteger : Int -> a`: a body would have to unify the `Int`
    // argument with the `Integer` field and fails with "cannot unify value type
    // Int with Integer". An omitted method fails LOUDLY at the call site
    // instead of silently building a wrong value; stage 7b corrects the
    // signature to `Integer -> a` and adds the body.
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
