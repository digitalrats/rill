//! Value-track Bool: literals, comparisons, logic, `not`, and predicates.

use rill_lang::ffi::ForeignRegistry;
use rill_lang::test_eval;

#[test]
fn not_negates_bool() {
    let ffi = ForeignRegistry::<f32>::new();
    let prog = test_eval::<f32, 256>("", "not true", &ffi, 44_100.0).unwrap();
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(false)
    );
}

#[test]
fn not_rejects_non_bool_at_compile_time() {
    let ffi = ForeignRegistry::<f32>::new();
    assert!(test_eval::<f32, 256>("", "not 1.0", &ffi, 44_100.0).is_err());
    assert!(test_eval::<f32, 256>("", "not true false", &ffi, 44_100.0).is_err());
}

#[test]
fn comparisons_and_logic() {
    let ffi = ForeignRegistry::<f32>::new();
    let prog = test_eval::<f32, 256>("", "1.0 < 2.0 && 3.0 > 1.0", &ffi, 44_100.0).unwrap();
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(true)
    );
}

#[test]
fn not_combines_with_member() {
    let ffi = ForeignRegistry::<f32>::new();
    let prog = test_eval::<f32, 256>(
        "s = insert 5 (empty_set);",
        "not (member 9 s)",
        &ffi,
        44_100.0,
    )
    .unwrap();
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(true)
    );
}
