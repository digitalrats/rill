//! Head-arg-aware instance matching: `C Int`, `C (Pair Int Int)` and
//! `C (Pair Int (Pair Int Int))` coexist as distinct instances and dispatch by
//! the FULL applied type, not by the selector's head constructor.
//!
//! The probe class `C` is a throwaway stand-in for `Ix` (the real class lands
//! with the array surface): its method takes a `Pair`-shaped index, so the
//! class variable is only recoverable by unifying the method's parameter
//! pattern (`Pair a a`) against the concrete argument type.

use rill_lang::ast::Expr;
use rill_lang::mode::Mode;
use rill_lang::types::infer::infer_program;
use rill_lang::types::ty::TypeEnv;

/// Infer `src` and return its type environment (the instance registry).
fn env_of(src: &str) -> TypeEnv {
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    infer_program(&prog, Mode::Strict).unwrap().type_env
}

/// Compile and run `src`, returning `main`'s Float value.
fn run(src: &str) -> f64 {
    let mut prog = rill_lang::compile::<f32>(src).unwrap();
    prog.run_value().unwrap();
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v) {
        Some(rill_lang::arena::Value::Float(x)) => *x,
        other => panic!("expected Float, got {other:?}"),
    }
}

/// Three instances of one probe class whose heads share a `Pair` prefix — the
/// exact collision the head-name dispatch cannot resolve.
const THREE_INDEX_INSTANCES: &str = r#"
    typeclass C a where { size: Pair a a -> Float; }
    instance C Int where { size p = 1.0; }
    instance C (Pair Int Int) where { size p = 2.0; }
    instance C (Pair Int (Pair Int Int)) where { size p = 3.0; }
"#;

#[test]
fn instance_heads_register_under_distinct_full_type_keys() {
    let src = format!("{THREE_INDEX_INSTANCES}\n main = size (0,9);");
    let env = env_of(&src);
    let by_ty = env.instances.get("C").expect("class C is registered");
    for key in ["Int", "Pair Int Int", "Pair Int (Pair Int Int)"] {
        assert!(
            by_ty.contains_key(key),
            "instance `C {key}` must register under its full applied index type; \
             keys present: {:?}",
            by_ty.keys().collect::<Vec<_>>()
        );
    }
}

#[test]
fn resolve_method_uses_the_full_applied_type_as_key() {
    // The registry lookup is by the FULL type: the nested instance's body is
    // reachable even though its head constructor (`Pair`) is shared with the
    // 2-D instance.
    let src = format!("{THREE_INDEX_INSTANCES}\n main = size (0,9);");
    let env = env_of(&src);
    let (_, _, body) = env
        .resolve_method("size", "Pair Int (Pair Int Int)")
        .expect("nested instance resolves by its full type");
    assert!(
        matches!(&body, Expr::Float(v, _) if *v == 3.0),
        "expected the 3-D instance body, got {body:?}"
    );
    let (_, _, body) = env
        .resolve_method("size", "Pair Int Int")
        .expect("2-D instance resolves by its full type");
    assert!(
        matches!(&body, Expr::Float(v, _) if *v == 2.0),
        "expected the 2-D instance body, got {body:?}"
    );
}

#[test]
fn dispatch_solves_class_var_from_pair_shaped_index() {
    // `size` takes one `Pair a a` selector. The class variable is not the
    // selector's head (`Pair`) — it is the type UNIFIED into `a`:
    //   (0,9)                  :: Pair Int Int                → a := Int
    //   ((0,0),(2,2))          :: Pair (Pair Int Int) (Pair Int Int)
    //                                                          → a := Pair Int Int
    let one_d = format!("{THREE_INDEX_INSTANCES}\n main = size (0,9);");
    assert_eq!(run(&one_d), 1.0, "`C Int` must take `size (0,9)`");

    let two_d = format!("{THREE_INDEX_INSTANCES}\n main = size ((0,0),(2,2));");
    assert_eq!(
        run(&two_d),
        2.0,
        "`C (Pair Int Int)` must take `size ((0,0),(2,2))`"
    );

    let three_d = format!("{THREE_INDEX_INSTANCES}\n main = size ((0,(0,0)),(9,(9,9)));");
    assert_eq!(
        run(&three_d),
        3.0,
        "`C (Pair Int (Pair Int Int))` must take the nested index"
    );
}

#[test]
fn dispatch_nullary_method_ref_with_applied_head() {
    // A bare reference to a result-directed method (`empty: a`) carries no
    // argument to select on: the expected result type does. For an applied-head
    // instance that type is `Pair Int Int`, so lowering must key by the FULL
    // type — keying by the head name alone (`Pair`) names no instance.
    //
    // The record field pins the expected type (a plain user signature of a
    // record type reaches lowering as a bare `Pair`, and an unused parameter is
    // never lowered, so both would hide the dispatch).
    let src = r#"
        typeclass C a where { empty: a; }
        instance C (Pair Int Int) where { empty = (0,9); }
        data Holder = { h: Pair Int Int };
        sum2 p = p.first + p.second;
        main = sum2 (Holder { h: empty }).h;
    "#;
    assert_eq!(run(src), 9.0, "`empty` must resolve `C (Pair Int Int)`");
}

#[test]
fn dispatch_selects_the_class_var_argument_when_it_is_not_first() {
    // `pick`'s class-var argument is the SECOND one (`Pair a a`); the `Float`
    // at 0 says nothing about the instance. Selecting on the first argument
    // would unify `Pair a a` against `Float`, fail, and fall back to the wrong
    // head — the Haskell-2010-natural `index: Array a -> i -> a` shape.
    let src = r#"
        typeclass Pick a where { pick: Float -> Pair a a -> Float; }
        instance Pick Int where { pick b p = 1.0; }
        instance Pick (Pair Int Int) where { pick b p = 2.0; }
    "#;
    assert_eq!(
        run(&format!("{src}\n main = pick 1.0 (0,9);")),
        1.0,
        "`Pair Int Int` must solve the class variable to `Int`"
    );
    assert_eq!(
        run(&format!("{src}\n main = pick 1.0 ((0,0),(2,2));")),
        2.0,
        "the nested index must select `Pick (Pair Int Int)`"
    );
}

#[test]
fn dispatch_still_uses_the_head_name_for_bare_constructor_instances() {
    // Backward compatibility: a class whose instance head is a bare constructor
    // (`Monoid List` / a probe `C2 List`) is keyed by that head name, while the
    // selector's recovered full type is applied (`List Float`). Resolution must
    // fall back to the head name.
    let src = r#"
        typeclass C2 a where { describe: a -> Float; }
        instance C2 Float where { describe x = 1.0; }
        instance C2 List where { describe xs = 2.0; }
        main = describe [1.0, 2.0] + describe 3.0;
    "#;
    assert_eq!(run(src), 3.0);
}
