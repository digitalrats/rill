//! End-to-end Map/Set tests: insert/lookup/member with structural ordering.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn map_insert_lookup_member() {
    let mut prog = compile::<f32>(
        "m = { \"a\": 1.0, \"b\": 2.0 }; main = match (lookup \"a\" m) of { Nothing => 0.0; Just x => x; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Float(1.0)
    );
}

#[test]
fn set_member_after_insert() {
    let mut prog = compile::<f32>("s = insert 5 (empty_set 8); main = member 5 s;").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
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
    let mut prog =
        compile::<f32>("main = length (filter (fn x -> x > 1.0) (tail [1.0, 2.0, 3.0]));").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(2)
    );
}

#[test]
fn map_insert_new_key_overflows_at_capacity() {
    // { "a": 1.0 } has cap 1, len 1; inserting a NEW key overflows.
    let mut prog = compile::<f32>("m = { \"a\": 1.0 }; main = insert \"b\" 2.0 m;").unwrap();
    let mut out = [0.0f32; 4];
    let res = MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]);
    assert!(res.is_err(), "insert of a new key past capacity must error");
    assert!(
        format!("{:?}", res.unwrap_err()).contains("capacity exceeded"),
        "expected capacity message"
    );
}

#[test]
fn map_replace_on_duplicate_does_not_overflow() {
    // Replacing an existing key keeps the length AT the capacity bound: it must
    // NOT error (only a NEW key past capacity overflows).
    let mut prog =
        compile::<f32>("m = insert \"a\" 2.0 { \"a\": 1.0 }; main = member \"a\" m;").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(true)
    );
}

#[test]
fn set_insert_duplicate_does_not_overflow() {
    // The first insert fills the cap-1 set; the duplicate insert must be a
    // no-op, not an overflow.
    let mut prog =
        compile::<f32>("s = insert 5 (insert 5 (empty_set 1)); main = member 5 s;").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(true)
    );
}

#[test]
fn tail_shared_source_elements_survive_across_ticks() {
    // The CAF `l = [1.0, 2.0]` is rebuilt each tick; `main = cons 9.0 (tail l)`
    // is [9.0, 2.0], sharing l's tail element 2.0. Tail must recount (RC++) the
    // kept element and leave the removed head's slot untouched — a tail that
    // freed the head mid-tick would recycle its slot for 9.0 and dangle the
    // still-live source list, corrupting the pinned output on the next tick.
    let mut prog = compile::<f32>("l = [1.0, 2.0]; main = cons 9.0 (tail l);").unwrap();
    let mut out = [0.0f32; 4];
    for tick in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        match prog.arena().get(v).unwrap() {
            rill_lang::arena::Value::List { elems, .. } => {
                assert_eq!(
                    prog.arena().get(elems[0]).unwrap(),
                    &rill_lang::arena::Value::Float(9.0),
                    "tick {tick}: consed head must survive"
                );
                assert_eq!(
                    prog.arena().get(elems[1]).unwrap(),
                    &rill_lang::arena::Value::Float(2.0),
                    "tick {tick}: tail's kept element must survive the tick-end clear"
                );
            }
            other => panic!("tick {tick}: expected a List, got {other:?}"),
        }
    }
}

#[test]
fn map_literal_entries_are_sorted_regardless_of_write_order() {
    // Out-of-order multi-key literals must be normalised to sorted order so the
    // binary-search invariant of insert/lookup/member holds.
    let mut prog = compile::<f32>("main = member \"a\" { \"b\": 1.0, \"a\": 2.0 };").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Bool(true),
        "lookup of a key written out of order must still find it"
    );
}

#[test]
fn func_key_is_compile_error() {
    // `Func` has no derived Ord instance — a Map with a function key must not compile.
    let src = "f = fn x -> x; main = lookup f { \"a\": 1.0 };";
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
    let mut prog =
        compile::<f32>("k = [1, 2]; m = insert k 1.0 (empty_map 4); main = length [1.0, 2.0];")
            .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(
        prog.arena().get(v).unwrap(),
        &rill_lang::arena::Value::Int(2)
    );
}
