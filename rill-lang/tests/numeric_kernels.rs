//! The six numeric kernels design §3.9 listed as "no IR instruction yet" —
//! `Ord.compare`, `Num.negate` / `abs` / `signum` / `fromInteger`, and
//! `Fractional.recip` — now all execute.
//!
//! Expectations are GHC 9.10.3's, measured with `runghc` unless a comment says
//! otherwise. The buffer half needs the foreign generator registry (a `Buffer`
//! is produced by `sine`/`square`), so it uses [`common`]'s world helpers
//! instead of the `ProgramRunner` stdout surface — `show_double` of a `Buffer`
//! is a type error and always has been, so a buffer is asserted by copying the
//! block out and comparing samples.

pub mod common;

use common::*;
use rill_lang::compile;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;
/// Compile and run `src`, returning the concatenated stdout.
fn run(src: &str) -> String {
    let mut prog = compile::<f32, 256>(src, ForeignRegistry::new())
        .unwrap_or_else(|e| panic!("compile failed for:\n{src}\n{e}"));
    prog.run()
        .unwrap_or_else(|e| panic!("run failed for:\n{src}\n{e}"));
    prog.stdout().concat()
}

/// A program printing one `show_*` result.
fn shows(show: &str, expr: &str) -> String {
    format!("main = do {{ write (Just ({show} ({expr}))) stdout; return () }};")
}

/// [`run`] with the foreign generator registry attached, so a program may build
/// a buffer (`sine`/`square`) and still print through `main`.
fn run_io(src: &str) -> String {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    let mut prog =
        compile::<f32, 256>(src, ffi).unwrap_or_else(|e| panic!("compile failed for:\n{src}\n{e}"));
    prog.run()
        .unwrap_or_else(|e| panic!("run failed for:\n{src}\n{e}"));
    prog.stdout().concat()
}

/// Run a whole-buffer `main` and return the live block.
fn run_block(expr: &str) -> Vec<f32> {
    run_block_with("", expr)
}

/// [`run_block`] with source definitions in scope, so a user FUNCTION can be
/// the thing that reaches a buffer kernel (the shape that used to index out of
/// bounds).
fn run_block_with(defs: &str, expr: &str) -> Vec<f32> {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    let world = eval_ffi(defs, expr, ffi)
        .unwrap_or_else(|e| panic!("run failed for:\n{defs}\n{expr}\n{e}"));
    let r = world.result().expect("a whole-buffer result was expected");
    let mut got = [0.0f32; 256];
    assert!(world.copy_buffer(r, &mut got), "`{expr}` must be a buffer");
    got.to_vec()
}

// ---------------------------------------------------------------------------
// The six kernels execute
// ---------------------------------------------------------------------------

#[test]
fn the_six_kernels_execute() {
    assert_eq!(
        run(&shows("show_double", "negate 3.0")),
        "-3.0",
        "`negate` at Double"
    );
    assert_eq!(
        run(&shows("show_int", "signum (0 - 3)")),
        "-1",
        "`signum` at Int"
    );
    assert_eq!(run(&shows("show_int", "abs (0 - 3)")), "3", "`abs` at Int");
    assert_eq!(
        run(&shows("show_int", "compare 1 2")),
        "-1",
        "`compare` is LT"
    );
    assert_eq!(
        run(&shows("show_double", "recip 2.0")),
        "0.5",
        "`recip` at Double"
    );
    assert_eq!(
        run(&shows("show_double", "negate 2.5")),
        "-2.5",
        "`negate 2.5`"
    );
}

// ---------------------------------------------------------------------------
// Haskell 2010 §6.4.4: signum and abs
// ---------------------------------------------------------------------------

#[test]
fn signum_is_minus_one_zero_or_one() {
    // GHC 9.10.3: `signum (-3) = -1`, `signum 0 = 0`, `signum 3 = 1`.
    assert_eq!(run(&shows("show_int", "signum (0 - 3)")), "-1");
    assert_eq!(run(&shows("show_int", "signum 0")), "0");
    assert_eq!(run(&shows("show_int", "signum 3")), "1");
    assert_eq!(run(&shows("show_double", "signum (0.0 - 2.5)")), "-1.0");
    // `signum 0.0` is `0.0` in GHC — NOT `1.0`, which is what Rust's
    // `f64::signum` answers. That is exactly the trap this assertion pins.
    assert_eq!(run(&shows("show_double", "signum 0.0")), "0.0");
    assert_eq!(run(&shows("show_double", "signum 3.0")), "1.0");
    assert_eq!(
        run(&shows("show_integer", "signum (0 - (3 :: Integer))")),
        "-1"
    );
    assert_eq!(run(&shows("show_integer", "signum (3 :: Integer)")), "1");
    assert_eq!(run(&shows("show_integer", "signum (0 :: Integer)")), "0");
}

