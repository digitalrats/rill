//! SP-2: Kleisli + value-track Arrow. HKT data fields, channel tuples, arrow.
pub mod common;

use common::*;
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;

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

fn out_float(prog: &WorldState<f32, 256>) -> f64 {
    match prog.arena().get(prog.result().unwrap()).unwrap() {
        rill_lang::arena::Value::Double(n) => *n,
        other => panic!("expected Double, got {other:?}"),
    }
}

/// Compile `src` on a thread with a generous stack and report whether it
/// compiled.
///
/// The deepest Arrow default chains (`fan` → `both` → `second`, all re-entering
/// `compose`'s default body) inline instance bodies recursively at LOWERING, so
/// the DEBUG-build stack frames of that recursion need more than the test
/// harness's default thread stack (~2 MiB). The requirement is also
/// codegen-sensitive: unrelated code added elsewhere in the crate can push the
/// chain over the limit. The bar these tests assert is COMPILE success, so give
/// the recursion room instead of letting the stack size decide.
///
/// Keep the bound GENEROUS. A stack overflow on a scoped thread is not a
/// catchable panic — it ABORTS the whole test binary (every other test in
/// `tests/arrow.rs` is lost and the failure reads as a signal, not a diagnostic).
fn compiles_with_deep_stack(src: &str) -> bool {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn_scoped(scope, || {
                compile_eval::<f32, 256>(src, "main", Mode::Strict).is_ok()
            })
            .expect("the compile thread must spawn")
            .join()
            .expect("the compile thread must not panic")
    })
}

#[test]
fn value_tuple_is_pair() {
    // `(1.0, 2.0)` in value position is a `Pair { first, second }`.
    let prog = run("main = (1.0, 2.0);");
    match prog.arena().get(prog.result().unwrap()).unwrap() {
        rill_lang::arena::Value::Record(fields) => {
            assert_eq!(fields.len(), 2);
            assert_eq!(
                prog.arena().get(fields[0]).unwrap(),
                &rill_lang::arena::Value::Double(1.0)
            );
            assert_eq!(
                prog.arena().get(fields[1]).unwrap(),
                &rill_lang::arena::Value::Double(2.0)
            );
        }
        other => panic!("expected Pair Record, got {other:?}"),
    }
}

#[test]
fn value_tuple_field_projection_is_first() {
    // `.first` on a value tuple forces the Pair into a value-consuming
    // position: `(1.0, 2.0).first` yields `Double(1.0)`.
    let prog = run("main = (1.0, 2.0).first;");
    assert_eq!(out_float(&prog), 1.0);
}

#[test]
fn mixed_value_signal_tuple_is_error() {
    // `value , signal` in a tuple cannot share one track — the mixed case is a
    // compile error, not a silent fallback to the signal parallel composition.
    assert!(compile_eval::<f32, 256>("main = 1.0 , _;", "main", Mode::Strict).is_err());
}

#[test]
fn tuple_type_desugars_to_pair() {
    // `(Double, Double)` in type position is sugar for `Pair Double Double`.
    // Compilation alone proves the typeclass signature parses and resolves —
    // `main` is a trivial 1.0.
    let src = r#"
        typeclass T a where { m: a (Double, Double) -> Double; }
        main = 1.0;
    "#;
    let _prog = run(src);
}

#[test]
fn constraint_instance_container_arg_discharges() {
    // The constructor-class resolution path: `unwrap (Kleisli {..})` binds the
    // container argument `a b c` against a CONCRETE `Kleisli Maybe Double Double`,
    // so the head arg `m := Maybe` is concrete when the `Monad m` constraint
    // discharges (strictly, by instance lookup), and the class pattern matches
    // the container args AFTER the bound head (`[Double, Double]`).
    let src = r"
        typeclass Unwrap a where { unwrap: a b c -> c; }
        data Kleisli m a b = { unKleisli: a -> m b };
        instance (Monad m) => Unwrap (Kleisli m) where {
            unwrap k = k.unKleisli;
        }
        main = case ((unwrap (Kleisli { unKleisli: \x -> Just x })) 3.0)
                of { Just v -> v; Nothing -> 0.0; };
    ";
    let prog = run(src);
    assert_eq!(out_float(&prog), 3.0);
}

