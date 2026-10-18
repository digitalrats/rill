//! Immutable array storage primitives (Task 7): the flat, type-agnostic
//! `arrayFromList` / `arrayBounds` / `arrayElems` / `arrayIndexFlat` /
//! `arrayUpdate` / `arrayAccum` operations and the `FixedBuffer` bridge
//! (`arrayFromBuffer` / `bufferFromArray`).
//!
//! These tests drive the whole pipeline (infer → reduce → lower → interpreter)
//! through `test_eval`, asserting the resulting run result and the latched
//! fault channel for out-of-range flat indices.
pub mod common;

use common::*;
use rill_lang::arena::{ArenaRef, Value};
use rill_lang::ffi::ForeignRegistry;
use rill_lang::program::WorldState;
use rill_lang::register;

/// Compile `src`, run it once via [`test_eval`] against an empty foreign
/// registry, and return the already-run program.
fn run(src: &str) -> WorldState<f32, 256> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    eval_ffi(defs, expr, ffi).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"))
}

/// Compile `src`, run it once, and return the single run result's arena value
/// (cloned out of the program's arena).
fn run_value_output(src: &str) -> Value {
    output(&run(src))
}

/// The single run result of an already-run program.
fn output(prog: &WorldState<f32, 256>) -> Value {
    let v = prog.result().expect("the run must produce a single result");
    prog.arena()
        .get(v)
        .expect("the result ref must resolve")
        .clone()
}

/// Run `src` expecting the latched value-track fault, returning its detail.
fn fault(src: &str) -> String {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    match eval_ffi(defs, expr, ffi) {
        Ok(_) => panic!("expected a fault, but the program ran\nsrc:\n{src}"),
        Err(rill_lang::CompileError::Unsupported(msg)) => msg,
        Err(other) => panic!("expected a latched processing fault, got {other:?}\nsrc:\n{src}"),
    }
}

/// The two field refs of a `Pair` record output.
fn pair_refs(v: &Value) -> (ArenaRef, ArenaRef) {
    match v {
        Value::Record(fields) if fields.len() == 2 => (fields[0], fields[1]),
        other => panic!("expected a Pair record, got {other:?}"),
    }
}

/// The two field VALUES of a `Pair` record output, resolved through the arena.
fn pair_values(prog: &WorldState<f32, 256>, v: &Value) -> (Value, Value) {
    let (a, b) = pair_refs(v);
    (
        prog.arena().get(a).expect("first resolves").clone(),
        prog.arena().get(b).expect("second resolves").clone(),
    )
}

/// The `Float` payloads of a `List` value, read through the arena.
fn list_floats(prog: &WorldState<f32, 256>, v: &Value) -> Vec<f64> {
    let elems = match v {
        Value::List { elems } => elems,
        other => panic!("expected a List, got {other:?}"),
    };
    elems
        .iter()
        .map(|r| match prog.arena().get(*r) {
            Some(Value::Float(f)) => *f,
            other => panic!("expected a Float element, got {other:?}"),
        })
        .collect()
}

/// A one-dimensional array expression with bounds `0..=2` and elements
/// `[10.0, 20.0, 30.0]`.
const ARR_0_TO_2: &str = "(arrayFromList (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0])";

/// Run a buffer-bridge program against the foreign generator registry (the
/// `square` generator is an FFI builtin) and return its live block.
fn run_buffer(src: &str) -> Vec<f32> {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = eval_ffi(defs, expr, ffi).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    let r = prog.result().expect("whole-buffer result expected");
    let mut got = [0.0f32; 256];
    assert!(prog.copy_buffer(r, &mut got), "the output must be a buffer");
    got.to_vec()
}

/// Run a buffer-bridge program expecting a latched fault, returning its detail.
fn buffer_fault(src: &str) -> String {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    match eval_ffi(defs, expr, ffi) {
        Ok(_) => panic!("expected a fault, but the program ran\nsrc:\n{src}"),
        Err(rill_lang::CompileError::Unsupported(msg)) => msg,
        Err(other) => panic!("expected a latched processing fault, got {other:?}\nsrc:\n{src}"),
    }
}

