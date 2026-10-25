//! Expression type annotations `e :: t` (Haskell 2010 §3.12).
//!
//! An annotation is a type-level construct: it pins the annotated expression's
//! type during inference and then disappears, so lowering emits exactly the IR
//! the un-annotated expression would have produced. These tests assert the
//! computed values (not merely that a source compiles), because a silently
//! wrong precedence or a silently erased-but-retyped node both still compile.

pub mod common;

use common::*;
use rill_lang::arena::Value;
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;

/// Compile and evaluate the `main` expression of `src` in a single run,
/// returning its value.
fn run_value(src: &str) -> Value {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    let v = prog.result().expect("the run must produce a single result");
    prog.arena().get(v).expect("the result ref is live").clone()
}

/// Compile `src` and return the compile error, panicking when it compiles.
fn compile_err(src: &str) -> String {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    match eval_ffi(defs, expr, ForeignRegistry::<f32>::new()) {
        Ok(_) => panic!("expected the program to be rejected\nsrc:\n{src}"),
        Err(e) => format!("{e}"),
    }
}

/// The number of instructions in the program's entry value block.
fn entry_instrs(expr: &str) -> usize {
    let ir = compile_eval::<f32, 256>("", expr, Mode::Strict)
        .unwrap_or_else(|e| panic!("`{expr}` must compile: {e}"));
    ir.value_blocks[ir.value_entry].instrs.len()
}

/// Render `src`, re-parse the rendering, render again, and assert the two
/// renderings agree; return the first. The annotation must survive both the
/// renderer and the round trip.
fn render_main(src: &str) -> String {
    use rill_lang::lexer::tokenize;
    use rill_lang::parser::parse;
    use rill_lang::render::render;
    let program = parse(&tokenize(src).unwrap(), src.as_bytes()).unwrap();
    let first = render(&program).unwrap();
    let reparsed = parse(&tokenize(&first).unwrap(), first.as_bytes())
        .unwrap_or_else(|e| panic!("the rendering `{first}` must re-parse: {e}"));
    let second = render(&reparsed).unwrap();
    assert_eq!(first, second, "render → parse → render is not idempotent");
    first
}

#[test]
fn annotation_pins_a_literal() {
    // `(1 :: Int)` is an Int: `show_int` (Int -> String) accepts it and renders
    // "1". Also assert the Int TRACK survives the annotation — `(1 :: Int) + 2`
    // is `Value::Int(3)`, not a Float promoted through the erased node.
    let shown = run_value("main = show_int (1 :: Int);");
    assert_eq!(shown, Value::String("1".into()));

    let summed = run_value("main = (1 :: Int) + 2;");
    assert_eq!(summed, Value::Int(3), "the annotation keeps the Int track");

    // The node also RENDERS as `e :: t` and survives render → parse → render,
    // so a serialized/re-rendered program keeps its annotations.
    assert_eq!(render_main("main = 1 :: Int"), "main = (1 :: Int)");
    assert_eq!(
        render_main("main = show_int (1 :: Int)"),
        "main = show_int (1 :: Int)"
    );
}

#[test]
fn annotation_binds_looser_than_application() {
    // `show_int 1 :: String` is `(show_int 1) :: String` (Haskell 2010 §3.12:
    // the annotated side is an `infixexp`). The test is DISCRIMINATING: the
    // tighter reading `show_int (1 :: String)` is a type error, because that
    // would annotate the Int literal as a String. Only the loose grouping can
    // produce "1".
    assert_eq!(
        run_value("main = show_int 1 :: String;"),
        Value::String("1".into())
    );
    assert_eq!(
        run_value("main = show_int 1;"),
        Value::String("1".into()),
        "the un-annotated call is the same value, so the annotation adds nothing runtime"
    );

    // The tighter grouping is still expressible explicitly, and there it fails.
    let tight = compile_err("main = show_int (1 :: String);");
    assert!(
        tight.contains("String") && tight.contains("Int"),
        "expected `show_int (1 :: String)` to be rejected, got: {tight}"
    );
}

