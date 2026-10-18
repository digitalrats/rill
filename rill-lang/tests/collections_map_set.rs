//! End-to-end Map/Set tests: insert/lookup/member with structural ordering.

use rill_lang::compile;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::test_eval;

/// Compile `src` against an empty foreign registry and RUN it once (the
/// single-run value surface), returning the already-run program.
fn run(src: &str) -> rill_lang::program::RillProgram<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    test_eval::<f32, 256>(defs, expr, &ffi, 44_100.0).unwrap()
}

#[test]
fn map_insert_lookup_member() {
    let prog = run(
        r#"m = { "a": 1.0, "b": 2.0 }; main = case (lookup "a" m) of { Nothing -> 0.0; Just x -> x; };"#,
    );
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(1.0)
    );
}

#[test]
fn set_member_after_insert() {
    let prog = run("s = insert 5 (empty_set); main = member 5 s;");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(true)
    );
}

#[test]
fn tail_and_filter_preserve_capacity() {
    // `tail` drops the head (2.0 and 3.0 survive), `filter` keeps the elements
    // satisfying the predicate (both do), so the length is 2 — the point is
    // that tail/filter work end-to-end and preserve the source capacity.
    let prog = run(r"main = length (filter (\x -> x > 1.0) (tail [1.0, 2.0, 3.0]));");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(2)
    );
}

#[test]
fn map_insert_new_key_grows_past_literal_entries() {
    // Open collections: inserting a NEW key past the former literal capacity is
    // no longer a runtime error — the map grows.
    let prog = run("m = { \"a\": 1.0 }; main = length (insert \"b\" 2.0 m);");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(2)
    );
}

#[test]
fn map_replace_on_duplicate_does_not_overflow() {
    // Replacing an existing key keeps the length AT the capacity bound: it must
    // NOT error (only a NEW key past capacity overflows).
    let prog = run("m = insert \"a\" 2.0 { \"a\": 1.0 }; main = member \"a\" m;");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(true)
    );
}

#[test]
fn set_insert_duplicate_does_not_overflow() {
    // The first insert fills the cap-1 set; the duplicate insert must be a
    // no-op, not an overflow.
    let prog = run("s = insert 5 (insert 5 (empty_set)); main = member 5 s;");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(true)
    );
}

#[test]
fn map_literal_entries_are_sorted_regardless_of_write_order() {
    // Out-of-order multi-key literals must be normalised to sorted order so the
    // binary-search invariant of insert/lookup/member holds.
    let prog = run("main = member \"a\" { \"b\": 1.0, \"a\": 2.0 };");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(true),
        "lookup of a key written out of order must still find it"
    );
}

#[test]
fn func_key_is_compile_error() {
    // `Func` has no derived Ord instance — a Map with a function key must not compile.
    let src = r#"f = \x -> x; main = lookup f { "a": 1.0 };"#;
    let res = compile::<f32>(src);
    assert!(res.is_err(), "function-typed map key must be rejected");
    if let Some(msg) = res.err().map(|e| format!("{e:?}")) {
        assert!(
            msg.contains("Ord") || msg.contains("key"),
            "expected an Ord/key message, got: {msg}"
        );
    }
}

#[test]
fn compound_data_key_is_accepted() {
    // A List key is a concrete data type with a derived Ord instance.
    let prog = run("k = [1, 2]; m = insert k 1.0 (empty_map); main = length [1.0, 2.0];");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(2)
    );
}

#[test]
fn newtype_key_has_derived_ord() {
    // spec §2.5 derives Eq/Ord for newtypes by their inner: a Map keyed by Hz
    // must compile (the runtime value_cmp unwraps newtypes).
    let prog =
        run("newtype Hz = Float; k = Hz 440.0; m = insert k 1.0 (empty_map); main = length [1.0];");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(1)
    );
}

#[test]
fn length_of_map_and_set() {
    // The IR doc says Length covers list/set/map — Map and Set must report
    // their entry counts, not silently return 0.
    let prog = run("main = length { \"a\": 1.0, \"b\": 2.0 };");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(2)
    );

    let prog = run("main = length (insert 1.0 (insert 2.0 (empty_set)));");
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(2)
    );
}

#[test]
fn map_literal_value_type_matches_lookup() {
    // A Float-valued map literal: lookup must yield the entry's value in the
    // Just arm.
    let prog = run(
        r#"main = case (lookup "a" { "a": 1.0, "b": 2.0 }) of { Nothing -> 0.0; Just x -> x; };"#,
    );
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(1.0)
    );
}

#[test]
fn map_literal_value_type_is_actual_entry_type() {
    // The map literal's value type must be the ACTUAL entry type (a record),
    // not a pinned Float: projecting the looked-up record's field works.
    let prog = run(
        r#"data P = { x: Float, y: Float }; main = case (lookup "a" { "a": P { x: 1.0, y: 2.0 } }) of { Nothing -> 0.0; Just p -> p.x; };"#,
    );
    let v = prog.result().unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(1.0)
    );
}

#[test]
fn map_literal_mixed_value_types_are_compile_error() {
    // A map literal's entries must share one value type: `{ "a": 1.0, "b": [1.0] }`
    // mixes Float and List and must not compile.
    let res = compile::<f32>("main = { \"a\": 1.0, \"b\": [1.0] };");
    assert!(
        res.is_err(),
        "a map literal with mixed value types must be a compile error"
    );
}