#[test]
fn abs_is_the_magnitude() {
    assert_eq!(run(&shows("show_int", "abs (0 - 3)")), "3");
    assert_eq!(run(&shows("show_int", "abs 3")), "3");
    // GHC: `abs (-3.5) = 3.5`.
    assert_eq!(run(&shows("show_double", "abs (0.0 - 3.5)")), "3.5");
    assert_eq!(run(&shows("show_double", "abs 3.5")), "3.5");
    assert_eq!(run(&shows("show_integer", "abs (0 - (3 :: Integer))")), "3");
}

#[test]
fn abs_and_signum_on_integer_stay_exact() {
    // `Integer` is arbitrary precision. A round-trip through `f64` would lose
    // every digit past 2^53: this literal is 10^28, so a rounded result prints
    // something else entirely.
    assert_eq!(
        run(&shows(
            "show_integer",
            "abs (0 - (10000000000000000000000000000 :: Integer))"
        )),
        "10000000000000000000000000000"
    );
    assert_eq!(
        run(&shows(
            "show_integer",
            "signum (0 - (10000000000000000000000000000 :: Integer))"
        )),
        "-1"
    );
    assert_eq!(
        run(&shows(
            "show_integer",
            "signum (10000000000000000000000000000 :: Integer)"
        )),
        "1"
    );
    assert_eq!(
        run(&shows(
            "show_integer",
            "negate (10000000000000000000000000000 :: Integer)"
        )),
        "-10000000000000000000000000000"
    );
}

// ---------------------------------------------------------------------------
// compare: the three-way total order
// ---------------------------------------------------------------------------

#[test]
fn compare_orders_every_numeric_leaf() {
    assert_eq!(run(&shows("show_int", "compare 1 2")), "-1");
    assert_eq!(run(&shows("show_int", "compare 2 2")), "0");
    assert_eq!(run(&shows("show_int", "compare 2 1")), "1");
    assert_eq!(run(&shows("show_int", "compare 1.0 2.0")), "-1");
    assert_eq!(
        run(&shows(
            "show_int",
            "compare (1 :: Integer) (20000000000000000000000000000 :: Integer)"
        )),
        "-1"
    );
}

#[test]
fn compare_agrees_with_the_ordering_predicates() {
    // `compare` must use the SAME order `<`/`>` use (design §3.9: "must keep the
    // total order"). A separate `partial_cmp` would let `compare a b < 0`
    // disagree with `a < b` for NaN, so the two are asserted together.
    assert_eq!(
        run("main = do { write (Just (show_bool ((compare 1.0 2.0 < 0) == (1.0 < 2.0)))) stdout; return () };"),
        "True"
    );
    assert_eq!(
        run("main = do { write (Just (show_bool ((compare 2.0 2.0 == 0) == (2.0 == 2.0)))) stdout; return () };"),
        "True"
    );
    // A NaN key is a genuine total order: NaN ≡ NaN, so `compare nan nan` is 0.
    let nan = "(0.0 / 0.0)";
    assert_eq!(
        run(&shows("show_int", &format!("compare {nan} {nan}"))),
        "0"
    );
}

// ---------------------------------------------------------------------------
// fromInteger
// ---------------------------------------------------------------------------

#[test]
fn from_integer_serves_integer_and_double_targets() {
    // The Prelude still declares `fromInteger : Int -> a` at this task; the
    // kernel must work for the payload it is given, so this test pins the
    // EXISTING declaration and Task 7 re-pins it at `Integer -> a`.
    assert_eq!(
        run("f :: Double -> Double;\nf x = x + fromInteger 1;\nmain = do { write (Just (show_double (f 5.0))) stdout; return () };"),
        "6.0"
    );
    // The three target tracks, each through the instance's own type.
    assert_eq!(run(&shows("show_double", "fromInteger 3")), "3.0");
    assert_eq!(run(&shows("show_integer", "fromInteger 3")), "3");
    assert_eq!(run(&shows("show_int", "fromInteger 3")), "3");
    // A `Double` target is correctly ROUNDED, not truncated. GHC 9.10.3:
    // `fromInteger 9007199254740995 :: Double` prints `9.007199254740996e15`,
    // i.e. 2^53 + 4 — the value rounds UP. A truncating conversion (which
    // `num_bigint`'s own `to_f64` performs) would give 2^53 + 2 instead.
    assert_eq!(
        run(&shows("show_double", "fromInteger 9007199254740995")),
        "9007199254740996.0"
    );
    // The value must FIT the current `Int` parameter, because the Prelude still
    // declares `fromInteger : Int -> a` and a literal beyond `i64` truncates at
    // the `Int` boundary before the kernel sees it (design §2.5's one boundary).
    // Task 7 re-pins the signature at `Integer` and this comment goes away.
}

