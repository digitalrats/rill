# rill-lang: GHC-faithful lenient `listArray` / `newListArray` — Implementation Plan (M9)

**Spec:** `docs/superpowers/specs/2026-10-19-rill-lang-array-lenient-design.md`
(user-approved).
**Branch:** `feature/rill-lang-arrays` (kept in place — no merge/PR).
**Method:** TDD, one failing-first test per sub-step, gate after every task.
**Gates after every task:** `cargo test -p rill-lang --features dsp` and
`cargo test -p rill-adrift` green, `cargo clippy -p rill-lang --features dsp --all-targets`
zero warnings, `cargo fmt -- --check`. Code comments and docs in **English**.
Commit messages use **single quotes** (`git commit -m '...'`), never double quotes.

**Scope summary (from the approved spec):** remove the rill-only strict-length
deviation from `listArray` and `newListArray`. Both become GHC-faithful:
`length == rangeSize` unchanged; `length < rangeSize` pads the tail with the
**one shared lazy hole** (the M8 `array_hole_frag`, fault
`"array: undefined array element"` on force — same message as `array`); `length
> rangeSize` silently drops the extras. Applies to both `ArrayFromListChecked`
(immutable) and `NewArrayFromListFlat` (ST). `array`/`accumArray`/`update`/
`accum`/`newArray`/read paths/ST cell shape/fault texts are untouched; Q1
(custom-`Ix` reads via `listArray` + `!`) must stay green.

**Key mechanism (verified during planning):** in both arms the element buffer
holds **borrowed** refs (no RC change) and one trailing claim loop
(`for e in &elems { _ = world.arena.copy(*e); }`) claims every slot exactly
once. Padding therefore pushes the **raw hole ref** (`elems.push(hole)`, NO
`copy`), lets the shared claim loop claim each padded slot, and only then
releases the builder's own claim (`world.arena.drop_ref(hole)` AFTER the loop —
a freed hole slot would make the loop's `copy` a dangling ref). The hole
allocation sequence (frag → `ThunkCell(Unforced)` → `Record(vec![])` env →
`Callable`) is copied from the `ArrayFromAssocs` arm (`interp.rs:3087-3149`).

---

## Task 1 — lenient immutable `listArray` (`ArrayFromListChecked`)

**Goal.** A `listArray` whose element list is shorter than `rangeSize` builds an
array with a **faulting hole tail**; a longer list builds a silently truncated
array. The `array_hole_frag` exists whenever `arrayFromListChecked` is lowered.

**Files.** `src/lower.rs`, `src/backend/interp.rs`,
`tests/ix.rs`, `tests/array_immutable.rs`.

**Interfaces.**
- `lower.rs:3851` — change the `("arrayFromListChecked", 3)` arm from a bare
  `ArrayFromListChecked` to a block that sets `self.uses_array_hole = true;`
  then yields `ArrayFromListChecked` (mirror the `arrayFromAssocs` arm at
  `lower.rs:3838-3845`). This makes the assembly step (`lower.rs:7976-7994`)
  append the synthetic hole fragment (already counted in `max_call_depth`).
- `backend/interp.rs` `ArrayFromListChecked` arm (lines 3241-3301) — replace the
  strict `if elems.len() != expected` fault (lines 3272-3285) with:
  1. `let expected = expected.max(0);` (defensive; prelude `Ix` is never
     negative, inverted bounds → 0).
  2. If `elems.len() > expected`: `elems.truncate(expected as usize)` — the
     excess were borrowed refs, so a plain drop; the claim loop below never
     sees them.
  3. If `elems.len() < expected`:
     - Ensure capacity: if `elems.capacity() < expected`, take a fresh pooled
       buffer (`world.arena.take_buf(expected)`; on `Err` → `put_buf(elems)`,
       `latch_exhaustion`, `dst = None`), `extend_from_slice(&elems)` (copies
       the borrowed ref values, no RC), then `put_buf(elems)` and swap.
     - Build the ONE hole exactly like `ArrayFromAssocs`
       (`interp.rs:3087-3149`): `Some(frag) = ir.array_hole_frag` else
       `put_buf` + fault `"listArray: no hole fragment"`; allocate the
       `ThunkCell`, the empty-env `Record`, and the `Callable`, releasing
       partial refs on each failure (the M8 cleanup discipline).
     - `while elems.len() < expected { elems.push(hole); }` — raw ref, no copy.
     - Do NOT `drop_ref(hole)` here — it must outlive the claim loop below.
  4. Existing bounds copy and claim loop stay; **after** the claim loop, when
     padding happened, `world.arena.drop_ref(hole);`.
  5. Rewrite the arm's doc comment (lines 3242-3248): it currently calls the
     constructor "strict … a deliberate deviation from GHC's lazy tolerance" —
     replace with the lenient pad-or-truncate description.

