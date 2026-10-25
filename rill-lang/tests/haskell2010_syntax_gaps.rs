//! Haskell 2010 surface-syntax gaps that the lexer/parser must fill before the
//! language can claim conformance. Each test asserts a COMPUTED value (or the
//! exact diagnostic), never merely that compilation succeeds.
//!
//! Task 2 covers the lexical layer:
//!
//! * backtick infix application (§3.4) — ``a `f` b`` ≡ `f a b`
//! * `++` list append (Prelude, `infixr 5`)
//! * nested block comments (§2.2) — `{- … {- … -} … -}`
//! * `{-# … #-}` pragmas must keep lexing as pragmas, not as block comments
//!
//! Later tasks in this file extend the suite (guards/sections/`where`, `let`
//! bindings of function values, list comprehensions); the module is organised
//! one `mod` per task group so those additions do not disturb the assertions
//! below.
pub mod common;

use common::*;
use rill_lang::arena::Value;
use rill_lang::error::CompileError;

/// Compile `defs` + the value expression `expr`, run it once, and return the
/// run's result value.
fn value_of(defs: &str, expr: &str) -> Value {
    let world = eval(defs, expr).expect("the source must compile and run");
    out_value(&world)
}

/// The compile error for a source that must be rejected.
fn error_of(defs: &str, expr: &str) -> CompileError {
    match eval(defs, expr) {
        Ok(_) => panic!("expected a compile error for `{expr}`"),
        Err(e) => e,
    }
}

/// The `Float` payloads of the `List` that `defs` + `expr` evaluates to.
///
/// Order-sensitive: the `++` fixity tests assert ELEMENT ORDER, because
/// `length`/sum are invariant under regrouping and would pass under both the
/// correct and the incorrect reading.
fn list_of(defs: &str, expr: &str) -> Vec<f64> {
    let world = eval(defs, expr).expect("the source must compile and run");
    match out_value(&world) {
        Value::List { elems } => elems
            .iter()
            .map(|r| match world.arena().get(*r) {
                Some(Value::Float(f)) => *f,
                other => panic!("expected a Float element, got {other:?}"),
            })
            .collect(),
        other => panic!("expected a List result, got {other:?}"),
    }
}

/// The `Float` payload of the value expression.
fn float_of(defs: &str, expr: &str) -> f64 {
    match value_of(defs, expr) {
        Value::Float(f) => f,
        other => panic!("expected a Float result, got {other:?}"),
    }
}

/// The `Bool` payload of the value expression.
fn bool_of(defs: &str, expr: &str) -> bool {
    match value_of(defs, expr) {
        Value::Bool(b) => b,
        other => panic!("expected a Bool result, got {other:?}"),
    }
}

/// Haskell 2010 §3.4 — backtick infix application.
mod backticks {
    use super::*;

    #[test]
    fn backtick_infix_application() {
        // `` 1.0 `add` 2.0 `` desugars to `add 1.0 2.0`.
        assert_eq!(
            value_of("add a b = a + b;", "1.0 `add` 2.0"),
            Value::Float(3.0)
        );
    }

    #[test]
    fn backtick_binds_tighter_than_arithmetic() {
        // A backticked function has default fixity `infixl 9`, so it binds
        // tighter than every builtin operator including `*` (`infixl 7`):
        // `2.0 * 1.0 `add` 2.0` ≡ `2.0 * (add 1.0 2.0)` = 6.0.
        assert_eq!(
            value_of("add a b = a + b;", "2.0 * 1.0 `add` 2.0"),
            Value::Float(6.0)
        );
        // ... while juxtaposed application still binds tighter than a backtick:
        // `1.0 `add` neg 2.0` ≡ `add 1.0 (neg 2.0)` = -1.0.
        assert_eq!(
            value_of("add a b = a + b; neg x = 0.0 - x;", "1.0 `add` neg 2.0"),
            Value::Float(-1.0)
        );
    }

    #[test]
    fn backtick_requires_two_backticks() {
        // An opening backtick with no closing one is a parse error, not a panic.
        let err = error_of("add a b = a + b;", "1.0 `add 2.0");
        assert!(
            matches!(err, CompileError::Parse { .. }),
            "expected a parse error, got {err}"
        );
    }
}

/// `++` — list append (`infixr 5` in Haskell 2010 §4.4.2), a first-class
/// Prelude function `(++) :: [a] -> [a] -> [a]`.
///
/// Every assertion below is ORDER-sensitive: element order (or a numeric value
/// that differs between groupings), never `length` or a sum. Regrouping a
/// concatenation cannot change its length, so a length assertion would pass
/// under both the correct and the incorrect reading and prove nothing.
mod list_append {
    use super::*;

    #[test]
    fn list_append_operator() {
        // Element order, not just the count.
        assert_eq!(list_of("", "[1.0, 2.0] ++ [3.0]"), vec![1.0, 2.0, 3.0]);
        assert_eq!(list_of("", "[] ++ [7.0]"), vec![7.0]);
        assert_eq!(list_of("", "[7.0] ++ []"), vec![7.0]);
    }

    #[test]
    fn list_append_is_right_associative_at_value_level() {
        // `infixr 5` associativity is only observable through an operator whose
        // two groupings differ. List concatenation is associative, so the
        // Prelude `++` cannot show it; a USER `++` shadows the Prelude one (the
        // Prelude def is dropped when a user def has the same name) and packs
        // `a` and `b` as `a * 10 + b`:
        //   right: 1 * 10 + (2 * 10 + 3) = 33
        //   left:  (1 * 10 + 2) * 10 + 3 = 123
        let defs = "(++) a b = a * 10.0 + b;";
        assert_eq!(float_of(defs, "1.0 ++ 2.0 ++ 3.0"), 33.0);
        // The parenthesized spellings pin what the numbers mean.
        assert_eq!(float_of(defs, "(1.0 ++ 2.0) ++ 3.0"), 123.0);
        assert_eq!(float_of(defs, "1.0 ++ (2.0 ++ 3.0)"), 33.0);
    }

