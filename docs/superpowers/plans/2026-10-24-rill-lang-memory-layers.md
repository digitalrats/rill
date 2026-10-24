# rill-lang: Layered Value Memory (mark-region ST regions with chunk ownership handoff) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every `runST` region a memory **layer** over the value arena — zero RC traffic inside regions, an O(live + dead-on-survivor-chunks + pure-dead-chunks) teardown at region exit, and a zero-copy result escape via mark-region + chunk ownership handoff.

**Architecture:** `rill_core::memory::Arena<V>` gains a LIFO stack of bump layers. The base layer keeps the embedded RC free list; while a region is active, allocations bump above the region's mark and are never reference-counted. The slot store is a **first-class multi-chunk page store** (power-of-two `SLOT_CHUNK_CAP` chunks): memory inside a chunk is always contiguous, pages can be handed off wholesale, and fragmentation risk is lowered. At region exit the interpreter runs `region_collapse(result)`: trace the (already-forced) result with mark bits + per-chunk live counters, hand survivor chunks to the parent layer by ownership (refs stay valid — they are arena indices), sweep **every dead layer slot** — on survivor and pure-dead chunks alike — releasing base child refs in place (wholly-dead chunks relink into the free list in one pass), release cell base refs, and retire the mark. The result value is never copied and never re-allocated.

**Tech Stack:** Rust workspace crates `rill-core` (arena) and `rill-lang` (interpreter). No new dependencies. No `unsafe` (`#![deny(unsafe_code)]` in both crates).

**Spec:** `docs/superpowers/specs/2026-10-19-rill-lang-memory-layers-design.md` (layer model §3, collapse algorithm §3.3, interaction points §4, allocator requirements §5). Context: `docs/superpowers/specs/2026-10-20-rill-lang-world-state-io-st-unification-design.md` §4.2/§5 — layers are the value-store runtime mechanism; `runST` is GHC-exact (`runST` works exactly as in GHC, orthogonal to the evaluation mode); Strict-mode OOM is the existing `value_error`/`ProcessError` path.

## Global Constraints

- RT safety: nothing allocates on the signal path beyond the pre-sized arena. The collapse trace uses a **reused** `mark_bits` `Vec<bool>` and **preallocated** per-chunk counters, both sized to arena capacity at build time; the pass is deterministic single-threaded work (§5).
- `runST` stays exactly as today at the type/lowering level: fresh region per call, rank-2 `forall s` prevents STRef escape, strict exit boundary forces the result while cells are alive. No semantics change to `Mode::Strict`/`Mode::Complete` — mode only decides evaluation strategy (§4.3 of the context spec).
- Existing ST tests must keep passing unchanged: the strict exit boundary, rank-2 invariant, and bottom-as-value semantics are preserved (§2 goal 6).
- `#![deny(unsafe_code)]` — no `unsafe` anywhere in `rill-core` or `rill-lang`. Chunk recycling links slots via the embedded free list (safe).
- No new external crates in `Cargo.toml`.
- Docs and code comments in English; commit messages single-quoted (`git commit -m '...'`).

## Review Focus

Failure modes the spec implies but no single task's tests exercise — each pins a test in the owning task:

1. **Base child refs shared into a region result** — a region reads a base value into its result (or a leftover register holds a base ref at exit): the sweep must NOT release live base children of survivor slots, and dead slots must release their base children exactly once. Test in Task 2.1 (`base_children_of_survivor_kept`), balancing RC via repeated regions.
2. **Nested regions with inner result read by the outer** — the inner collapse hands survivor chunks to the outer layer (not the base); the outer's sweep covers handed-over chunks; no double release, no lost marks. Test in Task 1.3 (`nested_collapse_hands_to_outer_layer`) and Task 2.1 integration.
3. **Region separated from its `copy_owned` result copy** — current code shallow-copies the result at exit; layers make that copy unnecessary. A result that is an **escaping closure** (`Value::Callable` with a region env) must stay valid after exit. Test in Task 2.1 (`escaping_closure_env_survives_collapse`).
4. **Register drain and `drops_scratch` inside a region** — `run_call`'s epilogue and `drops_scratch` drains currently `drop_ref` unconditionally; with layers they must skip in-layer refs (`r >= active mark`). Missing this leaks nothing (layer dies) but **corrupts the free list** if skipped refs are freed twice. Test in Task 2.2 (`drain_skips_in_layer_refs`).
5. **Pure-dead chunks** — a region's temps interleave with result-reachable values, so pages are almost never survivor-pure; the mark pass makes "chunk has zero live slots" an O(1) counter check for wholesale-eligible chunks. Dead slots in **every** chunk (including pure-dead ones) are still swept per-slot to release base children (R4), then wholly-dead chunks relink in one pass. Test in Task 1.2/1.3 (`zero_live_chunk_bulk_recycled`).
6. **Bottom results** — `Bottom` collapses trivially: the layer dies wholesale, no trace work beyond copies-out. Test in Task 1.3 (`bottom_result_retires_layer`) and Task 2.1.

---

## Phase 0 — Baseline (measure first)

The spec (§7 cost 5) sets a benchmark bar: adoption gated on a measured win on an ST/array workload, before vs after. This plan keeps that gate: a dedicated `lang_bench` ST workload with numbers recorded before and after.

### Task 0.1: ST/array workload in `lang_bench` + record baseline

**Files:**
- Modify: `rill-lang/benches/lang_bench.rs`

**Interfaces:**
- Consumes: existing `compile_eval`-style harness helpers in the bench (the bench already exists and compiles: `prog.process(...)` / `compile::<f32>` API).
- Produces: a bench function `st_array_workload` that exercises region churn (array build + indexed read inside a single `runST`, repeated), so the before/after delta of RC removal is measurable.

- [ ] **Step 1: Write the failing-behavior-is-measured check (no failure; record baseline)**

Add to `lang_bench.rs` a bench named `st_array_workload` whose program does, inside one `runST`:
- `newSTArray` of, say, 64 elements, fill via `writeSTArray` in a strict loop, read a few elements, return a `Pair`/record reduced to the final result, then `runST` again on the next tick frame.