**TDD.**
1. New test in `tests/ix.rs` — `list_array_pads_a_short_list_with_faulting_holes`:
   - `listArray (0,2) [1.0, 2.0]` (rangeSize 3): `! 0` → `1.0`, `! 1` → `2.0`
     (listed reads work); forcing the tail faults — mirror the M8 hole test
     (`tests/ix.rs:386-417`): `fault` on
     `main = fold (\a -> \b -> a + b) 0.0 (arrayElems (listArray (0,2) [1.0, 2.0]));`
     contains `"array: undefined array element"`. RED now (strict fault says
     `listArray: 2 element(s) but rangeSize is 3`).
2. New test in `tests/ix.rs` — `list_array_truncates_a_long_list`:
   `listArray (0,2) [0.0, 1.0, 2.0, 9.0]` → `array_1d` yields `[0.0,1.0,2.0]`
   and bounds `(0,2)` (the `9.0` extra dropped silently). RED now (strict
   fault).
3. Rewrite `tests/ix.rs:359-374`
   `list_array_rejects_a_length_that_does_not_match_range_size` → split into the
   three new-semantics cases: `(0,2)` with 2 elems → hole (via step 1's fold
   fault); `(0,2)` with 4 → truncation (step 2); `(0,-1)` with 1 → **empty**
   array (`array_1d` → `[], (0,-1)`, rangeSize 0 → extra dropped).
4. Rewrite `tests/array_immutable.rs:298-323`
   `two_dimensional_list_array_rejects_a_length_that_does_not_match_range_size`
   → `two_dimensional_list_array_pads_or_truncates: b2(0,0,1,1)` (4 cells) with
   3 elements → the tail hole faults on a fold; with 5 → silently 4; `b2(0,0,2,1)`
   (6 cells) with 5 → hole; with 6 → exact (unchanged assertion style).
5. Gate + commit:
   `feat(rill-lang): make listArray lenient per GHC (pad holes, drop extras)`

---

## Task 2 — lenient ST `newListArray` (`NewArrayFromListFlat`)

**Goal.** `newListArray` gets the same lenient semantics in ST: a short list
leaves the hole ref in the flat cell (read surfaces it unforced; forcing
faults), a long list is silently truncated to `rangeSize`.

**Files.** `src/lower.rs`, `src/backend/interp.rs`, `tests/ix.rs`.

**Interfaces.**
- `lower.rs` — mark `uses_array_hole` for the `newArrayFromListFlat` effect-op
  path. The op lowers through `lower_starray_effect` (`lower.rs:1040-1094`,
  dispatched from the STArray name match at `lower.rs:1881-1889`). Add, inside
  `lower_starray_effect` (or at the dispatch site), a guard such that
  `op == ValueEffectOp::NewArrayFromListFlat` sets `self.uses_array_hole = true`.
- `backend/interp.rs` `NewArrayFromListFlat` arm (lines 4293-4343) — same
  pad-or-truncate as Task 1, inserted between the `expected` resolution and the
  existing `copy_owned(bounds_src)`:
  1. `let expected = expected.max(0);`.
  2. `elems.truncate(expected)` when long (borrowed excess — plain drop).
  3. When short: capacity re-take (`pooled_elems_of_ref` already used
     `take_buf`; if `elems.capacity() < expected`, re-take + extend +
     `put_buf` old), build the hole (frag/cell/env/callable with the M8
     failure cleanup), `while elems.len() < expected { elems.push(hole); }`.
  4. Existing claim loop stays; after it, when padded,
     `world.arena.drop_ref(hole);`.
  5. Rewrite the arm's doc comment (lines 4294-4302) — drop the "strict" /
     "clean length fault" wording, document pad-or-truncate and that a padded
     hole ref is returned unforced by `readArrayFlat` (fault lands on the
     consumer), consistent with GHC's `readArray` on `arrEleBottom`.
  - No change needed in `freeze_starray` / `GetArrayElems` / `GetArrayBounds`:
    they copy refs without forcing, so a padded cell freezes into an immutable
    array with the same hole model.

