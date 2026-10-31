//! Pre-existing Haskell 2010 conformance gaps (the Task 2.5 audit set).
//!
//! Every test asserts a COMPUTED value that changes when the behaviour is
//! wrong, or the exact loud diagnosis for a source the standard rejects. A
//! test that would pass under both the correct and the incorrect behaviour is
//! not evidence (AGENTS.md, "rill-lang conformance: Haskell 2010 is the
//! standard").
//!
//! Two properties are pinned here and must not regress:
//!
//! * **Operators and class methods are ordinary first-class values**
//!   (`(+) 1.0 2.0`, `fold (+) 0 xs`, sections `(+ e)` / `(e +)` / `(++ e)`,
//!   `fold mappend`). Haskell 2010 §3.5 and the Prelude.
//! * **No list operation answers silently.** A bulk list op whose operand is
//!   absent, bottom, or not a list must fault (or be rejected at compile
//!   time) — never coerce to `0`/empty.
//!
//! `Mode::Complete` matters: [`rill_lang::compile`] defaults to Complete
//! (`lib.rs`), so the lazy-list paths (`Value::Cons`) are the DEFAULT
//! executable paths. A Strict-only assertion would pass under the old
//! behaviour and prove nothing, so the `Complete` variants are explicit.
pub mod common;

use common::*;
use rill_lang::arena::Value;
use rill_lang::error::CompileError;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;

/// Compile `defs` + `expr` in Strict mode, run it once, and return the result.
fn strict(defs: &str, expr: &str) -> Result<Value, CompileError> {
    eval(defs, expr).map(|world| out_value(&world))
}

/// The same, in Complete (lazy) mode — the mode `compile` defaults to.
fn complete(defs: &str, expr: &str) -> Result<Value, CompileError> {
    eval_mode::<256>(defs, expr, ForeignRegistry::new(), Mode::Complete)
        .map(|world| out_value(&world))
}

/// The `Double` payload of a Strict-mode result.
fn float_of(defs: &str, expr: &str) -> f64 {
    match strict(defs, expr).expect("the source must compile and run") {
        Value::Double(f) => f,
        other => panic!("expected a Double result, got {other:?}"),
    }
}

/// The `Int` payload of a Strict-mode result.
fn int_of(defs: &str, expr: &str) -> i64 {
    match strict(defs, expr).expect("the source must compile and run") {
        Value::Int(i) => i,
        other => panic!("expected an Int result, got {other:?}"),
    }
}

/// The `String` payload of a Strict-mode result.
fn string_of(defs: &str, expr: &str) -> String {
    match strict(defs, expr).expect("the source must compile and run") {
        Value::String(s) => s,
        other => panic!("expected a String result, got {other:?}"),
    }
}

/// The `Double` payloads of a `List` result — ORDER-sensitive.
fn list_of(defs: &str, expr: &str) -> Vec<f64> {
    let world = eval(defs, expr).expect("the source must compile and run");
    match out_value(&world) {
        Value::List { elems } => elems
            .iter()
            .map(|r| match world.arena().get(*r) {
                Some(Value::Double(f)) => *f,
                other => panic!("expected a Double element, got {other:?}"),
            })
            .collect(),
        other => panic!("expected a List result, got {other:?}"),
    }
}

/// Assert that `defs` + `expr` is a LOUD diagnosis in `mode`, never a value.
///
/// A compile-time rejection and a clean value-track fault both satisfy the
/// "no silent partial support" rule; a wrong VALUE does not. Returns the
/// diagnostic text so callers can pin its shape.
fn loud(mode: Mode, defs: &str, expr: &str) -> String {
    match eval_mode::<256>(defs, expr, ForeignRegistry::new(), mode) {
        Ok(world) => match world.result() {
            Some(r) => match world.arena().get(r) {
                // A `Bottom`/fault placeholder is a clean latch, not an answer.
                Some(Value::Bottom) => String::new(),
                Some(other) => panic!(
                    "expected a loud diagnosis in {mode:?} for `{expr}`, \
                     but it produced the value {other:?}"
                ),
                None => String::new(),
            },
            None => String::new(),
        },
        Err(e) => e.to_string(),
    }
}

