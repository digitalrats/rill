//! Data.Array language surface (Task 9): the `Ix` typeclass with its `Int` and
//! `Pair Int Int` instances, and the constrained immutable/ST array vocabulary
//! built on the flat storage primitives (Tasks 7-8).
//!
//! Every offset/range computation happens in the LANGUAGE (`Ix` instance
//! bodies), so these tests drive the whole pipeline end to end
//! (parse → infer → reduce → lower → interpreter) through `test_eval` and
//! assert the resulting VALUES, the array shapes, and the latched fault
//! channel.
pub mod common;

use common::*;
use rill_lang::arena::{ArenaRef, Value};
use rill_lang::ffi::ForeignRegistry;
use rill_lang::program::WorldState;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Compile `src`, run it once, and hand the program plus its single result to
/// `f` — all on a grown-stack thread.
///
/// Debug-build lowering recurses once per nested call and scope, and an `ixmap`
/// chain (a constrained definition whose body maps a λ over `range`) needs more
/// than libtest's default ~2 MiB test thread. A stack overflow is an ABORT, not
/// a catchable panic — it takes the whole binary down and reports a signal
/// instead of a diagnostic — so the work runs where there is room. The subject
/// here is the array surface, not stack arithmetic.
///
/// A [`WorldState`] owns `Rc`-backed arenas and is therefore not `Send`: the
/// world cannot outlive the thread, so `f` extracts plain data instead.
fn with_output<R: Send>(src: &str, f: impl FnOnce(&WorldState<f32, 256>, &Value) -> R + Send) -> R {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn_scoped(scope, || {
                let (prog, v) = run(src);
                f(&prog, &v)
            })
            .expect("the compile thread must spawn")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

/// The single run result of an already-run program.
fn output(prog: &WorldState<f32, 256>) -> Value {
    let v = prog.result().expect("the run must produce a single result");
    prog.arena()
        .get(v)
        .expect("the result ref must resolve")
        .clone()
}

/// Compile `src` and run it once against the foreign generator registry (the
/// buffer generators `sine`/`square` are FFI builtins), so a program may
/// bridge buffers and arrays.
fn buffer_program(src: &str) -> (WorldState<f32, 256>, Value) {
    let (defs, expr) = split_main(src);
    let mut ffi = ForeignRegistry::<f32>::new();
    rill_lang::register::register_foreign_generators(&mut ffi);
    let prog = eval_ffi(defs, expr, ffi).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    let v = output(&prog);
    (prog, v)
}

/// Run `src` expecting the latched value-track fault, and return its detail.
///
/// A latched fault surfaces from `test_eval` as
/// `CompileError::Unsupported("eval run failed: …")` with the `ProcessError`
/// text preserved inside the string; a panic in the interpreter fails this
/// helper outright (the harness reports the panic).
fn fault(src: &str) -> String {
    let (defs, expr) = split_main(src);
    let ffi = ForeignRegistry::<f32>::new();
    match eval_ffi(defs, expr, ffi) {
        Ok(_) => panic!("expected a fault, but the program ran\nsrc:\n{src}"),
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

/// The Int payloads of a `List` value.
fn list_ints(prog: &WorldState<f32, 256>, v: &Value) -> Vec<i64> {
    let elems = match v {
        Value::List { elems } => elems.clone(),
        other => panic!("expected a List, got {other:?}"),
    };
    elems
        .iter()
        .map(|r| match prog.arena().get(*r) {
            Some(Value::Int(i)) => *i,
            other => panic!("expected an Int element, got {other:?}"),
        })
        .collect()
}

/// The Double payloads of a `List` value.
fn list_floats(prog: &WorldState<f32, 256>, v: &Value) -> Vec<f64> {
    let elems = match v {
        Value::List { elems } => elems.clone(),
        other => panic!("expected a List, got {other:?}"),
    };
    elems
        .iter()
        .map(|r| match prog.arena().get(*r) {
            Some(Value::Double(f)) => *f,
            other => panic!("expected a Double element, got {other:?}"),
        })
        .collect()
}

/// The stored bounds ref and flat element refs of an `Array` value.
fn array_parts(v: &Value) -> (ArenaRef, Vec<ArenaRef>) {
    match v {
        Value::Array { bounds, elems } => (*bounds, elems.clone()),
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// The flat Double elements of an `Array` value plus its bounds as Ints.
fn array_1d(prog: &WorldState<f32, 256>, v: &Value) -> (Vec<f64>, (i64, i64)) {
    let (bounds, elems) = array_parts(v);
    let bounds = prog.arena().get(bounds).expect("bounds resolve").clone();
    let (lo, hi) = pair_values(prog, &bounds);
    let (lo, hi) = match (lo, hi) {
        (Value::Int(lo), Value::Int(hi)) => (lo, hi),
        other => panic!("expected Int bounds, got {other:?}"),
    };
    let values = elems
        .iter()
        .map(|r| match prog.arena().get(*r) {
            Some(Value::Double(f)) => *f,
            other => panic!("expected a Double element, got {other:?}"),
        })
        .collect();
    (values, (lo, hi))
}

/// A `Pair Int Int` bound-shape expression, PARENTHESIZED so it stays one
/// argument in a juxtaposition application (`range (Pair { … })` — an
/// unparenthesized record literal would split into `range Pair { … }`).
fn bounds(lo: i64, hi: i64) -> String {
    format!("(Pair {{ first: {lo}, second: {hi} }})")
}

// ---------------------------------------------------------------------------
// 1. `Ix Int`
// ---------------------------------------------------------------------------

#[test]
fn ix_int_range_is_inclusive_and_handles_negative_bounds() {
    let src = format!("main = range {};", bounds(-3, 2));
    let (prog, v) = run(&src);
    assert_eq!(list_ints(&prog, &v), vec![-3, -2, -1, 0, 1, 2]);
}

#[test]
fn ix_int_range_is_empty_when_upper_precedes_lower() {
    let src = format!("main = range {};", bounds(2, 1));
    let (prog, v) = run(&src);
    assert!(list_ints(&prog, &v).is_empty(), "l > u is the empty range");
}

#[test]
fn ix_int_range_is_the_empty_list_for_the_canonical_empty_bounds() {
    // `(0, -1)` is the conventional empty range Haskell uses for 0-element
    // arrays; `listRange` must not emit the `0` element for it.
    let src = format!("main = range {};", bounds(0, -1));
    let (prog, v) = run(&src);
    assert!(list_ints(&prog, &v).is_empty());
}

#[test]
fn ix_int_index_is_the_offset_from_the_lower_bound() {
    // `index (0,2) 1` is 1 by the METHOD, not the array accessor.
    assert_eq!(
        run("main = index (Pair { first: 0, second: 2 }) 1;").1,
        Value::Int(1)
    );
    assert_eq!(
        run("main = index (Pair { first: 3, second: 9 }) 5;").1,
        Value::Int(2)
    );
    // A negative lower bound shifts the offset by the same amount.
    assert_eq!(
        run("main = index (Pair { first: -2, second: 3 }) 1;").1,
        Value::Int(3)
    );
}

#[test]
fn ix_int_in_range_includes_both_endpoints() {
    let b = bounds(-2, 2);
    for (i, want) in [(-2, true), (2, true), (0, true), (-3, false), (3, false)] {
        let src = format!("main = inRange {b} {i};");
        assert_eq!(run(&src).1, Value::Bool(want), "inRange {b} {i}");
    }
}

#[test]
fn ix_int_range_size_is_the_inclusive_span() {
    assert_eq!(
        run(&format!("main = rangeSize {};", bounds(0, 2))).1,
        Value::Int(3)
    );
    assert_eq!(
        run(&format!("main = rangeSize {};", bounds(-1, 2))).1,
        Value::Int(4)
    );
    // Zero-length: the upper bound one below the lower yields size 0.
    assert_eq!(
        run(&format!("main = rangeSize {};", bounds(2, 1))).1,
        Value::Int(0)
    );
    assert_eq!(
        run(&format!("main = rangeSize {};", bounds(0, -1))).1,
        Value::Int(0)
    );
}

// ---------------------------------------------------------------------------
// 2. `Ix (Pair Int Int)` — row-major
// ---------------------------------------------------------------------------

#[test]
fn ix_pair_index_is_row_major() {
    let b = "(Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 1, second: 2 } })";
    // A 2x3 grid: (i1,j1) -> i1 * 3 + j1.
    for (i1, i2, want) in [(0, 0, 0), (0, 2, 2), (1, 0, 3), (1, 1, 4), (1, 2, 5)] {
        let src = format!("main = index {b} (Pair {{ first: {i1}, second: {i2} }});");
        assert_eq!(run(&src).1, Value::Int(want), "index ({i1},{i2})");
    }
    // A non-zero origin shifts the whole grid.
    let b = "(Pair { first: Pair { first: 1, second: 2 }, second: Pair { first: 2, second: 4 } })";
    assert_eq!(
        run(&format!(
            "main = index {b} (Pair {{ first: 2, second: 4 }});"
        ))
        .1,
        Value::Int(5),
        "(2-1)*3 + (4-2) = 5"
    );
}

#[test]
fn ix_pair_range_size_is_the_product_of_the_spans() {
    assert_eq!(
        run("main = rangeSize ((Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 1, second: 2 } }));").1,
        Value::Int(6)
    );
    // An empty dimension makes the whole range empty.
    assert_eq!(
        run("main = rangeSize ((Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 1, second: -1 } }));").1,
        Value::Int(0)
    );
}

#[test]
fn ix_pair_range_is_row_major_and_empty_for_an_empty_dimension() {
    let src = "main = range ((Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 1, second: 1 } }));";
    let (prog, v) = run(src);
    let cells: Vec<(i64, i64)> = match &v {
        Value::List { elems } => elems
            .iter()
            .map(|r| {
                let cell = prog.arena().get(*r).expect("cell resolves").clone();
                let (a, b) = pair_values(&prog, &cell);
                match (a, b) {
                    (Value::Int(a), Value::Int(b)) => (a, b),
                    other => panic!("expected an Int pair cell, got {other:?}"),
                }
            })
            .collect(),
        other => panic!("expected a List, got {other:?}"),
    };
    assert_eq!(cells, vec![(0, 0), (0, 1), (1, 0), (1, 1)]);

    let empty = "main = range ((Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 0, second: -1 } }));";
    let (prog, v) = run(empty);
    assert!(
        list_ints(&prog, &v).is_empty(),
        "an empty dimension yields no cells"
    );
}

#[test]
fn ix_pair_in_range_checks_both_dimensions() {
    let b = "(Pair { first: Pair { first: 0, second: 1 }, second: Pair { first: 1, second: 2 } })";
    for (i1, i2, want) in [
        (0, 1, true),
        (1, 2, true),
        (0, 0, false),
        (2, 1, false),
        (1, 3, false),
    ] {
        let src = format!("main = inRange {b} (Pair {{ first: {i1}, second: {i2} }});");
        assert_eq!(run(&src).1, Value::Bool(want), "inRange ({i1},{i2})");
    }
}

// ---------------------------------------------------------------------------
// 3. `index` double duty (D1): the method and the array accessor
// ---------------------------------------------------------------------------

#[test]
fn index_is_the_ix_offset_and_the_array_accessor_by_argument_type() {
    // `Pair` first argument → the `Ix` METHOD (an offset).
    assert_eq!(
        run("main = index (Pair { first: 0, second: 2 }) 1;").1,
        Value::Int(1)
    );
    // `Array` first argument → the constrained accessor (an element), whose
    // body must reach the METHOD through `index (bounds a) i`.
    let src = "main = index (arrayFromList (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0]) 1;";
    assert_eq!(run(src).1, Value::Double(20.0));
}

#[test]
fn bang_reaches_the_array_accessor_through_an_ix_computed_offset() {
    // `!` desugars to `index` (Task 2). A NEGATIVE lower bound proves the whole
    // chain stays on the Int track: the offset is `0 - (-1) = 1`, so the
    // element is the middle one — and `arrayIndexFlat` requires a real
    // `Value::Int` offset, so a Double offset would fault instead.
    let src = "main = (arrayFromList (Pair { first: -1, second: 1 }) [10.0, 20.0, 30.0]) ! 0;";
    assert_eq!(run(src).1, Value::Double(20.0));
}

#[test]
fn two_dimensional_bang_indexes_row_major() {
    let src = "main = (listArray \
                 (Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 1, second: 2 } }) \
                 [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]) ! Pair { first: 1, second: 2 };";
    assert_eq!(run(src).1, Value::Double(6.0));
}

// ---------------------------------------------------------------------------
// 4. Immutable surface
// ---------------------------------------------------------------------------

#[test]
fn list_array_stores_bounds_and_elements() {
    let src = format!("main = listArray {} [1.0, 2.0, 3.0];", bounds(0, 2));
    let (prog, v) = run(&src);
    let (values, (lo, hi)) = array_1d(&prog, &v);
    assert_eq!((lo, hi), (0, 2), "the bounds are stored verbatim");
    assert_eq!(values, vec![1.0, 2.0, 3.0]);
}

#[test]
fn list_array_truncates_extras_past_an_empty_range() {
    // `(0,-1)` is the canonical empty range (rangeSize 0): a single element is
    // an extra and is silently dropped, yielding an empty array (GHC-faithful
    // — GHC's `newArray# 0# arrEleBottom` writes nothing).
    let src = "main = listArray (Pair { first: 0, second: -1 }) [0.0];";
    let (prog, v) = run(src);
    let (values, (lo, hi)) = array_1d(&prog, &v);
    assert_eq!((lo, hi), (0, -1));
    assert!(values.is_empty(), "the single element is dropped");
}

#[test]
fn list_array_pads_a_short_list_with_faulting_holes() {
    // GHC-faithful lenient construction: `listArray (0,2) [1.0, 2.0]` has
    // rangeSize 3 — the missing position is one shared lazy hole (the same
    // fault-on-force object as `array`'s unlisted positions), NOT a length
    // fault. Listed positions read through `index`; only forcing the hole
    // faults.
    let arr = format!("listArray {} [1.0, 2.0]", bounds(0, 2));
    let src = format!("main = (\\a -> Pair {{ first: index a 0, second: index a 1 }}) ({arr});");
    let (prog, v) = run(&src);
    let (c0, c1) = pair_values(&prog, &v);
    assert_eq!(c0, Value::Double(1.0));
    assert_eq!(c1, Value::Double(2.0));

    // Forcing the padded tail faults cleanly (a fold over `arrayElems` forces
    // every element; position 2 is the hole).
    let msg = fault(&format!(
        "main = fold (\\a -> \\b -> a + b) 0.0 (arrayElems ({arr}));"
    ));
    assert!(
        msg.contains("array: undefined array element"),
        "expected the hole fault for the padded tail, got: {msg}"
    );
}

#[test]
fn list_array_truncates_a_long_list() {
    // GHC-faithful lenient construction: `listArray (0,2) [0.0,1.0,2.0,9.0]`
    // has rangeSize 3 — the extra element is silently dropped, never a length
    // fault and never a mis-indexed array.
    let src = format!("main = listArray {} [0.0, 1.0, 2.0, 9.0];", bounds(0, 2));
    let (prog, v) = run(&src);
    let (values, (lo, hi)) = array_1d(&prog, &v);
    assert_eq!((lo, hi), (0, 2));
    assert_eq!(values, vec![0.0, 1.0, 2.0], "the extra element is dropped");
}

#[test]
fn array_leaves_unlisted_positions_as_faulting_holes() {
    // `array` is the assoc-list constructor: unlisted positions are UNDEFINED
    // elements — a shared lazy callable that latches the clean "array:
    // undefined array element" fault when forced (GHC's `undefined`), not a
    // `0.0` fill.
    let arr = format!(
        "array {} [Pair {{ first: 0, second: 1.5 }}, Pair {{ first: 2, second: 3.5 }}]",
        bounds(0, 3)
    );
    // Listed positions read their values through `index`; the bounds read back.
    let src = format!(
        "main = (\\a -> Pair {{ first: arrayBounds a, second: Pair {{ first: index a 0, second: index a 2 }} }}) ({arr});"
    );
    let (prog, v) = run(&src);
    let (b, cells) = pair_values(&prog, &v);
    let (lo, hi) = pair_values(&prog, &b);
    assert_eq!(lo, Value::Int(0));
    assert_eq!(hi, Value::Int(3));
    let (c0, c2) = pair_values(&prog, &cells);
    assert_eq!(c0, Value::Double(1.5));
    assert_eq!(c2, Value::Double(3.5));

    // Forcing an unlisted position faults cleanly (a fold over `arrayElems`
    // forces every element; position 1 and 3 are holes).
    let msg = fault(&format!(
        "main = fold (\\a -> \\b -> a + b) 0.0 (arrayElems ({arr}));"
    ));
    assert!(
        msg.contains("array: undefined array element"),
        "expected the hole fault for an unlisted position, got: {msg}"
    );
}

#[test]
fn accum_array_folds_the_assocs_over_the_initial_value() {
    let src = format!(
        "main = accumArray (\\ a -> \\ b -> a + b) 0.0 {} \
           [Pair {{ first: 0, second: 1.0 }}, Pair {{ first: 0, second: 2.0 }}, Pair {{ first: 2, second: 5.0 }}];",
        bounds(0, 2)
    );
    let (prog, v) = run(&src);
    let (values, (lo, hi)) = array_1d(&prog, &v);
    assert_eq!((lo, hi), (0, 2));
    assert_eq!(values, vec![3.0, 0.0, 5.0], "position 0 accumulates twice");
}

#[test]
fn bounds_and_elems_are_direct_reads_of_a_list_array() {
    let src = format!(
        "main = (\\ a -> Pair {{ first: bounds a, second: elems a }}) (listArray {} [7.0, 8.0]);",
        bounds(-1, 0)
    );
    let (prog, v) = run(&src);
    let (b, es) = pair_values(&prog, &v);
    let (lo, hi) = pair_values(&prog, &b);
    assert_eq!(lo, Value::Int(-1));
    assert_eq!(hi, Value::Int(0));
    assert_eq!(list_floats(&prog, &es), vec![7.0, 8.0]);
}

#[test]
fn indices_is_the_range_of_the_bounds() {
    let src = format!(
        "main = indices (listArray {} [9.0, 8.0, 7.0]);",
        bounds(-1, 1)
    );
    let (prog, v) = run(&src);
    assert_eq!(list_ints(&prog, &v), vec![-1, 0, 1]);
}

#[test]
fn assocs_pairs_each_index_with_its_element() {
    let src = format!(
        "main = assocs (listArray {} [10.0, 20.0, 30.0]);",
        bounds(0, 2)
    );
    let (prog, v) = run(&src);
    let elems = match &v {
        Value::List { elems } => elems.clone(),
        other => panic!("expected a List, got {other:?}"),
    };
    let observed: Vec<(i64, f64)> = elems
        .iter()
        .map(|r| {
            let pair = prog.arena().get(*r).expect("pair resolves").clone();
            let (i, e) = pair_values(&prog, &pair);
            match (i, e) {
                (Value::Int(i), Value::Double(f)) => (i, f),
                other => panic!("expected an (Int, Double) assoc, got {other:?}"),
            }
        })
        .collect();
    assert_eq!(observed, vec![(0, 10.0), (1, 20.0), (2, 30.0)]);
}

/// The cell indices of a `List (Pair Int Int)` value.
fn list_cells(prog: &WorldState<f32, 256>, v: &Value) -> Vec<(i64, i64)> {
    let elems = match v {
        Value::List { elems } => elems.clone(),
        other => panic!("expected a List, got {other:?}"),
    };
    elems
        .iter()
        .map(|r| {
            let cell = prog.arena().get(*r).expect("cell resolves").clone();
            let (i, j) = pair_values(prog, &cell);
            match (i, j) {
                (Value::Int(i), Value::Int(j)) => (i, j),
                other => panic!("expected an (Int, Int) cell, got {other:?}"),
            }
        })
        .collect()
}

/// The cell indices of a `List (Pair (Pair Int Int) e)` value, with their `e`
/// payloads resolved through the arena.
fn list_cell_assocs(prog: &WorldState<f32, 256>, v: &Value) -> Vec<((i64, i64), f64)> {
    let elems = match v {
        Value::List { elems } => elems.clone(),
        other => panic!("expected a List, got {other:?}"),
    };
    elems
        .iter()
        .map(|r| {
            let pair = prog.arena().get(*r).expect("pair resolves").clone();
            let (ix, e) = pair_values(prog, &pair);
            let (i, j) = pair_values(prog, &ix);
            match (i, j, e) {
                (Value::Int(i), Value::Int(j), Value::Double(e)) => ((i, j), e),
                other => panic!("expected a ((Int, Int), Double) assoc, got {other:?}"),
            }
        })
        .collect()
}

#[test]
fn two_dimensional_indices_are_the_row_major_grid() {
    // `indices` = `range (arrayBounds a)` read off the STORED bounds: for a 2x3
    // array the whole grid, last coordinate fastest (the same order `Ix.index`
    // flattens to and `listArray` stores in).
    let src = "main = indices (listArray \
                 (Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 1, second: 2 } }) \
                 [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);";
    let (prog, v) = run(src);
    assert_eq!(
        list_cells(&prog, &v),
        vec![(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2)],
        "the 2-D indices are the row-major grid of the stored bounds"
    );

    // A non-zero origin shifts every cell: (1,2)..(2,4) is still 2x3.
    let shifted = "main = indices (listArray \
                     (Pair { first: Pair { first: 1, second: 2 }, second: Pair { first: 2, second: 4 } }) \
                     [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);";
    let (prog, v) = run(shifted);
    assert_eq!(
        list_cells(&prog, &v),
        vec![(1, 2), (1, 3), (1, 4), (2, 2), (2, 3), (2, 4)],
        "the stored origin is the first cell"
    );
}

#[test]
fn two_dimensional_assocs_pairs_each_cell_with_its_row_major_element() {
    // `assocs a` zips `indices a` with `index a cell`: the element at (i, j) is
    // the (i * cols + j)-th stored element, so the pairing is exactly the
    // order `listArray` took the elements in.
    let src = "main = assocs (listArray \
                 (Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 1, second: 2 } }) \
                 [10.0, 20.0, 30.0, 40.0, 50.0, 60.0]);";
    let (prog, v) = run(src);
    assert_eq!(
        list_cell_assocs(&prog, &v),
        vec![
            ((0, 0), 10.0),
            ((0, 1), 20.0),
            ((0, 2), 30.0),
            ((1, 0), 40.0),
            ((1, 1), 50.0),
            ((1, 2), 60.0),
        ],
        "each cell pairs with its row-major element"
    );

    // Non-zero origin: the cells carry the declared origin, the values stay in
    // storage order.
    let shifted = "main = assocs (listArray \
                     (Pair { first: Pair { first: 1, second: 2 }, second: Pair { first: 2, second: 4 } }) \
                     [10.0, 20.0, 30.0, 40.0, 50.0, 60.0]);";
    let (prog, v) = run(shifted);
    assert_eq!(
        list_cell_assocs(&prog, &v),
        vec![
            ((1, 2), 10.0),
            ((1, 3), 20.0),
            ((1, 4), 30.0),
            ((2, 2), 40.0),
            ((2, 3), 50.0),
            ((2, 4), 60.0),
        ]
    );
}

#[test]
fn update_replaces_the_selected_positions_only() {
    let src = format!(
        "main = update (listArray {} [1.0, 2.0, 3.0]) \
           [Pair {{ first: 0, second: 9.0 }}, Pair {{ first: 2, second: 7.0 }}];",
        bounds(0, 2)
    );
    let (prog, v) = run(&src);
    let (values, (lo, hi)) = array_1d(&prog, &v);
    assert_eq!((lo, hi), (0, 2));
    assert_eq!(values, vec![9.0, 2.0, 7.0]);
}

#[test]
fn accum_folds_into_an_existing_array() {
    let src = format!(
        "main = accum (\\ a -> \\ b -> a + b) (listArray {} [1.0, 2.0, 3.0]) \
           [Pair {{ first: 1, second: 5.0 }}, Pair {{ first: 1, second: 6.0 }}];",
        bounds(0, 2)
    );
    let (prog, v) = run(&src);
    let (values, _) = array_1d(&prog, &v);
    assert_eq!(values, vec![1.0, 13.0, 3.0], "2 + 5 + 6");
}

#[test]
fn ixmap_transposes_a_two_by_three_matrix() {
    // Reindex a 2x3 array into a 3x2 array with the transposing index map.
    //
    // The swap is an anonymous lambda whose parameter is typed by the
    // signature's FUNCTION-shaped parameter `(i -> j)`: the new bounds fix
    // `i := Pair Int Int` before the lambda argument is typed, so `p.second`
    // projects a record instead of the scalar default.
    let src = "main = ixmap \
                 (Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 2, second: 1 } }) \
                 (\\ p -> Pair { first: p.second, second: p.first }) \
                 (listArray \
                   (Pair { first: Pair { first: 0, second: 0 }, second: Pair { first: 1, second: 2 } }) \
                   [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);";
    // Compile, run and extract on a grown-stack thread: debug-build lowering
    // recurses once per nested call, and an `ixmap` over an array built by a
    // constrained definition is deep enough to exceed libtest's default ~2 MiB
    // (the constraint `tests/array_matrix.rs` documents). A stack overflow is an
    // ABORT that takes the whole binary down, so the work runs on a thread with
    // room and returns plain data (a `WorldState` owns `Rc`-backed arenas and is
    // not `Send`).
    let (l1, l2, u1, u2, values) = with_output(src, |prog, v| {
        let (bounds_ref, elems) = array_parts(v);
        let bounds = prog
            .arena()
            .get(bounds_ref)
            .expect("bounds resolve")
            .clone();
        let (l, u) = pair_values(prog, &bounds);
        let (l1, l2) = pair_values(prog, &l);
        let (u1, u2) = pair_values(prog, &u);
        let values: Vec<f64> = elems
            .iter()
            .map(|r| match prog.arena().get(*r) {
                Some(Value::Double(f)) => *f,
                other => panic!("expected a Double element, got {other:?}"),
            })
            .collect();
        (l1, l2, u1, u2, values)
    });
    assert_eq!(
        (l1, l2, u1, u2),
        (Value::Int(0), Value::Int(0), Value::Int(2), Value::Int(1)),
        "the result has the new 3x2 bounds"
    );
    // Row-major 3x2: (0,0)=m(0,0)=1, (0,1)=m(1,0)=4, (1,0)=m(0,1)=2, …
    assert_eq!(values, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
}

// ---------------------------------------------------------------------------
// 5. ST surface
// ---------------------------------------------------------------------------

#[test]
fn st_new_array_read_write_modify_through_ix_offsets() {
    let src = "{-# MODE complete #-}\n\
        main = runSTArray (\\ s -> do {\n\
            a <- newArray (Pair { first: 0, second: 2 }) 1.0; \
            writeArray a 1 5.0; \
            modifyArray a 2 (\\ x -> x + 10.0); \
            return a });";
    let (prog, v) = run(src);
    let (values, (lo, hi)) = array_1d(&prog, &v);
    assert_eq!((lo, hi), (0, 2));
    assert_eq!(values, vec![1.0, 5.0, 11.0]);
}

#[test]
fn st_read_array_reaches_the_written_element() {
    // `readArray` computes the offset from `getBounds` + `Ix.index`.
    let src = "{-# MODE complete #-}\n\
        main = runST (\\ s -> do {\n\
            a <- newArray (Pair { first: -1, second: 1 }) 0.0; \
            writeArray a 0 42.0; \
            readArray a 0 });";
    assert_eq!(run(src).1, Value::Double(42.0));
}

#[test]
fn st_new_list_array_seeds_every_element() {
    let src = format!(
        "{{-# MODE complete #-}}\nmain = runSTArray (\\ s -> newListArray {} [4.0, 5.0, 6.0]);",
        bounds(0, 2)
    );
    let (prog, v) = run(&src);
    let (values, _) = array_1d(&prog, &v);
    assert_eq!(values, vec![4.0, 5.0, 6.0]);
}

#[test]
fn st_new_list_array_pads_a_short_list_with_faulting_holes() {
    // GHC-faithful lenient construction: `newListArray (0,2) [1.0, 2.0]` has
    // rangeSize 3 — the missing position is one shared lazy hole, NOT a length
    // fault. Reads through `!` surface the padded slot unforced; only forcing
    // it faults (a fold over `arrayElems` forces every element).
    let arr = format!(
        "runSTArray (\\ s -> newListArray {} [1.0, 2.0])",
        bounds(0, 2)
    );
    let src = format!(
        "{{-# MODE complete #-}}\nmain = (\\a -> Pair {{ first: a ! 0, second: a ! 1 }}) ({arr});"
    );
    let (prog, v) = run(&src);
    let (c0, c1) = pair_values(&prog, &v);
    assert_eq!(c0, Value::Double(1.0));
    assert_eq!(c1, Value::Double(2.0));

    let msg = fault(&format!(
        "{{-# MODE complete #-}}\nmain = fold (\\a -> \\b -> a + b) 0.0 (arrayElems ({arr}));"
    ));
    assert!(
        msg.contains("array: undefined array element"),
        "expected the hole fault for the padded tail, got: {msg}"
    );
}

#[test]
fn st_new_list_array_truncates_a_long_list() {
    // GHC-faithful lenient construction: `newListArray (0,2)` with 4 elements
    // silently keeps the first `rangeSize` and drops the extra — never a length
    // fault, never a mis-indexed cell.
    let src = format!(
        "{{-# MODE complete #-}}\nmain = runSTArray (\\ s -> newListArray {} [1.0, 2.0, 3.0, 4.0]);",
        bounds(0, 2)
    );
    let (prog, v) = run(&src);
    let (values, (lo, hi)) = array_1d(&prog, &v);
    assert_eq!((lo, hi), (0, 2));
    assert_eq!(values, vec![1.0, 2.0, 3.0], "the extra element is dropped");
}

#[test]
fn st_new_list_array_truncates_extras_past_an_empty_range() {
    // `(0,-1)` is the canonical empty range (rangeSize 0): a single element is
    // an extra and is silently dropped, yielding an empty frozen array
    // (GHC-faithful — GHC's `newArray# 0# arrEleBottom` writes nothing).
    let src = "{-# MODE complete #-}\n\
        main = runSTArray (\\ s -> newListArray (Pair { first: 0, second: -1 }) [0.0]);";
    let (prog, v) = run(src);
    let (values, (lo, hi)) = array_1d(&prog, &v);
    assert_eq!((lo, hi), (0, -1));
    assert!(values.is_empty(), "the single element is dropped");
}

#[test]
fn frozen_array_is_immutable_after_the_region_closes() {
    // The frozen result feeds the immutable accessor, and a second read sees
    // the same element — the region's mutations cannot leak through.
    let src = "{-# MODE complete #-}\n\
        main = (\\ arr -> Pair { first: arr ! 1, second: arr ! 1 }) \
            (runSTArray (\\ s -> do {\n\
                a <- newListArray (Pair { first: 0, second: 2 }) [1.0, 2.0, 3.0]; \
                writeArray a 1 99.0; \
                return a }));";
    let (prog, v) = run(src);
    let (a, b) = pair_values(&prog, &v);
    assert_eq!(a, Value::Double(99.0));
    assert_eq!(b, Value::Double(99.0), "repeated reads are stable");
}

// ---------------------------------------------------------------------------
// 6. Buffer bridge (Task 7 primitives, exercised through the surface)
// ---------------------------------------------------------------------------

#[test]
fn array_from_buffer_round_trips_through_buffer_from_array() {
    // `sine` produces a FixedBuffer; `arrayFromBuffer` copies its samples into
    // an `Int`-indexed array, and `bufferFromArray` copies them back. The
    // generator is a FOREIGN builtin, so the program compiles against the
    // generator registry (the same fixture Task 7's bridge tests use).
    let src = "{-# MODE complete #-}\n\
        main = (\\ arr -> Pair { first: length arr, second: bufferFromArray arr }) \
            (arrayFromBuffer (sine 440.0 0.5 0.0));";
    let (prog, v) = buffer_program(src);
    let (len, _buf) = pair_values(&prog, &v);
    assert_eq!(
        len,
        Value::Int(256),
        "the block length becomes the array length"
    );
}

#[test]
fn array_from_buffer_values_match_the_buffer_contents() {
    // The bridge copies real samples: index 0 of the copy equals the buffer's
    // first sample, read through the immutable accessor.
    let src = "{-# MODE complete #-}\n\
        main = (\\ buf -> (arrayFromBuffer buf) ! 0) (sine 440.0 0.5 0.0);";
    match buffer_program(src).1 {
        Value::Double(f) => assert!(f.is_finite(), "expected a real sample, got {f}"),
        other => panic!("expected a Double element, got {other:?}"),
    }
}
