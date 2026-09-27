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
    // The dead `instance Functor Pair` must be rejected by the KIND CHECK in
    // validate_instances: Pair is arity 2, Functor needs arity 1. The old form
    // called `fmap` from `main`, which errored at the CALL SITE ("argument type
    // does not apply f") — the test passed for the wrong reason and would pass
    // even if the kind check were deleted.
    let src = r#"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        instance Functor Pair where { fmap g p = p; }
        main = _;
    "#;
    let res = compile::<f32>(src);
    assert!(res.is_err(), "Pair has arity 2; Functor needs arity 1");
    if let Some(msg) = res.err().map(|e| format!("{e:?}")) {
        assert!(
            msg.contains("arity") || msg.contains("Functor"),
            "expected a kind-arity message, got: {msg}"
        );
    }
}

#[test]
fn parameterized_user_sum_instance_is_kind_error() {
    // User parameterized sums are NOT registered in `data_arities` (only user
    // RECORDS are): the sum's match pin / ctor-construction paths stay
    // monomorphic `Data(name, [])`, so a parameterized sum as a typeclass
    // instance would hit the confusing "cannot unify Data(\"Opt\", [Var(_)])
    // with Data(\"Opt\", [])" error. v1 rejects it up front with a clean
    // kind/arity message instead. Sums still work as ordinary data types
    // (construction + match); they just cannot be instances.
    let src = r#"
        typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
        data Opt a = Some a | None;
        instance Functor Opt where { fmap g o = o; }
        main = _;
    "#;
    let res = compile::<f32>(src);
    assert!(
        res.is_err(),
        "parameterized user sum Opt must be rejected as an instance"
    );
    if let Some(msg) = res.err().map(|e| format!("{e:?}")) {
        assert!(
            msg.contains("arity") || msg.contains("type constructor"),
            "expected a kind-arity message, got: {msg}"
        );
    }
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
