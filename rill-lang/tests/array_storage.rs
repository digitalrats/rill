//! Immutable array storage primitives (Task 7): the flat, type-agnostic
//! `arrayFromList` / `arrayBounds` / `arrayElems` / `arrayIndexFlat` /
//! `arrayUpdate` / `arrayAccum` operations and the `FixedBuffer` bridge
//! (`arrayFromBuffer` / `bufferFromArray`).
//!
//! These tests drive the whole pipeline (infer → reduce → lower → interpreter)
//! through `compile`, asserting the resulting value outputs and the latched
//! fault channel for out-of-range flat indices.

use rill_lang::arena::{ArenaRef, Value};
use rill_lang::compile;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::program::RillProgram;
use rill_lang::register;

/// Compile `src`, run one tick, and return the single value output's arena
/// value (cloned out of the program's arena).
fn run_value_output(src: &str) -> Value {
    let mut prog = compile::<f32>(src).expect("the program must compile");
    prog.run_value().expect("the program must run");
    output(&prog)
}

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

/// The two field refs of a `Pair` record output.
fn pair_refs(v: &Value) -> (ArenaRef, ArenaRef) {
    match v {
        Value::Record(fields) if fields.len() == 2 => (fields[0], fields[1]),
        other => panic!("expected a Pair record, got {other:?}"),
    }
}

/// The two field VALUES of a `Pair` record output, resolved through the arena.
fn pair_values(prog: &RillProgram<f32, 256>, v: &Value) -> (Value, Value) {
    let (a, b) = pair_refs(v);
    (
        prog.arena().get(a).expect("first resolves").clone(),
        prog.arena().get(b).expect("second resolves").clone(),
    )
}

/// The `Float` payloads of a `List` value, read through the arena.
fn list_floats(prog: &RillProgram<f32, 256>, v: &Value) -> Vec<f64> {
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

#[test]
fn array_from_list_bounds_and_elems_read_back() {
    // Build an array and read both halves back through the primitives: the
    // bounds are the stored `Pair i i` value, the elems the flat `List e`.
    let src = format!(
        "main = (\\arr -> Pair {{ first: arrayBounds arr, second: arrayElems arr }}) {ARR_0_TO_2};"
    );
    let mut prog = compile::<f32>(&src).expect("the program must compile");
    prog.run_value().expect("the program must run");

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
    let mut prog = compile::<f32>(&src).expect("the program must compile");
    let err = prog
        .run_value()
        .expect_err("an out-of-range flat index must be a clean runtime error");
    let msg = format!("{err}");
    assert!(
        msg.contains("arrayIndexFlat") && msg.contains("out of range"),
        "expected the out-of-range fault, got: {msg}"
    );
}

#[test]
fn array_update_returns_a_new_array_and_leaves_the_original_unchanged() {
    // The updated array is observed FIRST, the original SECOND: an in-place
    // update would leak the new element into the original's read.
    let src = format!(
        "main = (\\arr -> Pair {{ first: arrayElems (arrayUpdate arr [Pair {{ first: 1, second: 99.0 }}]), second: arrayElems arr }}) {ARR_0_TO_2};"
    );
    let mut prog = compile::<f32>(&src).expect("the program must compile");
    prog.run_value().expect("the program must run");

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
    let mut prog = compile::<f32>(&src).expect("the program must compile");
    prog.run_value().expect("the program must run");

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
    let mut prog = compile::<f32>(&src).expect("the program must compile");
    let err = prog
        .run_value()
        .expect_err("an out-of-range flat update must be a clean runtime error");
    let msg = format!("{err}");
    assert!(
        msg.contains("arrayUpdate") && msg.contains("out of range"),
        "expected the out-of-range fault, got: {msg}"
    );
}

/// Compile a signal program against the generator registry (needed for the
/// `FixedBuffer` operand of the buffer bridge).
fn buffer_program(src: &str) -> RillProgram<f32, 256> {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    rill_lang::compile_with_ffi::<f32>(src, &ffi, 44_100.0).expect("the program must compile")
}

#[test]
fn buffer_bridge_roundtrip_preserves_contents() {
    // `bufferFromArray (arrayFromBuffer b)` must equal `b`: the bridge copies
    // out and back without disturbing the samples.
    let mut direct = buffer_program("main = square 0.0 0.5 0.0;");
    direct.run_value().expect("the direct program must run");
    let mut expected = [0.0f32; 256];
    assert!(
        direct.copy_output_samples(0, &mut expected),
        "the direct program's output must be a buffer"
    );

    let mut roundtrip =
        buffer_program("main = bufferFromArray (arrayFromBuffer (square 0.0 0.5 0.0));");
    roundtrip
        .run_value()
        .expect("the roundtrip program must run");
    let mut got = [0.0f32; 256];
    assert!(
        roundtrip.copy_output_samples(0, &mut got),
        "the roundtrip program's output must be a buffer"
    );
    assert_eq!(got, expected, "the bridge must preserve every sample");
}