/// Haskell 2010 Prelude / §4.4.2: an operator is an ordinary function value.
mod operator_values {
    use super::*;

    #[test]
    fn prefix_operator_application_computes() {
        // `(+) 1.0 2.0` ≡ `1.0 + 2.0`. The operand pair is chosen so that a
        // pass-through/zero answer (0.0) cannot be mistaken for the result.
        assert_eq!(float_of("", "(+) 1.0 2.0"), 3.0);
        assert_eq!(float_of("", "(*) 2.0 3.0"), 6.0);
        assert_eq!(float_of("", "(-) 9.0 4.0"), 5.0);
        assert_eq!(float_of("", "(/) 8.0 2.0"), 4.0);
    }

    #[test]
    fn prefix_operator_is_a_first_class_value() {
        // Bound to a name and applied later, and passed to a higher-order
        // function — the same binding must serve both spellings.
        assert_eq!(float_of("plus = (+);", "plus 1.0 2.0"), 3.0);
        assert_eq!(float_of("", "fold (+) 0.0 [1.0,2.0]"), 3.0);
    }

    #[test]
    fn operator_ref_partially_applied() {
        // Curried like any other function: one argument leaves a function.
        assert_eq!(float_of("inc = (+) 1.0;", "inc 2.0"), 3.0);
        assert_eq!(list_of("", "map ((+) 1.0) [1.0,2.0]"), vec![2.0, 3.0]);
    }
}

/// `fold (+)` — the operator value reaches a higher-order consumer.
mod fold_takes_an_operator {
    use super::*;

    #[test]
    fn fold_with_operator_value() {
        // Left fold over [1,2,3,4]: 0+1+2+3+4 = 10. A zero-answer bug would
        // report 0; an empty-fold bug would report 0 too, so the seed is 0 and
        // the elements are non-zero.
        assert_eq!(float_of("", "fold (+) 0.0 [1.0,2.0,3.0,4.0]"), 10.0);
        // Multiplication distinguishes a sum fallback from the real operator.
        assert_eq!(float_of("", "fold (*) 1.0 [2.0,3.0,4.0]"), 24.0);
    }

    #[test]
    fn fold_with_operator_over_a_generated_range() {
        // `listRange (0, 3)` = 0,1,2,3 and folding `+` sums them. The seed `0`
        // and the elements are INTEGRAL literals with no context fixing them, so
        // `default (Integer, Double)` takes its FIRST entry. GHC 9.10.3 measures
        // exactly that — the Core is `foldl … (+ $dNum) (IS 0#) …`, an `Integer`.
        // The assertion therefore reads the value integrally instead of pinning
        // the `Int` track.
        assert_eq!(
            integral(
                &strict("", "fold (+) 0 (listRange (Pair { first: 0, second: 3 }))")
                    .expect("the source must compile and run")
            ),
            6
        );
    }

    #[test]
    fn fold_with_operator_preserves_the_fold_direction() {
        // `fold` is a LEFT fold here. With a non-associative operator the two
        // directions differ: left = (3-1)-1 = 1, right = 3-(1-1) = 3.
        assert_eq!(float_of("", "fold (-) 3.0 [1.0,1.0]"), 1.0);
    }
}

/// Haskell 2010 §3.5 — operator sections desugar to unary lambdas.
mod operator_sections {
    use super::*;

    #[test]
    fn right_section_computes() {
        // `(+ 1.0)` ≡ `\x -> x + 1.0`. The operand order is observable:
        // `\x -> 1.0 + x` would give the same value for `+`, so `-` pins it.
        assert_eq!(float_of("", "(+ 1.0) 2.0"), 3.0);
        assert_eq!(float_of("", "(1.0 -) 5.0"), -4.0);
    }

