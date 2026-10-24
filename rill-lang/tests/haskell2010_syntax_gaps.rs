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
