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

/// `++` — list append (`infixr 5` in the Haskell 2010 Prelude).
mod list_append {
    use super::*;

    #[test]
    fn list_append_operator() {
        // Length and element sum, so the test pins the CONTENT, not just the
        // result type.
        assert_eq!(value_of("", "length ([1.0, 2.0] ++ [3.0])"), Value::Int(3));
        assert_eq!(
            value_of(
                "",
                "fold (\\a -> \\b -> a + b) 0.0 ([1.0, 2.0] ++ [3.0, 4.0])"
            ),
            Value::Float(10.0)
        );
    }

    #[test]
    fn list_append_is_right_associative() {
        // `infixr 5`: `xs ++ ys ++ zs` ≡ `xs ++ (ys ++ zs)`.
        assert_eq!(
            value_of(
                "",
                "fold (\\a -> \\b -> a + b) 0.0 ([1.0] ++ ([2.0, 3.0] ++ [4.0, 5.0]))"
            ),
            Value::Float(15.0)
        );
        assert_eq!(
            value_of("", "length ([1.0] ++ [2.0, 3.0] ++ [4.0, 5.0])"),
            Value::Int(5)
        );
    }

    #[test]
    fn list_append_binds_tighter_than_comparison() {
        // `++` (5) binds tighter than comparison (4): the comparison sees the
        // CONCATENATED list, so `length ([1.0] ++ [2.0]) == 2` is true. A
        // looser `++` would try to compare a Bool/Int and fail to type.
        assert_eq!(
            value_of("", "length ([1.0] ++ [2.0]) == 2"),
            Value::Bool(true)
        );
    }

    #[test]
    fn cons_tail_takes_the_whole_append() {
        // `1.0 : [2.0] ++ [3.0]` ≡ `1.0 : ([2.0] ++ [3.0])` — the cons tail is
        // the whole concatenation, so the list has three elements.
        assert_eq!(
            value_of("", "length (1.0 : ([2.0] ++ [3.0]))"),
            Value::Int(3)
        );
        assert_eq!(value_of("", "length (1.0 : [2.0] ++ [3.0])"), Value::Int(3));
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
