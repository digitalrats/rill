//! SP-2: Kleisli + value-track Arrow. HKT data fields, channel tuples, arrow.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

fn run(src: &str) -> rill_lang::program::RillProgram<f32, 256> {
    compile::<f32>(src).unwrap()
}

fn out_float(prog: &rill_lang::program::RillProgram<f32, 256>, i: usize) -> f64 {
    match prog.arena().get(prog.value_outputs()[i].unwrap()).unwrap() {
        rill_lang::arena::Value::Float(n) => *n,
        other => panic!("expected Float, got {other:?}"),
    }
}

#[test]
fn value_tuple_is_pair() {
    // `(1.0, 2.0)` in value position is a `Pair { first, second }`.
    let mut prog = run("main = (1.0, 2.0);");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    match prog.arena().get(prog.value_outputs()[0].unwrap()).unwrap() {
        rill_lang::arena::Value::Record(fields) => {
            assert_eq!(fields.len(), 2);
            assert_eq!(
                prog.arena().get(fields[0]).unwrap(),
                &rill_lang::arena::Value::Float(1.0)
            );
            assert_eq!(
                prog.arena().get(fields[1]).unwrap(),
                &rill_lang::arena::Value::Float(2.0)
            );
        }
        other => panic!("expected Pair Record, got {other:?}"),
    }
}

#[test]
fn value_tuple_field_projection_is_first() {
    // `.first` on a value tuple forces the Pair into a value-consuming
    // position: `(1.0, 2.0).first` yields `Float(1.0)`.
    let mut prog = run("main = (1.0, 2.0).first;");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 1.0);
}

#[test]
fn mixed_value_signal_tuple_is_error() {
    // `value , signal` in a tuple cannot share one track — the mixed case is a
    // compile error, not a silent fallback to the signal parallel composition.
    assert!(compile::<f32>("main = 1.0 , _;").is_err());
}