#[test]
fn array_from_list_bounds_and_elems_read_back() {
    // Build an array and read both halves back through the primitives: the
    // bounds are the stored `Pair i i` value, the elems the flat `List e`.
    let src = format!(
        "main = (\\arr -> Pair {{ first: arrayBounds arr, second: arrayElems arr }}) {ARR_0_TO_2};"
    );
    let prog = run(&src);

    let out = output(&prog);
    let (bounds, elems) = pair_values(&prog, &out);
    let (lo, hi) = pair_values(&prog, &bounds);
    assert_eq!(lo, Value::Int(0), "lower bound reads back");
    assert_eq!(hi, Value::Int(2), "upper bound reads back");
    assert_eq!(list_floats(&prog, &elems), vec![10.0, 20.0, 30.0]);
}

#[test]
fn array_length_is_the_flat_element_count() {
    // `length` of an immutable array is its flat element count (the same
    // "direct primitive read" the Array storage model implies).
    let src = format!("main = length {ARR_0_TO_2};");
    assert_eq!(run_value_output(&src), Value::Int(3));
}

#[test]
fn array_index_flat_in_range_reads_the_element() {
    let src = format!("main = arrayIndexFlat {ARR_0_TO_2} 1;");
    assert_eq!(run_value_output(&src), Value::Float(20.0));
}

#[test]
fn array_index_flat_out_of_range_is_a_clean_fault() {
    // Out-of-range flat indices take the latched fault channel: a
    // `ProcessError`, never a panic (design §Fault policy).
    let src = format!("main = arrayIndexFlat {ARR_0_TO_2} 3;");
    let msg = fault(&src);
    assert!(
        msg.contains("arrayIndexFlat") && msg.contains("out of range"),
        "expected the out-of-range fault, got: {msg}"
    );
}

#[test]
fn array_seed_fills_every_position_with_the_same_value() {
    // `arraySeed bounds init` is the M8 construction helper behind
    // `accumArray`: every position receives the seed. Reading the flat elems
    // back shows N copies of the seed.
    let src = "main = arrayElems (arraySeed (Pair { first: 0, second: 2 }) 7.0);";
    let prog = run(src);
    assert_eq!(list_floats(&prog, &output(&prog)), vec![7.0, 7.0, 7.0]);
}

#[test]
fn array_seed_on_a_matrix_shape_seeds_every_cell() {
    // 2-D bounds `((0,0),(1,1))` — a 2x2 grid, row-major: four cells.
    let src = "main = arrayElems (arraySeed (Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 1, second: 1 } }) 2.5);";
    let prog = run(src);
    assert_eq!(list_floats(&prog, &output(&prog)), vec![2.5, 2.5, 2.5, 2.5]);
}

#[test]
fn array_seed_on_inverted_bounds_is_a_valid_empty_array() {
    // `lo > hi` is a VALID EMPTY array (Q3): zero elements, no fault.
    let src = "main = arrayElems (arraySeed (Pair { first: 0, second: -1 }) 1.0);";
    let prog = run(src);
    assert_eq!(list_floats(&prog, &output(&prog)), vec![]);
    let src = "main = arrayBounds (arraySeed (Pair { first: 0, second: -1 }) 1.0);";
    let prog = run(src);
    let (lo, hi) = pair_values(&prog, &output(&prog));
    assert_eq!(lo, Value::Int(0));
    assert_eq!(hi, Value::Int(-1));
}

#[test]
fn array_seed_rejects_an_unsupported_index_type() {
    // Float bounds are not a prelude index shape: a clean fault (Q1).
    let src = "main = arraySeed (Pair { first: 0.5, second: 2.0 }) 1.0;";
    let msg = fault(src);
    assert!(
        msg.contains("arraySeed") && msg.contains("unsupported index type"),
        "expected the unsupported index type fault, got: {msg}"
    );
}

#[test]
fn array_from_assocs_unlisted_positions_are_faulting_holes() {
    // M8: unlisted positions of an `arrayFromAssocs` array are ONE shared lazy
    // hole. A fold forces every element, so hitting an unlisted position
    // latches the clean "undefined array element" fault (GHC's `undefined`).
    let src = "main = fold (\\a -> \\b -> a + b) 0.0 (arrayElems (arrayFromAssocs (Pair { first: 0, second: 3 }) [Pair { first: 0, second: 1.5 }, Pair { first: 2, second: 3.5 }]));";
    let msg = fault(src);
    assert!(
        msg.contains("array: undefined array element"),
        "expected the hole fault, got: {msg}"
    );
}

