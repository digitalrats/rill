//! The reference DEMO for `Data.Array`: 2x2 and 3x3 `Array (Pair Int Int) Float`
//! matrices, addressed by `Pair Int Int` cells.
//!
//! Every assertion pins an actual VALUE, not just a shape: the matrices are the
//! canonical `[[1,2],[3,4]]` and `1..=9` grids, so a reader can check the demo by
//! eye. The four things the design's demo asks for are all here:
//!
//! 1. construction (`listArray`) and indexing (`!` / `index`, row-major),
//! 2. `assocs` — the cell/value pairing the storage order implies,
//! 3. transpose via `ixmap` (including its involution on a square matrix),
//! 4. accumulation via `accum` (fold into a matrix) and `accumArray` (build one).
//!
//! Structural equality/order on arrays is exercised at the end: `==`/`<` compare
//! bounds first and then the elements (the derived `Eq`/`Ord` path, no
//! hand-written instance needed), so `transpose . transpose == id` is checkable
//! inside the language.
//!
//! Deliberately NOT duplicated here (pinned by the sibling targets): the `Ix`
//! arithmetic and 1-D storage (`tests/ix.rs`, `tests/array_storage.rs`), the
//! fault policy and the per-function surface (`tests/array_immutable.rs`), and
//! the ST/buffer bridge (`tests/st_array.rs`, `tests/array_buffer.rs`).

use rill_lang::arena::{ArenaRef, Value};
use rill_lang::compile;
use rill_lang::program::RillProgram;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compile `src`, run one tick, and hand the program plus its single value
/// output to `f` — all on a thread with a generous stack.
///
/// # Why the dedicated thread
///
/// Lowering recurses once per nested call and scope, and a DEBUG-build
/// `lower_value_expected` frame is on the order of 80 KiB: the `ixmap` over
/// `ixmap` and the structural `==` chains this demo writes need more than
/// libtest's default ~2 MiB test thread. A stack overflow is an ABORT, not a
/// catchable panic — it would take the whole binary down and report a signal
/// instead of a diagnostic. `tests/arrow.rs` documents the same constraint for
/// the Arrow defaults and uses the same mitigation. The subject here is the
/// array surface, not stack arithmetic, so give the recursion room.
///
/// `RillProgram` owns an `Rc<RefCell<WorldState>>` and is therefore not
/// `Send`: the program cannot outlive the thread, so `f` extracts plain data
/// (numbers, bools, vectors) instead.
fn with_output<R: Send>(
    src: &str,
    f: impl FnOnce(&RillProgram<f32, 256>, &Value) -> R + Send,
) -> R {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn_scoped(scope, || {
                let mut prog = compile::<f32>(src)
                    .unwrap_or_else(|e| panic!("compile failed: {e}\nsrc:\n{src}"));
                prog.run_value()
                    .unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
                let v = output(&prog);
                f(&prog, &v)
            })
            .expect("the compile thread must spawn")
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
    })
}

/// Run `src` and return its single `Float` value (the demo's scalar results).
fn run_float(src: &str) -> f64 {
    with_output(src, |_prog, v| match v {
        Value::Float(f) => *f,
        other => panic!("expected a Float, got {other:?}\nsrc:\n{src}"),
    })
}

/// Run `src` and return its single `Bool` value.
fn run_bool(src: &str) -> bool {
    with_output(src, |_prog, v| match v {
        Value::Bool(b) => *b,
        other => panic!("expected a Bool, got {other:?}\nsrc:\n{src}"),
    })
}

/// Run `src` and read its value output as a matrix.
#[allow(clippy::type_complexity)]
fn matrix_of(src: &str) -> (Vec<f64>, ((i64, i64), (i64, i64))) {
    with_output(src, matrix)
}

