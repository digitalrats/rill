//! Kind polymorphism: typeclasses over type constructors (Functor/Foldable).

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn fmap_over_maybe_matches() {
    let src = r#"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor Maybe where { fmap g m = match m of { Nothing => Nothing; Just x => Just (g x); }; }
        main = match (fmap (fn x -> x + 1.0) (Just 1.0)) of { Nothing => 0.0; Just x => x; };
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(2.0)
    );
}

#[test]
fn fmap_over_list_is_compile_time_inlined() {
    let src = r#"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor List where { fmap g xs = map g xs; }
        main = length (fmap (fn x -> x * 2.0) [1.0, 2.0, 3.0, 4.0]);
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(4)
    );
}

#[test]
fn kind_mismatch_is_compile_error() {
    let src = r#"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor Pair where { fmap g p = p; }
        main = fmap (fn x -> x) (Pair { first: 1.0, second: 2.0 });
    "#;
    let res = compile::<f32>(src);
    assert!(res.is_err(), "Pair has arity 2; Functor needs arity 1");
}

#[test]
fn foldable_over_list() {
    let src = r#"
        typeclass Foldable f where { foldr: (a -> b -> b) -> b -> f a -> b; }
        instance Foldable List where { foldr f z xs = fold f z xs; }
        main = foldr (fn a b -> a + b) 0.0 [1.0, 2.0, 3.0];
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(6.0)
    );
}

#[test]
fn fmap_over_user_parameterized_data() {
    let src = r#"
        data Box a = { value: a };
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor Box where { fmap g b = Box { value: g b.value }; }
        main = (fmap (fn x -> x * 10.0) (Box { value: 3.0 })).value;
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(30.0)
    );
}
