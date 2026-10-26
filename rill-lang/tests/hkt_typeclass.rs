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
        main = 1.0;
    "#;
    let res = compile_eval::<f32, 256>(src, "main", Mode::Strict);
    let msg = format!(
        "{}",
        res.err()
            .expect("a parameterized user sum instance must be rejected")
    );
    assert!(
        msg.contains("is not a type constructor") && msg.contains("`Functor` expects kind"),
        "expected the kind diagnostic for an unrecorded constructor, got: {msg}"
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