/// Run `src` and read its value output as an `assocs` list.
fn assocs_of(src: &str) -> Vec<((i64, i64), f64)> {
    with_output(src, assoc_pairs)
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

fn int_pair(prog: &RillProgram<f32, 256>, v: &Value) -> (i64, i64) {
    match pair_values(prog, v) {
        (Value::Int(lo), Value::Int(hi)) => (lo, hi),
        other => panic!("expected an Int pair, got {other:?}"),
    }
}

/// `(flat elements, ((lo1, hi1), (lo2, hi2)))` of a matrix value.
#[allow(clippy::type_complexity)]
fn matrix(prog: &RillProgram<f32, 256>, v: &Value) -> (Vec<f64>, ((i64, i64), (i64, i64))) {
    let (bounds, elems) = match v {
        Value::Array { bounds, elems } => (*bounds, elems.clone()),
        other => panic!("expected an Array, got {other:?}"),
    };
    let values = elems
        .iter()
        .map(|r| match prog.arena().get(*r) {
            Some(Value::Float(f)) => *f,
            other => panic!("expected a Float element, got {other:?}"),
        })
        .collect();
    let bounds = prog.arena().get(bounds).expect("bounds resolve").clone();
    let (low, high) = pair_values(prog, &bounds);
    (values, (int_pair(prog, &low), int_pair(prog, &high)))
}

/// The `((row, col), value)` pairs of an `assocs` list.
fn assoc_pairs(prog: &RillProgram<f32, 256>, v: &Value) -> Vec<((i64, i64), f64)> {
    let elems: Vec<ArenaRef> = match v {
        Value::List { elems } => elems.clone(),
        other => panic!("expected a List, got {other:?}"),
    };
    elems
        .iter()
        .map(|r| {
            let pair = prog.arena().get(*r).expect("assoc resolves").clone();
            let (ix, e) = pair_values(prog, &pair);
            let (i, j) = int_pair(prog, &ix);
            match e {
                Value::Float(f) => ((i, j), f),
                other => panic!("expected a Float element, got {other:?}"),
            }
        })
        .collect()
}

/// A 0-origin 2-D bounds expression `((0,0),(rows-1,cols-1))`.
fn bounds(rows: i64, cols: i64) -> String {
    format!(
        "(Pair {{ first: Pair {{ first: 0, second: 0 }}, \
          second: Pair {{ first: {}, second: {} }} }})",
        rows - 1,
        cols - 1
    )
}

/// A `Pair Int Int` cell literal.
fn cell(i: i64, j: i64) -> String {
    format!("(Pair {{ first: {i}, second: {j} }})")
}

/// A `(cell, value)` association literal.
fn assoc(i: i64, j: i64, v: f64) -> String {
    format!("Pair {{ first: {}, second: {v:?} }}", cell(i, j))
}

/// A row-major `listArray` of a `rows x cols` matrix.
fn matrix_expr(rows: i64, cols: i64, values: &[f64]) -> String {
    let elems: Vec<String> = values.iter().map(|v| format!("{v:?}")).collect();
    format!("(listArray {} [{}])", bounds(rows, cols), elems.join(", "))
}

/// The square 2x2 demo matrix `[[1,2],[3,4]]`.
fn m2() -> String {
    matrix_expr(2, 2, &[1.0, 2.0, 3.0, 4.0])
}

/// The 3x3 demo matrix `1..=9`.
fn m3() -> String {
    matrix_expr(3, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0])
}

/// The transposing `ixmap` of a `size x size` 0-origin matrix expression.
///
/// Parenthesized: `!` and `==` must not be swallowed by the call's argument
/// list (`f a b c ! d` would read `c ! d` as the third argument).
fn transpose_expr(m: &str, size: i64) -> String {
    format!(
        "(ixmap {} (\\ p -> Pair {{ first: p.second, second: p.first }}) {m})",
        bounds(size, size)
    )
}

// ---------------------------------------------------------------------------
// 1. The 2x2 demo
// ---------------------------------------------------------------------------

#[test]
fn two_by_two_indexes_every_cell_and_reads_the_shape() {
    let (values, (low, high)) = matrix_of(&format!("main = {};", m2()));
    assert_eq!(low, (0, 0));
    assert_eq!(high, (1, 1));
    assert_eq!(values, vec![1.0, 2.0, 3.0, 4.0], "row-major storage order");

    // Every cell through `!`, and the same cells through the named accessor.
    for (i, j, want) in [(0, 0, 1.0), (0, 1, 2.0), (1, 0, 3.0), (1, 1, 4.0)] {
        let src = format!("main = {} ! {};", m2(), cell(i, j));
        assert_eq!(run_float(&src), want, "`!` at cell ({i},{j})");
        let src = format!("main = index {} {};", m2(), cell(i, j));
        assert_eq!(run_float(&src), want, "`index` at cell ({i},{j})");
    }
}