    #[test]
    fn left_section_computes() {
        // `(1.0 +)` ≡ `\x -> 1.0 + x`.
        assert_eq!(float_of("", "(1.0 +) 2.0"), 3.0);
        // Direction discriminator: `(10.0 -) 3.0` = 10 - 3 = 7, whereas the
        // mirrored section `\x -> x - 10.0` would give -7.
        assert_eq!(float_of("", "(10.0 -) 3.0"), 7.0);
        // §3.5: `(- 10.0)` is unary NEGATION (a value), not a subtraction
        // section — so it is -10.0 and applying it is an error, which is the
        // very thing that distinguishes the two readings.
        assert_eq!(float_of("", "(- 10.0)"), -10.0);
        assert!(
            strict("", "((- 10.0)) 3.0").is_err(),
            "`(- 10.0)` is negation, not a subtraction section"
        );
    }

    #[test]
    fn sections_are_usable_as_higher_order_arguments() {
        assert_eq!(list_of("", "map (+ 1.0) [1.0,2.0]"), vec![2.0, 3.0]);
        assert_eq!(list_of("", "map (1.0 +) [1.0,2.0]"), vec![2.0, 3.0]);
        // A section reaches `map` in a longer pipeline: `(2.0 *)` is
        // `\x -> 2.0 * x`.
        assert_eq!(
            list_of("", "map (2.0 *) [1.0,2.0]"),
            vec![2.0, 4.0],
            "`(2.0 *)` doubles"
        );
        // Direction discriminator: `(10.0 -)` subtracts FROM the literal.
        assert_eq!(
            list_of("", "map (10.0 -) [1.0, 3.0]"),
            vec![9.0, 7.0],
            "`(10.0 -)` is `\\x -> 10.0 - x`, not `\\x -> x - 10.0`"
        );
    }