**TDD.**
1. New test in `tests/ix.rs` — `st_new_list_array_pads_a_short_list_with_faulting_holes`:
   - Exact-length reads first: `st_new_list_array_seeds_every_element`
     (line 684) already covers `(0,2)` with 3. Add the padded case: freeze
     `runSTArray (\s -> newListArray (0,2) [1.0, 2.0])`, read `! 0` → `1.0`,
     `! 1` → `2.0`, and `fault` on a fold over `arrayElems` of the frozen array
     → contains `"array: undefined array element"`. RED now (strict fault).
2. New test in `tests/ix.rs` — `st_new_list_array_truncates_a_long_list`:
   `runSTArray (\s -> newListArray (0,2) [1.0, 2.0, 3.0, 4.0])` → after freeze,
   `array_1d` gives `[1.0,2.0,3.0]` (4.0 dropped), bounds `(0,2)`. RED now.
3. Rewrite `tests/ix.rs:694-705`
   `st_new_list_array_rejects_a_length_that_does_not_match_range_size` per the
   new semantics (2 @ rangeSize 3 → hole; a 4th element variant → truncation),
   keeping the `{-# MODE complete #-}` header used by the ST tests.
4. Gate + commit:
   `feat(rill-lang): make newListArray lenient per GHC (pad holes, drop extras)`

---

## Task 3 — documentation

**Goal.** The language doc block, the prelude comments, and the README describe
the lenient construction; no stale "strict" wording remains anywhere in the
source tree.

**Files.** `src/types/ty.rs`, `rill-lang/README.md`.

**Interfaces.**
- `src/types/ty.rs` Data.Array doc block, lines 568-571: remove the "STRICT"
  deviation bullet. Replace it with the lenient contract, noting it matches
  GHC (`listArray`/`newListArray` share `arrEleBottom` with `array`'s hole):
  short list pads the tail with the same shared hole (fault on force), long
  list drops extras. Keep the `array` undefined-elements bullet (572-577)
  unchanged; adjust the "Two deliberate deviations" lead-in (line 568) since
  there is now only the documented index-shape limitation to describe.
- `src/types/ty.rs:647-650`: rewrite the `listArray` comment ("The strict
  constructor: the element count must equal `rangeSize` …") to describe
  pad-or-truncate (`arrayFromListChecked` carries the pre-computed size; the
  interpreter pads holes or drops extras).
- `src/types/ty.rs:687` (`newListArray`): adjust the surrounding ST-section
  comment if it mirrors the strict wording; the def line itself needs no
  language change.
- `rill-lang/README.md` "Arrays and `STArray` (Data.Array surface)" section,
  lines 584-611: add one sentence noting `listArray`/`newListArray` follow GHC
  — a list shorter than the range pads with undefined elements (forcing them
  faults), a longer list is truncated to the range size.

**TDD.**
1. Grep the crate for stale strict-wording before editing:
   `rg -n "strict constructor|length must equal|listArray.*strict|newListArray.*strict" rill-lang/src rill-lang/README.md`.
2. Make the three edits above; re-grep — zero matches (the only remaining
   strictness mentions are the unrelated `strict`/`complete` language modes).
3. Gate + commit:
   `docs(rill-lang): document lenient listArray/newListArray construction`

---

## Task 4 — whole-change verification

**Goal.** The shipped state is coherent across both forms, Q1 is green, and the
executed M8 documents are untouched.

**Checks.**
1. Q1: `q1_user_ix_instances_keep_reads_working_but_array_construction_faults`
   (`array_immutable.rs:704-741`) still passes — it is an exact-match
   `listArray` over a custom `Ix` (`rangeSize b = 1`, 1 element), so the
   relaxation does not touch it.
2. Both strict-length test names from M8 are gone; their replacements assert
   pad-or-truncate (grep for `rejects_a_length` in `tests/` — zero matches).
3. No test pins `array_hole_frag == None` for
   `listArray`/`newListArray`-only programs (verified during planning —
   `array_hole_frag` appears only in `src/`).
4. Full gates: `cargo test -p rill-lang --features dsp`,
   `cargo test -p rill-adrift`, clippy (`--all-targets`) zero warnings,
   `cargo fmt -- --check`.
5. `docs/superpowers/plans/2026-10-19-rill-lang-array-hole.md` and
   `docs/superpowers/specs/2026-10-18-rill-lang-array-hole-design.md` remain
   byte-identical (the strictness non-goal is reversed in the NEW
   `2026-10-19-rill-lang-array-lenient-design.md`, not edited in place).
6. Working tree clean; branch `feature/rill-lang-arrays` left in place (no
   merge/PR). Update the git-ignored `progress.md` ledger with the commit
   range and any PARK notes.

**Commit (if any fix is needed):** fold into the relevant task commit, do not
add a separate verification commit.