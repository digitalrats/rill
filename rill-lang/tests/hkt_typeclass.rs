//! Kind polymorphism: typeclasses over type constructors (Functor/Foldable).
pub mod common;

use common::*;
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;

/// Compile `src` (which must bind `main` to the evaluated expression) and RUN
/// it once on the single-run value surface, returning the already-run program.
fn run(src: &str) -> WorldState<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    eval_ffi(defs, expr, ffi).unwrap()
}

#[test]
fn fmap_over_maybe_matches() {
    let src = r"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor Maybe where { fmap g m = case m of { Nothing -> Nothing; Just x -> Just (g x); }; }
        main = case (fmap (\x -> x + 1.0) (Just 1.0)) of { Nothing -> 0.0; Just x -> x; };
    ";
    let prog = run(src);
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(2.0)
    );
}

#[test]
fn fmap_over_list_is_compile_time_inlined() {
    let src = r"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor List where { fmap g xs = map g xs; }
        main = length (fmap (\x -> x * 2.0) [1.0, 2.0, 3.0, 4.0]);
    ";
    let prog = run(src);
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(4)
    );
}

#[test]
fn kind_mismatch_is_compile_error() {
    // The dead `instance Functor Pair` must be rejected by the KIND CHECK in
    // validate_instances: `Functor` binds a `* -> *` constructor, `Pair` is
    // `* -> * -> *`. GHC's message is a kind mismatch, so the assertion pins
    // the KIND vocabulary rather than a bare arity count (an arity-worded
    // check would satisfy the older assertion without ever naming a kind).
    let src = r#"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor Pair where { fmap g p = p; }
        main = 1.0;
    "#;
    let res = compile_eval::<f32, 256>(src, "main", Mode::Strict);
    let msg = format!("{}", res.err().expect("`Functor Pair` must be rejected"));
    assert!(
        msg.contains("`Functor` expects a type constructor of kind `* -> *`")
            && msg.contains("`Pair` has kind `* -> * -> *`"),
        "expected GHC's kind mismatch, got: {msg}"
    );
}

#[test]
fn parameterized_user_sum_instance_is_accepted() {
    // Haskell 2010 §4.1.1: `data Opt a = Some a | None` declares a type
    // constructor of kind `* -> *`, so `instance Functor Opt` is a legal
    // instance. This test used to assert the opposite — a documented v1
    // restriction that existed only because user sums were left out of the
    // arity table (their monomorphic `Data(name, [])` pins could not unify with
    // the parameterized spelling). There is one constructor application shape
    // now, the arity table records the declaration's parameters, and the
    // instance is accepted and runs.
    let src = r#"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        data Opt a = Some a | None;
        instance Functor Opt where {
            fmap g o = case o of { Some x -> Some (g x); None -> None; };
        }
        main = case fmap (\ x -> x + 1.0) (Some 1.0) of { Some y -> y; None -> 0.0; };
    "#;
    let ir = compile_eval::<f32, 256>(src, "main", Mode::Strict)
        .expect("a parameterized user sum instance is a legal Haskell 2010 program");
    let mut world = WorldState::<f32, 256>::new(rill_lang::program::WorldConfig {
        ffi: Some(rill_lang::ffi::ForeignRegistry::new()),
        ..Default::default()
    });
    rill_lang::test_eval(&ir, &mut world).unwrap();
    let r = world.result().unwrap();
    assert_eq!(
        world.arena().get(r).unwrap(),
        &rill_lang::arena::Value::Double(2.0)
    );
}

#[test]
fn foldable_over_list() {
    let src = r"
        typeclass Foldable f where { foldr: (a -> b -> b) -> b -> f a -> b; }
        instance Foldable List where { foldr f z xs = fold f z xs; }
        main = foldr (\a -> \b -> a + b) 0.0 [1.0, 2.0, 3.0];
    ";
    let prog = run(src);
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(6.0)
    );
}

#[test]
fn fmap_over_user_parameterized_data() {
    let src = r"
        data Box a = { value: a };
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor Box where { fmap g b = Box { value: g b.value }; }
        main = (fmap (\x -> x * 10.0) (Box { value: 3.0 })).value;
    ";
    let prog = run(src);
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Double(30.0)
    );
}