#[test]
fn field_projection_applies_as_function() {
    // `(b.f) 3.0` applies the projected closure.
    let src = r"
        data Box = { f: Double -> Double };
        b = Box { f: \x -> x * 2.0 };
        main = (b.f) 3.0;
    ";
    let prog = run(src);
    assert_eq!(out_float(&prog), 6.0);
}

#[test]
fn let_bound_projected_closure_applies() {
    // `let u = b.f in u x`: a nullary func-value def whose body is a field
    // projection (not a `Ref`) must not drop the application during reduce.
    let src = r"
        data Box = { f: Double -> Double };
        b = Box { f: \x -> x * 2.0 };
        apply x = let u = b.f in u x;
        main = apply 3.0;
    ";
    let prog = run(src);
    assert_eq!(out_float(&prog), 6.0);
}

#[test]
fn bare_field_projection_applies_as_function() {
    // `b.f 3.0` — a bare field projection applied as the callee (the shape
    // Task 9's Arrow instance body writes: `bind (k.unKleisli p.first) …`).
    let src = r"
        data Box = { f: Double -> Double };
        b = Box { f: \x -> x * 2.0 };
        main = b.f 3.0;
    ";
    let prog = run(src);
    assert_eq!(out_float(&prog), 6.0);
}

#[test]
fn hkt_data_field_applies_type_parameter() {
    // `m b` in `data K m a b = { f: a -> m b }`: the type parameter m is
    // applied as a constructor. Unify m := Maybe, a := Double, b := Double.
    //
    // The field projection `k.f` is passed to a first-class closure (`apply_f
    // = fn f x -> f x`) because applying a projected closure through a `let`
    // binding (`let u = k.f in u x`) is blocked by a pre-existing lowering gap
    // (reduce drops the application when inlining a nullary func-value def
    // whose body is not a `Ref`). This test verifies the TYPE behavior of
    // TyConApp: without it, constructing `K { f: fn x -> Just x }` fails to
    // unify `m b` with `Maybe Double`.
    let src = r"
        data K m a b = { f: a -> m b };
        k = K { f: \x -> Just x };
        apply_f = \f -> \x -> f x;
        main = case (apply_f k.f 3.0) of { Just v -> v; Nothing -> 0.0; };
    ";
    let prog = run(src);
    assert_eq!(out_float(&prog), 3.0);
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
    let res = compile_eval::<f32, 256>(src, "main", Mode::Strict);
    assert!(res.is_ok() || matches!(res.err(), Some(rill_lang::CompileError::Type { .. })));
}

#[test]
fn apply_expr_non_function_callee_is_error() {
    // `(5.0) 3.0` — a non-function callee must be a compile error.
    let res = compile_eval::<f32, 256>("main = (5.0) 3.0;", "main", Mode::Strict);
    assert!(res.is_err(), "expected a compile error, got success");
}

#[test]
fn apply_expr_lambda_callee() {
    // `(fn x -> x) 1.0` applies a lambda literal directly.
    let prog = run(r"main = (\x -> x * 2.0) 3.0;");
    assert_eq!(out_float(&prog), 6.0);
}

#[test]
fn constraint_instance_resolves() {
    // User-declared equivalents of Kleisli/Arrow/instance with a Monad
    // constraint: `arr f` must resolve `(Monad m) => Arrow (Kleisli m)`.
    // `lifted` is a top-level def so reduce β-reduces the `apply` call
    // (a def-call inside a match scrutinee is not β-reduced).
    let src = r"
        typeclass Arrow a where { arr: (b -> c) -> a b c; }
        data Kleisli m a b = { unKleisli: a -> m b };
        instance (Monad m) => Arrow (Kleisli m) where {
            arr f = Kleisli { unKleisli: \x -> Just (f x) };
        }
        apply k x = k.unKleisli x;
        lifted = apply (arr (\x -> x * 2.0)) 3.0;
        main = case lifted of { Just v -> v; Nothing -> 0.0; };
    ";
    let prog = run(src);
    assert_eq!(out_float(&prog), 6.0);
}