    #[test]
    fn cons_and_append_share_level_five() {
        // §4.4.2: `:` and `++` are BOTH `infixr 5`, so they mix without
        // parentheses and nest to the right:
        //   `[1.0] ++ 2.0 : [3.0]` ≡ `[1.0] ++ (2.0 : [3.0])` = [1,2,3]
        // The WRONG grouping, `([1.0] ++ 2.0) : [3.0]`, does not even type:
        // it conses a List onto a List, where `:` wants one element and a list.
        assert_eq!(list_of("", "[1.0] ++ 2.0 : [3.0]"), vec![1.0, 2.0, 3.0]);
        assert_eq!(
            list_of("", "[1.0] ++ (2.0 : [3.0])"),
            vec![1.0, 2.0, 3.0],
            "the parenthesized form must agree with the inferred grouping"
        );
        assert!(
            eval("", "([1.0] ++ 2.0) : [3.0]").is_err(),
            "the WRONG grouping must not type — otherwise this test cannot \
             distinguish the two readings"
        );

        // The mirror direction: `1.0 : [2.0] ++ [3.0]` ≡ `1.0 : ([2.0] ++ [3.0])`.
        assert_eq!(list_of("", "1.0 : [2.0] ++ [3.0]"), vec![1.0, 2.0, 3.0]);
        assert_eq!(list_of("", "1.0 : ([2.0] ++ [3.0])"), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn append_binds_tighter_than_comparison() {
        // `++` (5) binds tighter than `==` (4), so the comparison sees the
        // CONCATENATED list. Order-sensitive: the right-hand list is a literal
        // equal to the concatenation only in the correct grouping.
        assert!(bool_of("", "[1.0] ++ [2.0] == [1.0, 2.0]"));
        assert!(!bool_of("", "[1.0] ++ [2.0] == [2.0, 1.0]"));
    }

    #[test]
    fn append_is_usable_as_a_first_class_function() {
        // Haskell 2010 Prelude: `(++) :: [a] -> [a] -> [a]` is an ordinary
        // function value, not only infix sugar.
        assert_eq!(list_of("", "(++) [1.0] [2.0, 3.0]"), vec![1.0, 2.0, 3.0]);
        assert_eq!(
            list_of("app = (++);", "app [1.0, 2.0] [3.0]"),
            vec![1.0, 2.0, 3.0]
        );
    }
}

/// Haskell 2010 §4.4.2 fixity: the logic operators and non-associativity.
///
/// `true || false && false` is the canonical discriminator: with `&&` at level
/// 3 and `||` at level 2 it is `True`; with BOTH at the same level (the bug this
/// table had) it is `False`, because `false && false` is grouped away.
mod fixity_4_4_2 {
    use super::*;

    #[test]
    fn and_binds_tighter_than_or() {
        // `infixr 2 ||`, `infixr 3 &&`: `true || (false && false)` = True.
        assert!(bool_of("", "true || false && false"));
        // The mirror: with `||` tighter this would be `(true || false) && false`
        // = False. Both spellings therefore pin the ORDER, not just the value
        // `False` that the old flat table produced.
        assert!(!bool_of("", "(true || false) && false"));
        assert!(bool_of("", "true || (false && false)"));
    }

    #[test]
    fn logic_operators_are_right_associative() {
        // `infixr` on both. Associativity of `&&`/`||` is only observable under
        // a NON-associative operand, so the SHAPE is pinned at the parser level
        // (see `logic_operators_are_right_associative_and_ordered`). The
        // computed values below confirm the two spellings agree, which is the
        // value-level half of the contract.
        assert!(!bool_of("", "true && false && true"));
        assert_eq!(
            bool_of("", "true && false && true"),
            bool_of("", "true && (false && true)")
        );
        assert_eq!(
            bool_of("", "false || true || false"),
            bool_of("", "false || (true || false)")
        );
    }

    #[test]
    fn comparison_operators_are_non_associative() {
        // `infix 4`: `a == b == c` is a precedence error, not a left fold.
        // Checked as a parse error through the compile path, with the exact
        // diagnostic shape.
        for src in ["1.0 == 2.0 == 3.0", "1.0 < 2.0 <= 3.0", "a != b == c"] {
            let err = error_of("", src);
            assert!(
                matches!(err, CompileError::Parse { .. }),
                "`{src}` must be a parse error, got {err}"
            );
            assert!(
                err.to_string().contains("non-associative"),
                "`{src}`: the diagnostic must name the rule, got {err}"
            );
        }
        // Parenthesizing restores it — proving the error is about associativity,
        // not about comparing three values.
        assert!(!bool_of("", "(1.0 == 2.0) == 3.0"));
        assert!(bool_of("", "1.0 < 2.0 && 2.0 < 3.0"));
    }

    #[test]
    fn comparison_binds_looser_than_arithmetic() {
        // `infix 4` vs `infixl 6`/`infixl 7`: `1.0 + 2.0 * 3.0 < 8.0` ≡
        // `(1.0 + (2.0 * 3.0)) < 8.0` = True. With the operands chosen so a
        // mis-grouping changes the RESULT: `1.0 + 2.0 * 3.0` is 7.0, but
        // `(1.0 + 2.0) * 3.0` would be 9.0 and the comparison False.
        assert!(bool_of("", "1.0 + 2.0 * 3.0 < 8.0"));
        assert!(!bool_of("", "(1.0 + 2.0) * 3.0 < 8.0"));
        assert_eq!(float_of("", "1.0 + 2.0 * 3.0"), 7.0);
    }

    #[test]
    fn subtraction_is_left_associative() {
        // `infixl 6`: `10.0 - 3.0 - 2.0` ≡ `(10.0 - 3.0) - 2.0` = 5.0.
        // Right-associative would be `10.0 - (3.0 - 2.0)` = 9.0.
        assert_eq!(float_of("", "10.0 - 3.0 - 2.0"), 5.0);
        assert_eq!(float_of("", "(10.0 - 3.0) - 2.0"), 5.0);
        assert_eq!(float_of("", "10.0 - (3.0 - 2.0)"), 9.0);
    }

    #[test]
    fn division_is_left_associative_and_tighter_than_addition() {
        // `infixl 7 /` over `infixl 6 +`: `2.0 + 8.0 / 4.0` = 4.0, and
        // `8.0 / 4.0 / 2.0` = `(8.0 / 4.0) / 2.0` = 1.0 (right would be 4.0).
        assert_eq!(float_of("", "2.0 + 8.0 / 4.0"), 4.0);
        assert_eq!(float_of("", "8.0 / 4.0 / 2.0"), 1.0);
    }
}

/// Haskell 2010 §2.2 — nested block comments.
mod block_comments {
    use super::*;

    #[test]
    fn nested_block_comment_is_skipped() {
        // Three levels of nesting, straddling a definition boundary.
        let defs = "{- outer {- middle {- deepest -} -} still outer -}\nx = 1.0;";
        assert_eq!(value_of(defs, "x + 2.0"), Value::Float(3.0));

        // The same inside an expression, with code on both sides.
        assert_eq!(
            value_of("", "1.0 {- a {- b -} c -} + 2.0"),
            Value::Float(3.0)
        );
    }

    #[test]
    fn block_comment_may_span_lines() {
        // A multi-line comment is whitespace: the definitions before and after
        // it stay separate top-level definitions (layout must not fuse them).
        let defs = "x = 1.0;\n{- a comment\n   spanning {- two -}\n   lines -}\ny = 2.0;";
        assert_eq!(value_of(defs, "x + y"), Value::Float(3.0));
    }

    #[test]
    fn unterminated_block_comment_is_a_clean_lex_error() {
        // An unbalanced `{-` must be reported, never panic or silently run.
        let err = error_of("{- outer {- inner -}", "1.0");
        match err {
            CompileError::Lex { ref msg, .. } => {
                assert!(
                    msg.contains("unterminated block comment"),
                    "unexpected message: {msg}"
                );
            }
            other => panic!("expected a lex error, got {other}"),
        }
    }
}

/// `{-# … #-}` pragmas keep their meaning: pragma detection runs BEFORE block
/// comment handling.
mod pragmas {
    use super::*;

    #[test]
    fn mode_pragma_still_works() {
        // `MODE complete` flips the program into lazy mode, where user
        // recursion is legal: `sum_to 2.0` = 3.0. If the pragma were swallowed
        // as a `{- … -}` comment the program would stay in Strict mode and be
        // rejected as recursive — so the computed value proves both the lexing
        // and the effect of the pragma. (Depth is kept at 2: the value arena
        // caps deeper recursion independently of this change.)
        let defs = "{-# MODE complete #-}\n\
                    sum_to n = if n == 0.0 then 0.0 else n + sum_to (n - 1.0);";
        assert_eq!(value_of(defs, "sum_to 2.0"), Value::Float(3.0));
    }

    #[test]
    fn mode_pragma_is_not_a_comment_even_after_a_comment() {
        // A block comment first, then the pragma: the pragma must still be
        // recognised as the program's mode directive.
        let defs = "{- a leading comment -}\n\
                    {-# MODE complete #-}\n\
                    sum_to n = if n == 0.0 then 0.0 else n + sum_to (n - 1.0);";
        assert_eq!(value_of(defs, "sum_to 2.0"), Value::Float(3.0));
    }

    #[test]
    fn strict_pragma_still_rejects_recursion() {
        // The mirror image: `MODE strict` must keep Strict semantics, i.e. the
        // pragma is interpreted (not ignored as a comment) in both directions.
        let defs = "{-# MODE strict #-}\n\
                    sum_to n = if n == 0.0 then 0.0 else n + sum_to (n - 1.0);";
        let err = error_of(defs, "sum_to 3.0");
        assert!(
            err.to_string().contains("recursive"),
            "expected a recursion diagnostic, got {err}"
        );
    }
}

/// Haskell 2010 §4.4.3.3 — a `where` clause sees the declaration's own formal
/// parameters.
///
/// The clause is desugared to a `let` INSIDE the definition body (`desugar.rs`),
/// so the bindings resolve in the enclosing λ-environment. `f x = g where …`
/// must therefore read `x`; a `where` written around an explicit lambda
/// (`f = \x -> …`) must NOT — those are different scopes in Haskell 2010.
mod where_clause_scope {
    use super::*;

    #[test]
    fn where_sees_the_declaration_parameters() {
        // `g` closes over `x`; `f 1.0` = 1.0 + 1.0.
        assert_eq!(
            value_of("f x = g where { g = x + 1.0; };", "f 1.0"),
            Value::Float(2.0)
        );
        // A parameterised `where` binding closes over it too.
        assert_eq!(
            value_of("f x = g 1.0 where { g y = y + x; };", "f 1.0"),
            Value::Float(2.0)
        );
        // Two parameters, one of them used only inside the `where` clause.
        assert_eq!(
            value_of("f x z = g z where { g y = y + x; };", "f 1.0 2.0"),
            Value::Float(3.0)
        );
    }

    #[test]
    fn where_bindings_are_visible_in_the_body() {
        assert_eq!(
            value_of("f x = g 10.0 where { g y = y * h; h = x + 1.0; };", "f 1.0"),
            Value::Float(20.0)
        );
    }

    #[test]
    fn where_captures_only_the_enclosing_lambda_parameters() {
        // `where` around the CURRIED nest (`f x y = … where …`) sees both
        // parameters.
        assert_eq!(
            value_of("f x y = g where { g = x * 10.0 + y; };", "f 2.0 3.0"),
            Value::Float(23.0)
        );
        // `where` written around an EXPLICIT lambda captures nothing from it:
        // `f = \x -> e where { g = … }` leaves `x` out of the `where`'s scope,
        // so the `where` body sees only globals. The value proves the `x`
        // inside the `where` is the GLOBAL `x` (= 100.0), not the λ-parameter.
        assert_eq!(
            value_of(
                "x = 100.0; f = \\y -> z + y where { z = x + 1.0; };",
                "f 1.0"
            ),
            Value::Float(102.0)
        );
    }

    #[test]
    fn where_inside_a_let_block_sees_the_let_bound_names() {
        // A `where` binding whose own right-hand side is a `let` expression,
        // over names bound in that `let`.
        assert_eq!(
            value_of(
                "f x = g where { g = let { a = x + 1.0; } in a * 2.0; };",
                "f 1.0"
            ),
            Value::Float(4.0)
        );
    }
}

/// Haskell 2010 §4.4.3.1 — guards on a function definition.
///
/// Every alternative is a `(guard, body)` pair, desugared into a `case` arm's
/// `guards`; `otherwise` is the ordinary Prelude constant `otherwise = True`.
mod function_guards {
    use super::*;

    #[test]
    fn function_guards_with_otherwise() {
        let defs = "f x | x > 0.0 = 1.0 | otherwise = 2.0;";
        // Both sides of the guard, so the test distinguishes a working guard
        // from one that always takes the first alternative.
        assert_eq!(value_of(defs, "f 1.0"), Value::Float(1.0));
        assert_eq!(value_of(defs, "f (0.0 - 1.0)"), Value::Float(2.0));
    }

    #[test]
    fn guards_are_tried_in_source_order() {
        // `x > 1.0` is checked first: 2.0 takes the first body, 0.5 the second.
        let defs = "f x | x > 2.0 = 10.0 | x > 1.0 = 20.0 | otherwise = 30.0;";
        assert_eq!(value_of(defs, "f 3.0"), Value::Float(10.0));
        assert_eq!(value_of(defs, "f 1.5"), Value::Float(20.0));
        assert_eq!(value_of(defs, "f 0.0"), Value::Float(30.0));
    }

    #[test]
    fn guard_bodies_may_reference_the_parameters() {
        assert_eq!(
            value_of("f x | x > 0.0 = x * 2.0 | otherwise = 0.0 - x;", "f 3.0"),
            Value::Float(6.0)
        );
    }

    #[test]
    fn guarded_function_without_fallback_is_rejected() {
        // No `otherwise` and no `case`, so nothing covers the negative input:
        // the definition is non-exhaustive and must be a compile error rather
        // than a silent fall-through.
        let err = error_of("f x | x > 0.0 = 1.0;", "f 0.0");
        assert!(
            err.to_string().contains("non-exhaustive"),
            "expected a non-exhaustive diagnostic, got {err}"
        );
    }

    #[test]
    fn a_literal_true_guard_counts_as_the_fallback() {
        // `otherwise` has no special status in the rule: a literal `true`
        // covers the definition just as well.
        assert_eq!(
            value_of("f x | x > 0.0 = 1.0 | true = 2.0;", "f (0.0 - 1.0)"),
            Value::Float(2.0)
        );
    }

    #[test]
    fn a_guard_that_is_not_a_bool_is_a_type_error() {
        let err = error_of("f x | x + 1.0 = 1.0 | otherwise = 2.0;", "f 1.0");
        assert!(
            err.to_string().contains("guard must be a Bool"),
            "expected a guard type diagnostic, got {err}"
        );
    }

    #[test]
    fn otherwise_is_a_value_not_a_keyword() {
        // The Prelude binds `otherwise = True`, so it is readable, rebindable
        // and usable away from a guard.
        assert!(bool_of("", "otherwise"));
        assert!(bool_of("otherwise = false;", "otherwise") == false);
    }

    #[test]
    fn guards_combine_with_argument_patterns() {
        let defs = "f (Just x) | x > 2.0 = 10.0 | otherwise = 0.0; f Nothing = 99.0;";
        assert_eq!(value_of(defs, "f (Just 3.0)"), Value::Float(10.0));
        assert_eq!(value_of(defs, "f (Just 1.0)"), Value::Float(0.0));
        assert_eq!(value_of(defs, "f Nothing"), Value::Float(99.0));
    }

    #[test]
    fn where_scopes_over_a_guarded_equation() {
        assert_eq!(
            value_of(
                "f x | x > 0.0 = x + y | otherwise = 0.0 where { y = 10.0; };",
                "f 1.0"
            ),
            Value::Float(11.0)
        );
    }
}

/// Haskell 2010 §4.4.3.1 — patterns in function arguments.
///
/// `f 0.0 = …; f x = …` matches the equations in source order; the desugar pass
/// builds that as a chain of `case` arms over the formal parameters.
mod argument_patterns {
    use super::*;

    #[test]
    fn patterns_in_function_arguments() {
        let defs = "f 0.0 = 1.0; f x = x;";
        // The first equation wins for 0.0; the variable equation covers the
        // rest. Both directions are asserted, so a group that always took the
        // first (or the last) equation fails.
        assert_eq!(value_of(defs, "f 0.0"), Value::Float(1.0));
        assert_eq!(value_of(defs, "f 3.0"), Value::Float(3.0));
    }

    #[test]
    fn equations_are_tried_in_source_order() {
        // `f 0.0 0.0`, then `f 0.0 y`, then the catch-all — each more general
        // than the last, so a wrong order changes every answer.
        let defs = "f 0.0 0.0 = 1.0; f 0.0 y = 2.0; f x y = x + y;";
        assert_eq!(value_of(defs, "f 0.0 0.0"), Value::Float(1.0));
        assert_eq!(value_of(defs, "f 0.0 5.0"), Value::Float(2.0));
        assert_eq!(value_of(defs, "f 5.0 6.0"), Value::Float(11.0));
    }

    #[test]
    fn constructor_patterns_in_arguments() {
        let defs = "f Nothing = 1.0; f (Just x) = x;";
        assert_eq!(value_of(defs, "f Nothing"), Value::Float(1.0));
        assert_eq!(value_of(defs, "f (Just 3.0)"), Value::Float(3.0));
    }

    #[test]
    fn wildcard_and_integer_patterns_in_arguments() {
        assert_eq!(
            value_of("f 0 = 1.0; f 1 = 2.0; f _ = 3.0;", "f 1"),
            Value::Float(2.0)
        );
        assert_eq!(
            value_of("f 0 = 1.0; f 1 = 2.0; f _ = 3.0;", "f 7"),
            Value::Float(3.0)
        );
    }

    #[test]
    fn an_incomplete_pattern_group_is_rejected() {
        // Only `Just` is covered, so `Nothing` has no equation: Haskell 2010
        // calls that a non-exhaustive pattern match and so must this compiler.
        let err = error_of("f (Just x) = x;", "f Nothing");
        assert!(
            err.to_string().contains("non-exhaustive"),
            "expected a non-exhaustive diagnostic, got {err}"
        );
    }

    #[test]
    fn equations_must_agree_on_arity() {
        let err = error_of("f 0.0 = 1.0; f x y = x;", "f 1.0");
        assert!(
            err.to_string().contains("argument pattern"),
            "expected an arity diagnostic, got {err}"
        );
    }
}

/// Haskell 2010 §4.4.1 — a standalone type signature attached to the binding(s)
/// that follow it.
mod standalone_signatures {
    use super::*;

    #[test]
    fn standalone_type_signature() {
        assert_eq!(
            value_of("f :: Float -> Float;\nf x = x;", "f 2.0"),
            Value::Float(2.0)
        );
    }

    #[test]
    fn standalone_signature_types_the_parameters() {
        // The signature is a real constraint, not decoration: `Int -> Int`
        // makes the arithmetic integer arithmetic, so the result is an `Int`.
        assert_eq!(
            value_of("f :: Int -> Int;\nf x = x + 1;", "f 1"),
            Value::Int(2)
        );
        // The same body without the signature stays on the inferred track.
        assert_eq!(value_of("f x = x + 1;", "f 1"), Value::Int(2));
    }

    #[test]
    fn standalone_signature_attaches_to_a_later_binding() {
        // The signature precedes its binding, separated by another declaration.
        assert_eq!(
            value_of("f :: Float -> Float;\ng = 5.0;\nf x = x + g;", "f 1.0"),
            Value::Float(6.0)
        );
    }

    #[test]
    fn standalone_signature_attaches_to_an_equation_group() {
        let defs = "f :: Float -> Float;\nf 0.0 = 1.0; f x = x;";
        assert_eq!(value_of(defs, "f 0.0"), Value::Float(1.0));
        assert_eq!(value_of(defs, "f 3.0"), Value::Float(3.0));
    }

    #[test]
    fn standalone_signature_attaches_to_a_guarded_group() {
        let defs = "f :: Float -> Float;\nf x | x > 0.0 = 1.0 | otherwise = 2.0;";
        assert_eq!(value_of(defs, "f 1.0"), Value::Float(1.0));
        assert_eq!(value_of(defs, "f (0.0 - 1.0)"), Value::Float(2.0));
    }

    #[test]
    fn standalone_signature_needs_no_semicolon() {
        // Haskell layout: the signature ends where the next declaration begins.
        assert_eq!(
            value_of("f :: Float -> Float\nf x = x + 1.0\n", "f 1.0"),
            Value::Float(2.0)
        );
    }

    #[test]
    fn a_signature_without_a_binding_declares_nothing() {
        // Nothing binds `g`, so the declaration is inert; the program still
        // compiles and evaluates its own expression.
        assert_eq!(value_of("g :: Float -> Float;", "1.0"), Value::Float(1.0));
    }
}

/// Haskell 2010 §3.5 — operator sections, as a REGRESSION guard.
///
/// Sections already worked before this task (they landed with the fixity
/// work); these assertions pin them so the desugar pass cannot regress them.
mod operator_sections {
    use super::*;

    #[test]
    fn operator_section_right() {
        // `(+ 1.0)` ≡ `\x -> x + 1.0`.
        assert_eq!(value_of("", "(+ 1.0) 2.0"), Value::Float(3.0));
    }

    #[test]
    fn operator_section_left() {
        // `(1.0 +)` ≡ `\x -> 1.0 + x`.
        assert_eq!(value_of("", "(1.0 +) 2.0"), Value::Float(3.0));
    }

    #[test]
    fn sections_keep_their_operand_order() {
        // Subtraction is not commutative, so the order pins which side the
        // operand took: `(10.0 -)` is `\x -> 10.0 - x` = 7.0, `(- 3.0)` (unary
        // negation, not a section) differs. The left section is the
        // discriminating case.
        assert_eq!(value_of("", "(10.0 -) 3.0"), Value::Float(7.0));
    }

    #[test]
    fn sections_are_usable_as_values() {
        assert_eq!(value_of("inc = (+ 1.0);", "inc 41.0"), Value::Float(42.0));
    }
}

/// Equation ORDER and backtracking, the subtle half of Haskell 2010 §4.4.3.1.
///
/// Equations are tried in source order and each is matched argument by argument;
/// a failure anywhere in one equation falls through to the NEXT equation, from
/// its FIRST argument. A column-by-column reading (match every equation's first
/// pattern, then every second) gives different answers once a catch-all equation
/// is interleaved with more specific ones.
mod equation_order {
    use super::*;

    #[test]
    fn a_catch_all_equation_keeps_its_place_in_the_order() {
        // `f x y = x + y` is written LAST but its first argument is a variable,
        // so it must still apply to every first argument except those consumed
        // earlier — including 0.0, which the first two equations only cover for
        // their SECOND argument.
        let defs = "f 0.0 0.0 = 1.0; f 0.0 2.0 = 3.0; f x y = x + y;";
        assert_eq!(value_of(defs, "f 0.0 0.0"), Value::Float(1.0));
        assert_eq!(value_of(defs, "f 0.0 2.0"), Value::Float(3.0));
        // Not covered by either literal equation, so the catch-all runs.
        assert_eq!(value_of(defs, "f 0.0 5.0"), Value::Float(5.0));
        assert_eq!(value_of(defs, "f 5.0 6.0"), Value::Float(11.0));
    }

    #[test]
    fn a_narrower_equation_wins_over_a_later_general_one() {
        // The reverse order: the general equation comes first, so the specific
        // one is unreachable (Haskell warns about that; it does not warn about
        // the order above). The computed value proves which equation ran.
        let defs = "f x y = x + y; f 0.0 0.0 = 1.0;";
        assert_eq!(value_of(defs, "f 0.0 0.0"), Value::Float(0.0));
    }

    #[test]
    fn equations_match_their_own_arguments_in_order() {
        // Two arguments, each equation pinning a different one: Haskell tries
        // `f 0.0 y` first (it covers 0.0 in the first position) and only then
        // `f x 0.0`.
        let defs = "f 0.0 y = 10.0 + y; f x 0.0 = 20.0 + x; f x y = x + y;";
        assert_eq!(value_of(defs, "f 0.0 0.0"), Value::Float(10.0));
        assert_eq!(value_of(defs, "f 1.0 0.0"), Value::Float(21.0));
        assert_eq!(value_of(defs, "f 1.0 2.0"), Value::Float(3.0));
    }

    #[test]
    fn a_where_block_belongs_to_its_own_equation() {
        // Each equation's `where` scopes over that equation only.
        let defs = "f 0.0 = y where { y = 10.0; }; f x = x;";
        assert_eq!(value_of(defs, "f 0.0"), Value::Float(10.0));
        assert_eq!(value_of(defs, "f 5.0"), Value::Float(5.0));
    }
}

/// The PRODUCTION path: `compile()` of a `main : IO ()` program.
///
/// Every module above asserts through `compile_eval` (a definition list plus a
/// value expression). That surface shares the parser, desugar and inference with
/// `compile`, but not the *entry*: `compile` runs a `main : IO ()` action to
/// completion. A desugaring bug that changes the shape of a top-level `Def`
/// (`params` vs a λ-nest) can work on one and break the other, so each construct
/// this task added is asserted here as a real program whose `stdout` is checked.
///
/// Definitions here mostly use a distinctive name (`dbl`, not `f`): the
/// `f`/`m`/`return` collisions that used to break this path are OD-9, closed by
/// capture-avoiding method instantiation — see
/// [`capture_avoiding_instantiation`].
mod io_program_path {
    use super::*;
    use rill_lang::compile;
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::run_on_stack;

    /// Compile and run a `main : IO ()` program, returning its `stdout`.
    ///
    /// The run happens on a grown stack: the lazy `IO` interpreter nests one
    /// `bind` per `do` statement, which exceeds libtest's default 2 MiB thread
    /// stack in a debug build.
    fn run_stdout(body: &str) -> Vec<String> {
        run_stdout_of("dbl", body)
    }

    /// Like [`run_stdout`], with the definition the `main` calls named
    /// explicitly — the OD-9 cases need a definition literally named `f`/`m`.
    fn run_stdout_of(name: &str, body: &str) -> Vec<String> {
        let src = format!(
            "{body}\nmain = do {{ write (Just (show_float ({name} 1.5))) stdout; return () }};\n"
        );
        run_on_stack(16 << 20, move || {
            let mut runner = compile::<f32, 256>(&src, ForeignRegistry::new())
                .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"));
            runner
                .run()
                .unwrap_or_else(|e| panic!("run must succeed: {e}\nsource:\n{src}"));
            runner.stdout().to_vec()
        })
    }

    #[test]
    fn a_plain_parameterised_definition_runs() {
        // The controller's regression: `f x = x` became unusable on this path.
        assert_eq!(run_stdout("dbl x = x;"), ["1.5"]);
    }

    #[test]
    fn function_guards_run() {
        // 1.5 takes the first alternative; the `otherwise` fallback is exercised
        // by the value, not merely parsed.
        assert_eq!(
            run_stdout("dbl x | x > 0.0 = 1.0 | otherwise = 2.0;"),
            ["1.0"]
        );
        assert_eq!(
            run_stdout("dbl x | x > 2.0 = 1.0 | otherwise = 2.0;"),
            ["2.0"]
        );
    }

    #[test]
    fn argument_patterns_run() {
        // The literal equation matches (1.5 is not 0.0, so the variable
        // equation runs); a 0.0 call takes the literal one.
        assert_eq!(run_stdout("dbl 0.0 = 42.0; dbl x = x;"), ["1.5"]);
        assert_eq!(
            run_on_stack(16 << 20, || {
                let src = "dbl 0.0 = 42.0; dbl x = x;\n\
                           main = do { write (Just (show_float (dbl 0.0))) stdout; return () };";
                let mut r = compile::<f32, 256>(src, ForeignRegistry::new()).unwrap();
                r.run().unwrap();
                r.stdout().to_vec()
            }),
            ["42.0"]
        );
    }

    #[test]
    fn a_standalone_signature_run() {
        let src = "dbl :: Float -> Float;\n\
                   dbl x = x;\n\
                   main = do { write (Just (show_float (dbl 1.5))) stdout; return () };";
        assert_eq!(
            run_on_stack(16 << 20, || {
                let mut r = compile::<f32, 256>(src, ForeignRegistry::new()).unwrap();
                r.run().unwrap();
                r.stdout().to_vec()
            }),
            ["1.5"]
        );
    }

    #[test]
    fn where_capture_runs() {
        // The `where` binding reads the definition's own parameter.
        assert_eq!(run_stdout("dbl x = g where { g = x + 1.0; };"), ["2.5"]);
    }

    #[test]
    fn a_where_bound_function_runs() {
        // A `where`-bound FUNCTION (not a plain value) takes its own parameter
        // and closes over the enclosing one — the shape that exercises the
        // local-function lowering on this path.
        assert_eq!(run_stdout("dbl x = g 1.0 where { g y = y + x; };"), ["2.5"]);
        // ... including a sibling `where` binding, which is not a parameter.
        assert_eq!(
            run_stdout("dbl x = g 10.0 where { g y = y * h; h = x + 1.0; };"),
            ["25.0"]
        );
    }

    #[test]
    fn argument_patterns_with_a_where_block_run() {
        assert_eq!(
            run_stdout("dbl 0.0 = y where { y = 42.0; }; dbl x = x;"),
            ["1.5"]
        );
    }

    #[test]
    fn a_two_equation_guard_group_runs() {
        assert_eq!(
            run_stdout("dbl x | x > 0.5 = 10.0 | x > 2.0 = 20.0 | otherwise = 30.0;"),
            ["10.0"]
        );
    }

    /// OD-9 regression pin. The Prelude's `Monad IO` instance is spelled
    /// `bind m f = …`; before capture-avoiding instantiation, inlining that
    /// body made a top-level user definition named `f` (or `m`) resolve to the
    /// METHOD PARAMETER's slot instead of the user's own definition, so the
    /// program faulted at run time (`arithmetic operand is absent`, `thunk body
    /// produced no result`) or was rejected (`field projection requires a
    /// record`, `` `main` is not an executable `IO ()` action ``).
    ///
    /// The fix renames each instance method's own parameters to fresh
    /// `$`-prefixed names before the body enters the caller, so a method
    /// parameter can never capture a caller's identifier. Every colliding name
    /// below must therefore now compute the value its own body defines.
    #[test]
    fn name_collisions_with_method_parameters_now_work() {
        // The baseline: a non-colliding name was always fine.
        assert_eq!(run_stdout("dbl x = x;"), ["1.5"]);
        // OD-9's two statement-shaped collisions: a user definition named `f`
        // and one named `m`. Each prints the value its own body computes
        // (1.5 + 1.0 = 2.5), which is unreachable if the method parameter
        // captures it.
        assert_eq!(run_stdout_of("f", "f x = x + 1.0;"), ["2.5"]);
        assert_eq!(run_stdout_of("m", "m x = x + 1.0;"), ["2.5"]);
        // `return` is the third colliding spelling: the `Monad` result-directed
        // method is also named `return`, and its own parameter must not capture
        // the user's definition. `main` here is the `write` effect, because the
        // user's `return` shadows the class method for the `IO ()` tail.
        let src = "return x = x + 1.0;\n\
                   main = write (Just (show_float (return 1.5))) stdout;\n";
        let out = run_on_stack(16 << 20, move || {
            compile::<f32, 256>(src, ForeignRegistry::new())
                .and_then(|mut r| r.run().map(|()| r.stdout().to_vec()))
                .map_err(|e| e.to_string())
        });
        assert_eq!(out.expect("a user `return` definition must work"), ["2.5"]);
    }
}

/// OD-9: capture-avoiding instantiation of typeclass method bodies.
///
/// An instance method body is spliced into the caller's expression, so the
/// method's OWN parameter names become caller-visible identifiers. The Prelude
/// spells `instance Monad IO` as `bind m f = …`, which made a user's top-level
/// `f`/`m` resolve to the method parameter instead of their own definition.
///
/// Every test below asserts a COMPUTED value that is DIFFERENT under the
/// defective reading, and each is pinned on both value surfaces: `compile_eval`
/// (the expression surface) and `compile()` + `run()` (the `main : IO ()`
/// surface, where the `do`/`bind` desugaring is what triggers the collision).
mod capture_avoiding_instantiation {
    use rill_lang::compile;
    use rill_lang::ffi::ForeignRegistry;

    /// The `stdout` of a `main : IO ()` program, run on a grown stack (the lazy
    /// `IO` interpreter nests one `bind` frame per `do` statement).
    fn stdout_of(src: &str) -> Vec<String> {
        let src = src.to_string();
        rill_lang::run_on_stack(16 << 20, move || {
            let mut runner = compile::<f32, 256>(&src, ForeignRegistry::new())
                .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"));
            runner
                .run()
                .unwrap_or_else(|e| panic!("run must succeed: {e}\nsource:\n{src}"));
            runner.stdout().to_vec()
        })
    }

    #[test]
    fn user_def_named_f_is_usable() {
        // `f` collides with `bind m f`'s second parameter. Under the defect the
        // IO path faults (`arithmetic operand is absent`) before printing.
        assert_eq!(
            stdout_of(
                "f x = x + 1.0;\n\
                 main = do { write (Just (show_float (f 1.0))) stdout; return () };"
            ),
            ["2.0"]
        );
        // The same definition on the value surface, where `do` is absent: this
        // proves the collision is about the METHOD PARAMETER, not about `do`.
        assert_eq!(super::float_of("f x = x + 1.0;", "f 1.0"), 2.0);
    }

    #[test]
    fn user_def_named_m_is_usable() {
        // `m` is `bind`'s FIRST parameter. Under the defect the IO path exhausts
        // the value track (`thunk body produced no result`).
        assert_eq!(
            stdout_of(
                "m x = x + 1.0;\n\
                 main = do { write (Just (show_float (m 1.0))) stdout; return () };"
            ),
            ["2.0"]
        );
        assert_eq!(super::float_of("m x = x + 1.0;", "m 1.0"), 2.0);
    }

    #[test]
    fn user_def_named_return_is_usable() {
        // `return` is the result-directed `Monad` method, and its own parameter
        // is `x`. The user's `return` must compute its own value; the METHOD's
        // `x` must not capture a caller's `x`.
        //
        // A source that names a top-level `x` AND keeps a `return ()` tail is
        // well-typed Haskell (the method's `x` binds the element, the top-level
        // `x` is just an unused function), so it must RUN. Pre-fix it failed at
        // run time with `` `main` must have type `IO ()`: the final action's
        // result is not `()` ``: the method parameter `x` resolved to the user's
        // top-level `x`, so the `IO` record the instance builds was the wrong
        // shape and `return ()` produced a non-unit action.
        assert_eq!(
            stdout_of(
                "x q = q * 2.0;\n\
                 main = do { write (Just \"a\") stdout; write (Just \"b\") stdout; return () };"
            ),
            ["a", "b"]
        );
        // The user's own `return` definition still computes its own value inside
        // a `do` block (two statements, so `bind` is instantiated around it).
        assert_eq!(
            stdout_of(
                "return x = x + 1.0;\n\
                 main = do { write (Just \"a\") stdout;\n\
                             write (Just (show_float (return 1.0))) stdout; };"
            ),
            ["a", "2.0"]
        );
        assert_eq!(super::float_of("return x = x + 1.0;", "return 1.0"), 2.0);
    }

    #[test]
    fn user_params_named_like_method_params() {
        // A user definition whose OWN name AND parameter are spelled like a
        // `Monad` method parameter. `f m = m + 1.0` applied to 3.0 is 4.0 only if
        // the user's `m` keeps its own binding; under the defect the body's `m`
        // resolved to `bind`'s parameter slot and the run faulted.
        assert_eq!(
            stdout_of(
                "f m = m + 1.0;\n\
                 main = do { write (Just (show_float (f 3.0))) stdout; return () };"
            ),
            ["4.0"]
        );
        assert_eq!(super::float_of("f m = m + 1.0;", "f 3.0"), 4.0);
        // The mirror image: the definition is `m` and the parameter `f`.
        assert_eq!(
            stdout_of(
                "m f = f + 1.0;\n\
                 main = do { write (Just (show_float (m 3.0))) stdout; return () };"
            ),
            ["4.0"]
        );
        assert_eq!(super::float_of("m f = f + 1.0;", "m 3.0"), 4.0);
        // A definition with TWO parameters named after the two `bind`
        // parameters: both must keep their own bindings, so the sum is 7.0 and
        // not a re-application of one to the other.
        assert_eq!(
            stdout_of(
                "add m f = m + f;\n\
                 main = do { write (Just (show_float (add 3.0 4.0))) stdout; return () };"
            ),
            ["7.0"]
        );
        assert_eq!(super::float_of("add m f = m + f;", "add 3.0 4.0"), 7.0);
    }

    #[test]
    fn method_called_with_colliding_local() {
        // A user definition named `f` used as a first-class VALUE inside a `do`
        // block: `let g = f in g 1.0` must apply the user's `f` (1.0 + 1.0 =
        // 2.0). Under the defect the reference resolved to `bind`'s parameter
        // slot and the run faulted with `arithmetic operand is absent`.
        assert_eq!(
            stdout_of(
                "f x = x + 1.0;\n\
                 main = do { let g = f; write (Just (show_float (g 1.0))) stdout; return () };"
            ),
            ["2.0"]
        );
        // The mirror image with `m`, whose defective diagnostic is `thunk body
        // produced no result`.
        assert_eq!(
            stdout_of(
                "m x = x + 1.0;\n\
                 main = do { let u = m; write (Just (show_float (u 1.0))) stdout; return () };"
            ),
            ["2.0"]
        );
        // A genuine class-method call with a COLLIDING local argument: `fmap`'s
        // instance body is `map g xs`, so the caller's lambda parameter named
        // `g` sits where the method parameter `g` would be. The lambda triples
        // its argument, so the result is 6.0 — a captured `g` (the method's own
        // parameter) would project a different function.
        assert_eq!(
            stdout_of(
                "main = do { write (Just (show_float (\
                   case fmap (\\g -> g * 3.0) (Just 2.0) of { Nothing -> 0.0; Just v -> v }\
                 ))) stdout; return () };"
            ),
            ["6.0"]
        );
        assert_eq!(
            super::float_of(
                "",
                "case fmap (\\g -> g * 3.0) (Just 2.0) of { Nothing -> 0.0; Just v -> v }"
            ),
            6.0
        );
    }
}
