# rill-lang: polymorphic `array` via the shared lazy hole — Implementation Plan (M8)

**Spec:** `docs/superpowers/specs/2026-10-18-rill-lang-array-hole-design.md` (v2, user-approved).
**Branch:** `feature/rill-lang-arrays` (branch head `8b79211`, kept in place — no merge/PR).
**Method:** TDD, one failing-first test per sub-step, gate after every task.
**Gates after every task:** `cargo test -p rill-lang --features dsp` and
`cargo test -p rill-adrift` green, `cargo clippy -p rill-lang --features dsp --all-targets`
zero warnings, `cargo fmt -- --check`. Code comments and docs in **English**.
Commit messages use **single quotes** (`git commit -m '...'`), never double quotes.

**Scope summary (from the approved spec):** `array` stops being a Float-only
`0.0`-fill constructor. The four construction helpers (`array`, `accumArray`,
`update`, `accum`) stop computing offsets in the language; the primitives take
**index-typed** associations `List (Pair i e)` and resolve them against the
stored/construction bounds internally. Unlisted positions of an `array`-built
array become **one shared lazy hole** (`Value::Callable` thunk) that latches
the clean fault `"array: undefined array element"` on force — mirroring GHC's
`undefined`. Two prelude index shapes (`Int`, `Pair Int Int`) are interpreted
structurally; anything else is a clean construction fault
`"{op}: unsupported index type"`. `listArray`/`newListArray` strictness stays
(explicit non-goal). See the spec for the Decisions (Q1–Q3) and the full
testing section.

**Task ordering note (from the pre-flight scan):** a Rust `match` over
`ValueBuiltinOp` is exhaustive, so the enum-variant task (Task 1) must also
carry interpreter arms for the new ops. More importantly, Task 1 **does not**
change the `arrayUpdate`/`arrayAccum` signature entries: the prelude still
maps flat `checkedOffset` offsets into those ops, and an index-typed entry
would fail to typecheck against the still-flat prelude. The signature flip,
the interpreter-arm rework, and the prelude rewrite are one **atomic** step
(Task 3) — each intermediate state either fails to compile (flat prelude vs
index-typed sig) or breaks the 2-D surface tests (flat assocs fed to an
index-resolving arm).

---

## Task 1 — register `arrayFromAssocs`/`arraySeed`; land `arraySeed` + the shared shape helper

**Goal.** The two new ops are resolvable by name at lowering, typed by the
signature table and `value_builtin_ty`, listed in the smoke test, and executed
by a first real arm (`ArraySeed`). This also introduces the single shared
shape-discrimination helper that all four primitives will use (Q1/Q3
substrate). The `arrayUpdate`/`arrayAccum` signature entries stay FLAT and the
prelude is untouched in this task.

**Files.** `src/ir.rs`, `src/lower.rs`, `src/types/ty.rs`, `src/backend/interp.rs`,
`tests/array_storage.rs`.

