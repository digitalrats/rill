// Prelude-vs-user scoping rules, pinned end to end.
//
// Two independent rules meet here:
//
// * Strict mode rejects USER recursion, while the Prelude is trusted library
//   code: a recursive Prelude definition must not make every Strict-mode
//   program fail.
// * A user definition always SHADOWS a Prelude definition of the same name, in
//   inference AND in lowering. Typing the Prelude's scheme while executing the
//   user's body is the silent-mistyping failure mode this pins.
use rill_lang::compile;
use rill_lang::ffi::ForeignRegistry;

#[test]
fn strict_mode_still_rejects_user_recursion() {
    // `compile` defaults to COMPLETE mode (recursion allowed), so Strict mode
    // must be pinned explicitly — otherwise this assertion could never fail.
    let src = r#"{-# MODE strict #-}
loop n = loop n; main = do { write (Just "x") stdout; return () };"#;
    assert!(compile::<f32, 256>(src, ForeignRegistry::new()).is_err());
}

#[test]
fn user_recursion_is_rejected_even_when_it_shadows_a_prelude_name() {
    // Scoping the recursion check to user defs must not create a hole: a user
    // definition named like a Prelude one is still a USER definition, so its
    // recursion is rejected on its own body, not excused by the Prelude.
    let src = r#"{-# MODE strict #-}
from n = cons n (from (n + 1.0));
main = do { write (Just "x") stdout; return () };"#;
    let err = compile::<f32, 256>(src, ForeignRegistry::new())
        .err()
        .expect("user recursion must be rejected in Strict mode");
    assert!(
        err.to_string().contains("recursive function call: `from`"),
        "expected the recursion diagnostic, got: {err}"
    );
}

#[test]
fn recursive_prelude_def_does_not_fail_strict_mode() {
    // The Prelude declares `from` recursively. Strict mode bans recursion in
    // USER definitions only: the recursion check runs before the Prelude defs
    // are merged in, so merely compiling a Strict-mode program must not trip
    // "recursive function call" on Prelude code. This program does not mention
    // `from`; the check that rejected it lived in the merge, not in the
    // program.
    let src = r#"main = do { write (Just "x") stdout; return () };"#;
    let mut r =
        compile::<f32, 256>(src, ForeignRegistry::new()).expect("strict program must compile");
    r.run().expect("run must succeed");
    assert_eq!(r.stdout(), ["x"]);
}

#[test]
fn user_definition_shadows_a_prelude_definition() {
    // The user's `from` takes TWO arguments; the Prelude's takes one. If
    // inference kept the Prelude scheme for the name (its old filter excluded
    // user defs named like Prelude defs) this call would be rejected with an
    // arity error even though lowering runs the user's body. Shadowing means
    // the user's ARITY and body both win.
    let src = r#"from a b = a + b;
main = do { write (Just (show_float (from 1.0 2.0))) stdout; return () };"#;
    let mut r = compile::<f32, 256>(src, ForeignRegistry::new())
        .expect("user definition must shadow the Prelude one");
    r.run().expect("run must succeed");
    assert_eq!(r.stdout(), ["3.0"]);
}

#[test]
fn user_recursive_from_still_shadows_prelude_from_in_complete_mode() {
    // The Prelude `from` steps by 1.0; the user's steps by 2.0 and offsets the
    // head. Complete mode allows the user recursion. If lowering ran the
    // Prelude body (or inference typed the Prelude scheme for the user's
    // definition), the value would be the Prelude's 10/11/12 chain.
    let src = r#"{-# MODE complete #-}
from n = cons (n + 100.0) (from (n + 1.0));
first_two xs = cons (case (head xs) of { Nothing -> 0.0; Just x -> x }) (cons (case (head (tail xs)) of { Nothing -> 0.0; Just x -> x }) (list));
main = do { xs <- fromList (first_two (from 10.0));
    ys <- toList xs;
    write (Just (fold (\acc -> \x -> concat_string acc (show_float x)) "" ys)) stdout;
    return () };
"#;
    let mut r = compile::<f32, 256>(src, ForeignRegistry::new()).expect("program must compile");
    r.run().expect("run must succeed");
    assert_eq!(r.stdout(), ["110.0111.0"]);
}