The point is not one number — it is that the workload is stable (its output value is asserted to be the expected constant) and reproducible before/after.

Run: `cargo bench -p rill-lang --bench lang_bench -- st_array_workload` (workspace gates in `rill/AGENTS.md`).
Note: record the per-iteration ns number for this bench in the branch commit message or a comment above the bench (`// baseline: <ns>/iter, 2026-10-24`), to be updated at the end by Task 3.2.

- [ ] **Step 2: Verify the bench compiles and runs**

Run: `cargo bench -p rill-lang --bench lang_bench -- --test` (test mode, not full run) then the actual bench once.
Expected: compiles, runs, prints `st_array_workload`.

- [ ] **Step 3: Commit**

```bash
git add rill-lang/benches/lang_bench.rs
git commit -m 'bench(lang): ST array workload baseline for memory layers'
```

---

## Phase 1 — Arena layer support in `rill_core::memory`

### Task 1.1: Arena layer mark + bump allocation + `is_base_ref`

**Files:**
- Modify: `rill-core/src/memory/arena.rs`
- Test: `rill-core/src/memory/arena.rs` (module `tests`)

**Interfaces:**
- Consumes: `Arena<V: ArenaValue>` as built in SP-4 (`with_capacity`, `alloc`, `drop_ref`, `copy`, `get`, `get_mut`, `rc`, `mutate`, `free`, `slots: ChunkedVec<Slot<V>>`, `Slot::Occupied{rc, val: Option<V>}` / `Slot::Free{next}`).
- Produces (used by Tasks 1.2+ and all of Phase 2):
  - `pub fn push_layer(&mut self) -> usize` — push a new region layer; returns its mark for the caller. Allocates nothing.
  - `pub fn pop_layer(&mut self)` — retire the top layer's bump extent wholesale (no trace), relinking its slots into the free list in DESCENDING index order (R3), so the head of the new run is the mark.
  - `pub fn layers(&self) -> usize` — number of active layers (0 = base layer active).
  - `pub fn is_base_ref(&self, r: ArenaRef) -> bool` — `r < global_base` where `global_base` is the bottom-most layer mark (or `u32::MAX` when no layer is active).
  - `pub fn base_mark(&self) -> usize` — the bottom layer mark (`usize::MAX` if none).
  - `pub(crate) fn with_capacity_and_chunk(capacity: usize, chunk_cap: usize) -> Self` — explicit chunk capacity (power of two). `with_capacity` keeps its exact signature and delegates with the production `SLOT_CHUNK_CAP` const (see Design). **Multi-chunk is a first-class production feature (R1), not a test fixture.**
- Constant: `global_base` semantics per spec §3.1 — RC-managed iff `r < global_base`; refs `>= global_base` are layer-owned and never RC'd.

**Design (pinned):**
- `Arena` gains: `marks: Vec<usize>` (LIFO marks; empty = base), `bump: usize` (next slot to allocate in the top layer).
- **Multi-chunk slot store (R1):** the arena's slot store is built across power-of-two chunks. `const SLOT_CHUNK_CAP: usize = 64` (production default, power of two) is used by `with_capacity` (capacity 900 → 15 chunks); `with_capacity_and_chunk(capacity, chunk_cap)` exposes chunk sizing for callers/tests. The test helper `arena(n)` builds via `with_capacity_and_chunk(n, 8)` so layer marks align with chunk boundaries deterministically. The growable path already appends `slots.chunk_cap()`-sized chunks. Rationale (user-confirmed): direct page-sized chunk allocation keeps buffers/arrays contiguous within a chunk and lowers memory-fragmentation risk; chunks can later be handed off wholesale.
- The **base free list keeps its exact current shape** (`free: Option<ArenaRef>` and `Slot::Free{next}` links). It is only ever used for allocation *below* the bottom mark / with no layer active. **`push_layer` severs the free list at the mark (R2):** walk the chain and terminate it below the first slot `>= mark`, so base allocs during a layer can only ever pop slots `< mark`; the region's bump extent `[mark, …)` is never reachable from the base free list.
- While a layer is active, `alloc` must bump: take `bump` if `bump < capacity` (or growable path), else fall back to an arena-full error. The spec pins the base free list for the base layer; region allocation is bump-only (§5 "Bump region mode: a region mark (usize) above which allocations bump; the base layer keeps the embedded RC free list").

- [ ] **Step 1: Write the failing tests**

In `rill-core/src/memory/arena.rs` `mod tests`:

```rust
#[test]
fn push_layer_marks_above_free_list() {
    let mut a = arena(8);
    let r0 = a.alloc(leaf()).unwrap();          // base slot
    assert!(a.is_base_ref(r0));
    let mark = a.push_layer();
    assert!(a.is_base_ref(r0));
    let r1 = a.alloc(leaf()).unwrap();
    assert!(!a.is_base_ref(r1), "layer allocs sit above the mark");
    assert!(r1 >= mark as u32);
    let r2 = a.alloc(leaf()).unwrap();
    assert_eq!(r2, r1 + 1, "region allocation bumps contiguously");
    a.pop_layer();
}

#[test]
fn layer_allocs_do_not_touch_base_free_list() {
    let mut a = arena(4);
    let r0 = a.alloc(leaf()).unwrap();
    a.drop_ref(r0);                             // returns r0 to the free list
    let _ = a.push_layer();
    let r1 = a.alloc(leaf()).unwrap();
    assert_ne!(r1, r0, "region must bump, not reuse the base free slot");
    a.pop_layer();
}

#[test]
fn nested_layers_share_the_bottom_mark() {
    let mut a = arena(16);
    for _ in 0..4 { a.alloc(leaf()).unwrap(); }
    let m1 = a.push_layer();
    let _ = a.push_layer();                     // nested region
    a.alloc(leaf()).unwrap();
    assert_eq!(a.base_mark(), m1, "global_base is the bottom-most mark");
    assert_eq!(a.layers(), 2);
    a.pop_layer();
    a.pop_layer();
    assert_eq!(a.layers(), 0);
}

#[test]
fn pop_layer_returns_bump_extent_to_free_list() {
    let mut a = arena(8);
    for _ in 0..2 { a.alloc(leaf()).unwrap(); }
    let _ = a.push_layer();
    let r1 = a.alloc(leaf()).unwrap();
    let r2 = a.alloc(leaf()).unwrap();
    a.pop_layer();
    // extent above the mark is free again: base can re-alloc r1.
    let r3 = a.alloc(leaf()).unwrap();
    assert_eq!(r3, r1);
    assert_eq!(a.rc(r2), 0);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-core memory::arena::tests::push_layer_marks_above_free_list` etc.