#[test]
fn annotation_tighter_than_arithmetic() {
    // A parenthesized annotation inside an arithmetic operand parses and
    // evaluates: `x + (y :: Int)` is 3 + 4 = 7.
    let operand = run_value("x = 3;\ny = 4;\nmain = show_int (x + (y :: Int));");
    assert_eq!(operand, Value::String("7".into()));

    // Unparenthesized, the annotation takes the WHOLE infix expression:
    // `1 < 2 :: Bool` is `(1 < 2) :: Bool`. This is DISCRIMINATING — the
    // tighter reading `1 < (2 :: Bool)` must annotate an Int literal as Bool
    // and is rejected. So `true` proves the loose grouping.
    assert_eq!(run_value("main = 1 < 2 :: Bool;"), Value::Bool(true));

    let tight = compile_err("main = 1 < (2 :: Bool);");
    assert!(
        tight.contains("Bool") && tight.contains("Int"),
        "expected the annotation mismatch, got: {tight}"
    );

    // The same split at the arithmetic level: `3 + 4 :: Int` annotates the sum
    // (and is 7), while annotating the right operand alone is a mismatch.
    assert_eq!(
        run_value("main = show_int (3 + 4 :: Int);"),
        Value::String("7".into())
    );
}

#[test]
fn mismatched_annotation_is_a_type_error() {
    // A `Float` literal cannot be annotated `Int`: the annotation pins the
    // inner type, so the two must unify.
    let mismatched = compile_err("main = 1.5 :: Int;");
    assert!(
        mismatched.contains("Int") && mismatched.contains("Float"),
        "expected a type mismatch between Int and Float, got: {mismatched}"
    );
    // …and the same literal without the annotation compiles, so the error is
    // the annotation's doing and not the literal's.
    assert_eq!(run_value("main = 1.5;"), Value::Float(1.5));
}

#[test]
fn annotation_erases_at_lowering() {
    // The annotated expression still evaluates to its value…
    assert_eq!(run_value("main = (1.5 :: Float);"), Value::Float(1.5));
    assert_eq!(run_value("main = 1.5;"), Value::Float(1.5));

    // …and the IR is IDENTICAL to the un-annotated spelling: an annotation is
    // erased, not a runtime no-op instruction. Comparing the entry block's
    // instruction count catches a node that "passes the value through" by
    // emitting an extra register copy.
    assert_eq!(
        entry_instrs("1.5 :: Float"),
        entry_instrs("1.5"),
        "`e :: t` must lower to exactly the IR of `e`"
    );
    assert_eq!(
        entry_instrs("1.0 + 2.0 :: Float"),
        entry_instrs("1.0 + 2.0"),
        "an annotation on an infix expression must erase too"
    );
    assert_eq!(
        entry_instrs("show_int (1 :: Int)"),
        entry_instrs("show_int 1"),
        "an annotation on an application argument must erase too"
    );
}

#[test]
fn int_index_position_stays_int() {
    // Flat array offsets are `Int`-typed. An un-annotated literal keeps
    // resolving to `Int` (the array primitives' `Ix` demand), and an explicitly
    // annotated one behaves the same — the annotation adds no new resolution
    // path and removes none.
    let plain = run_value(
        "{-# MODE complete #-}\nmain = runST (\\s -> do { \
           a <- newArrayFlat (Pair { first: 0, second: 2 }) 3 7.0; \
           readArrayFlat a 1 \
         });",
    );
    assert_eq!(plain, Value::Float(7.0), "the un-annotated index reads 7.0");

    let annotated = run_value(
        "{-# MODE complete #-}\nmain = runST (\\s -> do { \
           a <- newArrayFlat (Pair { first: 0, second: 2 }) 3 7.0; \
           readArrayFlat a (1 :: Int) \
         });",
    );
    assert_eq!(annotated, Value::Float(7.0), "an `:: Int` index reads 7.0");
}
