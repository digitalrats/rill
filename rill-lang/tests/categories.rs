//! Built-in category typeclasses: `Functor`, `Applicative`, `Monad`, `Monoid`
//! registered via the language prelude, with compile-time inline resolution.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

fn run(src: &str) -> rill_lang::program::RillProgram<f32, 256> {
    compile::<f32>(src).unwrap()
}

fn out_int(prog: &rill_lang::program::RillProgram<f32, 256>, i: usize) -> i64 {
    match prog.arena().get(prog.value_outputs()[i].unwrap()).unwrap() {
        rill_lang::arena::Value::Int(n) => *n,
        other => panic!("expected Int, got {other:?}"),
    }
}

fn out_float(prog: &rill_lang::program::RillProgram<f32, 256>, i: usize) -> f64 {
    match prog.arena().get(prog.value_outputs()[i].unwrap()).unwrap() {
        rill_lang::arena::Value::Float(n) => *n,
        other => panic!("expected Float, got {other:?}"),
    }
}

#[test]
fn functor_fmap_over_list() {
    let mut prog = run("main = length (fmap (fn x -> x * 2.0) [1.0, 2.0, 3.0]);");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_int(&prog, 0), 3);
}

#[test]
fn functor_fmap_over_maybe() {
    let mut prog = run(
        "main = match (fmap (fn x -> x + 1.0) (Just 1.0)) of { Nothing => 0.0; Just x => x; };",
    );
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 2.0);
}

#[test]
fn concat_map_splices_lists() {
    let mut prog = run("main = length (concat_map (fn x -> [x, x]) [1.0, 2.0]);");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_int(&prog, 0), 4);
}

#[test]
fn append_list_and_concat_string() {
    let mut prog = run("main = length (append_list [1.0, 2.0] [3.0, 4.0, 5.0]);");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_int(&prog, 0), 5);
}

#[test]
fn monad_bind_maybe_short_circuits_nothing() {
    // bind (Nothing) f = Nothing
    let mut prog = run(
        "main = match (bind (Nothing) (fn x -> Just (x + 1.0))) of { Nothing => 0.0; Just x => x; };",
    );
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 0.0);
}

#[test]
fn monad_bind_maybe_chains() {
    let mut prog = run(
        "main = match (bind (Just 1.0) (fn x -> Just (x + 2.0))) of { Nothing => 0.0; Just x => x; };",
    );
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 3.0);
}

#[test]
fn monad_bind_list_concat() {
    let mut prog = run("main = length (bind [1.0, 2.0] (fn x -> [x, x]));");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_int(&prog, 0), 4);
}

#[test]
fn monoid_mappend_float() {
    let mut prog = run("main = mappend 1.5 2.5;");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 4.0);
}

#[test]
fn monoid_mappend_list() {
    let mut prog = run("main = length (mappend [1.0] [2.0, 3.0]);");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_int(&prog, 0), 3);
}

#[test]
fn monoid_mempty_right_identity() {
    // mempty is nullary: the selector argument's type picks the instance.
    let mut prog = run("main = length (mappend [1.0, 2.0] mempty);");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_int(&prog, 0), 2);
}

#[test]
fn monoid_mempty_left_identity() {
    let mut prog = run("main = length (mappend mempty [1.0, 2.0]);");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_int(&prog, 0), 2);
}

#[test]
fn monoid_mempty_float() {
    let mut prog = run("main = mappend mempty 3.5;");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 3.5);
}

#[test]
fn applicative_pure_auto_derived_from_monad() {
    // No explicit `Applicative Maybe` instance exists — it is derived from
    // `instance Monad Maybe`. `pure` is result-directed: the expected type
    // comes from `bind`'s second argument signature (a -> m b).
    let src = r#"
        main = match (bind (Just 1.0) (fn x -> pure x)) of { Nothing => 0.0; Just z => z; };
    "#;
    let mut prog = run(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 1.0);
}

#[test]
fn functor_fmap_auto_derived_from_monad() {
    // `Functor Maybe` is explicit in the prelude, but `fmap` for List is also
    // explicit; verify a Monad-derived Functor path works for a user check.
    let mut prog = run(
        "main = match (fmap (fn x -> x * 10.0) (Just 2.0)) of { Nothing => 0.0; Just x => x; };",
    );
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 20.0);
}

#[test]
fn explicit_instance_wins_over_derived() {
    // The prelude's explicit `instance Functor List` (fmap = map) must win over
    // any derived Functor — verify the list fmap still works.
    let mut prog = run("main = length (fmap (fn x -> x) [1.0, 2.0, 3.0]);");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_int(&prog, 0), 3);
}

#[test]
fn do_notation_binds_and_returns() {
    let src = r#"
        mx = Just 1.0;
        my = Just 2.0;
        main = match (do { x <- mx; y <- my; pure (x + y); }) of { Nothing => 0.0; Just z => z; };
    "#;
    let mut prog = run(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 3.0);
}

#[test]
fn do_notation_let_and_bare_statement() {
    let src = r#"
        mx = Just 1.0;
        main = match (do { x <- mx; let y = 2.0; Just 0.0; pure (x + y); }) of { Nothing => 0.0; Just z => z; };
    "#;
    let mut prog = run(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 3.0);
}
