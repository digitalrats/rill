//! Value-track Bool: literals, comparisons, logic, `not`, and predicates.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn not_negates_bool() {
    let mut prog = compile::<f32>("main = not true;").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(false)
    );
}

#[test]
fn not_rejects_non_bool_at_compile_time() {
    assert!(compile::<f32>("main = not 1.0;").is_err());
    assert!(compile::<f32>("main = not true false;").is_err());
}

#[test]
fn comparisons_and_logic() {
    let mut prog = compile::<f32>("main = 1.0 < 2.0 && 3.0 > 1.0;").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(true)
    );
}

#[test]
fn not_combines_with_member() {
    let mut prog = compile::<f32>("s = insert 5 (empty_set); main = not (member 9 s);").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(true)
    );
}