#[test]
fn array_from_assocs_length_does_not_force_holes() {
    // `length`/`arrayElems` read the element list WITHOUT forcing elements, so
    // an array full of holes still reports its flat size (the hole mechanism
    // only bites when a value is demanded).
    let src = "main = length (arrayElems (arrayFromAssocs (Pair { first: 0, second: 3 }) [Pair { first: 0, second: 1.5 }, Pair { first: 2, second: 3.5 }]));";
    assert_eq!(run_value_output(src), Value::Int(4));
}

#[test]
fn array_from_assocs_last_association_for_a_cell_wins() {
    // The assocs are overlaid in order; a repeated index leaves the LAST value
    // (like `arrayUpdate`'s duplicate-offset rule, but index-typed).
    let src = "main = fold (\\a -> \\b -> a + b) 0.0 (arrayElems (arrayFromAssocs (Pair { first: 0, second: 1 }) [Pair { first: 0, second: 1.0 }, Pair { first: 0, second: 7.0 }, Pair { first: 1, second: 5.0 }]));";
    let prog = run(src);
    assert_eq!(run_value_output(src), Value::Float(12.0));
    let _ = prog;
}

#[test]
fn array_from_assocs_resolves_2d_cells_against_the_stored_bounds() {
    // Bounds `((1,2),(2,3))`: a 2x2 grid (rows 1-2, cols 2-3), row-major.
    // Cells resolve to offsets (1,2)->0, (1,3)->1, (2,2)->2, (2,3)->3; the
    // fold of all four listed values is 5.0+11.0+13.0+9.0 = 38.0.
    let src = "main = fold (\\a -> \\b -> a + b) 0.0 (arrayElems (arrayFromAssocs (Pair { first: Pair { first: 1, second: 2 }, second: Pair { first: 2, second: 3 } }) [Pair { first: Pair { first: 1, second: 2 }, second: 5.0 }, Pair { first: Pair { first: 1, second: 3 }, second: 11.0 }, Pair { first: Pair { first: 2, second: 2 }, second: 13.0 }, Pair { first: Pair { first: 2, second: 3 }, second: 9.0 }]));";
    assert_eq!(run_value_output(src), Value::Float(38.0));
}

#[test]
fn array_from_assocs_rejects_an_out_of_rectangle_cell() {
    // Bounds `((0,0),(2,1))` (3x2): cell (0,2) has a FLAT offset (2) inside the
    // 6-slot buffer, but column 2 exceeds the bounded rectangle — the
    // rectangle check fires before any flat-offset arithmetic (Q3).
    let src = "main = arrayElems (arrayFromAssocs (Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 2, second: 1 } }) [Pair { first: Pair { first: 0, second: 2 }, second: 1.0 }]);";
    let msg = fault(src);
    assert!(
        msg.contains("arrayFromAssocs") && msg.contains("out of range"),
        "expected the out-of-range fault, got: {msg}"
    );
}

#[test]
fn array_from_assocs_rejects_an_unsupported_index_type() {
    // Float bounds are not a prelude index shape: a clean fault (Q1). The
    // assoc list carries the same Float index so the program type-checks and
    // the FAULT is a runtime one, not a compile-time type error.
    let src = "main = arrayFromAssocs (Pair { first: 0.5, second: 2.0 }) [Pair { first: 0.5, second: 1.0 }];";
    let msg = fault(src);
    assert!(
        msg.contains("arrayFromAssocs") && msg.contains("unsupported index type"),
        "expected the unsupported index type fault, got: {msg}"
    );
}

#[test]
fn array_from_assocs_on_inverted_bounds_is_a_valid_empty_array() {
    // (0,-1) is the canonical empty range: zero elements, no fault. The empty
    // assocs list is built by mapping over an inverted `range` — a typed empty
    // `List (Pair Int Float)` (the bare `[]` literal defaults to `List Float`
    // and cannot unify with `List (Pair Int e)`).
    let src = "main = arrayElems (arrayFromAssocs (Pair { first: 0, second: -1 }) (map (\\i -> Pair { first: i, second: 0.0 }) (range (Pair { first: 1, second: 0 }))));";
    let prog = run(src);
    assert_eq!(list_floats(&prog, &output(&prog)), vec![]);
}

#[test]
fn array_from_assocs_wide_range_with_few_assocs_builds_cheaply() {
    // One shared hole claimed by thousands of slots: no per-slot arena
    // allocation, so a wide unlisted range builds without exhaustion and the
    // listed cells are still addressable by flat footprint.
    let src = "main = length (arrayElems (arrayFromAssocs (Pair { first: 0, second: 999 }) [Pair { first: 999, second: 3.5 }]));";
    assert_eq!(run_value_output(src), Value::Int(1000));
}

