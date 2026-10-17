//! Mutable `STArray` cells inside `runST` (Task 8): the six flat ST effects
//! (`newArrayFlat` / `readArrayFlat` / `writeArrayFlat` / `modifyArrayFlat` /
//! `getBounds` / `getElems`) and the `runSTArray` freeze to an immutable
//! `Array`.
//!
//! Every flat-size and offset computation lives in the LANGUAGE here — the
//! `Ix` class lands in a later task — so the primitives stay type-agnostic and
//! the tests pass literal flat sizes. The whole pipeline (infer → reduce →
//! lower → interpreter) is driven through `compile`.

use rill_lang::arena::{ArenaRef, Value};
use rill_lang::compile;
use rill_lang::program::RillProgram;

/// The single value output of an already-run program.
fn output(prog: &RillProgram<f32, 256>) -> Value {
    let outputs = prog.value_outputs();
    assert_eq!(outputs.len(), 1, "expected exactly one value output");
    let v = outputs[0].expect("the value output must be bound");
    prog.arena()
        .get(v)
        .expect("the output ref must resolve")
        .clone()
}

/// The two field VALUES of a `Pair` record output, resolved through the arena.
fn pair_values(prog: &RillProgram<f32, 256>, v: &Value) -> (Value, Value) {
    let fields = match v {
        Value::Record(fields) if fields.len() == 2 => fields,
        other => panic!("expected a Pair record, got {other:?}"),
    };
    (
        prog.arena().get(fields[0]).expect("first resolves").clone(),
        prog.arena()
            .get(fields[1])
            .expect("second resolves")
            .clone(),
    )
}