#[test]
fn tuple_type_desugars_to_pair() {
    // `(Float, Float)` in type position is sugar for `Pair Float Float`.
    // Compilation alone proves the typeclass signature parses and resolves —
    // `main` is a trivial 1.0.
    let src = r#"
        typeclass T a where { m: a (Float, Float) -> Float; }
        main = 1.0;
    "#;
    let mut prog = run(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
}

#[test]
fn constraint_instance_container_arg_discharges() {
    // The constructor-class resolution path: `unwrap (Kleisli {..})` binds the
    // container argument `a b c` against a CONCRETE `Kleisli Maybe Float Float`,
    // so the head arg `m := Maybe` is concrete when the `Monad m` constraint
    // discharges (strictly, by instance lookup), and the class pattern matches
    // the container args AFTER the bound head (`[Float, Float]`).
    let src = r#"
        typeclass Unwrap a where { unwrap: a b c -> c; }
        data Kleisli m a b = { unKleisli: a -> m b };
        instance (Monad m) => Unwrap (Kleisli m) where {
            unwrap k = k.unKleisli;
        }
        main = match ((unwrap (Kleisli { unKleisli: fn x -> Just x })) 3.0)
                of { Just v => v; Nothing => 0.0; };
    "#;
    let mut prog = run(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 3.0);
}

#[test]
fn field_projection_applies_as_function() {
    // `(b.f) 3.0` applies the projected closure.
    let src = r#"
        data Box = { f: Float -> Float };
        b = Box { f: fn x -> x * 2.0 };
        main = (b.f) 3.0;
    "#;
    let mut prog = run(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 6.0);
}

#[test]
fn let_bound_projected_closure_applies() {
    // `let u = b.f in u x`: a nullary func-value def whose body is a field
    // projection (not a `Ref`) must not drop the application during reduce.
    let src = r#"
        data Box = { f: Float -> Float };
        b = Box { f: fn x -> x * 2.0 };
        apply x = let u = b.f in u x;
        main = apply 3.0;
    "#;
    let mut prog = run(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 6.0);
}

#[test]
fn bare_field_projection_applies_as_function() {
    // `b.f 3.0` — a bare field projection applied as the callee (the shape
    // Task 9's Arrow instance body writes: `bind (k.unKleisli p.first) …`).
    let src = r#"
        data Box = { f: Float -> Float };
        b = Box { f: fn x -> x * 2.0 };
        main = b.f 3.0;
    "#;
    let mut prog = run(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 6.0);
}

#[test]
fn hkt_data_field_applies_type_parameter() {
    // `m b` in `data K m a b = { f: a -> m b }`: the type parameter m is
    // applied as a constructor. Unify m := Maybe, a := Float, b := Float.
    //
    // The field projection `k.f` is passed to a first-class closure (`apply_f
    // = fn f x -> f x`) because applying a projected closure through a `let`
    // binding (`let u = k.f in u x`) is blocked by a pre-existing lowering gap
    // (reduce drops the application when inlining a nullary func-value def
    // whose body is not a `Ref`). This test verifies the TYPE behavior of
    // TyConApp: without it, constructing `K { f: fn x -> Just x }` fails to
    // unify `m b` with `Maybe Float`.
    let src = r#"
        data K m a b = { f: a -> m b };
        k = K { f: fn x -> Just x };
        apply_f = fn f x -> f x;
        main = match (apply_f k.f 3.0) of { Just v => v; Nothing => 0.0; };
    "#;
    let mut prog = run(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 3.0);
}

#[test]
fn parses_constraint_instance_head() {
    // `instance (Monad m) => Arrow (Kleisli m)` — a constraint list before
    // the class name and a parenthesized partial-application head. The
    // instance may not fully RESOLVE yet (SP-2 Task 6) — this proves the
    // constraint syntax PARSES and the instance REGISTERS without a parse
    // error.
    let src = r#"
        typeclass Arrow a where { arr: (b -> c) -> a b c; }
        instance (Monad m) => Arrow (Kleisli m) where { arr f = f; }
        main = 1.0;
    "#;
    let res = compile::<f32>(src);
    assert!(res.is_ok() || matches!(res.err(), Some(rill_lang::CompileError::Type { .. })));
}

#[test]
fn apply_expr_non_function_callee_is_error() {
    // `(5.0) 3.0` — a non-function callee must be a compile error.
    let res = compile::<f32>("main = (5.0) 3.0;");
    assert!(res.is_err(), "expected a compile error, got success");
}

#[test]
fn apply_expr_lambda_callee() {
    // `(fn x -> x) 1.0` applies a lambda literal directly.
    let mut prog = run("main = (fn x -> x * 2.0) 3.0;");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 6.0);
}

#[test]
fn constraint_instance_resolves() {
    // User-declared equivalents of Kleisli/Arrow/instance with a Monad
    // constraint: `arr f` must resolve `(Monad m) => Arrow (Kleisli m)`.
    // `lifted` is a top-level def so reduce β-reduces the `apply` call
    // (a def-call inside a match scrutinee is not β-reduced).
    let src = r#"
        typeclass Arrow a where { arr: (b -> c) -> a b c; }
        data Kleisli m a b = { unKleisli: a -> m b };
        instance (Monad m) => Arrow (Kleisli m) where {
            arr f = Kleisli { unKleisli: fn x -> Just (f x) };
        }
        apply k x = k.unKleisli x;
        lifted = apply (arr (fn x -> x * 2.0)) 3.0;
        main = match lifted of { Just v => v; Nothing => 0.0; };
    "#;
    let mut prog = run(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(out_float(&prog, 0), 6.0);
}

#[test]
fn single_field_record_newtype_style() {
    // `Box (fn x -> x * 2.0)` constructs a single-field record from the field
    // VALUE — newtype-style, equivalent to `Box { f: fn x -> x * 2.0 }`.
    let src = r#"
        data Box = { f: Float -> Float };
        b = Box (fn x -> x * 2.0);
        main = 1.0;
    "#;
    let mut prog = run(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
}