#[test]
fn array_update_returns_a_new_array_and_leaves_the_original_unchanged() {
    // The updated array is observed FIRST, the original SECOND: an in-place
    // update would leak the new element into the original's read.
    let src = format!(
        "main = (\\arr -> Pair {{ first: arrayElems (arrayUpdate arr [Pair {{ first: 1, second: 99.0 }}]), second: arrayElems arr }}) {ARR_0_TO_2};"
    );
    let prog = run(&src);

    let out = output(&prog);
    let (updated, original) = pair_values(&prog, &out);
    assert_eq!(
        list_floats(&prog, &updated),
        vec![10.0, 99.0, 30.0],
        "arrayUpdate must return the updated array"
    );
    assert_eq!(
        list_floats(&prog, &original),
        vec![10.0, 20.0, 30.0],
        "arrayUpdate must not mutate the original array"
    );
}

#[test]
fn array_accum_returns_a_new_array_and_leaves_the_original_unchanged() {
    let src = format!(
        "main = (\\arr -> Pair {{ first: arrayElems (arrayAccum arr (\\a -> \\b -> a + b) [Pair {{ first: 1, second: 5.0 }}]), second: arrayElems arr }}) {ARR_0_TO_2};"
    );
    let prog = run(&src);

    let out = output(&prog);
    let (accumulated, original) = pair_values(&prog, &out);
    assert_eq!(
        list_floats(&prog, &accumulated),
        vec![10.0, 25.0, 30.0],
        "arrayAccum applies the accumulator to the old value and the update"
    );
    assert_eq!(
        list_floats(&prog, &original),
        vec![10.0, 20.0, 30.0],
        "arrayAccum must not mutate the original array"
    );
}

#[test]
fn array_update_out_of_range_is_a_clean_fault() {
    let src = format!(
        "main = arrayElems (arrayUpdate {ARR_0_TO_2} [Pair {{ first: 7, second: 99.0 }}]);"
    );
    let msg = fault(&src);
    assert!(
        msg.contains("arrayUpdate") && msg.contains("out of range"),
        "expected the out-of-range fault, got: {msg}"
    );
}

#[test]
fn array_accum_out_of_range_is_a_clean_fault() {
    let src = format!(
        "main = arrayElems (arrayAccum {ARR_0_TO_2} (\\a -> \\b -> a + b) [Pair {{ first: 5, second: 1.0 }}]);"
    );
    let msg = fault(&src);
    assert!(
        msg.contains("arrayAccum") && msg.contains("out of range"),
        "expected the out-of-range fault, got: {msg}"
    );
}

#[test]
fn array_update_keeps_the_last_association_for_a_duplicate_offset() {
    // Two associations target the same flat offset: the last one wins.
    let src = format!(
        "main = arrayElems (arrayUpdate {ARR_0_TO_2} [Pair {{ first: 1, second: 99.0 }}, Pair {{ first: 1, second: 77.0 }}]);"
    );
    let prog = run(&src);
    assert_eq!(
        list_floats(&prog, &output(&prog)),
        vec![10.0, 77.0, 30.0],
        "the last association for a duplicate flat offset wins"
    );
}

#[test]
fn array_update_resolves_associations_against_non_unit_lower_bounds() {
    // The association's `first` is an INDEX into the STORED bounds, resolved by
    // the interpreter against the shape — index 3 in `(2,4)` is offset 1, so
    // the second element (20.0) is replaced, not the fourth.
    let src =
        "main = arrayElems (arrayUpdate (arrayFromList (Pair { first: 2, second: 4 }) [10.0, 20.0, 30.0]) [Pair { first: 3, second: 99.0 }]);";
    let prog = run(src);
    assert_eq!(
        list_floats(&prog, &output(&prog)),
        vec![10.0, 99.0, 30.0],
        "index 3 of (2,4) must resolve to offset 1"
    );
}

