# rill-lang: GHC-faithful lenient `listArray` / `newListArray` (M9)

Status: design (approved shape)
Date: 2026-10-19
Branch: `feature/rill-lang-arrays`
Related: `docs/superpowers/specs/2026-10-18-rill-lang-array-hole-design.md` (M8,
whose non-goal this design reverses), `docs/superpowers/specs/2026-10-17-rill-lang-arrays-design.md`
(the parent array design)

## Intent

M8 adopted GHC's *undefined-element* model for the assoc constructor `array`
(one shared lazy hole per array). Its explicit non-goal left `listArray` /
`newListArray` **strict**: `length elems` had to equal `rangeSize bounds`,
otherwise a clean length fault. This design removes that rill-only deviation and
makes both constructors **lenient, exactly like GHC**:

- `length == rangeSize` — unchanged.
- `length < rangeSize` — the tail is padded with **the one shared lazy hole**
  (the same `array_hole_frag`, same fault `"array: undefined array element"`
  on force, same message as `array`).
- `length > rangeSize` — extras are **silently dropped** (truncation).

The relaxation applies to **both forms**: the immutable `listArray`
(`ArrayFromListChecked`) and the ST `newListArray` (`NewArrayFromListFlat`).

## GHC ground truth (verified from hackage, `array-0.5.8.0`)

- `GHC.Arr.listArray` (base): allocates with `newArray# n# arrEleBottom`,
  writes the elements in order from flat offset `0`, and **stops at
  `i == n`** — extras past the range are dropped; a short list leaves the
  `arrEleBottom` tail.
- `Data.Array.Base.newListArray` (the `MArray` method covering `STArray` /
  `IOArray` / `STUArray`): identical in shape — the MArray is allocated
  pre-filled with `arrEleBottom`, `foldr` writes `es` from offset `0`, and the
  fold stops at `n`.
- `arrEleBottom = error "(Array.!): undefined array element"` — the **same**
  bottom for both `array` and `listArray`. rill's existing message
  `"array: undefined array element"` matches it, so **one shared hole fragment
  and one fault message are GHC-consistent** across `array`, `listArray`, and
  `newListArray`.
- Conclusion: **GHC has no length check in either form.** The strictness was a
  rill-only deviation, now reversed.

## Decisions (confirmed with the user, 2026-10-19)

- **D1 — Full GHC fidelity for both forms.** `ArrayFromListChecked`
  (`listArray`) and `NewArrayFromListFlat` (`newListArray`) pad-or-truncate as
  described above. No length fault remains in either arm.