#[test]
fn two_by_two_assocs_pair_each_cell_with_its_value() {
    let pairs = assocs_of(&format!("main = assocs {};", m2()));
    assert_eq!(
        pairs,
        vec![((0, 0), 1.0), ((0, 1), 2.0), ((1, 0), 3.0), ((1, 1), 4.0)],
        "the pairings follow the row-major storage order"
    );
}

#[test]
fn two_by_two_transpose_swaps_the_off_diagonal_cells() {
    let (values, (low, high)) = matrix_of(&format!("main = {};", transpose_expr(&m2(), 2)));
    assert_eq!(
        (low, high),
        ((0, 0), (1, 1)),
        "a square transpose keeps the shape"
    );
    assert_eq!(
        values,
        vec![1.0, 3.0, 2.0, 4.0],
        "the off-diagonal cells swap, the diagonal cells stay"
    );
    // Written out cell by cell: the transposed (0,1) is the original (1,0).
    let src = format!("main = {} ! {};", transpose_expr(&m2(), 2), cell(0, 1));
    assert_eq!(run_float(&src), 3.0);
}

// ---------------------------------------------------------------------------
// 2. The 3x3 demo
// ---------------------------------------------------------------------------

#[test]
fn three_by_three_indexes_every_cell_row_major() {
    // All nine cells, written as a 3x3 grid so the row-major flattening is
    // visible in the assertion itself.
    let grid = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
    for (i, row) in grid.iter().enumerate() {
        for (j, want) in row.iter().enumerate() {
            let src = format!("main = {} ! {};", m3(), cell(i as i64, j as i64));
            assert_eq!(run_float(&src), *want, "cell ({i},{j})");
        }
    }
}

#[test]
fn three_by_three_assocs_pair_every_cell_with_its_value() {
    let pairs = assocs_of(&format!("main = assocs {};", m3()));
    let expected: Vec<((i64, i64), f64)> = (0..3)
        .flat_map(|i| (0..3).map(move |j| ((i, j), (i * 3 + j + 1) as f64)))
        .collect();
    assert_eq!(pairs, expected);
}

#[test]
fn three_by_three_element_sum_folds_the_element_list() {
    // `elems` is the storage list, so folding it sums the matrix: 1 + … + 9.
    let src = format!("main = fold (\\ a -> \\ b -> a + b) 0.0 (elems {});", m3());
    assert_eq!(run_float(&src), 45.0);
}

#[test]
fn three_by_three_transpose_is_the_mirrored_grid() {
    let (values, (low, high)) = matrix_of(&format!("main = {};", transpose_expr(&m3(), 3)));
    assert_eq!((low, high), ((0, 0), (2, 2)));
    assert_eq!(
        values,
        vec![1.0, 4.0, 7.0, 2.0, 5.0, 8.0, 3.0, 6.0, 9.0],
        "columns become rows"
    );
    // A few cells of the mirrored grid, checked individually.
    for (i, j, want) in [(0, 1, 4.0), (1, 0, 2.0), (2, 2, 9.0), (1, 2, 8.0)] {
        let src = format!("main = {} ! {};", transpose_expr(&m3(), 3), cell(i, j));
        assert_eq!(run_float(&src), want, "transposed cell ({i},{j})");
    }
}

#[test]
fn transpose_is_an_involution_checked_with_structural_equality() {
    // `transpose . transpose == id`, asserted INSIDE the language: `==` on an
    // `Array` compares the stored bounds and then the elements.
    let twice = transpose_expr(&transpose_expr(&m3(), 3), 3);
    assert!(
        run_bool(&format!("main = {twice} == {};", m3())),
        "double transpose must reconstruct the original matrix"
    );
    assert!(!run_bool(&format!("main = {twice} != {};", m3())));
    // A single transpose is NOT the original.
    assert!(!run_bool(&format!(
        "main = {} == {};",
        transpose_expr(&m3(), 3),
        m3()
    )));
}

// ---------------------------------------------------------------------------
// 3. Accumulation into and from matrices
// ---------------------------------------------------------------------------