#[test]
fn single_field_record_newtype_style() {
    // `Box (fn x -> x * 2.0)` constructs a single-field record from the field
    // VALUE — newtype-style, equivalent to `Box { f: fn x -> x * 2.0 }`.
    let src = r"
        data Box = { f: Double -> Double };
        b = Box (\x -> x * 2.0);
        main = b.f 21.0;
    ";
    let prog = run(src);
    assert_eq!(out_float(&prog), 42.0);
}

#[test]
fn kleisli_arrow_end_to_end() {
    // The full category prelude Arrow over Kleisli, generic over the monad:
    // `compose (arr f) (arr g)` resolves the constraint instance
    // `(Monad m) => Arrow (Kleisli m)` at a CONCRETE container (`m := Maybe`),
    // and the instance bodies use `return`/`bind` (resolved via expected-type
    // threading) rather than a hardcoded `Just`.
    let src = r"
        apply k x = let u = k.unKleisli in u x;
        main = case (apply (compose (arr (\x -> x + 1.0)) (arr (\y -> y * 2.0))) 3.0) of {
            Just v -> v; Nothing -> 0.0;
        };
    ";
    let prog = run(src);
    assert_eq!(out_float(&prog), 8.0); // (3+1)*2
}

#[test]
fn arrow_default_both_composes() {
    // `both f g` default: `compose (first f) (second g)`. The method-lifting
    // composition reaches `compose` again through `second`'s default body —
    // that is NOT recursion (compose's body never calls compose). Regression:
    // the method_lifting recursion guard must only fire on BODY re-entry.
    let src = r"
        apply k x = let u = k.unKleisli in u x;
        main = case (apply (both (arr (\x -> x + 1.0)) (arr (\y -> y * 2.0))) (Pair { first: 3.0, second: 4.0 })) of {
            Just v -> v.first; Nothing -> 0.0;
        };
    ";
    // PRIMARY bar: `both` must COMPILE — before the recursion-guard fix this
    // fails with "recursive typeclass method `compose` for type `Kleisli`"
    // (the key was held while `second g`'s default body re-entered `compose`).
    //
    // NOTE: the program passes a `Pair` input and projects `.first` because
    // Arrow `both` maps a PAIR (`a (Pair b d) (Pair c e)`); the older Double
    // input only compiled by accident (a placeholder-variable leak in record
    // projections left the arrow's input type unconstrained). The compile bar
    // is the default-chain typecheck, not the scaffold.
    assert!(
        compiles_with_deep_stack(src),
        "both's default must compile (recursion guard must only fire on body re-entry)"
    );
    // NOTE: runtime execution of both/fan is deferred — arena-capacity
    // undercount for deep closure chains (the compiled program panics with
    // "value buffer pool exhausted at build time"). The PRIMARY bar is the
    // compile above; the recursion-guard false positive is a COMPILE error.
}

#[test]
fn arrow_default_fan_composes() {
    // `fan f g` default: `compose (arr dup) (both f g)` — the deepest default
    // chain in the Arrow prelude: fan → both → second, all re-entering
    // `compose` through a NESTED method's default body. None of these bodies
    // call their own method; the recursion guard must not fire.
    let src = r"
        apply k x = let u = k.unKleisli in u x;
        main = case (apply (fan (arr (\x -> x + 1.0)) (arr (\y -> y * 2.0))) 3.0) of {
            Just v -> v.first; Nothing -> 0.0;
        };
    ";
    // `fan` maps a scalar to a PAIR (`a b (Pair c d)`), so the input is a
    // Double (`3.0`) and the result projects `.first`.
    assert!(
        compiles_with_deep_stack(src),
        "fan's default must compile (recursion guard must only fire on body re-entry)"
    );
    // NOTE: runtime execution of both/fan is deferred — arena-capacity
    // undercount for deep closure chains. The PRIMARY bar is the compile above.
}
