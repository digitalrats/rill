//! Mutable `STArray` cells inside `runST` (Task 8): the six flat ST effects
//! (`newArrayFlat` / `readArrayFlat` / `writeArrayFlat` / `modifyArrayFlat` /
//! `getBounds` / `getElems`) and the `runSTArray` freeze to an immutable
//! `Array`.
//!
//! Every flat-size and offset computation lives in the LANGUAGE here — the
//! `Ix` class lands in a later task — so the primitives stay type-agnostic and
//! the tests pass literal flat sizes. The whole pipeline (infer → reduce →
//! lower → interpreter) is driven through `compile`/`test_eval`.
pub mod common;

use common::*;
use rill_lang::arena::{ArenaRef, Value};
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;
use rill_lang::program::WorldState;

/// Split `src` at the `main = ` binding into `(defs, expr)` for the eval
/// surface: `main` is not special inside `test_eval`'s defs, so the entry
/// expression is passed separately (and its trailing `;` dropped).
fn split_main(src: &str) -> (&str, &str) {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    (defs, expr.trim().trim_end_matches(';'))
}

/// Compile `src`, run it once, and return the program and its single result.
fn run(src: &str) -> (WorldState<f32, 256>, Value) {
    let (defs, expr) = split_main(src);
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    let v = output(&prog);
    (prog, v)
}

/// The single run result of an already-run program.
fn output(prog: &WorldState<f32, 256>) -> Value {
    let v = prog.result().expect("the run must produce a single result");
    prog.arena()
        .get(v)
        .expect("the result ref must resolve")
        .clone()
}

/// Run `src` expecting a clean latched runtime fault, and return its detail.
///
/// A latched fault surfaces from `test_eval` as
/// `CompileError::Unsupported("eval run failed: …")` with the `ProcessError`
/// text preserved inside the string; a panic in the interpreter fails this
/// helper outright.
fn fault(src: &str) -> String {
    let (defs, expr) = split_main(src);
    let ffi = ForeignRegistry::<f32>::new();
    match eval_ffi(defs, expr, ffi) {
        Ok(_) => panic!("expected a runtime fault, but the program ran\nsrc:\n{src}"),
        Err(rill_lang::CompileError::Unsupported(msg)) => msg,
        Err(other) => panic!("expected a latched processing fault, got {other:?}\nsrc:\n{src}"),
    }
}

/// The two field VALUES of a `Pair` record output, resolved through the arena.
fn pair_values(prog: &WorldState<f32, 256>, v: &Value) -> (Value, Value) {
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
    let (prog, v) = run(&src);
    let (written, modified) = pair_values(&prog, &v);
    assert_eq!(written, Value::Double(42.0), "writeArrayFlat stores");
    assert_eq!(
        modified,
        Value::Double(1.0),
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
    let (prog, v) = run(&src);
    let (bounds, elems) = pair_values(&prog, &v);
    let (lo, hi) = pair_values(&prog, &bounds);
    assert_eq!(integral(&lo), 0, "the lower bound reads back");
    assert_eq!(integral(&hi), 2, "the upper bound reads back");
    let (count, overwritten) = pair_values(&prog, &elems);
    assert_eq!(count, Value::Int(3), "getElems returns every element");
    assert_eq!(
        overwritten,
        Value::Double(9.0),
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
    let (prog, v) = run(&src);
    let (bounds, elems) = array_parts(&v);

    let values: Vec<f64> = elems
        .iter()
        .map(|r| match prog.arena().get(*r) {
            Some(Value::Double(f)) => *f,
            other => panic!("expected a Double element, got {other:?}"),
        })
        .collect();
    assert_eq!(
        values,
        vec![1.0, 5.0, 1.0],
        "the frozen elements are intact"
    );

    let bounds = prog.arena().get(bounds).expect("bounds resolve").clone();
    let (lo, hi) = pair_values(&prog, &bounds);
    assert_eq!(integral(&lo), 0, "the frozen array keeps its bounds");
    assert_eq!(integral(&hi), 2, "the frozen array keeps its bounds");
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
    let (prog, v) = run(&src);
    let (len, last) = pair_values(&prog, &v);
    assert_eq!(len, Value::Int(3), "the frozen array's flat length");
    assert_eq!(last, Value::Double(30.0), "the frozen last element");
}

#[test]
fn freeze_keeps_the_element_claims_balanced() {
    // The cell table lives in the shared arena: a freeze takes one fresh claim
    // per element + bounds, then the region's own claims are released. After the
    // tick every element and the bounds record is owned by the frozen array
    // ALONE — rc == 1.
    //
    // What this pins is the CLAIM ACCOUNTING: a leaked cell claim would read 2,
    // and a double-free would have taken the slot out from under the array. It
    // canNOT distinguish a refcount transfer from a deep element copy — a copy
    // is a FRESH slot, and a fresh slot also reads rc == 1. The no-copy property
    // is guaranteed by construction (`freeze_starray` only calls `Arena::copy`,
    // never `arena.clone` of a value) and is verified by reading the code, not
    // by this assertion.
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
    let (prog, v) = run(&src);
    let (bounds, elems) = array_parts(&v);
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
            Some(Value::Double(f)) => *f,
            other => panic!("expected a Double element, got {other:?}"),
        })
        .collect();
    assert_eq!(values, vec![10.0, 20.0, 30.0]);
}