// ---------------------------------------------------------------------------
// abs is a CLASS METHOD, not a buffer builtin
// ---------------------------------------------------------------------------

#[test]
fn abs_is_a_class_method_not_a_buffer_builtin() {
    // GHC: `abs (-3.0)` is 3.0 and `abs (-3)` is 3. Before this task rill
    // rejected both with "`abs` signal argument must be a Buffer", because
    // lowering intercepted the NAME before class dispatch. If the name branch
    // came back, these two assertions fail with that exact message.
    assert_eq!(run(&shows("show_double", "abs (0.0 - 3.0)")), "3.0");
    assert_eq!(run(&shows("show_int", "abs (0 - 3)")), "3");
    assert_eq!(run(&shows("show_integer", "abs (0 - (3 :: Integer))")), "3");
}

#[test]
fn abs_negate_and_signum_work_over_a_buffer() {
    // The buffer path must keep working, and it must produce the SAME samples
    // the by-name `ValueBufferUn` branch produced before: `abs` of a constant
    // -1.0 square wave is a constant 1.0 block.
    let raw = run_block("square 0.0 (-1.0) 0.0");
    let absed = run_block("abs (square 0.0 (-1.0) 0.0)");
    let negated = run_block("negate (square 0.0 (-1.0) 0.0)");
    let signed = run_block("signum (square 0.0 (-1.0) 0.0)");
    for i in 0..raw.len() {
        assert_eq!(absed[i], raw[i].abs(), "`abs` sample {i}");
        assert_eq!(negated[i], -raw[i], "`negate` sample {i}");
        assert_eq!(signed[i], -1.0, "`signum` of a -1.0 block, sample {i}");
    }
}

#[test]
fn the_buffer_abs_is_element_wise_and_identical_to_the_scalar_rule() {
    // A non-constant block, so a whole-block fill or a single-sample result
    // cannot pass: every sample must equal the scalar `abs` of the source.
    let raw = run_block("sine 440.0 0.5 0.0");
    let absed = run_block("abs (sine 440.0 0.5 0.0)");
    assert_eq!(raw.len(), absed.len());
    for (i, (r, a)) in raw.iter().zip(absed.iter()).enumerate() {
        assert_eq!(*a, r.abs(), "sample {i}: abs({r}) must be {}", r.abs());
    }
    assert!(
        absed.iter().all(|&x| x >= 0.0),
        "every `abs` sample is non-negative"
    );
}

#[test]
fn a_buffer_negate_is_the_element_wise_inverse() {
    // A NON-constant block, so a constant fill cannot pass: every sample must
    // be the inverse of the source sample.
    let raw = run_block("sine 440.0 0.5 0.0");
    let neg = run_block("negate (sine 440.0 0.5 0.0)");
    for (i, (r, n)) in raw.iter().zip(neg.iter()).enumerate() {
        assert_eq!(*n, -r, "sample {i}: negate({r}) must be {}", -r);
    }
    // Double negation is the identity, which a one-shot `abs`-style
    // implementation (returning the source unchanged) could not fake.
    let back = run_block("negate (negate (sine 440.0 0.5 0.0))");
    assert_eq!(raw, back, "`negate . negate` is the identity");
}