#[test]
fn array_update_rejects_an_out_of_rectangle_cell_even_with_an_in_range_flat() {
    // Bounds ((0,0),(2,1)) — a 3x2 grid, 6 elements. The cell (0,2) has flat
    // offset 2, which is INSIDE the 6-element storage, but column 2 is outside
    // the rectangle: the bounds-shape check fires first (Q3).
    let src = "main = arrayElems (arrayUpdate (arrayFromList (Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 2, second: 1 } }) [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]) [Pair { first: Pair { first: 0, second: 2 }, second: 99.0 }]);";
    let msg = fault(src);
    assert!(
        msg.contains("arrayUpdate") && msg.contains("out of range"),
        "expected the clean out-of-rectangle fault, got: {msg}"
    );
}

#[test]
fn array_from_list_with_a_cons_built_list_is_a_clean_fault() {
    // In Complete mode `:` builds a lazy cons chain, which has no flat element
    // Vec. `arrayFromList` reports that as a clean fault instead of leaving the
    // destination register silently unbound — a wrong result on the canonical
    // Haskell list syntax. (The reducer does not fold the chain into a list, so
    // this is the path that actually runs.)
    let src = "{-# MODE complete #-}\n\
               main = length (arrayFromList (Pair { first: 0, second: 2 }) (1.0 : 2.0 : []));";
    let msg = fault(src);
    assert!(
        msg.contains("arrayFromList") && msg.contains("expected a list of elements"),
        "expected the cons-built-list fault, got: {msg}"
    );
}

#[test]
fn empty_array_has_the_empty_range_bounds_and_no_elements() {
    // The empty-array shape is `(lo, hi) = (0, -1)` — the conventional empty
    // range — with zero elements. Reading and indexing it are both defined
    // (indexing latches the clean fault).
    let empty = "(arrayFromList (Pair { first: 0, second: -1 }) [])";
    let src =
        format!("main = (\\arr -> Pair {{ first: arrayBounds arr, second: length arr }}) {empty};");
    let prog = run(&src);
    let (bounds, len) = pair_values(&prog, &output(&prog));
    let (lo, hi) = pair_values(&prog, &bounds);
    // `arrayFromList` stores the bounds value VERBATIM, so the empty range
    // reads back exactly as written. Both bounds stay on the `Int` track:
    // value arithmetic preserves `Int` operands (and `neg` keeps the operand's
    // track), so `-1` reaches the array as `Value::Int(-1)`. Before the
    // Int-preserving arithmetic change this was a `Float`, which the
    // interpreter accepted only through its integral-float tolerance.
    assert_eq!(lo, Value::Int(0), "the empty lower bound reads back");
    assert_eq!(hi, Value::Int(-1), "the empty upper bound reads back");
    assert_eq!(len, Value::Int(0), "the empty array has no elements");

    let msg = fault(&format!("main = arrayIndexFlat {empty} 0;"));
    assert!(
        msg.contains("out of range"),
        "expected the out-of-range fault, got: {msg}"
    );
}

#[test]
fn buffer_from_array_zero_pads_a_short_array() {
    // A three-element array bridged into a buffer fills the FIRST three samples
    // of the live block and zeroes the rest, so the output is deterministic.
    let got = run_buffer(
        "main = bufferFromArray (arrayFromList (Pair { first: 0, second: 2 }) [1.5, 2.5, 3.5]);",
    );
    assert_eq!(&got[..3], &[1.5, 2.5, 3.5], "the array fills the head");
    assert!(
        got[3..].iter().all(|s| *s == 0.0),
        "the rest of the live block is zero-padded, got {:?}",
        &got[3..8]
    );
}

#[test]
fn buffer_from_array_rejects_an_array_longer_than_the_buffer() {
    // 300 elements do not fit a 256-sample block: a clean capacity fault, not
    // a silent truncation.
    let elems: Vec<String> = (0..300).map(|i| format!("{i}.0")).collect();
    let src = format!(
        "main = bufferFromArray (arrayFromList (Pair {{ first: 0, second: 299 }}) [{}]);",
        elems.join(", ")
    );
    let msg = buffer_fault(&src);
    assert!(
        msg.contains("bufferFromArray") && msg.contains("capacity"),
        "expected the capacity fault, got: {msg}"
    );
}

#[test]
fn buffer_bridge_roundtrip_preserves_contents() {
    // `bufferFromArray (arrayFromBuffer b)` must equal `b`: the bridge copies
    // out and back without disturbing the samples.
    let direct = run_buffer("main = square 0.0 0.5 0.0;");
    let roundtrip = run_buffer("main = bufferFromArray (arrayFromBuffer (square 0.0 0.5 0.0));");
    assert_eq!(roundtrip, direct, "the bridge must preserve every sample");
}
