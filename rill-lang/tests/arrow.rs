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