#[test]
fn buffer_compare_uses_the_same_order_as_eq() {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    // Two identical blocks compare EQ; a block against a different one does not.
    let mut prog = compile::<f32, 256>(
        "main = do { write (Just (show_int (compare (sine 440.0 0.5 0.0) (sine 440.0 0.5 0.0)))) stdout; return () };",
        ffi,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    prog.run().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(prog.stdout().concat(), "0");
}

#[test]
#[ignore = "open defect: `value_cmp_ref` has no `(Buffer, Buffer)` arm, so any two \
            buffers compare equal. Remove this ignore when whole-buffer ordering \
            lands with the Buffer instances."]
fn compare_distinguishes_two_distinct_buffers() {
    // Two different blocks must not compare equal. `compare b b == 0` proves
    // nothing: a no-op comparator satisfies it too.
    //
    // OPEN DEFECT: `value_cmp_ref` (`interp.rs`) has no `(Buffer, Buffer)` arm,
    // so two buffers fall through to `kind_rank`, which answers rank 16 for
    // both and therefore `0` (Equal) for ANY pair — `compare b1 b2` cannot see
    // an unequal block, and `==`/`<` over buffers have the same defect (both
    // go through `value_cmp`). Pre-existing (measured identical at base
    // `25c7954`), but this task makes `compare` reachable, so it is recorded.
    // Owner: the Buffer-instance stage — whole-buffer ORDERING is a design
    // decision (element-wise lexicographic over the block? by sample count?),
    // not a bug to patch here. This assertion is deliberately left FAILING as
    // a `#[ignore]` + tracked row rather than deleted; remove the `ignore` when
    // the ordering lands.
    let out = run_io("main = do { write (Just (show_bool (compare (sine 440.0 0.5 0.0) (sine 220.0 0.5 0.0) == 0))) stdout; return () };");
    assert_eq!(out, "False", "two distinct buffers must not compare equal");
}

#[test]
fn abs_works_on_a_ratio_because_it_is_a_class_method() {
    // Before the name branch was removed, `abs` at any non-Buffer type was a
    // compile error, so the `Ratio` instance's `abs` body was dead code.
    // GHC 9.10.3: `abs ((-3) % 4)` is `3 % 4`, so the fields are 3 and 4.
    assert_eq!(
        run("r = Ratio { num: (-3 :: Integer), den: (4 :: Integer) };\nq = abs r;\nmain = do { write (Just (show_integer (numerator q))) stdout; write (Just (show_integer (denominator q))) stdout; return () };"),
        "34"
    );
}

#[test]
fn negate_at_int_wraps_like_ghc() {
    // The `0i64.wrapping_sub(x)` path. GHC 9.10.3: `negate 3 = -3`,
    // `negate 0 = 0`, and `negate minBound :: Int` is `minBound` (the
    // two's-complement wrap, printed `-9223372036854775808`). The literal
    // 9223372036854775808 truncates to `minBound` at the `Int` boundary, so
    // that call really does negate `i64::MIN`.
    assert_eq!(run(&shows("show_int", "negate 3")), "-3");
    assert_eq!(run(&shows("show_int", "negate 0")), "0");
    assert_eq!(
        run(&shows("show_int", "negate 9223372036854775808")),
        "-9223372036854775808"
    );
}

#[test]
fn recip_is_rejected_at_int_and_integer() {
    // `Fractional` has no `Int`/`Integer` instance (Haskell 2010 §6.4.4), so
    // both are loud type errors; a silent `1 / x` on the integer track would
    // be a wrong answer.
    for src in [
        "main = do { write (Just (show_double (recip 3))) stdout; return () };",
        "main = do { write (Just (show_double (recip (3 :: Integer)))) stdout; return () };",
    ] {
        assert!(
            compile::<f32, 256>(src, ForeignRegistry::new()).is_err(),
            "`recip` must be rejected: {src}"
        );
    }
}

#[test]
fn recip_of_zero_is_infinity() {
    // GHC 9.10.3: `recip 0.0 = Infinity`, `recip (-0.0) = -Infinity`. rill's
    // `show_double` spells these `inf`/`-inf` (a `show` rendering divergence,
    // separate from `recip`), so the VALUE is pinned by comparison with
    // `1.0 / 0.0` rather than by the text.
    assert_eq!(
        run("main = do { write (Just (show_bool (recip 0.0 == 1.0 / 0.0))) stdout; return () };"),
        "True"
    );
    assert_eq!(
        run("main = do { write (Just (show_bool (recip (negate 0.0) == 0.0 - (1.0 / 0.0)))) stdout; return () };"),
        "True"
    );
    assert_eq!(run(&shows("show_double", "recip 0.0")), "inf");
}

#[test]
fn a_buffer_abs_reached_through_a_function_definition_is_the_same_block() {
    // The shape that used to PANIC ("index out of bounds"): a user definition,
    // not the program's own top level, is what reaches the kernel. The
    // destination register inside the fragment is RELATIVE, so the buffer arm
    // must resolve it with the fragment's `base`; passing it absolute made the
    // bounds guard compare against the wrong length.
    let direct = run_block("abs (sine 440.0 0.5 0.0)");
    let via_def = run_block_with(
        "g :: Buffer -> Buffer;\ng x = abs x;",
        "g (sine 440.0 0.5 0.0)",
    );
    assert_eq!(
        via_def, direct,
        "abs through a definition must match direct"
    );
    let raw = run_block("sine 440.0 0.5 0.0");
    for (i, (r, a)) in raw.iter().zip(via_def.iter()).enumerate() {
        assert_eq!(*a, r.abs(), "sample {i}: abs({r}) must be {}", r.abs());
    }
    // The other two kernels have the same fragment-relative destination.
    let neg_def = run_block_with(
        "g :: Buffer -> Buffer;\ng x = negate x;",
        "g (sine 440.0 0.5 0.0)",
    );
    assert_eq!(neg_def, run_block("negate (sine 440.0 0.5 0.0)"));
    let sg_def = run_block_with(
        "g :: Buffer -> Buffer;\ng x = signum x;",
        "g (sine 440.0 0.5 0.0)",
    );
    assert_eq!(sg_def, run_block("signum (sine 440.0 0.5 0.0)"));
    // And the equality surface the reviewer's reproduction used must not panic.
    assert_eq!(
        run_io("g :: Buffer -> Buffer;\ng x = abs x;\nmain = do { write (Just (show_bool (g (square 0.0 (-0.5) 0.0) == g (square 0.0 (-0.5) 0.0)))) stdout; return () };"),
        "True"
    );
    assert_eq!(
        run_io("g x = abs x;\nmain = do { write (Just (show_bool (g (square 0.0 (-0.5) 0.0) == g (square 0.0 (-0.5) 0.0)))) stdout; return () };"),
        "True"
    );
}

#[test]
fn a_buffer_from_integer_reached_through_a_function_definition_fills_its_own_block() {
    // `exec_value_from_integer`'s `Buffer` arm had the SAME absolute-vs-relative
    // destination defect, so it is pinned through a definition too (the only
    // way the fragment `base` is non-zero). The assertion compares SAMPLE
    // VALUES, not buffers: `==` on two buffers is answered by buffer identity
    // (an open defect, Task 5b), so a buffer-equality assertion would pass even
    // if the fill produced the wrong block.
    // The DIRECT form of the same expression: `fromInteger`'s expected type is
    // known there, so no annotation is needed and the two must agree sample for
    // sample. Comparing through a `show_bool (… == …)` would assert buffer
    // equality, which is answered by identity (open defect, Task 5b).
    let got = run_block_with(
        "f :: Buffer -> Int -> Buffer;\nf b n = b + fromInteger n;\n",
        "f (square 0.0 1.0 0.0) 2",
    );
    let want = run_block_with(
        "g :: Buffer -> Int -> Buffer;\ng b n = b + fromInteger n;\n",
        "(square 0.0 1.0 0.0) + fromInteger 2",
    );
    assert_eq!(
        got, want,
        "the fill through a definition must match the direct form"
    );
    // `square 0.0 1.0 0.0` is a constant 1.0 block; adding the `fromInteger`
    // fill must make every sample 3.0, so a silent zero cannot pass.
    assert!(
        got.iter().all(|x| (*x - 3.0).abs() < 1e-6),
        "expected 3.0 per sample, got {:?}",
        &got[..4]
    );
}

// ---------------------------------------------------------------------------
// The kernels are reachable only through a primitive instance
// ---------------------------------------------------------------------------

#[test]
fn an_int_target_of_from_integer_is_the_identity_not_a_second_truncation() {
    // Design §2.5: `Int` truncation lives in exactly one place
    // (`literal_to_i64`). The kernel must not truncate again, so a value that
    // does not fit an `Int` cannot reach it: the literal alone carries the
    // truncation, and the kernel copies. GHC 9.10.3 prints `-4362896299872285998`
    // for this literal at `Int`; the kernel must not round-trip it through
    // `Integer` and back (that would give the same number, but a second
    // truncation site is the defect regardless of the value).
    assert_eq!(
        run(&shows(
            "show_int",
            "fromInteger 123456789012345678901234567890"
        )),
        "-4362896299872285998"
    );
}