#[test]
fn escaping_starray_is_a_compile_error() {
    // Returning the handle out of `runST` leaves an `STArray s i e` in the
    // region result — it mentions the phantom `s` and must be rejected, exactly
    // like an escaping `STRef`.
    let err = match compile_eval::<f32, 256>(
        &format!(
            "{{-# MODE complete #-}}\nmain = runST (\\s -> do {{ a <- {}; return a }});",
            new_array_0_to_2("1.0")
        ),
        "main",
        Mode::Strict,
    ) {
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
    let err = match compile_eval::<f32, 256>(
        &format!(
            "{{-# MODE complete #-}}\nmain = runSTArray (\\s -> do {{ a <- {}; return 1.0 }});",
            new_array_0_to_2("1.0")
        ),
        "main",
        Mode::Strict,
    ) {
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
    let msg = fault(&src);
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
    let msg = fault(src);
    assert!(
        msg.contains("newArrayFlat") && msg.contains("negative"),
        "expected the negative-size fault, got: {msg}"
    );
}

#[test]
fn starray_size_beyond_the_arena_capacity_is_a_clean_fault() {
    // The flat size is computed by the LANGUAGE, so it is not part of the
    // build-time arena budget: a size larger than the arena's FREE slots is a
    // clean fault instead of a long futile allocation loop (or an OOM-ish
    // failure part-way through).
    let src = "{-# MODE complete #-}\nmain = runST (\\s -> do { \
               a <- newArrayFlat (Pair { first: 0, second: 999999 }) 1000000 1.0; \
               readArrayFlat a 0 \
             });";
    let msg = fault(src);
    assert!(
        msg.contains("newArrayFlat") && msg.contains("capacity"),
        "expected the over-capacity fault, got: {msg}"
    );
}

#[test]
fn wrong_kind_handles_are_rejected_at_compile_time() {
    // `STRef s a` and `STArray s i e` are distinct static types, so the
    // interpreter's kind faults are DEFENSIVE: no program text can pass one
    // where the other is expected (those paths are covered by the lib unit test
    // `st_cell_kind_mismatch_faults_instead_of_silently_dropping`). This pins
    // the type-level separation that makes them unreachable.
    for src in [
        // `getBounds` on an `STRef`.
        "{-# MODE complete #-}\nmain = runST (\\s -> do { r <- newSTRef 0.0; b <- getBounds r; b.first });",
        // `readSTRef` on an `STArray`.
        "{-# MODE complete #-}\nmain = runST (\\s -> do { a <- newArrayFlat (Pair { first: 0, second: 2 }) 3 0.0; x <- readSTRef a; x });",
    ] {
        let err = match compile_eval::<f32, 256>(src, "main", Mode::Strict) {
            Ok(_) => panic!("a mis-kinded handle must be a compile error: {src}"),
            Err(err) => err,
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("STRef") && msg.contains("STArray"),
            "expected a kind mismatch naming both types, got: {msg}"
        );
    }
}

#[test]
fn a_fractional_flat_offset_is_rejected_at_compile_time() {
    // The flat size/offset parameters are `Int`. The interpreter accepts an
    // integral `Double` too (a negated literal arrives on the Double track — the
    // documented wart), but a FRACTIONAL one is refused there rather than
    // truncated; the language already refuses it statically, so the interpreter
    // check is defensive (unit-tested in `forced_i64_accepts_int_and_integral_float_only`).
    for src in [
        "{-# MODE complete #-}\nmain = runST (\\s -> do { a <- newArrayFlat (Pair { first: 0, second: 2 }) 3 0.0; readArrayFlat a 1.5 });",
        "{-# MODE complete #-}\nmain = runST (\\s -> do { a <- newArrayFlat (Pair { first: 0, second: 2 }) 3.0 0.0; readArrayFlat a 0 });",
    ] {
        let err = match compile_eval::<f32, 256>(src, "main", Mode::Strict) {
            Ok(_) => panic!("a Double flat size/offset must be a compile error: {src}"),
            Err(err) => err,
        };
        assert!(
            format!("{err}").contains("Double with Int"),
            "expected a Double/Int mismatch, got: {err}"
        );
    }
}