- **D2 — One shared hole per array** (not per slot): the identical
  construction as `ArrayFromAssocs` — one `Value::Callable` over
  `ir.array_hole_frag`, a single memo cell, an empty env record. Padding
  pushes `expected - got` **copies of the same ref** into the element buffer;
  the existing claim loop then claims each slot exactly once. The hole's
  builder claim is released **after** the claim loop (never before — a freed
  hole slot would make the loop's `copy` a dangling ref).
- **D3 — ST hole is a plain arena ref in the flat cell.** No cell-shape
  change: `StCell::Array.elems` holds the hole ref unforced.
  `readArrayFlat` / `GetArrayElems` / `freeze_starray` copy refs without
  forcing, so the fault lands on the consumer (trying to read a hole to WHNF),
  exactly like `array` and like GHC's `readArray`.
- **D4 — `newArray` untouched.** GHC's `newArray` takes an explicit initial
  value and has no undefined elements; rill's `newArrayFlat` is unchanged.
- **D5 — buffer sizing.** Padding must never reallocate a pooled buffer on the
  RT path: when `got < expected`, re-take a pooled buffer sized for
  `expected`, copy the existing refs, return the old buffer, then push the
  hole copies. Truncation is a plain `truncate` — the excess refs were
  **borrowed** (never claimed), so no RC bookkeeping is needed.

## Non-goals / anti-goals (unchanged from M8)

- **No change to `array` / `accumArray` / `update` / `accum`.** Their
  index-resolution, duplicate rules, and hole behavior stay exactly as M8
  shipped them.
- **No change to the read path.** `!` / `index` / `assocs` / `elems` /
  `indices` / `ixmap` and the ST wrappers are untouched; language-side
  `checkedOffset` keeps computing read offsets.
- **No ST cell-shape change** (`StCell::Array` still `{ bounds, elems }`), no
  new `Ir` operations, no new fault texts.
- **No per-position diagnostics** (same as M8): the hole fault stays the
  generic `"array: undefined array element"`.
- **Q1 stands:** custom-`Ix` READS (`listArray` + `!`) keep working because the
  element count comes from the language-side `rangeSize`; custom-index
  `array`-CONSTRUCTION still faults `"unsupported index type"`. The Q1 test
  must stay green after this change (it is an exact-match construction).

## Design

### 1. Lowering: the hole fragment must exist for both constructors

`Ir.array_hole_frag` is only appended when `Lowerer.uses_array_hole` was set
(`lower.rs:7976-7994`). It is currently set for `arrayFromAssocs` only
(`lower.rs:3843`). This design sets it in two more places:

- `lower.rs:3851` — `("arrayFromListChecked", 3)` arm: set
  `self.uses_array_hole = true` before returning `ArrayFromListChecked`.
- the `newArrayFromListFlat` effect-op path — `lower_starray_effect`
  (`lower.rs:1040-1094`, dispatched from the STArray name match around
  `lower.rs:1881-1889`): set `self.uses_array_hole = true` when the op is
  `NewArrayFromListFlat`.

The synthetic hole fragment is already counted in `max_call_depth`
(`lower.rs:7973-7995`); programs using `listArray`/`newListArray` simply join
the count. No test pins `array_hole_frag == None` for such programs (verified:
`array_hole_frag` appears only in `src/`, never in `tests/`).

### 2. Immutable arm: `ArrayFromListChecked` (`backend/interp.rs:3241-3301`)

Replace the strict `if elems.len() != expected` fault with pad-or-truncate:

```
got = elems.len();
let expected = expected.max(0);          // defensive: prelude Ix is never negative
if got > expected {
    elems.truncate(expected as usize);   // extras are borrowed refs — plain drop
} else if got < expected {
    // ensure capacity: re-take a pooled buffer sized for `expected` if needed,
    // copying the existing refs (extend_from_slice, no RC), put_buf the old one
    // build the ONE shared hole exactly like ArrayFromAssocs
    //   (ir.array_hole_frag check -> ThunkCell(Unforced) -> Record(vec![]) env
    //    -> Value::Callable { fragment, env, cell: Some(cell) })
    //   on any alloc failure: cleanup refs + put_buf + latch_exhaustion, as the
    //   ArrayFromAssocs arm does
    while elems.len() < expected { elems.push(hole); }  // raw ref, NO copy
}
// ... existing bounds copy and the existing claim loop:
// for e in &elems { _ = world.arena.copy(*e); }   // claims EVERY slot once
// if we padded: world.arena.drop_ref(hole);       // AFTER the claim loop
world.value_regs[dst] = alloc_owned(ir, world, Value::Array { bounds, elems });
```

RC accounting: the source-list refs were borrowed (no claim) and the hole was
allocated with rc 1. The claim loop gives every slot exactly one claim
(incl. every hole copy). `drop_ref(hole)` restores the builder claim, leaving
one claim per padded slot — exactly what the array's drop will release.

### 3. ST arm: `NewArrayFromListFlat` (`backend/interp.rs:4293-4343`)

The same pad-or-truncate, inserted before the existing claim loop and the
`StCell::Array` push:

```
let Some(elems) = pooled_elems_of_ref(ir, world, *args.get(2)?) else { ... };
let expected = forced_i64(...).max(0);
// pad-or-truncate exactly as §2 (hole build + raw pushes + capacity re-take)
let bounds = copy_owned(ir, world, bounds_src)?;
for e in &elems { _ = world.arena.copy(*e); }   // claims every slot once
// if we padded: world.arena.drop_ref(hole);
world.st_cells.push(Some(StCell::Array { bounds, elems }));
```

`readArrayFlat` returns the element via `copy_owned` unforced; a hole ref in
the cell surfaces to the program as a callable whose force latches the fault.
`freeze_starray` transfers refs without forcing, so `runSTArray` of a padded
`newListArray` yields the same hole model as an immutable `listArray`
(GHC-consistent).

## Testing

### Migrate (behavior flipped — these tests must be REWRITTEN, not deleted silently)

- `tests/ix.rs:359-374` `list_array_rejects_a_length_that_does_not_match_range_size`
  — cases `(0,2,2)`, `(0,2,4)`, `(0,-1,1)`:
  - `(0,2,2)` → short → positions 0..1 read, position 2 is a hole (forcing
    faults `"array: undefined array element"`).
  - `(0,2,4)` → long → silently built with 3 elements (`!2` reads `2.0`, the
    `4.0` extra is gone).
  - `(0,-1,1)` → empty range, 1 element → silently empty array (extra
    dropped).
- `tests/array_immutable.rs:298-323`
  `two_dimensional_list_array_rejects_a_length_that_does_not_match_range_size`
  — 2×2 with 3 elements → hole at the last cell; with 5 → silently 4; 3×2 with
  5 → hole; with 6 → exact match.
- `tests/ix.rs:694-705`
  `st_new_list_array_rejects_a_length_that_does_not_match_range_size` — ST
  counterpart: short → hole in the cell (force on read faults); long → cell
  length is `rangeSize`.

### New (both forms, both directions)

- `list_array_pads_a_short_list_with_faulting_holes` — reads of listed
  positions work; a fold over `arrayElems` (which forces every element) faults
  `"array: undefined array element"` (mirrors the M8 hole test
  `tests/ix.rs:386-417`).
- `list_array_truncates_a_long_list` — `!` on in-range positions reads the
  first `rangeSize` elements; `arrayElems` length is `rangeSize`.
- ST: `st_new_list_array_pads_a_short_list_with_faulting_holes` and
  `st_new_list_array_truncates_a_long_list` — via `runSTArray` + `!`/`elems`
  after freeze, same assertions.
- 2-D variants in `array_immutable.rs` (row-major: the hole sits at the padded
  tail position).

### Stay green

- `q1_user_ix_instances_keep_reads_working_but_array_construction_faults`
  (`array_immutable.rs:704-741`) — exact-match `listArray` with a custom `Ix`.
- All exact-length `listArray`/`newListArray` tests (M8, Task 7-9 surface).
- `array_immutable.rs:280` empty-list `listArray` (exact 0).

## Documentation

- `src/types/ty.rs` Data.Array doc block, lines 568-571: **remove/rewrite** the
  "STRICT" deviation bullet; describe the lenient behavior and note it is the
  GHC model (shared `arrEleBottom`).
- `src/types/ty.rs:647-650` (`listArray` comment: "The strict constructor: the
  element count must equal …") and the ST section header: rewrite to describe
  pad-or-truncate. The one-line `newListArray` def (`ty.rs:687`) needs no
  language change — only its surrounding comment.
- `rill-lang/README.md` "Arrays and `STArray` (Data.Array surface)" section
  (lines 584-611): add a sentence on lenient construction (short → holes,
  long → truncation), mirroring the M8 "undefined elements" note already in
  the section context.