#[test]
fn accum_folds_associations_into_the_demo_matrix() {
    // `accum (+)` over a 3x3: cell (0,0) += 10, cell (1,1) += 5 then += 6,
    // cell (2,2) += 100.
    let src = format!(
        "main = accum (\\ a -> \\ b -> a + b) {} [{}, {}, {}, {}];",
        m3(),
        assoc(0, 0, 10.0),
        assoc(1, 1, 5.0),
        assoc(1, 1, 6.0),
        assoc(2, 2, 100.0)
    );
    let (values, (low, high)) = matrix_of(&src);
    assert_eq!((low, high), ((0, 0), (2, 2)), "the source shape is kept");
    assert_eq!(
        values,
        vec![11.0, 2.0, 3.0, 4.0, 16.0, 6.0, 7.0, 8.0, 109.0],
        "cell (1,1) folds twice (5 + 6 + its own 5)"
    );
}

#[test]
fn accum_array_builds_the_demo_matrix_from_associations() {
    // The assoc-list constructor: every cell starts at the seed `0.0`, and a
    // repeated cell folds. A 2x2 grid, so the result reads as a matrix.
    let src = format!(
        "main = accumArray (\\ a -> \\ b -> a + b) 0.0 {} [{}, {}, {}, {}];",
        bounds(2, 2),
        assoc(0, 0, 1.0),
        assoc(0, 1, 2.0),
        assoc(0, 1, 3.0),
        assoc(1, 0, 4.0)
    );
    let (values, (low, high)) = matrix_of(&src);
    assert_eq!((low, high), ((0, 0), (1, 1)));
    assert_eq!(
        values,
        vec![1.0, 5.0, 4.0, 0.0],
        "cell (0,1) accumulates to 2 + 3, cell (1,1) keeps the seed"
    );
}

#[test]
fn a_transposed_matrix_accumulates_into_its_own_shape() {
    // Accumulation after a reindex: the associations address the TRANSPOSED
    // grid, so cell (0,1) of the result is the original (1,0) plus the increment.
    let src = format!(
        "main = accum (\\ a -> \\ b -> a + b) {} [{}];",
        transpose_expr(&m2(), 2),
        assoc(0, 1, 10.0)
    );
    let (values, _) = matrix_of(&src);
    assert_eq!(
        values,
        vec![1.0, 13.0, 2.0, 4.0],
        "3 + 10 at transposed (0,1)"
    );
}

// ---------------------------------------------------------------------------
// 4. Structural equality and order on arrays
// ---------------------------------------------------------------------------

#[test]
fn structural_equality_compares_bounds_first_then_elements() {
    // Same bounds and elements: equal. One element different: unequal. Same
    // elements at different bounds: unequal (the bounds are part of the value).
    let same = m3();
    assert!(run_bool(&format!("main = {same} == {same};")));
    assert!(!run_bool(&format!(
        "main = {same} == {};",
        matrix_expr(3, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 99.0])
    )));
    // Same four elements at a different shape (2x2 vs 1x4): unequal, because
    // the bounds are part of the array value.
    let square = matrix_expr(2, 2, &[1.0, 2.0, 3.0, 4.0]);
    let row = matrix_expr(1, 4, &[1.0, 2.0, 3.0, 4.0]);
    assert!(!run_bool(&format!("main = {square} == {row};")));
}

#[test]
fn structural_order_is_bounds_then_lexicographic_elements() {
    // Two arrays with the same bounds compare element by element,
    // lexicographically: [1,2,3,9] is greater than [1,2,3,4].
    let a = matrix_expr(2, 2, &[1.0, 2.0, 3.0, 4.0]);
    let b = matrix_expr(2, 2, &[1.0, 2.0, 3.0, 9.0]);
    assert!(run_bool(&format!("main = {a} < {b};")));
    assert!(run_bool(&format!("main = {b} > {a};")));
    assert!(!run_bool(&format!("main = {b} < {a};")));
    // An array is only ordered against arrays of the same shape through the
    // bounds: `(0,0)..(1,1)` sorts before `(0,0)..(1,2)` because the upper
    // bound pair compares first.
    let narrower = matrix_expr(2, 2, &[1.0, 2.0, 3.0, 4.0]);
    let wider = matrix_expr(2, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    assert!(run_bool(&format!("main = {narrower} < {wider};")));
}
