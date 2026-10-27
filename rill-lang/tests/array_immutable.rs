//! Immutable `Data.Array` language surface and its FAULT POLICY.
//!
//! The subject here is the immutable surface as a *user* reaches it — `!` /
//! `index`, `listArray`, `array`, `update`, `accum`, `accumArray`, `ixmap`,
//! `bounds`, `elems` — with the fault policy as the distinguishing theme:
//! every out-of-range reference must latch a `ProcessError::Processing` (the
//! design's clean fault channel), never panic and never silently read a
//! neighbouring cell.
//!
//! Deliberately NOT duplicated here (already pinned by other targets):
//! - the `Ix` offset/range/size arithmetic and the 1-D storage assertions
//!   (`tests/ix.rs`, `tests/array_storage.rs`);
//! - the ST surface and the freeze rules (`tests/st_array.rs`, `tests/ix.rs`);
//! - the flat primitives' own fault messages (`tests/array_storage.rs`).
//!
//! This target asserts the SHAPES (`2-D` bounds/elements through the surface)
//! and the fault surface those targets do not cover.
pub mod common;

use common::*;
use rill_lang::arena::{ArenaRef, Value};
use rill_lang::ffi::ForeignRegistry;
use rill_lang::program::WorldState;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compile `src`, run it once, and return the program (and its single result).
fn run(src: &str) -> (WorldState<f32, 256>, Value) {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ForeignRegistry::<f32>::new();
    let prog = eval_ffi(defs, expr, ffi).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    let v = output(&prog);
    (prog, v)
}