Expected: compile error — no `push_layer`/`pop_layer`/`is_base_ref`/`layers`/`base_mark`.

- [ ] **Step 3: Implement layer state + bump allocation**

In `arena.rs`:

```rust
pub struct Arena<V: ArenaValue> {
    slots: ChunkedVec<Slot<V>>,
    free: Option<ArenaRef>,
    live: usize,
    capacity: usize,
    next_gen: u32,
    pub pool: BufferPool,
    /// Region layer marks, bottom-most first (LIFO). Empty = base layer.
    marks: Vec<usize>,
    /// Next slot to bump into for the innermost layer.
    bump: usize,
}
```

- `push_layer`: `let mark = self.bump; self.marks.push(mark); mark`.
- `pop_layer`: set every slot in `[mark, bump)` to `Slot::Free` linked into `self.free` in **descending index order (highest first, so head = mark)** (R3), decrement `live` by the count of `Occupied` slots reclaimed, then truncate `bump` back to `mark` and pop the mark.
- `alloc`: if `!self.marks.is_empty()`, bump: take `self.bump`, `self.bump += 1`; on `self.bump > capacity` return `Err((ArenaError::CapacityExceeded, val))` (growable path: append a page as today only when the arena is growable — see Task 1.4). If no layer is active, keep the existing free-list behavior exactly.
- `is_base_ref(r)`: `r < self.base_mark() as u32` where `base_mark()` returns `marks.first().copied().unwrap_or(usize::MAX)`.
- A `debug_assert!` in `drop_ref`/`copy` that `is_base_ref(r)`— dropping or copying a layer-owned ref is a bug in debug builds (the interpreter must never RC layer refs; see Phase 2).
- `live` accounting: layer allocations increment `live` as today (slots are occupied); `pop_layer` decrements.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-core memory::arena`
Expected: all arena tests green (existing + 4 new).

- [ ] **Step 5: Commit**

```bash
git add rill-core/src/memory/arena.rs
git commit -m 'feat(rill-core): arena layer marks and bump allocation'
```

### Task 1.2: Mark bits + per-chunk live counters

**Files:**
- Modify: `rill-core/src/memory/arena.rs`
- Modify: `rill-core/src/memory/chunked.rs` (only if a chunk iteration helper is needed; see below)
- Test: `rill-core/src/memory/arena.rs` (`mod tests`)

**Interfaces:**
- Consumes: `Arena` layer state from Task 1.1; `ChunkedVec` page machinery (`slots.num_chunks()`, `slots.chunk(c)`, `slots.chunk_mut(c)`, `slots.chunk_cap()`, `slots.at/_mut`).
- Produces (used by Task 1.3):
  - `fn mark_bits(&mut self) -> &mut Vec<bool>` (or a private helper; the bitmap is `Vec<bool>` sized to `capacity`, reused across collapses — spec §5 "a reused `Vec<bool>` sized to arena capacity").
  - `fn chunk_live(&mut self) -> &mut Vec<u32>` — per-chunk live counters, sized to `slots.num_chunks()`.
  - `fn chunk_of(r: ArenaRef) -> usize` — index of the chunk containing `r` (`r as usize / chunk_cap`).
  - `fn clear_marks(&mut self)` — zero the bitmap (both skipped-mark and full passes need a clean slate; the spec's O(1) reuse means we avoid reallocation, not necessarily O(capacity) clears — see step 3).
  - `fn trace(&mut self, root: Option<ArenaRef>) -> usize` — mark-reachability pass from `root`, filling `mark_bits` and `chunk_live`; returns the number of marked slots. Handles sharing via the mark bit (a slot is traced once).

**Design (pinned) — the trace (§3.3 step 1):**
- Worklist-free recursion is fine here (single-threaded interp; depth = value-tree depth, bounded by the arena's acyclic share structure — the interpreter is acyclic with `force` memoization).
- Walk the value graph of `root` (if `Some`), following only **child refs that are not base refs** (`r >= base_mark`): base children are leaves in the region graph by §3.5 (base values never reference region values — no new base values are created mid-region; pre-existing base values predate the region). Set `mark_bits[r]` once (guard against cycles/sharing), increment `chunk_live[chunk_of(r)]`, recurse into `arena.get(r).visit_refs(...)`.
- Root may itself be a base ref (e.g. result is a pure base value): then nothing is marked; the layer is entirely dead (§3.4/§3.5 — the result, if base, contains no region values).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn trace_marks_reachable_region_slots() {
    let mut a = arena(16);
    for _ in 0..4 { a.alloc(leaf()).unwrap(); }   // base slots r0..r3
    let mark = a.push_layer() as u32;
    let x = a.alloc(Node { kids: vec![a.alloc(leaf()).unwrap()] }).unwrap();
    let y = a.alloc(leaf()).unwrap();
    a.alloc(Node { kids: vec![x] }).unwrap();      // shares x
    let marked = a.trace(Some(x));
    assert!(marked >= 2, "x and its child are marked once each");
    assert!(a.mark_bits()[x as usize]);
    assert!(!a.mark_bits()[y as usize], "unreachable slot not marked");
    assert_eq!(a.chunk_live()[a.chunk_of(x as usize)], 2);
    a.pop_layer();
}

#[test]
fn trace_base_root_marks_nothing() {
    let mut a = arena(16);
    for _ in 0..4 { a.alloc(leaf()).unwrap(); }
    let _ = a.push_layer();
    a.alloc(leaf()).unwrap();
    let marked = a.trace(Some(2));                // a base ref root
    assert_eq!(marked, 0, "base root: no region slots reachable");
    a.pop_layer();
}

#[test]
fn trace_none_marks_nothing() {
    let mut a = arena(16);
    let _ = a.push_layer();
    a.alloc(leaf()).unwrap();
    assert_eq!(a.trace(None), 0);
    a.pop_layer();
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-core memory::arena::tests::trace_`
Expected: compile error — no `trace`/`mark_bits`/`chunk_live`/`chunk_of`.