    #[test]
    fn append_left_and_right_sections_differ_in_order() {
        // §3.5: `(++ [x])` is the RIGHT section `\y -> y ++ [x]`, so the
        // literal goes LAST; `([x] ++)` is the LEFT section, literal FIRST.
        // Element order — not the length — is the discriminator.
        assert_eq!(list_of("", "(++ [1.0]) [2.0,3.0]"), vec![2.0, 3.0, 1.0]);
        assert_eq!(list_of("", "([1.0] ++) [2.0,3.0]"), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn minus_prefix_section_stays_negation() {
        // §3.5: `(- e)` is NOT a section — it is unary negation (there is no
        // `\x -> x - e` reading). `(- 1.0)` is therefore -1.0, and the
        // subtraction section must be spelled `(0.0 -)` / `(- 1.0)` applied as
        // a value is an error (covered in `left_section_computes`).
        assert_eq!(float_of("", "(- 1.0)"), -1.0);
        assert_eq!(float_of("", "(0.0 - 1.0)"), -1.0);
        // The left section is the operator's own spelling.
        assert_eq!(float_of("", "(0.0 -) 1.0"), -1.0);
    }
}

/// A typeclass method is a first-class value, resolved by its class dictionary
/// (Haskell 2010 §4.3.3: class methods are ordinary variables).
mod method_values {
    use super::*;

    #[test]
    fn fold_with_a_method_value() {
        // `fold mappend "" ["a","b"]` = "ab". The seed and elements are chosen
        // so an empty-string fallback is indistinguishable from a real fold of
        // empty input — hence the non-empty elements.
        assert_eq!(string_of("", "fold mappend \"\" [\"a\",\"b\"]"), "ab");
        // Double dispatch through the same method value.
        assert_eq!(float_of("", "fold mappend 0.0 [1.0,2.0,3.0]"), 6.0);
        // List dispatch: `mappend` on lists delegates to `++` (Prelude).
        assert_eq!(
            list_of("", "fold mappend list [[1.0],[2.0],[3.0]]"),
            vec![1.0, 2.0, 3.0]
        );
    }

    #[test]
    fn method_value_applied_directly_computes() {
        // The same dictionary entry reachable by direct application. G7 of the
        // audit was NOT a defect: `1.0 `mappend` 2.0` is 3.0 against rill's
        // declared `instance Monoid Double`. Asserted here only because it is
        // the value the FOLD tests dispatch through — not as evidence for a
        // fix (it passed before this task).
        assert_eq!(float_of("", "mappend 1.0 2.0"), 3.0);
        assert_eq!(string_of("", "mappend \"a\" \"b\""), "ab");
    }
}

/// `(++) :: [a] -> [a] -> [a]` (Haskell 2010 Prelude) — monomorphic to lists.
///
/// It is NOT `Monoid.mappend` generalized: `mappend` is the general one. The
/// conformant form works; a non-list operand is rejected by the type checker,
/// exactly as GHC rejects `foldl (++) [] [1.0,2.0]` (`No instance for
/// Fractional [Double]`).
mod append_is_monomorphic_to_lists {
    use super::*;

    #[test]
    fn conformant_fold_append_works() {
        assert_eq!(
            list_of("", "fold (++) list [[1.0],[2.0],[3.0]]"),
            vec![1.0, 2.0, 3.0]
        );
        assert_eq!(list_of("", "fold (++) list [[1.0],[2.0]]"), vec![1.0, 2.0]);
        // An empty input list is the seed, unchanged.
        assert_eq!(list_of("", "fold (++) list list"), Vec::<f64>::new());
    }

    #[test]
    fn non_list_append_operand_is_rejected() {
        // The audit's G8 spelling. In Haskell 2010 the second operand of `++`
        // must be a list, so `[1.0,2.0]` (a list of Double) cannot be appended
        // to `list` (a list of List). Both modes must be LOUD — the old
        // behaviour was a silent `0` once the absent result reached `length`.
        for mode in [Mode::Strict, Mode::Complete] {
            let msg = loud(mode, "", "fold (++) list [1.0,2.0]");
            assert!(
                !msg.is_empty(),
                "{mode:?}: `fold (++) list [1.0,2.0]` must be a loud type error, \
                 not a silent result"
            );
        }
        // The same rule at the direct-append spelling, which used to reach the
        // runtime and answer `None`.
        for mode in [Mode::Strict, Mode::Complete] {
            assert!(
                !loud(mode, "", "list ++ 1.0").is_empty(),
                "{mode:?}: `list ++ 1.0` must be rejected — `++` is [a] -> [a] -> [a]"
            );
            assert!(
                !loud(mode, "", "[1.0] ++ 2.0").is_empty(),
                "{mode:?}: `[1.0] ++ 2.0` must be rejected"
            );
        }
    }
}

/// No list operation may answer silently when its operand is absent, bottom or
/// not a collection — the `None`-to-`0` coercion is the shared root of the
/// audit's three "silent wrong answer" rows.
mod no_silent_list_answers {
    use super::*;

    #[test]
    fn length_of_a_cons_chain_is_its_real_count() {
        // Complete mode is the DEFAULT executable path (`compile`), and `cons`
        // builds a lazy `Value::Cons` chain there. The old `length` reported 0
        // for every such chain. Strict already worked, so this test is
        // only meaningful in Complete — hence the explicit mode.
        assert_eq!(
            complete("", "length (cons 1.0 (cons 2.0 list))").expect("must compile and run"),
            Value::Int(2)
        );
        assert_eq!(
            complete("", "length (cons 1.0 list)").expect("must compile and run"),
            Value::Int(1)
        );
        assert_eq!(
            complete("", "length (cons 1.0 (cons 2.0 (cons 3.0 list)))")
                .expect("must compile and run"),
            Value::Int(3)
        );
        // `list` (the empty lazy list) is still 0.
        assert_eq!(
            complete("", "length list").expect("must compile and run"),
            Value::Int(0)
        );
    }

    #[test]
    fn length_of_a_local_cons_chain_counts() {
        // The same rule reached through a lambda parameter and a handler, so
        // the fix is in `length`, not in one call site.
        assert_eq!(
            complete("n = length (cons 4.0 (cons 5.0 list));", "n").expect("must compile and run"),
            Value::Int(2)
        );
    }

    #[test]
    fn length_of_a_non_list_is_loud() {
        // `length :: [a] -> Int`. A Double/Bool/String/record has no length, so
        // the audit's `0` was a silent wrong answer. Both modes must reject it
        // or fault — never answer.
        for mode in [Mode::Strict, Mode::Complete] {
            assert!(
                !loud(mode, "", "length 1.0").is_empty(),
                "{mode:?}: `length 1.0` must not answer"
            );
            assert!(
                !loud(mode, "", "length true").is_empty(),
                "{mode:?}: `length true` must not answer"
            );
            assert!(
                !loud(mode, "", "length \"ab\"").is_empty(),
                "{mode:?}: `length \"ab\"` must not answer"
            );
        }
    }

    #[test]
    fn length_of_a_failed_operand_is_loud_not_zero() {
        // The audit's actual G7 observation: the `0` came from wrapping an
        // expression whose result is not a list in `length`. `mappend` on two
        // Floats yields a Double, so `length` of it is a type error — the same
        // rule as G8, one root.
        for mode in [Mode::Strict, Mode::Complete] {
            assert!(
                !loud(mode, "", "length (1.0 `mappend` 2.0)").is_empty(),
                "{mode:?}: `length (mappend 1.0 2.0)` must not answer 0"
            );
            // An ABSENT result (an unbound operand) must fault too, not zero.
            assert!(
                !loud(mode, "", "length (head list)").is_empty(),
                "{mode:?}: `length Nothing` must not answer 0"
            );
        }
    }

    #[test]
    fn bulk_list_ops_on_a_non_list_are_loud() {
        // `map`, `filter`, `concat_map` and `fold` all need a list source. A
        // non-list / absent source must fault instead of producing an empty
        // result (which then reads as `0` everywhere downstream).
        for mode in [Mode::Strict, Mode::Complete] {
            assert!(
                !loud(mode, "", "map (\\x -> x) 1.0").is_empty(),
                "{mode:?}: `map` over a Double must not answer"
            );
            assert!(
                !loud(mode, "", "filter (\\x -> true) 1.0").is_empty(),
                "{mode:?}: `filter` over a Double must not answer"
            );
            assert!(
                !loud(mode, "", "fold (+) 0.0 1.0").is_empty(),
                "{mode:?}: `fold` over a Double must not answer"
            );
        }
    }
}

/// The IO path is the one a user actually runs: [`rill_lang::compile`] defaults
/// to Complete mode, so every fix above must hold there too.
mod io_executable_path {
    use super::*;

    /// Run a `main : IO ()` program and return what it wrote to stdout.
    fn stdout_of(src: &str) -> Vec<String> {
        let mut runner = rill_lang::compile::<f32, 256>(src, ForeignRegistry::new())
            .expect("the program must compile");
        runner.run().expect("the program must run without a fault");
        runner.stdout().to_vec()
    }

    #[test]
    fn length_of_a_cons_chain_prints_the_real_count() {
        assert_eq!(
            stdout_of("main = write (Just (show_int (length (cons 1.0 (cons 2.0 list))))) stdout;"),
            ["2"]
        );
    }

    #[test]
    fn a_failed_list_operation_never_prints_zero() {
        // `fold (++) list [1.0,2.0]` is ill-typed in Haskell 2010; the program
        // must be REJECTED at compile time rather than print "0".
        let err = rill_lang::compile::<f32, 256>(
            "main = write (Just (show_int (length (fold (++) list [1.0,2.0])))) stdout;",
            ForeignRegistry::new(),
        )
        .err()
        .expect("an ill-typed append must be rejected, not printed as 0");
        assert!(
            !err.to_string().is_empty(),
            "the rejection must carry a diagnostic"
        );
    }

    #[test]
    fn a_fold_result_fed_to_arithmetic_never_becomes_zero() {
        // THE discriminating test for the "no silent partial support" rule.
        //
        // `fold` over a cons-built list must produce the real sum (6.0), and an
        // operand it cannot consume must FAULT. What it must never do is let
        // `arithmetic_operand` read the absent result as `0.0` — which is
        // exactly what the reviewer reproduced: Complete printed `0.0` while
        // Strict printed `6.0`.
        //
        // Deliberately NOT wrapped in `length` (that guard would mask the
        // arithmetic coercion). This drives the value straight into `* 2.0`.
        assert_eq!(
            stdout_of(
                "{-# MODE complete #-}\n\
                 main = do { write (Just (show_double ((fold (\\a -> \\x -> a + x) 0.0 \
                 (cons 1.0 (cons 2.0 (list)))) * 2.0))) stdout; return () };"
            ),
            ["6.0"],
            "Complete must compute the real fold ((0+1+2) * 2.0), not a coerced 0.0"
        );
        // Strict mode is the reference answer for the same program. The
        // duplicate line is an UNRELATED pre-existing defect in `MODE strict`
        // + `do` (it reproduces on the base commit; see the report's open
        // defects), so the assertion is on the printed VALUE, not the count.
        let strict_out = stdout_of(
            "{-# MODE strict #-}\n\
             main = do { write (Just (show_double ((fold (\\a -> \\x -> a + x) 0.0 \
             (cons 1.0 (cons 2.0 (list)))) * 2.0))) stdout; return () };",
        );
        assert!(
            !strict_out.is_empty() && strict_out.iter().all(|s| s == "6.0"),
            "Strict mode's answer must stay 6.0, got {strict_out:?}"
        );
    }

    #[test]
    fn an_unsupported_operand_faults_instead_of_printing_zero() {
        // The other half: when the operand genuinely cannot be consumed (here an
        // infinite cons chain, which is Task 5's remaining work), the FAULT must
        // surface — never a `0` on stdout. Note the operand goes straight into
        // arithmetic, so the old `arithmetic_operand` fallback would have printed
        // `0.0`; and it is not wrapped in `length`, whose guard would hide that.
        let mut runner = rill_lang::compile::<f32, 256>(
            "{-# MODE complete #-}\n\
             main = do { write (Just (show_double ((fold (\\a -> \\x -> a + x) 0.0 \
             (fibs)) * 1.0))) stdout; return () };\n\
             fibs = cons 1.0 fibs;",
            ForeignRegistry::new(),
        )
        .expect("the program itself type-checks");
        let err = runner
            .run()
            .expect_err("an unconsumable list operand must FAULT, not answer 0");
        assert!(
            err.to_string().contains("fault"),
            "the failure must use the value-track fault channel, got: {err}"
        );
        assert!(
            runner.stdout().is_empty(),
            "nothing may be printed when the operand faults, got {:?}",
            runner.stdout()
        );
    }

    #[test]
    fn empty_list_append_works_in_complete_mode() {
        // `([] ++ [1.0])` is legitimate conformant Haskell. Complete mode used
        // to produce no result (the empty lazy `Nil` had no flat element Vec);
        // it must now answer `[1.0]`, matching Strict.
        assert_eq!(
            complete("", "length ([] ++ [1.0])").expect("empty append must work in Complete"),
            Value::Int(1),
        );
        // Order-sensitive: the appended element is present and last.
        assert_eq!(
            complete("", "length ([1.0] ++ [])").expect("empty right operand must work"),
            Value::Int(1),
        );
    }
}
