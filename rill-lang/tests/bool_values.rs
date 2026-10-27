//! Value-track Bool: literals, comparisons, logic, `not`, and predicates.
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;

#[test]
fn not_negates_bool() {
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi("", "not true", ffi).unwrap();
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::bool(false)
    );
}

#[test]
fn not_rejects_non_bool_at_compile_time() {
    assert!(eval("", "not 1.0").is_err());
    assert!(eval("", "not true false").is_err());
}

#[test]
fn comparisons_and_logic() {
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi("", "1.0 < 2.0 && 3.0 > 1.0", ffi).unwrap();
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::bool(true)
    );
}

#[test]
fn not_combines_with_member() {
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi("s = insert 5 (empty_set);", "not (member 9 s)", ffi).unwrap();
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::bool(true)
    );
}
