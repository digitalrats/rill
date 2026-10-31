//! Deferred (result-directed) dispatch: constraints are COLLECTED from every
//! input before any type is chosen.
//!
//! Haskell 2010's inference collects the constraints each subexpression
//! demands, unifies the equations, and picks a concrete type only at the
//! binding group's close (§4.3.4 defaulting) — never mid-expression. A
//! result-directed method (`mempty : m`) is the case this bites on: its class
//! variable is in no argument, so it is solved by the expected RESULT type, and
//! that type may still be a variable contributed by a SIBLING argument.
//!
//! `mappend mempty 3.5` is the canonical shape. `mempty :: m` contributes
//! `Monoid a`; `3.5 :: Num a` contributes `Fractional a` (hence `Num`, `Eq`,
//! `Ord`); both arrive at the SAME `a`. GHC 9.10.3 measures the whole thing as
//! `(Monoid a, Fractional a) => a`, and `mappend mempty 3.5` is an AMBIGUITY
//! there (no `Monoid Double` in `base`) — rill has `instance Monoid Double`, so
//! its answer is `3.5`.
//!
//! These tests pin that rill does not demand a concrete type before the sibling
//! has spoken.

/// Compile and run `src`, returning the concatenated stdout.
fn run(src: &str) -> String {
    let mut prog = rill_lang::compile::<f32, 256>(src, rill_lang::ffi::ForeignRegistry::new())
        .unwrap_or_else(|e| panic!("compile failed for:\n{src}\n{e}"));
    prog.run()
        .unwrap_or_else(|e| panic!("run failed for:\n{src}\n{e}"));
    prog.stdout().concat()
}

/// The compile error `src` produces, panicking when it compiles.
fn err(src: &str) -> String {
    match rill_lang::compile::<f32, 256>(src, rill_lang::ffi::ForeignRegistry::new()) {
        Ok(_) => panic!("must be a compile error:\n{src}"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn a_sibling_argument_collects_the_constraints_on_one_variable() {
    // What this file is about: `mempty`'s `m` and `3.5`'s `a` are ONE variable,
    // so the constraints collect rather than each being decided alone.
    //
    // KNOWN DIVERGENCE (OD-33): GHC then reports that variable AMBIGUOUS, since
    // `Monoid` is outside §4.3.4's standard class set. rill defaults it to
    // `Double` (it has `instance Monoid Double`) and answers `3.5`. The
    // collection is what is tested here; the defaulting is the divergence.
    assert_eq!(
        run("main = do { write (Just (show_double (mappend mempty 3.5))) stdout; return () };"),
        "3.5"
    );
    assert_eq!(
        run("main = do { write (Just (show_double (mappend 3.5 mempty))) stdout; return () };"),
        "3.5"
    );
}

#[test]
fn a_collapsed_list_operand_STILL_resolves() {
    // The sibling rule does work when the sibling's type is its own: a list
    // literal fixes the element type, so `Monoid (List a)` is selected with no
    // ambiguity. GHC accepts this program.
    assert_eq!(
        run("main = do { write (Just (show_int (length (mappend mempty [1.0, 2.0])))) stdout; return () };"),
        "2"
    );
}

#[test]
fn a_later_operand_solves_a_polymorphic_result() {
    // GHC: `g 0 + 2.5` is `3.5` — `b` unifies with the literal's `Double`.
    // Compiling but faulting at RUN time (`arithmetic mixes an Integer operand
    // with a floating one`) is the failure this rules out: it is a wrong
    // ANSWER, not a diagnostic.
    assert_eq!(
        run("g :: Num b => Int -> b;\ng x = 1;\nmain = do { write (Just (show_double (g 0 + 2.5))) stdout; return () };"),
        "3.5"
    );
    // And with the polymorphic call on the right.
    assert_eq!(
        run("g :: Num b => Int -> b;\ng x = 1;\nmain = do { write (Just (show_double (2.5 + g 0))) stdout; return () };"),
        "3.5"
    );
}

#[test]
fn collection_does_not_forget_the_other_operands_constraints() {
    // `mempty` at `String` must still work: the deferred decision has to pick
    // the type the OTHER operand demands, not a scalar default.
    assert_eq!(
        run("main = do { write (Just (mappend mempty (\"ab\"))) stdout; return () };"),
        "ab"
    );
    // A list: `mempty :: [a]` covers every element type, so no ambiguity.
    assert_eq!(
        run("main = do { write (Just (show_int (length (mappend mempty [1.0, 2.0])))) stdout; return () };"),
        "2"
    );
}