**Interfaces.**
- `ir.rs` `ValueBuiltinOp` (~line 159, after `ArrayAccum`): add
  `ArrayFromAssocs` and `ArraySeed` with doc comments matching the flat-block
  style ("Build an immutable array from its bounds and an index-typed
  `List (Pair i e)` of associations — unlisted positions become one shared lazy
  hole" / "Build an immutable array whose every position is the given seed
  value: `arraySeed bounds init`").
- `ir.rs` `value_builtin_ops_exist` (lines 1008–1043): the smoke list predates
  `ListRange`/`ArrayFromListChecked` (the enum has 32 variants, the list 30).
  Extend the list with those two **and** the two new ops, assert `34`. This
  makes the smoke test enumerate every variant.
- `lower.rs` `value_builtin` (lines 3776–3822): add
  `("arrayFromAssocs", 2) => ArrayFromAssocs,` and `("arraySeed", 2) => ArraySeed,`
  inside the immutable-array block (after `("arrayAccum", 3)`).
- `lower.rs` `value_builtin_ty` (after the `"arrayFromListChecked"` arm,
  ~line 4033):
  - `"arrayFromAssocs"`: `index = pair_index_ty(args.first())`; element type =
    the **second** type argument of the assoc-list element (`args[1]` →
    `List (Pair i e)` → `Pair`'s `e`). Add a tiny helper
    `fn pair_second_ty(t) -> Option<ValueTy>` mirroring `pair_index_ty` (line
    297) but returning `inner.get(1)`. Fall back to `concrete_or_float(Float)`;
    result `ValueTy::App("Array", [index, elem])`.
  - `"arraySeed"`: `index = pair_index_ty(args.first())`; element =
    `concrete_or_float(args.get(1).cloned().unwrap_or(Float))`; same result
    shape.
- `types/ty.rs` `builtin_sigs` (lines 1315–1398): **add only** the two new
  entries under `forall_ie`, deliberately UNCONSTRAINED (mirroring the
  siblings; the language defs carry the `Ix i =>`):
  - `arrayFromAssocs :: Pair i i -> List (Pair i e) -> Array i e`
  - `arraySeed :: Pair i i -> e -> Array i e`
  Do NOT repoint `flat_assocs` (line 1346) yet; `arrayUpdate`/`arrayAccum`
  keep `List (Pair Int e)` until Task 3. Trim the block comment (1315–1330)
  only insofar as it describes the (future) index-typed resolution; a full
  sentence is fine, the mechanism lands in Task 3.
- `backend/interp.rs` — the shared shape helper + the `ArraySeed` arm:
  - Add `enum ArrayShape { Dim1 { lo: i64, hi: i64 }, Dim2 { lo1, hi1, lo2, hi2 } }`
    and
    `fn array_shape<T, const BUF>(world, bounds_ref) -> Result<ArrayShape, &'static str>`
    near `flat_assoc` (line 2089). It discriminates from the bounds **value**
    shape: a 2-field `Record` whose first field (after `force`) is
    `Value::Int` → `Dim1`; a 2-field `Record` whose first field is a 2-field
    `Record` of `Value::Int`s (and second likewise) → `Dim2`; anything else →
    `Err("unsupported index type")`. Force each field ref to WHNF (via the
    existing `force`) before matching, so lazy bounds in Complete mode read
    correctly.
  - `impl ArrayShape`: `fn size(&self) -> i64` (wrapping i64, matching
    `exec_value_arith`; a negative result means **0 slots** — a valid empty
    array) and `fn offset(&self, world, index_ref) -> Option<i64>` — `inRange`
    **before** any arithmetic, reproducing the instance bodies from
    `types/ty.rs` lines 585–605 exactly:
    - `Dim1`: index must be `Value::Int(i)`; in range iff `lo <= i && i <= hi`;
      offset `i.wrapping_sub(lo)`.
    - `Dim2`: index must be a 2-field `Record` of `Value::Int`s; in range iff
      all four bounds hold; offset
      `(i1.wrapping_sub(lo1)).wrapping_mul(hi2.wrapping_sub(lo2).wrapping_add(1)).wrapping_add(i2.wrapping_sub(lo2))`.
  - `exec_value_call_builtin` (line 2155): add the `ArraySeed` arm (2 args:
    bounds, init). Resolve shape; `size <= 0` → valid empty array
    (`Value::Array { bounds, elems: vec![] }`, bounds claimed via `copy_owned`);
    else build a pooled `elems` of `size` claims on the **same** init ref
    (`world.arena.copy(init)` per slot — cheap, no per-slot allocation) and
    store. Faults: `"arraySeed: unsupported index type"`,
    `"arraySeed: bounds operand is not a Pair"`, pool/arena exhaustion via
    `latch_exhaustion`. Mirror the `ArrayFromList` arm's cleanup discipline
    (lines 2791–2813): release partial claims + `put_buf` before returning
    `None`.
  - Add the `ArrayFromAssocs` arm as a **stub** that latches a clean
    `"arrayFromAssocs: not implemented yet"` fault and unbound dst — Task 2
    replaces it. (Keeps the exhaustive match compilable with honest behavior.)

**TDD.**
1. `types/ty.rs` — sig-table unit test (in the `builtin_sigs` test module):
   `arrayFromAssocs`/`arraySeed` resolve to a `TFunc` of the documented shape;
   assert the existing `arrayUpdate`/`arrayAccum` entries are **unchanged**
   (still flat `Pair Int e`). Red initially (no sig entries).
2. `ir.rs` — `value_builtin_ops_exist` expects 34; add the four variants.
   `value_builtin("arraySeed", 2)` is `Some(ArraySeed)`,
   `value_builtin("arrayFromAssocs", 2)` is `Some(ArrayFromAssocs)`.
3. `array_storage.rs` direct-primitive test (mirroring the `arrayFromList`
   storage read-backs, lines 124–139):
   - `main = arraySeed (Pair { first: 0, second: 2 }) 7.0;` → `arrayElems` reads
     `[7.0, 7.0, 7.0]`, `arrayBounds` reads `(0, 2)` (seed fills every position).
   - 2-D: `arrayElems (arraySeed b2(0,0,1,1) 2.5)` → `[2.5 × 4]`, bounds `(0,0,
     1,1)`. Red until the arm lands.
   - Empty bounds: `arraySeed (Pair { first: 0, second: -1 }) 1.0` → zero
     elements, bounds read back verbatim (size-0 valid empty array).
   - Fault: `arraySeed (Pair { first: 0.5, second: 2.0 }) 1.0` →
     `msg.contains("unsupported index type")`.
4. Gate + commit:
   `feat(rill-lang): register arrayFromAssocs/arraySeed ops and land arraySeed`

---

## Task 2 — `arrayFromAssocs` with the shared lazy hole mechanism

**Goal.** `arrayFromAssocs` builds an array whose `size` positions start as
claims on **one shared** `Value::Callable` lazy thunk (the hole), then overlays
the listed associations (last wins). Forcing an unlisted position latches
`"array: undefined array element"`. Large holes are cheap: one callable, `size`
ref claims, no per-slot arena allocation. The prelude still does not use this
op yet (direct-primitive tests drive it).

**Files.** `src/ir.rs`, `src/lower.rs`, `src/backend/interp.rs`,
`src/program.rs` (test `Ir` literals), `tests/array_storage.rs`.

**Interfaces.**
- `ir.rs` `ValueInstr` (after `ValueSetError`, line 321): add unit variant
  `ValueArrayHoleFault` (no operands). Its exec (interp.rs `exec_value_instr`,
  after the `ValueSetError` arm at 1718) latches
  `latch_fault(ir, world, "array: undefined array element")` and writes no
  register — the only ever-instantiated copy lives in the synthetic hole
  fragment. (`ValueSetError`'s message is match-specific and cannot be reused.)
- `ir.rs` `Ir` (~after `fragments`, line 951): add
  `pub array_hole_frag: Option<u32>` — the index of the program's synthetic
  hole fragment, `Some` iff lowering emitted it.
- `lower.rs` `Lowerer` (line 599): add a flag field
  `uses_array_hole: bool` (default `false`). Set it in `value_builtin` when
  `("arrayFromAssocs", 2)` resolves.
- `lower.rs` assembly (near the `Ok(Ir { … })` at 7945, **before** the
  `max_call_depth`/`max_call_regs` computation at 7916): if
  `lw.uses_array_hole`, push one synthetic fragment and set the `Ir` field:
  ```rust
  let hole_id = lw.fragments.len() as u32;
  lw.fragments.push(std::sync::Arc::new(FragmentIr {
      value_blocks: vec![ValueBlock { instrs: vec![ValueInstr::ValueArrayHoleFault],
                                     term: ValueTerm::Halt }],
      entry: 0,
      num_value_regs: 1,
      output_value_regs: vec![],
      num_capture_cells: 0,
      sig: FuncSig { value_ins: 0, value_outs: 0, signal_ins: 0 },
  }));
  ```
  Append **before** `max_call_depth = lw.fragments.len()` so the call-stack
  allowance covers the extra frame. `array_hole_frag: Some(hole_id)` in the
  `Ir` literal; all other `Ir` literal sites (interp.rs tests 5388/5456/5558/
  6330, program.rs tests 1104/1141/1199/1253/1306) get `array_hole_frag: None`.
- `backend/interp.rs` — replace the Task 1 `ArrayFromAssocs` stub with the
  real arm (2 args: bounds, assocs). Steps, mirroring `ArrayFromList`'s
  cleanup discipline throughout:
  1. `shape = array_shape(...)`; `Err` → `"arrayFromAssocs: unsupported index type"`.
  2. `size = shape.size()`; if `<= 0` → valid empty array (no hole needed).
  3. Otherwise build the **hole**:
     `cell = alloc_owned(Value::ThunkCell(ThunkState::Unforced))`,
     `env = alloc_owned(Value::Record(vec![]))` (empty env — the fragment takes
     no captures), then
     `hole = alloc_owned(Value::Callable { fragment: ir.array_hole_frag (Some),
     env, cell: Some(cell) })` — exactly the `ValueCallLazy` construction shape
     (lines 1469–1485). If `ir.array_hole_frag` is `None` (should not happen),
     latch `"arrayFromAssocs: no hole fragment"` and abort.
  4. `elems`: pooled buffer of `size` claims, each `world.arena.copy(hole)`
     (same ref, RC++ per slot — one claim per slot, released on array drop).
  5. Overlay assocs: for each `Pair` element read its index ref and value ref;
     resolve via `shape.offset`; any `Err`/out-of-rectangle → clean
     `"arrayFromAssocs: index out of range"` (release all claims + `put_buf`
     before returning); else replace `elems[pos]` with
     `copy_owned(val)` and `drop_ref` the previous claim (the hole claim there).
     In-order application gives last-wins.
  6. Store `Value::Array { bounds: copy_owned(bounds), elems }`.
- `backend/interp.rs` hole-fragment path (for verification, no new code): when
  `force()` (5078) runs the hole fragment via `run_call`, the first instruction
  latches the fault; then `Halt` → `run_call` returns `None` → `force()`'s
  `latch_exhaustion("thunk body produced no result")` is a first-latch-wins
  no-op and `try_alloc_bottom` memoizes bottom into the shared cell. Subsequent
  forces of the same (shared) cell return the memoized bottom silently; the
  error surfaces at tick end.

**TDD.**
1. `array_storage.rs` construction tests:
   - Listed positions read; unlisted positions are holes: `arrayFromAssocs
     (Pair { first: 0, second: 3 }) [Pair { first: 0, second: 1.5 },
     Pair { first: 2, second: 3.5 }]` → reading elems by **forcing** (a fold
     over `arrayElems` with `+`) faults with
     `msg.contains("array: undefined array element")`.
   - Reads that do not force (plain `arrayElems`, `length`) succeed and see the
     hole refs; a raw `vector`-style read resolves the listed positions to
     Floats and faults or panics-free on holes only when forced.
   - Last wins: two assocs targeting one cell; the second is observed
     (e.g. `(0,1.0)` then `(0,7.0)` → element 0 reads 7.0 under a forced read).
   - Non-unit lower bound / 2-D: `arrayFromAssocs b2(1,2,2,2) [cell …]` resolves
     cell (1,2) → offset 0 and (2,2) → offset 3 (row-major, wrapped bounds
     math matching the spec's Q3 contract).
   - Out-of-rectangle cell with accidentally in-range flat: a 3×2 grid and cell
     `(0,2)` → `"arrayFromAssocs: index out of range"` (the flat offset would
     be in range but the rectangle check fires first).
   - Fault: `"arrayFromAssocs: unsupported index type"` for a Float bounds pair.
   - Empty: `arrayFromAssocs (Pair { first: 0, second: -1 }) []` → zero elements,
     no fault; wide range with few assocs (`(0, 1000000)`) builds and the
     listed positions read (cheap holes — no arena exhaustion).
2. `value_builtin_ops_exist` still 34; `Lowerer.uses_array_hole` is set only by
   `arrayFromAssocs` (a program using only `arraySeed`/`arrayFromList`/
   `listArray` yields `array_hole_frag == None`).
3. Gate + commit:
   `feat(rill-lang): arrayFromAssocs with a shared lazy hole that faults on force`

---

## Task 3 — the atomic index-typed flip: `arrayUpdate`/`arrayAccum` + prelude + docs + migrations

**Goal.** The flat `List (Pair Int e)` contract for `arrayUpdate`/`arrayAccum`
is replaced by index-typed `List (Pair i e)`, resolved internally against the
**stored bounds** of the source array through the shared shape helper. The
prelude stops computing offsets and becomes one-line wrappers; `array` becomes
element-polymorphic with the shared-lazy-hole semantics; the two
`0.0`-fill tests and the out-of-range message test migrate. This whole task is
**one** commit: the sig table, the interpreter arms, the prelude, and the tests
that depend on them form a single interface flip. Direct-primitive 1-D
lower-0 tests keep passing because index `N` == offset `N` for them.

**Files.** `src/types/ty.rs`, `src/backend/interp.rs`, `tests/array_storage.rs`,
`tests/array_immutable.rs`, `tests/ix.rs`.

**Interfaces.**
- `types/ty.rs` `builtin_sigs` (line 1346): replace
  `flat_assocs = list(pair(vec![tv("Int"), tv("e")]))` with
  `let assocs_ie = || list(pair(vec![tv("i"), tv("e")]));` and use it in the
  `arrayUpdate` (line 1378) and `arrayAccum` (line 1394) entries. Update the
  block comment (1315–1330) to state that construction/update offsets are now
  resolved by the interpreter for the two prelude shapes, while reads remain
  language-side.
- `backend/interp.rs` — rework the `ArrayUpdate | ArrayAccum` arm (line 3018):
  1. After `copy_array_elems` produces `out`, read the **stored bounds** of the
     source (`Some(Value::Array { bounds, .. })`, same read as lines 3116–3119)
     and `shape = array_shape(world, stored_bounds)`; `Err` →
     `"{op_name}: unsupported index type"`.
  2. Replace `flat_assoc` (line 2089) with an index-typed reader returning the
     pair's **index ref** and **value ref**; resolve each index via
     `shape.offset` instead of treating `first` as a flat offset.
  3. Out-of-rectangle → `"{op_name}: index out of range"` (drop the numeric
     flat-offset fragment from the message — the current tests only assert
     `contains("out of range")`); malformed pair keeps
     `"{op_name}: association is not a Pair"`; non-Int/non-Pair index keeps a
     shape-specific cause. `flat_index_in_range` stays (used by
     `ArrayIndexFlat` and the ST read-back paths); remove `flat_assoc` only if
     no other caller remains (grep before deleting).
  4. Everything else (accum closure folding through `call_closure_args`,
     replace-old-claim, fault cleanup, bounds copy + `Value::Array` store at
     3116–3128) is unchanged.
- `types/ty.rs` prelude (lines 643–655), exact replacement text:
  ```haskell
  --  `array assocs`: every unlisted position is ONE SHARED lazy hole whose
  --  FORCE latches the clean fault "array: undefined array element" (GHC's
  --  `undefined` for unlisted positions — not a `0.0` fill). Associations are
  --  applied in order; a repeated index leaves the last value. The interpreter
  --  resolves the index-typed associations against the construction bounds for
  --  the two prelude index shapes (`Int`, `Pair Int Int`).
  array b xs :: Ix i => Pair i i -> List (Pair i e) -> Array i e = arrayFromAssocs b xs;

  --  `accumArray f init assocs`: seed every position with `init`, then fold the
  --  associations in order (`f old new`). `arraySeed` fills all positions, so
  --  an `accumArray` has no holes.
  accumArray f init b xs :: Ix i => (e -> (e -> e)) -> e -> Pair i i -> List (Pair i e) -> Array i e
                          = arrayAccum (arraySeed b init) f xs;

  --  `(//)` — apply (index, value) associations, last wins, source untouched.
  update a xs :: Ix i => Array i e -> List (Pair i e) -> Array i e = arrayUpdate a xs;

  --  `accum f` — fold associations into an existing array.
  accum f a xs :: Ix i => (e -> (e -> e)) -> Array i e -> List (Pair i e) -> Array i e = arrayAccum a f xs;
  ```
  `checkedOffset` (now used only by reads), `index`, `bounds`, `elems`,
  `indices`, `assocs`, `ixmap`, `listArray` — lines 616–641, 657–664 — are
  unchanged.
- `types/ty.rs` Data.Array doc block (lines 551–579): rewrite the derivation
  story — offsets for construction/update/accum are now resolved **by the
  primitives** for the two prelude shapes (structural, doc'd limitation);
  reads still go through `checkedOffset`/`index`, so user `Ix` instances keep
  working for reads but `array`-construction over a non-prelude index shape
  faults `"unsupported index type"`. Delete the second deviation bullet (the
  `0.0`-fill / Float-only `array`) and replace it with the hole-model bullet:
  `array`'s unlisted positions are undefined elements — a shared lazy callable
  that latches `"array: undefined array element"` on force (like GHC's
  `undefined`), making `array` generic in the element type. Keep the
  strict-`listArray` bullet and the row-major contract paragraph.
- `tests/array_immutable.rs`:
  - `array_update_and_accum_array_reject_out_of_range_association_cells`
    (lines 325–356): the `array` case (line 335) now faults from
    `arrayFromAssocs` — change `msg.contains("arrayUpdate")` to
    `msg.contains("arrayFromAssocs")` (the `update`/`accumArray` cases keep
    their names).
  - `array_builds_a_matrix_with_the_zero_fill` (lines 472–485): rename to
    express the hole model and migrate: listed positions read their values;
    forcing an unlisted position (via `!` then arithmetic, or a fold over
    `arrayElems`) latches `"array: undefined array element"`. Keep shape
    assertions (bounds read back; listed cells correct).
- `tests/ix.rs`: `array_fills_unlisted_positions_with_the_zero_default`
  (lines 385–397): migrate to the same hole semantics (rename
  `array_leaves_unlisted_positions_as_faulting_holes`); listed reads return
  `1.5`/`3.5`, forcing an unlisted read faults with the hole message.
- Grep the crate (tests, examples, docs) for other `0.0`-fill assumptions or
  `Array i Float`-typed `array` uses that read unlisted positions raw —
  update any such test.

**TDD.** (The prelude cannot compile mid-step, so the flip is verified as one
unit; within it, each behavior gains a failing-first test where feasible.)
1. Sig-table test: `arrayUpdate`/`arrayAccum` entries now show a
   `List (Pair i e)` assoc argument (element type is a `Pair`, not `Int`).
2. Direct-primitive tests stay green **unchanged** where 1-D lower-0
   (`first: 1` on `ARR_0_TO_2` still lands on element 1; `first: 7` /
   `first: 5` still fault with `"arrayUpdate"/"arrayAccum ... out of range"`).
   Add index-typed direct tests:
   - Non-unit lower bound: `arrayUpdate (arrayFromList (Pair { first: 2,
     second: 4 }) [10.0, 20.0, 30.0]) [Pair { first: 3, second: 99.0 }]` →
     offset 1.
   - 2-D out-of-rectangle cell with in-range flat offset: bounds `((0,0),
     (2,1))` (3×2), assoc `(0,2)` → flat offset 2 is inside 6 elements but the
     rectangle check fires → clean `"arrayUpdate: index out of range"`.
3. The four migrated/in-motion language tests pass with the new semantics:
   the prelude's `array {} [assoc ...]` reads listed values and faults holes;
   `update`/`accum`/`accumArray` typecheck and behave; the out-of-range
   message test names `arrayFromAssocs`.
4. Full gates + commit:
   `feat(rill-lang): index-typed arrayUpdate/arrayAccum; polymorphic array via shared lazy hole`

---

## Task 4 — Q1 end-to-end test, final gates, clippy/fmt, review prep, ledger

**Goal.** Everything green on both gates, zero warnings, fmt clean, the
custom-index shape limitation is pinned by an end-to-end test, and the
task/fix history is logged per convention.

**Files.** `tests/array_immutable.rs`,
`rill/.superpowers/sdd/2026-10-19-rill-lang-array-hole/progress.md`.

**Checklist.**
1. Q1 end-to-end (pattern from `tests/constrained_defs.rs:27–31`): declare a
   user `instance Ix (Pair Int (Pair Int Int))` with trivial method bodies in
   a test program; an `array`-construction over it faults
   `"arrayFromAssocs: unsupported index type"` while a `listArray` + `!` read
   over the same index type still works (reads keep using the language
   instance). Add to `tests/array_immutable.rs`.
2. `cargo test -p rill-lang --features dsp` — green.
3. `cargo test -p rill-adrift` — green (umbrella crate; no SIG surface changes).
4. `cargo clippy -p rill-lang --features dsp --all-targets` — zero warnings
   (new arms/helpers must not trip `too_many_arguments`-style lints; no
   blanket `#[allow]`).
5. `cargo fmt -- --check` (100-col, 4-space).
6. Grep-review: no `main_fragment`/`eval_run`/`run_tick` phrasing in new
   comments/commits; docs in English; `automaton(s)` spelling respected.
7. Append the M8 task/fix history to the SDD progress ledger
   (`progress.md` in this plan's workspace).
8. Commit (if it contains only docs/ledger):
   `docs(rill-lang): close M8 array-hole work in the sdd ledger`

---

## Standing constraints

- **No merge/PR**: the branch stays in place; the user reviews the commits.
- Do **not** modify `docs/superpowers/plans/2026-10-18-rill-lang-arrays.md`
  (the executed arrays plan) — no STATUS marker.
- Non-goals from the spec stay untouched: `listArray`/`newListArray` strictness,
  the ST/`STArray` path, the buffer bridge, and `newtype`-and-custom-index
  flattening beyond the two prelude shapes (documented limitation).