/// Compile `src`, run it once, and extract data on a thread with more stack.
///
/// Nested `ixmap` calls build deep lowering chains in debug builds, so the
/// default libtest stack can overflow. Keep the program inside the thread (it
/// is not `Send`) and extract plain data instead.
fn with_output<R: Send>(src: &str, f: impl FnOnce(&WorldState<f32, 256>, &Value) -> R + Send) -> R {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn_scoped(scope, || {
                let (defs, expr) = src
                    .rsplit_once("main = ")
                    .expect("the source must bind `main` to the evaluated expression");
                let expr = expr.trim().trim_end_matches(';');
                let ffi = ForeignRegistry::<f32>::new();
                let prog = eval_ffi(defs, expr, ffi)
                    .unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
                let v = output(&prog);
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

/// Run `src` expecting the latched value-track fault, and return its detail.
///
/// A latched fault surfaces from [`test_eval`] as
/// `CompileError::Unsupported("eval run failed: …")` with the `ProcessError`
/// text preserved inside the string; a panic in the interpreter fails this
/// helper outright (the harness reports the panic), so every caller doubles as
/// the "clean error, never a panic" assertion.
fn fault(src: &str) -> String {
    // Same grown-stack thread as [`with_output`]: a nested `ixmap` builds a
    // deep lowering chain in debug builds, and the fault path compiles the
    // program before it fails.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn_scoped(scope, || {
                let (defs, expr) = src
                    .rsplit_once("main = ")
                    .expect("the source must bind `main` to the evaluated expression");
                let expr = expr.trim().trim_end_matches(';');
                let ffi = ForeignRegistry::<f32>::new();
                match eval_ffi(defs, expr, ffi) {
                    Ok(_) => panic!("expected a fault, but the program ran\nsrc:\n{src}"),
                    Err(rill_lang::CompileError::Unsupported(msg)) => msg,
                    Err(other) => {
                        panic!("expected a latched processing fault, got {other:?}\nsrc:\n{src}")
                    }
                }
            })
            .expect("the compile thread must spawn")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

/// The two field VALUES of a `Pair` record, resolved through the arena.
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

fn int_pair(prog: &WorldState<f32, 256>, v: &Value) -> (i64, i64) {
    match pair_values(prog, v) {
        (Value::Int(lo), Value::Int(hi)) => (lo, hi),
        other => panic!("expected an Int pair, got {other:?}"),
    }
}

/// The element refs of an `Array` value.
fn array_elems(v: &Value) -> Vec<ArenaRef> {
    match v {
        Value::Array { elems, .. } => elems.clone(),
        other => panic!("expected an Array, got {other:?}"),
    }
}

/// `(flat elements, ((lo1, hi1), (lo2, hi2)))` of a matrix-shaped `Array`.
#[allow(clippy::type_complexity)]
fn matrix(prog: &WorldState<f32, 256>, v: &Value) -> (Vec<f64>, ((i64, i64), (i64, i64))) {
    let elems = array_elems(v);
    let values = elems
        .iter()
        .map(|r| match prog.arena().get(*r) {
            Some(Value::Double(f)) => *f,
            other => panic!("expected a Double element, got {other:?}"),
        })
        .collect();
    let bounds = match v {
        Value::Array { bounds, .. } => prog.arena().get(*bounds).expect("bounds resolve").clone(),
        _ => unreachable!(),
    };
    let (low, high) = pair_values(prog, &bounds);
    (values, (int_pair(prog, &low), int_pair(prog, &high)))
}

/// `(flat elements, (lo, hi))` of a 1-D `Array`.
fn vector(prog: &WorldState<f32, 256>, v: &Value) -> (Vec<f64>, (i64, i64)) {
    let elems = array_elems(v);
    let values = elems
        .iter()
        .map(|r| match prog.arena().get(*r) {
            Some(Value::Double(f)) => *f,
            other => panic!("expected a Double element, got {other:?}"),
        })
        .collect();
    let bounds = match v {
        Value::Array { bounds, .. } => prog.arena().get(*bounds).expect("bounds resolve").clone(),
        _ => unreachable!(),
    };
    (values, int_pair(prog, &bounds))
}

/// A 2-D bounds expression `((r1, c1), (r2, c2))`, parenthesized so it stays
/// one argument in a juxtaposition application.
fn b2(r1: i64, c1: i64, r2: i64, c2: i64) -> String {
    format!(
        "(Pair {{ first: Pair {{ first: {r1}, second: {c1} }}, \
          second: Pair {{ first: {r2}, second: {c2} }} }})"
    )
}

/// A bounds expression for the 3x3 matrix `M3`.
fn m3_bounds() -> String {
    b2(0, 0, 2, 2)
}

/// The value expression of a 3x3 matrix holding 1.0..=9.0 (row-major).
fn m3() -> String {
    format!(
        "(listArray {} [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0])",
        m3_bounds()
    )
}

/// A `Pair Int Int` cell literal.
fn cell(i: i64, j: i64) -> String {
    format!("(Pair {{ first: {i}, second: {j} }})")
}

/// A `(cell, value)` association literal in the surface's `Pair i e` shape.
///
/// The value is formatted with `Debug` so it always carries a decimal point
/// (`10.0`, not `10`): an integer-looking literal would type as `Int` and a
/// `List (Pair i Double)` association list would not unify.
fn assoc(i: i64, j: i64, v: f64) -> String {
    format!("Pair {{ first: {}, second: {v:?} }}", cell(i, j))
}

/// Run `src` and return its `Double` value output.
fn float_of(src: &str) -> f64 {
    let (_prog, v) = run(src);
    match v {
        Value::Double(f) => f,
        other => panic!("expected a Double, got {other:?}\nsrc:\n{src}"),
    }
}

// ---------------------------------------------------------------------------
// 1. Fault policy — out-of-range reads
// ---------------------------------------------------------------------------

#[test]
fn bang_out_of_range_is_a_clean_fault_not_a_panic() {
    let arr = "(listArray (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0])";
    // Above the upper bound and below the lower bound: both are out of range.
    for i in ["3", "-1"] {
        let msg = fault(&format!("main = {arr} ! {i};"));
        assert!(
            msg.contains("arrayIndexFlat") && msg.contains("out of range"),
            "expected the clean out-of-range fault for `! {i}`, got: {msg}"
        );
    }
}

#[test]
fn the_named_index_accessor_faults_out_of_range_like_bang() {
    let arr = "(listArray (Pair { first: 0, second: 2 }) [10.0, 20.0, 30.0])";
    for i in ["3", "-1"] {
        let msg = fault(&format!("main = index {arr} {i};"));
        assert!(
            msg.contains("out of range"),
            "expected the named accessor to fault for index {i}, got: {msg}"
        );
    }
}

#[test]
fn bang_out_of_range_below_a_non_zero_origin_is_a_clean_fault() {
    // Bounds 2..=4: the offsets of 1 and 5 are -1 and 3, both outside the
    // stored element range. The origin must not turn them into in-range reads.
    let arr = "(listArray (Pair { first: 2, second: 4 }) [10.0, 20.0, 30.0])";
    for i in ["1", "5"] {
        let msg = fault(&format!("main = {arr} ! {i};"));
        assert!(
            msg.contains("out of range"),
            "expected the clean out-of-range fault for `! {i}` on 2..=4, got: {msg}"
        );
    }
}

#[test]
fn two_dimensional_bang_out_of_range_is_a_clean_fault() {
    // Every way a cell can leave a 3x3 grid. The column cases are the
    // interesting ones: a row-major flat offset of `r * cols + c` stays inside
    // the element array for a positive column overflow, so the cell — not just
    // the flat offset — has to be rejected.
    for (i, j) in [(3, 0), (-1, 0), (0, 3), (0, -1), (2, 3)] {
        let msg = fault(&format!("main = {} ! {};", m3(), cell(i, j)));
        assert!(
            msg.contains("out of range"),
            "expected the clean out-of-range fault for cell ({i},{j}), got: {msg}"
        );
    }
}

#[test]
fn two_dimensional_index_accessor_faults_out_of_range_like_bang() {
    // The named accessor shares the accessor body with `!`, so it must reject
    // the same cells — for both a `Pair Int Int` index and a column overflow.
    let msg = fault(&format!("main = index {} {};", m3(), cell(0, 3)));
    assert!(
        msg.contains("out of range"),
        "expected the named accessor to fault for cell (0,3), got: {msg}"
    );
}

#[test]
fn an_empty_matrix_dimension_reads_as_empty_and_access_faults() {
    // `(0,0)..(1,-1)` has an empty column span: zero elements, and any cell is
    // out of range rather than a wrapped read.
    let empty = format!("(listArray {} [])", b2(0, 0, 1, -1));
    let (_prog, elems) = run(&format!("main = elems {empty};"));
    match elems {
        Value::List { elems } => assert!(elems.is_empty(), "an empty span has no elements"),
        other => panic!("expected the element list, got {other:?}"),
    }
    let (prog, bounds) = run(&format!("main = bounds {empty};"));
    let (low, high) = pair_values(&prog, &bounds);
    assert_eq!(int_pair(&prog, &low), (0, 0));
    assert_eq!(int_pair(&prog, &high), (1, -1));

    let msg = fault(&format!("main = {empty} ! {};", cell(0, 0)));
    assert!(
        msg.contains("out of range"),
        "expected the clean out-of-range fault on an empty matrix, got: {msg}"
    );
}

#[test]
fn two_dimensional_list_array_pads_or_truncates_to_the_range_size() {
    // 2x2 = 4 cells (b2(0,0,1,1)): a 3-element list is SHORT — the missing
    // cell is one shared lazy hole (fault-on-force, GHC-faithful); a 5-element
    // list is LONG — the extra is silently dropped, yielding 4 elements.
    let with_3 = format!("main = listArray {} [0.0, 1.0, 2.0];", b2(0, 0, 1, 1));
    let (prog, v) = run(&with_3);
    // The padded tail is a hole (a `Callable`), not a Double — assert the flat
    // element count without forcing it.
    assert_eq!(array_elems(&v).len(), 4, "padded to the 4-cell size");
    let bounds = match &v {
        Value::Array { bounds, .. } => prog.arena().get(*bounds).expect("bounds resolve").clone(),
        _ => unreachable!(),
    };
    let (low, high) = pair_values(&prog, &bounds);
    assert_eq!(
        (int_pair(&prog, &low), int_pair(&prog, &high)),
        ((0, 0), (1, 1))
    );
    // The padded tail is a hole: a fold over `arrayElems` forces every element.
    let msg = fault(&format!(
        "main = fold (\\a -> \\b -> a + b) 0.0 (arrayElems (listArray {} [0.0, 1.0, 2.0]));",
        b2(0, 0, 1, 1)
    ));
    assert!(
        msg.contains("array: undefined array element"),
        "expected the hole fault at the padded cell, got: {msg}"
    );

    let with_5 = format!(
        "main = listArray {} [0.0, 1.0, 2.0, 3.0, 4.0];",
        b2(0, 0, 1, 1)
    );
    let (prog, v) = run(&with_5);
    let (values, _) = matrix(&prog, &v);
    assert_eq!(
        values,
        vec![0.0, 1.0, 2.0, 3.0],
        "the 5th element is dropped"
    );

    // 3x2 = 6 cells (b2(0,0,2,1)): a 5-element list is SHORT (tail hole, not a
    // Double); a 6-element list is exact.
    let with_5_3x2 = format!(
        "main = listArray {} [0.0, 1.0, 2.0, 3.0, 4.0];",
        b2(0, 0, 2, 1)
    );
    let (_, v) = run(&with_5_3x2);
    assert_eq!(array_elems(&v).len(), 6, "padded to the 6-cell size");

    let with_6 = format!(
        "main = listArray {} [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];",
        b2(0, 0, 2, 1)
    );
    let (prog, v) = run(&with_6);
    let (values, _) = matrix(&prog, &v);
    assert_eq!(values, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
}

#[test]
fn array_update_and_accum_array_reject_out_of_range_association_cells() {
    // A 2x2 grid addressed by cell (2,0): the row-major offset is 4, one past
    // the last element, so the association is rejected cleanly.
    let msg = fault(&format!(
        "main = array {} [{}];",
        b2(0, 0, 1, 1),
        assoc(2, 0, 1.5)
    ));
    assert!(
        msg.contains("arrayFromAssocs") && msg.contains("out of range"),
        "expected the clean out-of-range fault for an out-of-grid `array` assoc, got: {msg}"
    );

    let msg = fault(&format!("main = update {} [{}];", m3(), assoc(5, 0, 1.5)));
    assert!(
        msg.contains("arrayUpdate") && msg.contains("out of range"),
        "expected the clean out-of-range fault for an out-of-grid `update` cell, got: {msg}"
    );

    // A column past the last one in a 3x2 grid: the flat offset of (0,2) is 2,
    // which is INSIDE the 6-element storage — only the cell check catches it.
    let msg = fault(&format!(
        "main = accumArray (\\ a -> \\ b -> a + b) 1.0 {} [{}];",
        b2(0, 0, 2, 1),
        assoc(0, 2, 10.0)
    ));
    assert!(
        msg.contains("out of range"),
        "expected the clean out-of-range fault for an out-of-grid accumArray assoc, got: {msg}"
    );
}

#[test]
fn accum_rejects_out_of_range_association_cells() {
    let msg = fault(&format!(
        "main = accum (\\ a -> \\ b -> a + b) {} [{}];",
        m3(),
        assoc(0, 3, 10.0)
    ));
    assert!(
        msg.contains("out of range"),
        "expected the clean out-of-range fault for an accum assoc, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// 2. Matrix construction and the immutable operations
// ---------------------------------------------------------------------------

#[test]
fn matrix_bounds_and_elements_are_read_through_the_surface() {
    let (prog, v) = run(&format!("main = {};", m3()));
    let (values, (low, high)) = matrix(&prog, &v);
    assert_eq!(low, (0, 0));
    assert_eq!(high, (2, 2));
    assert_eq!(values, (1..=9).map(f64::from).collect::<Vec<_>>());
}

#[test]
fn update_on_a_matrix_replaces_the_selected_cells_only() {
    let src = format!(
        "main = update {} [{}, {}];",
        m3(),
        assoc(0, 0, 10.0),
        assoc(2, 1, 20.0)
    );
    let (prog, v) = run(&src);
    let (values, (low, high)) = matrix(&prog, &v);
    assert_eq!((low, high), ((0, 0), (2, 2)), "the shape is preserved");
    assert_eq!(
        values,
        vec![10.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 20.0, 9.0],
        "cell (0,0) and cell (2,1) take the new values, the rest is untouched"
    );
}

#[test]
fn update_keeps_the_last_association_for_a_repeated_cell() {
    let src = format!(
        "main = update {} [{}, {}];",
        m3(),
        assoc(1, 1, 5.0),
        assoc(1, 1, 7.0)
    );
    let (prog, v) = run(&src);
    let (values, _) = matrix(&prog, &v);
    assert_eq!(values[4], 7.0, "the second association wins");
}

#[test]
fn update_leaves_the_source_matrix_unchanged() {
    // The updated array is read FIRST and the original SECOND: an in-place
    // update would leak the new value into the original's read.
    let src = format!(
        "main = (\\arr -> Pair {{ first: (update arr [{}]) ! {}, \
          second: arr ! {} }}) {};",
        assoc(1, 1, 99.0),
        cell(1, 1),
        cell(1, 1),
        m3()
    );
    let (prog, v) = run(&src);
    let (updated, original) = pair_values(&prog, &v);
    assert_eq!(updated, Value::Double(99.0));
    assert_eq!(original, Value::Double(5.0), "the source keeps its element");
}

#[test]
fn accum_on_a_matrix_folds_the_associations_in_order() {
    let src = format!(
        "main = accum (\\ a -> \\ b -> a + b) {} [{}, {}, {}];",
        m3(),
        assoc(1, 1, 5.0),
        assoc(1, 1, 6.0),
        assoc(2, 2, 10.0)
    );
    let (prog, v) = run(&src);
    let (values, (low, high)) = matrix(&prog, &v);
    assert_eq!((low, high), ((0, 0), (2, 2)));
    assert_eq!(
        values,
        vec![1.0, 2.0, 3.0, 4.0, 16.0, 6.0, 7.0, 8.0, 19.0],
        "cell (1,1) accumulates twice (5 + 6 + its own 5)"
    );
}

#[test]
fn accum_array_builds_a_matrix_from_the_initial_value() {
    // 3x2 with initial 1.0: two associations land on two different cells, and
    // every other cell keeps the initial value.
    let src = format!(
        "main = accumArray (\\ a -> \\ b -> a + b) 1.0 {} [{}, {}];",
        b2(0, 0, 2, 1),
        assoc(0, 1, 10.0),
        assoc(1, 0, 100.0)
    );
    let (prog, v) = run(&src);
    let (values, (low, high)) = matrix(&prog, &v);
    assert_eq!((low, high), ((0, 0), (2, 1)));
    assert_eq!(
        values,
        vec![1.0, 11.0, 101.0, 1.0, 1.0, 1.0],
        "the initial value seeds every cell and the assocs fold on top"
    );
}

#[test]
fn array_builds_a_matrix_with_undefined_holes_for_unlisted_cells() {
    // `array` is element-polymorphic: unlisted positions are UNDEFINED elements
    // — a shared lazy callable that latches the clean "array: undefined array
    // element" fault when forced (GHC's `undefined`), not a `0.0` fill.
    let arr = format!(
        "array {} [{}, {}]",
        b2(0, 0, 1, 2),
        assoc(0, 1, 1.5),
        assoc(1, 2, 3.5)
    );
    // Bounds read back; the listed cells read their values through `index`.
    let src = format!(
        "main = (\\a -> Pair {{ first: arrayBounds a, second: Pair {{ first: index a {}, second: index a {} }} }}) ({arr});",
        cell(0, 1),
        cell(1, 2)
    );
    let (prog, v) = run(&src);
    let (bounds, cells) = pair_values(&prog, &v);
    let (low, high) = pair_values(&prog, &bounds);
    assert_eq!(int_pair(&prog, &low), (0, 0));
    assert_eq!(int_pair(&prog, &high), (1, 2));
    let (c1, c2) = pair_values(&prog, &cells);
    assert_eq!(c1, Value::Double(1.5));
    assert_eq!(c2, Value::Double(3.5));

    // Forcing an unlisted position faults cleanly (a fold over `arrayElems`
    // forces every element; the unlisted holes latch on the first push).
    let msg = fault(&format!(
        "main = fold (\\a -> \\b -> a + b) 0.0 (arrayElems ({arr}));"
    ));
    assert!(
        msg.contains("array: undefined array element"),
        "expected the hole fault for an unlisted matrix cell, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// 3. `ixmap` — reindexing between matrix shapes
// ---------------------------------------------------------------------------

#[test]
fn ixmap_transposes_a_three_by_two_matrix() {
    // 3x2 (rows 0..=2, cols 0..=1) reindexed into 2x3 with the transposing
    // index map. The sibling target covers 2x3 -> 3x2; this is the inverse
    // orientation, so an accidentally-symmetric implementation cannot pass
    // both.
    let src = format!(
        "main = ixmap {} (\\ p -> Pair {{ first: p.second, second: p.first }}) \
         (listArray {} [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);",
        b2(0, 0, 1, 2),
        b2(0, 0, 2, 1)
    );
    // `with_output` (the grown-stack helper): the program nests `ixmap` deep
    // enough that debug-build lowering needs more than libtest's default stack.
    let (values, (low, high)) = with_output(&src, matrix);
    assert_eq!((low, high), ((0, 0), (1, 2)), "the target bounds are 2x3");
    // Target (0,0)=s(0,0)=1, (0,1)=s(1,0)=3, (0,2)=s(2,0)=5, then the row 1.
    assert_eq!(values, vec![1.0, 3.0, 5.0, 2.0, 4.0, 6.0]);
}

#[test]
fn ixmap_with_shifted_bounds_reaches_the_source_cells_through_both_origins() {
    // Source 2x3 at origin (1,2), target 3x2 at origin (2,1) — the transpose of
    // the origin as well as of the shape, so the swap map alone lands on valid
    // source cells: target (i,j) -> source (j,i). Two `Ix` instances with
    // different origins are in play at once.
    //
    // The map stays a pure projection on purpose: arithmetic on a record
    // projection inside a λ-literal is not typeable today (`p.second - k` has
    // no Int operand to pin the literal), which is a general inference
    // limitation unrelated to arrays.
    let src = format!(
        "main = ixmap {} (\\ p -> Pair {{ first: p.second, second: p.first }}) \
         (listArray {} [10.0, 20.0, 30.0, 40.0, 50.0, 60.0]);",
        b2(2, 1, 4, 2),
        b2(1, 2, 2, 4)
    );
    let (values, (low, high)) = with_output(&src, matrix);
    assert_eq!((low, high), ((2, 1), (4, 2)), "the target origin is (2,1)");
    // Target row-major: (2,1)->s(1,2)=10, (2,2)->s(2,2)=40, (3,1)->s(1,3)=20, …
    assert_eq!(values, vec![10.0, 40.0, 20.0, 50.0, 30.0, 60.0]);
}

#[test]
fn ixmap_faults_when_the_index_map_leaves_the_source_bounds() {
    let msg = fault(&format!(
        "main = ixmap {} (\\ p -> Pair {{ first: 0, second: 3 }}) {};",
        b2(0, 0, 0, 0),
        m3()
    ));
    assert!(
        msg.contains("out of range"),
        "expected the clean out-of-range fault for an ixmap read, got: {msg}"
    );
}

#[test]
fn a_reindexed_matrix_is_indexable_and_remappable_without_a_binding() {
    // Composing the surface with itself at one call site: the accessor over an
    // `ixmap` result, and an `ixmap` over an `ixmap` result. Both are ordinary
    // nests — the inner call is the outer call's ARGUMENT, not a recursive
    // reference to the definition being expanded — so neither may be rejected
    // as a "recursive class-constrained definition".
    let transposed = format!(
        "(ixmap {} (\\ p -> Pair {{ first: p.second, second: p.first }}) {})",
        m3_bounds(),
        m3()
    );
    // Transposed (1,0) is the source's (0,1) = 2.0.
    let value = with_output(
        &format!("main = index {transposed} {};", cell(1, 0)),
        |_prog, v| match v {
            Value::Double(f) => *f,
            other => panic!("expected a Double, got {other:?}"),
        },
    );
    assert_eq!(value, 2.0);

    // `transpose . transpose` is the identity, read without ever binding the
    // intermediate array to a name.
    let twice = format!(
        "(ixmap {b} (\\ p -> Pair {{ first: p.second, second: p.first }}) \
         (ixmap {b} (\\ p -> Pair {{ first: p.second, second: p.first }}) {m}))",
        b = m3_bounds(),
        m = m3()
    );
    let (values, (low, high)) = with_output(&format!("main = {twice};"), matrix);
    assert_eq!((low, high), ((0, 0), (2, 2)));
    assert_eq!(values, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
}

// ---------------------------------------------------------------------------
// 4. The 1-D surface's fault path in vector form (the shape `ix.rs` leaves out)
// ---------------------------------------------------------------------------

#[test]
fn vector_update_and_accum_preserve_the_declared_origin() {
    // Non-zero origin: the offsets are relative to `2`, so cell 2 is the first
    // stored element and cell 4 the last.
    let arr = "(listArray (Pair { first: 2, second: 4 }) [10.0, 20.0, 30.0])";
    let (prog, v) = run(&format!(
        "main = accum (\\ a -> \\ b -> a + b) (update {arr} [Pair {{ first: 4, second: 1.0 }}]) \
           [Pair {{ first: 4, second: 30.0 }}];"
    ));
    let (values, (low, high)) = vector(&prog, &v);
    assert_eq!((low, high), (2, 4), "the origin survives both operations");
    assert_eq!(values, vec![10.0, 20.0, 31.0]);
}

// ---------------------------------------------------------------------------
// 5. The accessor in expression position, arrays as first-class values
// ---------------------------------------------------------------------------

#[test]
fn the_accessor_composes_inside_a_larger_expression() {
    // Every other assertion here reads one cell as the whole program. `!` is an
    // ordinary infix operator, so it must compose: two reads added together,
    // and a read whose index comes from a λ-parameter.
    assert_eq!(
        float_of(&format!(
            "main = ({} ! {}) + ({} ! {});",
            m3(),
            cell(0, 0),
            m3(),
            cell(1, 1)
        )),
        6.0
    );
    assert_eq!(
        float_of(&format!("main = (\\ i -> {} ! i) {};", m3(), cell(2, 0))),
        7.0
    );
}

#[test]
fn an_array_passes_through_a_function_as_a_value() {
    // Arrays are ordinary first-class values: a λ may take one and index it.
    // The parameter is typed `Array (Pair Int Int) Double` by the ARGUMENT, so
    // the accessor inside the body dispatches to the array definition rather
    // than to a numeric fallback.
    let src = format!("main = (\\ m -> m ! {}) {};", cell(1, 2), m3());
    assert_eq!(float_of(&src), 6.0);
}

#[test]
fn the_accessor_feeds_a_condition() {
    // The read participates in a Bool expression, so the accessor's result must
    // be a `Double` the comparison can consume — not a wrapped or deferred value.
    assert_eq!(
        float_of(&format!(
            "main = if 4.0 < ({} ! {}) then 1.0 else 0.0;",
            m3(),
            cell(1, 1)
        )),
        1.0
    );
    assert_eq!(
        float_of(&format!(
            "main = if 4.0 > ({} ! {}) then 1.0 else 0.0;",
            m3(),
            cell(0, 0)
        )),
        1.0
    );
    // Written from the other side, so a comparison that ignored the operand
    // order cannot pass both directions.
    assert_eq!(
        float_of(&format!(
            "main = if 4.0 > ({} ! {}) then 1.0 else 0.0;",
            m3(),
            cell(1, 1)
        )),
        0.0
    );
}

// NOTE: an `if` condition may not BEGIN with a parenthesis (`if (x) > y then`
// is a parse error, `if y < (x) then` is not) — a pre-existing general grammar
// limitation of `if`, unrelated to arrays. The conditions above are therefore
// written comparison-first.

// ---------------------------------------------------------------------------
// 6. Q1: custom index types — reads keep using the language `Ix` instance
// ---------------------------------------------------------------------------

// The documented Q1 split: READS (`listArray` + `!`) call the `Ix` instance
// methods in the language, so a USER instance with trivial bodies runs and
// computes the offsets; CONSTRUCTION (`array` = `arrayFromAssocs`) resolves the
// bounds shape in the INTERPRETER, which only knows the two prelude index
// shapes (`Int`, `Pair Int Int`) — so a custom index type faults cleanly
// ("unsupported index type") instead of being flattened.
#[test]
fn q1_user_ix_instances_keep_reads_working_but_array_construction_faults() {
    // A user `Ix (Pair Int (Pair Int Int))` with deliberately trivial bodies.
    // The read is driven entirely by these bodies: `listArray` takes its size
    // from `rangeSize` (1) and `!` takes its offset from `index` (0) through
    // `checkedOffset` — never from the interpreter's shape table.
    let ix = r#"
        instance Ix (Pair Int (Pair Int Int)) where {
            range b = [b.first];
            index b i = 0;
            inRange b i = true;
            rangeSize b = 1;
        }
    "#;
    let bounds = "Pair { first: Pair { first: 0, second: Pair { first: 0, second: 0 } }, second: Pair { first: 0, second: Pair { first: 0, second: 0 } } }";
    let index = "Pair { first: 0, second: Pair { first: 0, second: 0 } }";

    // Read: `listArray bounds [7.5]` demands `rangeSize bounds` == 1 element,
    // then `! index` reads flat offset `index bounds index` == 0 -> 7.5.
    let (_prog, v) = run(&format!(
        "{ix}\nbounds_ref = {bounds};\nix_ref = {index};\nmain = (listArray bounds_ref [7.5]) ! ix_ref;"
    ));
    assert_eq!(
        v,
        Value::Double(7.5),
        "the user Ix instance must drive the language-side read"
    );

    // Construction: the very same index type is out of the interpreter's shape
    // table, so `array` faults cleanly instead of flattening the custom index.
    let msg = fault(&format!(
        "{ix}\nbounds_ref = {bounds};\nmain = array bounds_ref [Pair {{ first: {index}, second: 9.0 }}];"
    ));
    assert!(
        msg.contains("arrayFromAssocs") && msg.contains("unsupported index type"),
        "expected the unsupported index type fault for a custom-index array, got: {msg}"
    );
}