- [ ] **Step 3: Implement the trace + bitmap/counters**

- Maintain `mark_bits: Vec<bool>` (capacity-sized, built in `with_capacity`) and `chunk_live: Vec<u32>` (num-chunks-sized, rebuilt when `slots.num_chunks()` changes).
- `trace`: visit-refs walk from root with a mark-bit guard. Only recurse into refs `>= base_mark`. Children of a marked slot may include base refs — skip them (they are leaves).
- `clear_marks`: `mark_bits.fill(false)`; when `bump` is small relative to capacity this is O(capacity) — accept it for now (single-threaded, deterministic; it is still O(1) *allocations*). Keep `chunk_live` zero by resetting only the chunks touched during trace (record them in a small local `Vec<usize>`), or `fill(0)` — pick the cheaper per the sizes; note the choice in a comment.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-core memory::arena`
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add rill-core/src/memory/arena.rs
git commit -m 'feat(rill-core): region trace with mark bits and per-chunk live counters'
```

### Task 1.3: `region_collapse` — bulk recycle + chunk ownership handoff + in-place sweep

**Files:**
- Modify: `rill-core/src/memory/arena.rs`
- Test: `rill-core/src/memory/arena.rs` (`mod tests`)

**Interfaces:**
- Consumes: `push_layer`/`pop_layer`/`is_base_ref` (Task 1.1); `trace`/`mark_bits`/`chunk_live`/`chunk_of` (Task 1.2).
- Produces:
  - `pub fn collapse_layer(&mut self, result: Option<ArenaRef>)` — the full collapse (§3.3): trace → bulk-recycle zero-live chunks → hand over survivor chunks to the parent layer **by ownership** → sweep dead slots on survivor chunks in place (release base child refs via `drop_ref`, recycle each slot into the base free list) → retire the mark. Returns nothing; the caller (rill-lang) handles cell base-ref release and `st_cells.truncate`.
  - `pub fn layer_dying_chunk(&self, c: usize) -> bool` — internal helper: true iff chunk `c` is entirely within the dying layer (`chunk_base >= mark && chunk_end <= bump`). Only wholly-in-layer chunks are eligible for the one-pass wholesale free-list relink; a chunk partially below the mark (base portion) or partially above `bump` is a *mixed* chunk and is always swept per-slot.
- Invariant pinned by this task: **a chunk is bulk-relinked wholesale only when it lies entirely within the dying layer.** Base allocation below the mark and region bumping above it can share a boundary chunk (base free slots `< mark` alongside layer slots `>= mark`), so a "chunk wholly in or wholly out" invariant does NOT hold for boundary chunks — mixed chunks are swept per-slot like survivor chunks. Verify the `chunk_base >= mark` guard with a `debug_assert!` before any wholesale relink.
- Also exposed for rill-lang tests: `pub fn live(&self) -> usize` (already public) — the collapse must decrement `live` for every slot it frees.

**Collapse algorithm (pinned, §3.3, per ruling R4):**

1. `trace(result)` → mark bits + per-chunk counters. (Already at the strict exit boundary: the result was forced before this is called; rill-lang guarantees it.)
2. **Sweep every dead layer slot in `[mark, bump)` — survivor chunks and pure-dead chunks alike.** For each `Occupied` slot that is *not* marked: move the value out, release **only its base children** (`drop_ref` on each child `< base_mark`; region children are never touched — they die with the chunk), detach pooled payloads via `detach_payload`/`put_buf` if the value owns one, and decrement `live`. This is where base children of dead region values are released exactly once — there is **no store-time recording** (§3.1's recording would over-release survivor-held base claims).
3. **Recycle:** slots from wholly-dead chunks (`layer_dying_chunk` true and `chunk_live[c] == 0`) are relinked into the base free list as ONE chain in a single pass (O(chunk_cap), one head update instead of N) — the per-slot sweep above already visited them, so nothing is skipped. All other dead slots join the base free list individually.
4. **Hand over by ownership:** chunks containing marked (live) slots stay allocated (parent-owned); refs stay valid; marked slots stay `Occupied` untouched — the result was forced before collapse, so no re-materialization is needed.
5. Retire the mark: pop `marks`; reset `bump` to the top layer's mark (or the base's extent boundary if no layer remains).

> **Cost note (R4):** pure-dead chunks cost O(chunk_cap) per chunk (the per-slot sweep to release base children) instead of O(1). The benchmark gate (Task 3.2, spec §7 cost 5) decides whether an O(1) fast path that skips base-child release on pure-dead chunks is ever needed; today every dead slot must be visited to guarantee base children are released exactly once.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn zero_live_chunk_bulk_recycled() {
    // arena(16) with the test helper's chunk_cap 8: chunk 0 = r0..r7, chunk 1 = r8..r15.
    let mut a = arena(16);
    for _ in 0..8 { a.alloc(leaf()).unwrap(); }       // base fills chunk 0 exactly
    let _ = a.push_layer();                           // mark = 8 = chunk 1 base
    for _ in 0..8 { a.alloc(leaf()).unwrap(); }       // layer fills chunk 1 exactly
    a.collapse_layer(None);
    assert_eq!(a.live(), 8, "all 8 region slots freed");
    let r = a.alloc(leaf()).unwrap();                 // base reuses a chunk-1 slot
    assert!(r >= 8, "recycled slot comes from the wholly-dead chunk 1");
    assert_eq!(a.rc(r), 1, "recycled slot is a fresh Occupied slot");
}

