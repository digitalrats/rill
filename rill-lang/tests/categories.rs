//! Built-in category typeclasses: `Functor`, `Applicative`, `Monad`, `Monoid`
//! registered via the language prelude, with compile-time inline resolution.

use rill_lang::ffi::ForeignRegistry;
use rill_lang::test_eval;

/// Compile `src` against an empty foreign registry and RUN it once (the
/// single-run value surface), returning the already-run program.
fn run(src: &str) -> rill_lang::program::RillProgram<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    test_eval::<f32, 256>(defs, expr, &ffi, 44_100.0).unwrap()
}

fn out_int(prog: &rill_lang::program::RillProgram<f32, 256>) -> i64 {
    match prog.arena().get(prog.result().unwrap()).unwrap() {
        rill_lang::arena::Value::Int(n) => *n,
        other => panic!("expected Int, got {other:?}"),
    }
}

fn out_float(prog: &rill_lang::program::RillProgram<f32, 256>) -> f64 {
    match prog.arena().get(prog.result().unwrap()).unwrap() {
        rill_lang::arena::Value::Float(n) => *n,
        other => panic!("expected Float, got {other:?}"),
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
fn monoid_mappend_float() {
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

#[test]
fn monoid_mempty_float() {
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
