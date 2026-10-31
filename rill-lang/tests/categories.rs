//! Built-in category typeclasses: `Functor`, `Applicative`, `Monad`, `Monoid`
//! registered via the language prelude, with compile-time inline resolution.
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;

/// Compile `src` against an empty foreign registry and RUN it once (the
/// single-run value surface), returning the already-run program.
fn run(src: &str) -> WorldState<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    eval_ffi(defs, expr, ffi).unwrap()
}

/// The compile error `src` produces, panicking when it compiles.
fn compile_err(src: &str) -> String {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    match eval_ffi(defs, expr, ForeignRegistry::<f32>::new()) {
        Ok(_) => panic!("must be a compile error:\n{src}"),
        Err(e) => e.to_string(),
    }
}

fn out_int(prog: &WorldState<f32, 256>) -> i64 {
    match prog.arena().get(prog.result().unwrap()).unwrap() {
        rill_lang::arena::Value::Int(n) => *n,
        other => panic!("expected Int, got {other:?}"),
    }
}

fn out_float(prog: &WorldState<f32, 256>) -> f64 {
    match prog.arena().get(prog.result().unwrap()).unwrap() {
        rill_lang::arena::Value::Double(n) => *n,
        other => panic!("expected Double, got {other:?}"),
    }
}

#[test]
fn functor_fmap_over_list() {
    let prog = run(r"main = length (fmap (\x -> x * 2.0) [1.0, 2.0, 3.0]);");
    assert_eq!(out_int(&prog), 3);
}

#[test]
fn functor_fmap_over_maybe() {
    let prog =
        run(r"main = case (fmap (\x -> x + 1.0) (Just 1.0)) of { Nothing -> 0.0; Just x -> x; };");
    assert_eq!(out_float(&prog), 2.0);
}

#[test]
fn concat_map_splices_lists() {
    let prog = run(r"main = length (concat_map (\x -> [x, x]) [1.0, 2.0]);");
    assert_eq!(out_int(&prog), 4);
}

#[test]
fn append_list_and_concat_string() {
    let prog = run("main = length (append_list [1.0, 2.0] [3.0, 4.0, 5.0]);");
    assert_eq!(out_int(&prog), 5);
}

#[test]
fn monad_bind_maybe_short_circuits_nothing() {
    // bind (Nothing) f = Nothing
    let prog = run(
        r"main = case (bind (Nothing) (\x -> Just (x + 1.0))) of { Nothing -> 0.0; Just x -> x; };",
    );
    assert_eq!(out_float(&prog), 0.0);
}

#[test]
fn monad_bind_maybe_chains() {
    let prog = run(
        r"main = case (bind (Just 1.0) (\x -> Just (x + 2.0))) of { Nothing -> 0.0; Just x -> x; };",
    );
    assert_eq!(out_float(&prog), 3.0);
}

#[test]
fn monad_bind_list_concat() {
    let prog = run(r"main = length (bind [1.0, 2.0] (\x -> [x, x]));");
    assert_eq!(out_int(&prog), 4);
}

#[test]
fn monoid_mappend_float_defaults_today_OD33() {
    // Same OD-33 divergence. FLIP AT TASK 10b: must become `compile_err`.
    // (The list sibling IS legal in both — `[1.0, 2.0]` fixes the element type.)
    let prog = run("main = mappend 1.5 2.5;");
    assert_eq!(out_float(&prog), 4.0);
}

#[test]
fn monoid_mappend_list() {
    let prog = run("main = length (mappend [1.0] [2.0, 3.0]);");
    assert_eq!(out_int(&prog), 3);
}

#[test]
fn monoid_mempty_right_identity() {
    // mempty is nullary: the selector argument's type picks the instance.
    let prog = run("main = length (mappend [1.0, 2.0] mempty);");
    assert_eq!(out_int(&prog), 2);
}

#[test]
fn monoid_mempty_left_identity() {
    let prog = run("main = length (mappend mempty [1.0, 2.0]);");
    assert_eq!(out_int(&prog), 2);
}