#[test]
fn survivor_chunk_handed_off_by_ownership() {
    let mut a = arena(16);
    for _ in 0..4 { a.alloc(leaf()).unwrap(); }
    let _ = a.push_layer();
    let s = a.alloc(Node { kids: vec![a.alloc(leaf()).unwrap()] }).unwrap();
    a.alloc(leaf()).unwrap();                          // dead temp
    let live_before = a.live();
    a.collapse_layer(Some(s));
    assert_eq!(a.live(), live_before - 1, "temp freed, survivor kept");
    assert!(a.get(s).is_some(), "survivor slot intact, ref unchanged");
    assert_eq!(a.rc(s), 1, "survivor now parent-owned with rc 1");
    let child = a.get(s).unwrap().kids[0];
    assert_eq!(a.rc(child), 1);
}

#[test]
fn dead_slot_sweep_releases_base_children() {
    let mut a = arena(16);
    a.pool = BufferPool::new(32, false);
    let base_child = a.alloc(leaf()).unwrap();
    for _ in 0..4 { a.alloc(leaf()).unwrap(); }
    let _ = a.push_layer();
    a.alloc(Node { kids: vec![base_child] }).unwrap(); // region value holding a base ref
    assert_eq!(a.rc(base_child), 1);
    a.collapse_layer(None);
    assert_eq!(a.rc(base_child), 0, "dead region slot released its base child");
}

#[test]
fn nested_collapse_hands_to_outer_layer() {
    let mut a = arena(32);
    for _ in 0..4 { a.alloc(leaf()).unwrap(); }
    let _ = a.push_layer();                             // region 1
    let inner_survivor = {
        let _ = a.push_layer();                         // region 2 (inner)
        let s = a.alloc(Node { kids: vec![a.alloc(leaf()).unwrap()] }).unwrap();
        a.alloc(leaf()).unwrap();                       // inner temp
        s
    };
    // inner collapses into outer: survivor is handed to region 1, temp freed
    a.collapse_layer(Some(inner_survivor));
    assert!(a.get(inner_survivor).is_some());
    assert_eq!(a.rc(inner_survivor), 1);
    // outer still has region-1 temps: collapse them all
    a.collapse_layer(Some(inner_survivor));
    assert_eq!(a.rc(inner_survivor), 1, "survivor survives outer collapse too");
}

