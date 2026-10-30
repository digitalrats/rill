//! `(!)` is the array accessor and `index` the `Ix` offset method: two names,
//! two signatures, never one dispatching on its first argument.
//!
//! GHC's Prelude has both:
//!
//! ```haskell
//! (!)   :: Ix i => Array i e -> i -> e      -- element
//! index :: Ix a => (a, a) -> a -> Int       -- offset
//! ```
//!
//! While `index` did DOUBLE DUTY, dispatch had to GUESS from the first
//! argument's type; an array whose type still carried variables (`Array i e`
//! before `i`/`e` are solved) let the guess bind the element type to the index
//! type, reporting `no instance of Ix for type Double`. Splitting the names
//! makes each one's role structural. The 2-D case with a literal index is the
//! shape that exposed the guessing — it is the regression this file pins.
//!
//! Programs run through the `IO` surface (`main : IO ()`), the shape in which
//! the accessor's element type is written to stdout.

use rill_lang::ffi::ForeignRegistry;
use rill_lang::{compile, run_on_stack};

/// Compile and run `src`, returning the concatenated captured stdout.
///
/// The `IO` interpreter nests one `bind` per statement, so the work runs on a
/// grown stack (see [`rill_lang::run_on_stack`]).
fn run(src: &str) -> String {
    let src = src.to_string();
    run_on_stack(16 << 20, move || {
        let mut r = compile::<f32, 256>(&src, ForeignRegistry::new())
            .unwrap_or_else(|e| panic!("compile failed for:\n{src}\n{e}"));
        r.run()
            .unwrap_or_else(|e| panic!("run failed for:\n{src}\n{e}"));
        r.stdout().concat()
    })
}

/// Compile `src`, expecting a TYPE error, and return its rendered message.
fn compile_err(src: &str) -> String {
    let src = src.to_string();
    run_on_stack(16 << 20, move || {
        match compile::<f32, 256>(&src, ForeignRegistry::new()) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("expected a compile error, got a successful compile:\n{src}"),
        }
    })
}

// ---------------------------------------------------------------------------
// 1. The two names, one role each.
// ---------------------------------------------------------------------------

#[test]
fn bang_is_the_array_accessor() {
    // `a ! 1` is the ELEMENT at offset 1, not an `Ix` offset.
    assert_eq!(
        run(
            "a = listArray (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0];\n\
             main = do { write (Just (show_double (a ! 1))) stdout; return () };"
        ),
        "20.0"
    );
}

#[test]
fn index_is_the_offset_method() {
    // GHC: `index (0,2) 1` is 1 and `index (5,9) 7` is 2 — OFFSETS, so the
    // second proves it is not an element lookup.
    assert_eq!(
        run("main = do { write (Just (show_int (index (Pair { first: 0, second: 2 }) 1))) stdout; return () };"),
        "1"
    );
    assert_eq!(
        run("main = do { write (Just (show_int (index (Pair { first: 5, second: 9 }) 7))) stdout; return () };"),
        "2"
    );
}

#[test]
fn a_literal_index_does_not_collapse_the_array_parameters() {
    // The regression Task 7 hit: an unsolved `Array i e` let the element type
    // bind to the index type. A 2-D array with a literal index is the shape
    // that exposed it — the element is a Double and the index a `Pair Int Int`.
    assert_eq!(
        run("a = listArray (Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 1, second: 1 } }) [1.0, 2.0, 3.0, 4.0];\n\
             main = do { write (Just (show_double (a ! Pair { first: 1, second: 0 }))) stdout; return () };"),
        "3.0"
    );
}

#[test]
fn a_numeric_bang_is_not_an_index() {
    // `!` has ONE meaning now: a non-array first argument is a TYPE error, not
    // a silent `index` call that would answer an offset.
    let err = compile_err("main = (1 :: Int) ! 2;");
    assert!(
        err.contains("cannot unify")
            || err.contains("Array")
            || err.contains("type")
            || err.contains("instance"),
        "a non-array `!` must be a type error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 2. `!` is ONE function: infix, prefix and section spellings are the same.
// ---------------------------------------------------------------------------

#[test]
fn prefix_bang_is_the_same_function_as_infix_bang() {
    // System FC: `a ! i` IS `(!) a i` — no operator node.
    for src in [
        "a = listArray (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0];\n\
         main = do { write (Just (show_double (a ! 1))) stdout; return () };",
        "a = listArray (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0];\n\
         main = do { write (Just (show_double ((!) a 1))) stdout; return () };",
    ] {
        assert_eq!(run(src), "20.0", "source:\n{src}");
    }
}

#[test]
fn a_bang_section_reads_the_same_cell() {
    // `(! 1)` is `\x -> x ! 1` (Haskell 2010 §3.5), the same function again.
    assert_eq!(
        run(
            "a = listArray (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0];\n\
             at1 = (! 1);\n\
             main = do { write (Just (show_double (at1 a))) stdout; return () };"
        ),
        "20.0"
    );
}

// ---------------------------------------------------------------------------
// 3. The roles do not leak into each other.
// ---------------------------------------------------------------------------

#[test]
fn index_does_not_read_array_elements() {
    // `index` is the `Ix` METHOD only. Applied to an `Array`, which has no `Ix`
    // instance, it is a type error — it must not quietly become the accessor
    // (the double duty the split removed).
    let err =
        compile_err("main = index (listArray (Pair { first: 0, second: 2 }) [1.0, 2.0, 3.0]) 1;");
    assert!(
        err.contains("Ix") && !err.contains("must have type"),
        "an `Array` is not an `Ix` index type, so `index` must be an `Ix` type error, got: {err}"
    );
}