/// The stored bounds and flat element refs of an immutable `Array` output.
fn array_parts(v: &Value) -> (ArenaRef, Vec<ArenaRef>) {
    match v {
        Value::Array { bounds, elems } => (*bounds, elems.clone()),
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// A three-element mutable array expression inside a region body: bounds
/// `0..=2`, flat size 3, all elements initialized to `init`.
fn new_array_0_to_2(init: &str) -> String {
    format!("newArrayFlat (Pair {{ first: 0, second: 2 }}) 3 {init}")
}

#[test]
fn starray_read_write_modify_inside_a_region() {
    // Two regions, each mutating one element and reading it back through the
    // flat ops: the first overwrites an element, the second modifies one in
    // place through a function. A region that ends on a DIRECT effect
    // application yields the read value itself — a `return`ed binding would
    // stay a lazy thunk in the value track.
    let new = new_array_0_to_2("0.0");
    let src = format!(
        "{{-# MODE complete #-}}\nmain = Pair {{ \
           first: runST (\\s -> do {{ a <- {new}; writeArrayFlat a 1 42.0; readArrayFlat a 1 }}), \
           second: runST (\\s -> do {{ a <- {new}; modifyArrayFlat a 2 (\\x -> x + 1.0); readArrayFlat a 2 }}) \
         }};"
    );
    let mut prog = compile::<f32>(&src).expect("the program must compile");
    prog.run_value().expect("the program must run");
    let (written, modified) = pair_values(&prog, &output(&prog));
    assert_eq!(written, Value::Float(42.0), "writeArrayFlat stores");
    assert_eq!(
        modified,
        Value::Float(1.0),
        "modifyArrayFlat applies the function to the stored element"
    );
}

#[test]
fn starray_bounds_and_elems_read_the_whole_cell() {
    // `getBounds` returns the bounds recorded at allocation; `getElems` returns
    // every current element in flat order (the overwrite included). Both are
    // BOUND in the region and then forced by the projections that follow: the
    // bounds pair is projected field by field, the element list is materialized
    // through `arrayFromList` and read back at its overwritten index.
    let src = format!(
        "{{-# MODE complete #-}}\nmain = runST (\\s -> do {{ \
           a <- {}; \
           writeArrayFlat a 1 9.0; \
           b <- getBounds a; \
           es <- getElems a; \
           return (Pair {{ \
             first: Pair {{ first: b.first, second: b.second }}, \
             second: Pair {{ first: length es, \
                            second: arrayIndexFlat (arrayFromList (Pair {{ first: 0, second: 2 }}) es) 1 }} \
           }}) \
         }});",
        new_array_0_to_2("7.0")
    );
    let mut prog = compile::<f32>(&src).expect("the program must compile");
    prog.run_value().expect("the program must run");
    let out = output(&prog);
    let (bounds, elems) = pair_values(&prog, &out);
    let (lo, hi) = pair_values(&prog, &bounds);
    assert_eq!(lo, Value::Int(0), "the lower bound reads back");
    assert_eq!(hi, Value::Int(2), "the upper bound reads back");
    let (count, overwritten) = pair_values(&prog, &elems);
    assert_eq!(count, Value::Int(3), "getElems returns every element");
    assert_eq!(
        overwritten,
        Value::Float(9.0),
        "the overwrite is visible in flat element order"
    );
}

#[test]
fn runst_array_freezes_to_an_immutable_array() {
    // `runSTArray` runs the region and freezes the cell into an ordinary
    // `Array i e`: the result is usable after the region has closed and holds
    // the elements written inside it.
    let src = format!(
        "{{-# MODE complete #-}}\nmain = runSTArray (\\s -> do {{ \
           a <- {}; \
           writeArrayFlat a 1 5.0; \
           return a \
         }});",
        new_array_0_to_2("1.0")
    );
    let mut prog = compile::<f32>(&src).expect("the program must compile");
    prog.run_value().expect("the program must run");
    let out = output(&prog);
    let (bounds, elems) = array_parts(&out);

    let values: Vec<f64> = elems
        .iter()
        .map(|r| match prog.arena().get(*r) {
            Some(Value::Float(f)) => *f,
            other => panic!("expected a Float element, got {other:?}"),
        })
        .collect();
    assert_eq!(
        values,
        vec![1.0, 5.0, 1.0],
        "the frozen elements are intact"
    );

    let bounds = prog.arena().get(bounds).expect("bounds resolve").clone();
    let (lo, hi) = pair_values(&prog, &bounds);
    assert_eq!(lo, Value::Int(0), "the frozen array keeps its bounds");
    assert_eq!(hi, Value::Int(2), "the frozen array keeps its bounds");
}

#[test]
fn frozen_array_is_usable_after_the_region_closes() {
    // The frozen result feeds the immutable flat primitives (`length`,
    // `arrayIndexFlat`) exactly like an `arrayFromList` array — proof that the
    // freeze produced a first-class value, not a dangling handle.
    let src = format!(
        "{{-# MODE complete #-}}\nmain = (\\arr -> Pair {{ first: length arr, second: arrayIndexFlat arr 2 }}) (runSTArray (\\s -> do {{ \
           a <- {}; \
           writeArrayFlat a 2 30.0; \
           return a \
         }}));",
        new_array_0_to_2("0.0")
    );
    let mut prog = compile::<f32>(&src).expect("the program must compile");
    prog.run_value().expect("the program must run");
    let (len, last) = pair_values(&prog, &output(&prog));
    assert_eq!(len, Value::Int(3), "the frozen array's flat length");
    assert_eq!(last, Value::Float(30.0), "the frozen last element");
}

#[test]
fn freeze_transfers_the_region_cells_refs_without_copies() {
    // The cell table lives in the shared arena: a freeze is a refcount
    // TRANSFER (one fresh claim per element + bounds, then the region's own
    // claims are released). After the tick every element and the bounds record
    // is owned by the frozen array ALONE — rc == 1 — so neither a leaked cell
    // (rc == 2) nor an element copy (a distinct slot) can hide here.
    let src = format!(
        "{{-# MODE complete #-}}\nmain = runSTArray (\\s -> do {{ \
           a <- {}; \
           writeArrayFlat a 0 10.0; \
           writeArrayFlat a 1 20.0; \
           writeArrayFlat a 2 30.0; \
           return a \
         }});",
        new_array_0_to_2("0.0")
    );
    let mut prog = compile::<f32>(&src).expect("the program must compile");
    prog.run_value().expect("the program must run");
    let out = output(&prog);
    let (bounds, elems) = array_parts(&out);
    assert_eq!(elems.len(), 3, "all three elements survive the freeze");
    for (i, e) in elems.iter().enumerate() {
        assert_eq!(
            prog.arena().rc(*e),
            1,
            "element {i} must be owned by the frozen array alone (the region cell released its claim)"
        );
    }
    assert_eq!(
        prog.arena().rc(bounds),
        1,
        "the bounds record must be owned by the frozen array alone"
    );
    let values: Vec<f64> = elems
        .iter()
        .map(|r| match prog.arena().get(*r) {
            Some(Value::Float(f)) => *f,
            other => panic!("expected a Float element, got {other:?}"),
        })
        .collect();
    assert_eq!(values, vec![10.0, 20.0, 30.0]);
}

#[test]
fn escaping_starray_is_a_compile_error() {
    // Returning the handle out of `runST` leaves an `STArray s i e` in the
    // region result — it mentions the phantom `s` and must be rejected, exactly
    // like an escaping `STRef`.
    let err = match compile::<f32>(&format!(
        "{{-# MODE complete #-}}\nmain = runST (\\s -> do {{ a <- {}; return a }});",
        new_array_0_to_2("1.0")
    )) {
        Ok(_) => panic!("an escaping STArray must be a compile error"),
        Err(err) => err,
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("ST region result escapes the phantom type s"),
        "expected the escaping-region type error, got: {msg}"
    );
}

#[test]
fn runst_array_rejects_a_non_starray_result() {
    // `runSTArray` freezes an `STArray`; a region that computes anything else
    // has no array to freeze and is a compile-time error.
    let err = match compile::<f32>(&format!(
        "{{-# MODE complete #-}}\nmain = runSTArray (\\s -> do {{ a <- {}; return 1.0 }});",
        new_array_0_to_2("1.0")
    )) {
        Ok(_) => panic!("a non-STArray runSTArray body must be a compile error"),
        Err(err) => err,
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("runSTArray body must produce"),
        "expected the runSTArray body-shape error, got: {msg}"
    );
}

#[test]
fn starray_flat_index_out_of_range_is_a_clean_fault() {
    // Out-of-range flat offsets take the latched fault channel (a
    // `ProcessError`), never a panic and never a Vec index.
    let src = format!(
        "{{-# MODE complete #-}}\nmain = runST (\\s -> do {{ a <- {}; readArrayFlat a 3 }});",
        new_array_0_to_2("1.0")
    );
    let mut prog = compile::<f32>(&src).expect("the program must compile");
    let err = prog
        .run_value()
        .expect_err("an out-of-range read must be a clean runtime error");
    let msg = format!("{err}");
    assert!(
        msg.contains("readArrayFlat") && msg.contains("out of range"),
        "expected the out-of-range fault, got: {msg}"
    );
}

#[test]
fn starray_negative_flat_size_is_a_clean_fault() {
    // A runtime flat size is not trusted: a negative one is a clean fault
    // instead of an allocation attempt. The region binds the handle (so the
    // result is a plain element, not an array escaping its region) and the
    // allocation is what faults.
    let src = "{-# MODE complete #-}\nmain = runST (\\s -> do { a <- newArrayFlat (Pair { first: 0, second: 0 }) -1 1.0; readArrayFlat a 0 });";
    let mut prog = compile::<f32>(src).expect("the program must compile");
    let err = prog
        .run_value()
        .expect_err("a negative flat size must be a clean runtime error");
    let msg = format!("{err}");
    assert!(
        msg.contains("newArrayFlat") && msg.contains("negative"),
        "expected the negative-size fault, got: {msg}"
    );
}