#[test]
fn bottom_result_retires_layer() {
    // Bottom is not an arena ref in rill-lang: the caller passes None and the
    // layer dies wholesale (spec §3.4) — no trace work, no survivor.
    let mut a = arena(16);
    for _ in 0..4 { a.alloc(leaf()).unwrap(); }
    let _ = a.push_layer();
    a.alloc(leaf()).unwrap();                           // region temp
    a.collapse_layer(None);
    assert_eq!(a.live(), 4, "layer retired wholesale, base state unchanged");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-core memory::arena::tests::` (new names)
Expected: compile error — no `collapse_layer`.

- [ ] **Step 3: Implement `collapse_layer`**

Follow the pinned algorithm above. Use the existing `Slot::Occupied`/`Slot::Free` and the embedded free list — the wholesale relink builds ONE chain of a wholly-dead chunk's slots into `self.free` in a single pass. The sweep processes **every** dead layer slot (survivor and pure-dead chunks alike, R4) and calls `drop_ref` only on base children (`r < base_mark`), per §3.5; region children are never touched. Pooled payloads (`detach_payload`) are returned to `self.pool` for dead slots; survivor slots keep theirs.

**Pay attention** to interleaved temps+result (§3.2): allocation order mixes temps and result-reachable values within a chunk, so the sweep must handle `Occupied` dead slots inside survivor chunks — this is precisely why the mark pass exists.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-core memory::arena`
Expected: green (all prior + new).

- [ ] **Step 5: Commit**

```bash
git add rill-core/src/memory/arena.rs
git commit -m 'feat(rill-core): region collapse with chunk ownership handoff'
```

### Task 1.4: growable path + pre-allocated scratch sizing

**Files:**
- Modify: `rill-core/src/memory/arena.rs`
- Test: `rill-core/src/memory/arena.rs` (`mod tests`)

**Interfaces:**
- Consumes: everything from Tasks 1.1–1.3.
- Produces: a guarantee used by the interpreter test Tasks (2.3): layer bumping on a **growable arena** (`pool.growable` runtime flag) appends pages without breaking marks, bitmap, or counters.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn growable_layer_bumps_across_pages() {
    let mut a = arena(2);
    a.pool.growable = true;
    for _ in 0..2 { a.alloc(leaf()).unwrap(); }
    let _ = a.push_layer();
    let r = a.alloc(leaf()).unwrap();               // needs page growth in-layer
    assert!(a.get(r).is_some());
    assert_eq!(a.live(), 3);
    a.collapse_layer(None);
    assert_eq!(a.live(), 2);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-core memory::arena::tests::growable_layer_bumps_across_pages`
Note: there is **no `growable-arena` Cargo feature** in `rill-core/Cargo.toml` (R5) — growability is the runtime `BufferPool.growable` flag the test sets directly. Do not add any `#[cfg(feature = ...)]`.
Expected: FAIL — layer bump does not grow pages yet.

- [ ] **Step 3: Implement**

- In `alloc`'s bump branch, when `bump >= capacity` and `self.pool.growable`: append a page exactly like the base path does today (via `slots.push_chunk`), extend `capacity`, extend `mark_bits`/`chunk_live` to the new sizes.
- `mark_bits.push(false)` per new slot; `chunk_live.push(0)` per new chunk.
- Keep the non-growable behavior: `Err(CapacityExceeded)` at exhaustion exactly as today (RT default → `value_error`/`ProcessError` latch in rill-lang).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p rill-core memory::arena`.
Expected: green in both the growable and non-growable test paths.

- [ ] **Step 5: Commit**

```bash
git add rill-core/src/memory/arena.rs
git commit -m 'feat(rill-core): growable arena under layer bumping'
```

---

## Phase 2 — Interpreter: `region_collapse` + layer-aware RC

### Task 2.1: `exec_runst` uses `region_collapse`; remove exit share-copy

> **Dispatch note (R6):** Tasks 2.1 and 2.2 execute as **ONE implementer dispatch** (the brief carries both task texts; two commits). Task 1.1 adds `debug_assert!(is_base_ref)` to `drop_ref`/`copy`, so any unconditional drain of in-layer refs panics — Task 2.1's own Step-4 gate ("existing ST tests must not regress") is unreachable until Task 2.2's layer-aware drains land in the same change set.

**Files:**
- Modify: `rill-lang/src/backend/interp.rs` (`exec_runst` at 5391, `finish_region` at 5496, `copy_owned` at 605, `freeze_starray` at 5466)
- Modify: `rill-lang/src/program.rs` (`RegionState` at 636 — add a `mark` field; `StCell` at 652)
- Test: `rill-lang/tests/` — extend an existing ST test file (e.g. `st_regions.rs` if it exists; otherwise `runst_regions.rs`) + `rill-lang/src/backend/interp.rs` inline tests where useful.

**Interfaces:**
- Consumes: `Arena::push_layer/pop_layer/collapse_layer/is_base_ref/base_mark/live` (Phase 1); `RegionState{cells, cell_base: usize, mark: usize}` (add `mark`); `st_cells: Vec<Option<StCell>>`; `copy_owned` (still used by `run_call` etc., NOT removed globally — only its use at the `runST` exit goes away).
- Produces (used by Task 2.2/2.3):
  - `fn region_collapse<T, const BUF: usize>(ir, world, mark: usize, result: Option<ArenaRef>)` — rill-lang-side collapse: for each cell id in the region's `cells`, release base cell refs if any (`world.arena.drop_ref` on the cell's arena ref when the cell holds a base ref — see Task 2.3 for cell-value base detection), `st_cells.truncate(cell_base)`, then `world.arena.collapse_layer(result)`.
  - `exec_runst` exit sequence becomes: force result (strict boundary) → `region_collapse(ir, world, mark, result)` → return the **unchanged result ref** (no `copy_owned` — §3.4: the result is already safe to leave in place).

**Changes pinned:**
- Delete `finish_region` (5496) — absorbed into `region_collapse`; its per-cell `take`+`drop_ref`+truncate logic is folded in but **cell value release becomes layer-aware** (Task 2.3 defines `release_cell_value`; for this task, keep the per-cell release but skip refs that are `>= mark`).
- `exec_runst` pushes the region layer (`let mark = world.arena.push_layer();`) when `RegionState` is pushed, and the returned arena ref from `exec_runst` is the forced result ref directly (shared, not copied).
- `RegionState` gains `mark: usize` (set at push); `region_id` stays the Opaque token payload.
- The `runSTArray` path (freeze) is handled in Task 2.3 — for now it can stay `freeze_starray` (existing per-claim copy) plus new `region_collapse`; Task 2.3 removes the now-unnecessary share-copy inside freeze.
- `copy_owned` at the exit (`interp.rs:5445` in the `freeze==false` branch) is deleted; the branch returns `Some(forced)` (the forced result ref).

- [ ] **Step 1: Write the failing tests**

```rust
// rill-lang/tests/runst_regions.rs
#[test]
fn base_children_of_survivor_kept() {
    // region reads a base value into its result: the base ref must NOT be
    // released by the sweep, and the result must be usable after exit.
    let src = r"data Box = Box Float;
b = Box 42.0;
main = runST (\s -> do
    r <- newSTRef b
    readSTRef r)";
    // compile_eval::<f32, 256> returns 42.0 — repeated eval must not corrupt.
    let mut out = [0.0f32; 256];
    for _ in 0..1000 { run(src, &mut out); }   // helper from existing tests
}

#[test]
fn escaping_closure_env_survives_collapse() {
    // region returns a closure whose env was allocated in the layer.
    let src = r"main = runST (\s -> do
        let x = 3.0
        return (\y -> x + y)) 7.0";
    // expected 10.0; repeated eval must be stable (env not freed twice).
}
```

Adapt the exact `Monad`/`ST` DSL to the existing ST syntax used by `rill-lang` tests (see `tests/` ST programs for the current `runST`/`do`/`newSTRef` spellings); the key assertions are (a) result correctness, (b) stability across many runs (RC/free-list corruption would surface as wrong values or a panicking `drop_ref` on a freed slot).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp runst_regions`
Expected: these tests fail or demonstrate the pre-change behavior (before implementing, the current code already passes — so write the tests to pin the invariant and confirm they FAIL only after Task 2.2/2.3 if relevant; at minimum they must be *compiling* and meaningful). See Step 3 for the workload that actually breaks pre-change.

- [ ] **Step 3: Implement `region_collapse` + rewired `exec_runst`**

- `exec_runst` (5391): after `world.regions.push(RegionState { cells, cell_base, mark })`, call `world.arena.push_layer()`. In the `None` (no ST computation) early path, `pop_layer` before returning. At the exit, replace the `copy_owned`-then-`finish_region` tail with `region_collapse(ir, world, mark, out)` returning `out` unchanged.
- `region_collapse`: iterate the popped region's `cells`; for each live cell, drop its arena ref only if it is a base ref (`world.arena.is_base_ref(r)`); truncate `st_cells` to `cell_base`; then `world.arena.collapse_layer(result)`.
- Keep `copy_owned` for its other call sites (register copies in `run_call`, `freeze_starray`'s element share for now).
- `debug_assert!` in drop paths that the ref is base (arena side) will catch mistakes in tests.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp`, `cargo fmt --check`.
Expected: green (existing ST tests must not regress).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/backend/interp.rs rill-lang/src/program.rs rill-lang/tests/runst_regions.rs
git commit -m 'feat(rill-lang): region collapse replaces exit share-copy in runST'
```

### Task 2.2: Register drain and `drops_scratch` become layer-aware

**Files:**
- Modify: `rill-lang/src/backend/interp.rs` (all `world.arena.drop_ref` call sites that run *inside* the value track: the `run_call` epilogue `for i in base..world.value_regs_top` drain at 5643, the `drops.drain` loops at 148/193/338/5621, and any `drop_ref` in `exec_value_instr` + value builtins reachable from a region fragment)
- Test: `rill-lang/src/backend/interp.rs` inline; `rill-lang/tests/runst_regions.rs`

**Interfaces:**
- Consumes: `Arena::is_base_ref` (Phase 1).
- Produces: a small helper used everywhere, keeping the change minimal:
  - `fn drop_base_ref<T, const BUF: usize>(ir, world, r)` — `if world.arena.is_base_ref(r) { world.arena.drop_ref(r) }`. (Name it e.g. `drop_owned_or_skip`; the point is a single layer-aware drop entry point.)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn drain_skips_in_layer_refs() {
    // A region whose body allocates many temps and returns a base result:
    // register drains inside the region must not drop layer-owned temps
    // (they die with the layer). Pre-change, drop_ref on a layer ref either
    // panics (debug_assert in Task 1.1) or corrupts the free list.
    let src = r"main = runST (\s -> do
        let r = newSTRef 1.0
        writeSTRef r 2.0
        readSTRef r)";
    // Expected 2.0, stable across 1000 runs.
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rill-lang --features dsp drain_skips` with the debug `drop_ref` assertion from Task 1.1 active.
Expected: FAIL (debug assert or corrupted free list) — this proves the drain is not layer-aware yet.

- [ ] **Step 3: Implement**

Replace each unconditional `world.arena.drop_ref(r)` in the value track (the listed drains) with the layer-aware helper. The epilogue drain at 5643 and the `drops.drain` loops drop each queued ref through `drop_base_ref`. This is the "register drain (`run_call` epilogue), `drops_scratch` → layer-aware: skip RC for in-layer refs (`r >= active mark`)" row of §4. Capture the mark from `RegionState.mark` (already set in Task 2.1) or read it via `world.arena.base_mark()` — the drain must compare against the **pre-reset** mark (drain happens before the region's layer is retired).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp`, `cargo fmt --check`.
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/backend/interp.rs rill-lang/tests/runst_regions.rs
git commit -m 'feat(rill-lang): layer-aware register drain and drops scratch'
```

### Task 2.3: Cell stores and `runSTArray` freeze under layers

**Files:**
- Modify: `rill-lang/src/backend/interp.rs` (`newSTRef`/`writeSTRef` handlers, `release_st_cell` at 4169, `freeze_starray` at 5466, `st_array_*` helpers)
- Modify: `rill-lang/src/program.rs` (`StCell` docs — the two variants' refs are now either base refs (RC-managed) or layer refs (never RC'd); note the semantics)
- Test: `rill-lang/src/backend/interp.rs` inline; `rill-lang/tests/runst_regions.rs`

**Interfaces:**
- Consumes: `RegionState.mark` (Task 2.1); `Arena::is_base_ref`.
- Produces:
  - `fn release_cell_value<T, const BUF: usize>(ir, world, cell: StCell, mark: usize)` — layer-aware cell release: for `StCell::Ref(r)` drop only if `is_base_ref(r)`; for `StCell::Array{bounds, elems}` drop `bounds` and each elem only if base, and `put_buf` the `elems` Vec back to the pool (the Vec itself is pool-backed regardless of ref kind). Used by `region_collapse` and by `writeSTRef` overwrite.
  - `freeze_starray` under layers: **keep the existing behavior for base element refs** (per-claim `copy`) but skip RC for layer-owned elements (they are handed to the parent by ownership, so no extra claim is needed — the frozen array's elements ARE the region's claims).

**Design (pinned):**
- `StCell::Array` elements: `elems: Vec<ArenaRef>` is a pool payload; `bounds` is a slot ref. Both may be base (`r < mark`) or layer refs. Base refs keep normal RC accounting on release; layer refs need no RC (they die with the chunk or are handed off).
- `newSTRef`/`writeSTRef` storing a base value into a cell: the cell then holds a base claim, released once when the region collapses (Task 2.1's `region_collapse` calls `release_cell_value`); layer-owned cell values need no release.
- `runSTArray`: freeze copies base elements and leaves layer elements in place (share, no copy). The result array is then traced by `region_collapse`, so layer elements stay alive via the result.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn cell_base_ref_released_once_at_collapse() {
    // region stores a BASE value into an STRef, never reads it back:
    // the cell's claim must be released exactly once at exit.
    let src = r"data Box = Box Float;
b = Box 7.0;
main = runST (\s -> do
    r <- newSTRef b
    writeSTRef r (Box 9.0)
    return 1.0)";
    // stable across many runs: leak would exhaust the arena (latch), double-drop would panic.
}

#[test]
fn runstarray_layers_freeze_keeps_result() {
    let src = r"main = runSTArray (\s -> do
        arr <- newSTArray (1, 4) 0.0
        writeSTArray arr 2 5.0
        return arr)";
    // result is an immutable Array; element 2 == 5.0; stable across runs.
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp runst_regions`
Expected: FAIL (double-drop panic / wrong value / exhaustion latch) before implementation.

- [ ] **Step 3: Implement**

- `release_cell_value`: implement the layer-aware release described above (guard every `drop_ref` with `is_base_ref`; pool-return the `elems` Vec).
- Wire it into `region_collapse` (Task 2.1) replacing the naive per-cell drop, and into the `writeSTRef` overwrite path (which currently calls the old `release_st_cell`).
- `freeze_starray`: for each elem, `if world.arena.is_base_ref(e) { world.arena.copy(e) }` else skip (layer element handed by ownership); same for `bounds`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp`, `cargo fmt --check`.
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/backend/interp.rs rill-lang/src/program.rs rill-lang/tests/runst_regions.rs
git commit -m 'feat(rill-lang): layer-aware ST cell release and runSTArray freeze'
```

### Task 2.4: Full ST verification suite + Strict-mode co-existence

**Files:**
- Test: `rill-lang/tests/runst_regions.rs` (extend); `rill-lang/tests/strict_st.rs` (new, if not already covered)

**Interfaces:**
- Consumes: all of Phase 2.
- Produces: the correctness surface §7.4: nested regions, base refs shared into results, bottom results, escaping closures with region envs.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn nested_regions_outer_reads_inner_result() {
    // inner runST result (an STArray) read by an outer runST.
    let src = r"main = runST (\s -> do
        let inner = runST (\s2 -> do
                        arr <- newSTArray (1, 4) 0.0
                        writeSTArray arr 2 5.0
                        return arr)
        ... )";
    // assert stable result across 1000 runs.
}

#[test]
fn bottom_result_in_region_collapses() {
    // region body hits bottom/undefined element -> ProcessError latch,
    // but the arena and later runs stay healthy.
}

#[test]
fn strict_mode_runst_still_compiles_and_runs() {
    // {-# MODE strict #-} program using runST: exact-GHC semantics, mode-orthogonal.
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p rill-lang --features dsp runst_regions strict_st`
Expected: FAIL where the semantics regressed; PASS for already-correct behaviors after Phase 2.

- [ ] **Step 3: Adjust**

Fix any residual issues surfaced (e.g. RC accounting bugs, missing layer pop on error paths). There is no new allocator code here — this task is the gate that the full correctness surface holds.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp`, `cargo fmt --check`.
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/tests/runst_regions.rs rill-lang/tests/strict_st.rs
git commit -m 'test(lang): ST regions correctness surface under memory layers'
```

---

## Phase 3 — Gates and docs

### Task 3.1: Workspace gates

- [ ] **Step 1: Full workspace verification**

```bash
cargo test --workspace
cargo clippy --all-features --workspace
cargo fmt --check
```

Expected: zero failures, zero warnings, clean fmt.

- [ ] **Step 2: Update docs**

Add a short "Memory layers" note to `docs/src/guides/rill-lang.md` (memory-model section) and `CHANGELOG.md` describing layer collapse and the benchmark result. All English.

- [ ] **Step 3: Commit**

```bash
git add docs/ CHANGELOG.md
git commit -m 'docs(lang): memory layers collapse note and benchmark result'
```

### Task 3.2: Benchmark after + gate

- [ ] **Step 1: Re-run the baseline bench**

Run: `cargo bench -p rill-lang --bench lang_bench -- st_array_workload`.
Record the after number next to the before, and update the `// baseline:` comment from Task 0.1.

- [ ] **Step 2: Compare and record**

Per spec §7 cost 5: adoption gated on a measured win. If the ST workload shows a regression (not a win), the branch does not merge as-is — report the delta and investigate (RC removal should at minimum not regress; teardown O(live+dead) beats O(cells) per-cell drops on region-heavy workloads). Record the before/after in the commit message or a brief note in `CHANGELOG.md`.

- [ ] **Step 3: Commit**

```bash
git add rill-lang/benches/lang_bench.rs CHANGELOG.md
git commit -m 'bench(lang): memory layers after measurement'
```

---

## Self-review checklist

- [ ] §3.1 layer model (base RC free list + per-region bump, `is_base_ref` boundary, region→base refs released **exactly once by the collapse sweep** — no store-time recording, R4) → Tasks 1.1, 2.1, 2.3.
- [ ] §3.2 interleaved temps/result (why pure page handoff fails) → the sweep in Task 1.3; `survivor_chunk_handed_off_by_ownership`.
- [ ] §3.3 collapse algorithm (trace → bulk-recycle → handoff → in-place sweep → cell base-ref release → reset) → Tasks 1.2, 1.3, 2.1, 2.3.
- [ ] §3.4 result safe to leave in place (strict boundary, escaping closures are WHNF, rank-2 prevents STRef escape) → Task 2.1 (`escaping_closure_env_survives_collapse`), Task 2.4.
- [ ] §3.5 base values never reference region values (trace soundness) → `trace_base_root_marks_nothing` in Task 1.2.
- [ ] §3.6 nested regions hand survivor chunks to the outer layer → `nested_collapse_hands_to_outer_layer` (Task 1.3), `nested_regions_outer_reads_inner_result` (Task 2.4).
- [ ] §4 interaction table (exec_runst, finish_region, register drain, newSTRef/writeSTRef, ValueUpdateField, registers at exit) → Tasks 2.1, 2.2, 2.3.
- [ ] §5 allocator requirements (bump mode, mark bits, per-chunk counters, multi-chunk slot store with `SLOT_CHUNK_CAP`, chunk-level wholesale relink + slot-level recycling) → Tasks 1.1–1.4.
- [ ] §7.4 correctness surface (nested regions, base refs shared into results, bottom results, escaping closures) → Task 2.4.
- [ ] §7.5 benchmark bar → Tasks 0.1, 3.2.
- [ ] No `unsafe`, no new dependencies, RT-safe (no new allocations beyond pre-sized bitmap/counters) → Phase 1 design + Global Constraints.
- [ ] Type consistency: `collapse_layer` (arena) vs `region_collapse` (rill-lang) are distinct, correctly named; `push_layer`/`pop_layer` pair; `is_base_ref`/`base_mark` consistent; `RegionState.mark` flows from `push_layer` into `region_collapse`.
- [ ] Review Focus lines each have a pinned test: (1) `base_children_of_survivor_kept` T2.1; (2) `nested_collapse_hands_to_outer_layer` T1.3 + `nested_regions_outer_reads_inner_result` T2.4; (3) `escaping_closure_env_survives_collapse` T2.1; (4) `drain_skips_in_layer_refs` T2.2; (5) `zero_live_chunk_bulk_recycled` T1.3; (6) `bottom_result_retires_layer` T1.3 + `bottom_result_in_region_collapses` T2.4.