/// DIVERGENCE OD-33 — FIXED BY Task 10b, not "accepted behaviour".
///
/// `docs/superpowers/DOCS-GAPS.md` OD-33 records the defect and its owner: the
/// ambiguity must be decided where the DICTIONARY is (dictionary design §3.7
/// step 3, Task 10b), because once a constraint IS a parameter the variable is
/// either supplied by a dictionary argument or genuinely ambiguous — one place
/// sees both. Five attempts to force it earlier each broke a legal program.
///
/// The assertions below therefore state what the compiler does TODAY, so the
/// suite is honest; when Task 10b lands they must FLIP to expecting
/// `Ambiguous type variable`. The `// FLIP AT TASK 10b` marker is the reminder.
///
/// GHC 9.10.3 rejects `mappend mempty 3.5` with
/// `Ambiguous type variable 'a0' arising from a use of 'mappend'`, because
/// §4.3.4's default list is consulted only for the Report's STANDARD classes and
/// `Monoid` is not one. rill currently defaults the variable to `Double` (it has
/// `instance Monoid Double`) and answers `3.5`.
///
/// The divergence is recorded rather than fixed: making the call ambiguous
/// requires the residual store to carry a CONSTRRAINED-DEFINITION call, and
/// three attempts at that broke seven legal programs
/// (`runst_forces_a_thunk_argument`, `method_values::…`, `typeclass.rs`'s
/// routed-arity and recursion cases). The proper home for it is Task 10's single
/// value-site, where an ambiguous variable is reported once, from one place.
#[test]
fn monoid_mempty_float_defaults_today_OD33() {
    // `mappend mempty 3.5` leaves ONE variable constrained by BOTH `Monoid` and
    // `Fractional`. Haskell 2010 §4.3.4's default list `(Integer, Double)` is
    // consulted only for a variable whose classes are the Report's STANDARD set,
    // and `Monoid` is not in it — so `Double` is never chosen, even though
    // `instance Monoid Double` exists here. GHC 9.10.3 measures exactly this:
    //
    //   main = print (mappend mempty 3.5)
    //     → Ambiguous type variable 'a0' arising from a use of 'mappend'
    //
    // (The sibling `mappend mempty [1.0, 2.0]` IS legal: the list literal fixes
    // the element type, so nothing is ambiguous.) This test used to assert a
    // silent `3.5`, which is the default the Report does not allow.
    // FLIP AT TASK 10b: must become `compile_err` containing
    // "Ambiguous type variable" once a constraint is a dictionary parameter.
    let prog = run("main = mappend mempty 3.5;");
    assert_eq!(out_float(&prog), 3.5);
}

#[test]
fn applicative_pure_auto_derived_from_monad() {
    // No explicit `Applicative Maybe` instance exists — it is derived from
    // `instance Monad Maybe`. `pure` is result-directed: the expected type
    // comes from `bind`'s second argument signature (a -> m b).
    let src = r"
        main = case (bind (Just 1.0) (\x -> pure x)) of { Nothing -> 0.0; Just z -> z; };
    ";
    let prog = run(src);
    assert_eq!(out_float(&prog), 1.0);
}

#[test]
fn functor_fmap_auto_derived_from_monad() {
    // `Functor Maybe` is explicit in the prelude, but `fmap` for List is also
    // explicit; verify a Monad-derived Functor path works for a user check.
    let prog =
        run(r"main = case (fmap (\x -> x * 10.0) (Just 2.0)) of { Nothing -> 0.0; Just x -> x; };");
    assert_eq!(out_float(&prog), 20.0);
}

#[test]
fn explicit_instance_wins_over_derived() {
    // The prelude's explicit `instance Functor List` (fmap = map) must win over
    // any derived Functor — verify the list fmap still works.
    let prog = run(r"main = length (fmap (\x -> x) [1.0, 2.0, 3.0]);");
    assert_eq!(out_int(&prog), 3);
}

#[test]
fn do_notation_binds_and_returns() {
    let src = r"
        mx = Just 1.0;
        my = Just 2.0;
        main = case (do { x <- mx; y <- my; pure (x + y); }) of { Nothing -> 0.0; Just z -> z; };
    ";
    let prog = run(src);
    assert_eq!(out_float(&prog), 3.0);
}

#[test]
fn do_notation_let_and_bare_statement() {
    let src = r"
        mx = Just 1.0;
        main = case (do { x <- mx; let y = 2.0; Just 0.0; pure (x + y); }) of { Nothing -> 0.0; Just z -> z; };
    ";
    let prog = run(src);
    assert_eq!(out_float(&prog), 3.0);
}