#[test]
fn strict_mode_does_not_expose_recursive_prelude_definitions() {
    // Strict mode's contract is an acyclic call graph, so it cannot EXECUTE a
    // recursive Prelude helper. Excluding such a def from the merged list makes
    // a Strict program's reference to it a clean "unknown identifier" instead
    // of an unbounded lowering recursion (which overflowed the compiler stack
    // when the def was merged in unconditionally).
    let src = r#"{-# MODE strict #-}
main = case head (from 1.0) of { Nothing -> 0.0; Just x -> x };"#;
    let err = compile::<f32, 256>(src, ForeignRegistry::new())
        .err()
        .expect("strict mode must not expose the recursive Prelude `from`");
    assert!(
        err.to_string().contains("unknown identifier `from`"),
        "expected an unknown-identifier diagnostic, got: {err}"
    );
}

#[test]
fn complete_mode_still_exposes_the_recursive_prelude_from() {
    // The counterpart: Complete mode is the one that supports recursion, so the
    // Prelude `from` must stay usable there. `head (from 1.0)` forces only the
    // first element of an infinite list.
    let src = r#"{-# MODE complete #-}
main = do { write (Just (show_float (case head (from 1.0) of { Nothing -> 0.0; Just x -> x }))) stdout; return () };"#;
    let mut r =
        compile::<f32, 256>(src, ForeignRegistry::new()).expect("complete program must compile");
    r.run().expect("run must succeed");
    assert_eq!(r.stdout(), ["1.0"]);
}

#[test]
fn capacity_bound_does_not_count_prelude_definition_bodies() {
    // B3: the arena budget is a static per-program bound over allocation sites,
    // so a recursive Prelude def is charged the same constant as an identical
    // user body and does NOT scale with demand. This pins the property the
    // benchmark size relies on: `take n (from 1)` compiles to the SAME capacity
    // for every `n`.
    fn capacity(n: usize) -> (usize, usize) {
        let src = format!(
            r#"take k xs = if k == 0.0 then list else cons (case (head xs) of {{ Nothing -> 0.0; Just x -> x }}) (take (k - 1.0) (tail xs));
main = do {{ xs <- fromList (take {n}.0 (from 1.0)); ys <- toList xs; write (Just (show_float (fold (\ a -> \ x -> a + x) 0.0 ys))) stdout; return () }};"#
        );
        let ir = rill_lang::compile_eval::<f32, 256>(&src, "main", rill_lang::mode::Mode::Complete)
            .expect("program must compile");
        (ir.value_state.capacity, ir.value_state.buffer_budget)
    }
    let one = capacity(1);
    for n in [5, 16, 64, 128, 256] {
        assert_eq!(capacity(n), one, "arena budget grew with demand at n = {n}");
    }
}

#[test]
fn benchmark_size_take_64_runs_within_the_arena_budget() {
    // The GHC-parity benchmark list size. The arena is a fixed budget, so this
    // pins that `n = 64` stays inside it (the measured wall is n = 84).
    let src = r#"take k xs = if k == 0.0 then list else cons (case (head xs) of { Nothing -> 0.0; Just x -> x }) (take (k - 1.0) (tail xs));
main = do { xs <- fromList (take 64.0 (from 1.0)); ys <- toList xs;
    write (Just (show_float (fold (\ a -> \ x -> a + x) 0.0 ys))) stdout;
    return () };
"#;
    let mut r = compile::<f32, 256>(src, ForeignRegistry::new()).expect("program must compile");
    r.run().expect("n = 64 must fit the arena budget");
    assert_eq!(r.stdout(), ["2080.0"], "1 + .. + 64");
}
