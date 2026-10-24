# rill-lang: Layered Value Memory — mark-region ST regions with chunk ownership handoff

> **Status:** Design only. Recorded for future reference; **explicitly not
> scheduled for implementation** and out of scope for the current branch
> (`feature/rill-lang-arrays-fp`). It is a standalone design, independent of the
> collections/arrays value-track work.
> **Date:** 2026-10-19
> **Related:** SP-4 (`docs/superpowers/specs/2026-10-04-rill-core-memory-memoryregion-design.md`),
> lazy value track (SP-5), rank-2 `runST` design
> (`2026-10-10-rill-lang-rank2-thunk-design.md`), JIT-readiness assessment
> (`jit-readiness-value-track-simplifications`, mind memory in `projects/rill`).

---

## 0. Executive summary

The value arena is a fixed-capacity, reference-counted, copy-on-write slot
arena (`rill-lang/src/arena.rs`; post-SP-4: `rill_core::memory::Arena`). Every
`runST` region currently runs **inside the same RC arena**: region temps churn
RC on every instruction, and `finish_region` releases each cell's value
individually (`interp.rs:3596`).

This design makes each ST region a **memory layer** — a bump sub-arena above a
region mark, where region-internal values need **no reference counting at all**
(they all die together). At region exit, the region **collapses into its final
value** via a *mark-region reclamation* pass with **chunk ownership handoff**:

- trace the (already-forced, strict-boundary) result and mark live slots;
- chunks with zero live slots → bulk-recycled (O(1) per chunk);
- chunks containing live slots → handed over to the parent layer **by
  ownership** — refs stay valid (they are arena indices; no pointer fixup) and
  their dead slots are swept in place, releasing any base (parent-owned) child
  refs;
- the result value is never copied and never re-allocated — it stays in its
  exact slots; the region's memory literally collapses into the survivor.

The win: **zero RC traffic inside ST regions** (the dominant value-track cost in
ST-heavy code — arrays, ix, tape, streams, stateful DSP setup), O(1)-ish
teardown for pure-dead pages, and a JIT-friendly shape: the region body compiles
to a tight imperative loop with no RC ops, and region exit is a single runtime
helper (`region_collapse`).

---

## 1. Context and current state

### 1.1 The value arena today

- `ArenaRef = u32` — an index into the arena's slot store; stable across growth.
- `Slot { Occupied { rc: u32, val }, Free { next: Option<ArenaRef> } }` — embedded
  free list; non-atomic RC; single-threaded, owned by one `RillProgram`.
- Registers hold `Option<ArenaRef>` — **even Int/Float/Bool are arena slots**.
- Collections are `Value::List/Map/Set` (Vec-backed pooled payloads); lazy lists
  via `Value::Cons/Nil`; thunks via `Value::Callable{fragment,env,cell}` +
  `Value::ThunkCell(ThunkState)` black-hole.
- SP-4 already moves this arena to `rill_core::memory::Arena<V>` over a paged
  `ChunkedVec` slot store with a single-page RT default. The paging that
  `memory-layers` relies on (§3.3, §5) is the same `ChunkedVec` page machinery —
  this design is a natural extension of SP-4, not a new storage primitive.

### 1.2 ST regions today (`interp.rs:3531-3607`)

`exec_runst` already contains **half** of this design:

1. Force the ST value to WHNF before projecting `unST` (the argument may arrive
   as a `ValueLazy` thunk).
2. Push a `RegionState { cells, cell_base, region_id }`, apply `unST` to a fresh
   `Opaque(region_id)` token.
3. Force the resulting pair to WHNF.
4. **Strict exit boundary** (`interp.rs:3572`): "leaving the region is a strict
   boundary, so the payload must be materialized while its cells are still
   alive" — the region result is forced before anything is released.
5. `copy_owned` the result (`interp.rs:3580`) — a **shallow share-copy**
   (RC++ on children).
6. `finish_region` (`interp.rs:3596`): pop the region, `take()` every cell and
   `drop_ref` its value (per-cell RC release), truncate the cell table.

What is missing — exactly the idea behind this design — is that **region temps
and cells currently live in the shared RC arena**: every instruction inside the
region pays RC churn (register drain in `run_call`, `drops_scratch`), and
teardown is O(cells) per-cell drops.

### 1.3 Where the cost concentrates

The ST machinery is the *only* value-track code that is genuinely imperative and
loop-shaped (array construction/indexing, ix monad, tape/stream setup, stateful
DSP init). It is also the code a future Cranelift JIT must compile into tight
native loops. Removing per-op RC there is the highest-leverage value-track
simplification after unboxed scalar registers (see the JIT-readiness
assessment).

---

## 2. Goals and non-goals

### Goals

1. **Zero RC inside ST regions.** Region-internal values are never reference
   counted: they all die with the region.
2. **Bulk release at region exit.** Pure-dead pages/chunks are recycled whole
   (O(1) per chunk), replacing per-cell `drop_ref` teardown.
3. **Zero-copy result escape.** The region result collapses out of the region
   **without deep-copying** and **without allocating new slots** — no 2×
   transient of the result tree.
4. **Chunk ownership handoff.** Pages containing live values are transferred to
   the parent layer by ownership; refs stay valid with no pointer fixup.
5. **JIT-friendly boundary.** Region exit is one runtime helper
   (`region_collapse`); region bodies compile with no RC ops in the hot path.
6. **Preserve existing semantics.** The strict exit boundary (result forced at
   exit) stays; the rank-2 ST invariant stays; bottom stays a value.

### Non-goals

- **No implementation now.** This document is a design record, not a plan; it
  is deliberately deferred and is **not** part of the arrays/collections work
  on `feature/rill-lang-arrays-fp`.
- **No `MemoryRegion<T>` change.** The tier-1 buffer slab of SP-4 is a
  separate mechanism; layers apply only to the *value* arena.
- **No change to ST typing or semantics.** `runST` still supplies a fresh region
  per call; the rank-2 `forall s` still prevents STRefs from escaping.
- **No snapshot/copy-on-write semantics.** Values are immutable (COW updates
  already allocate fresh values), so a region cannot disturb the parent — a
  layer is about *allocation placement + bulk free + result collapse*, not
  shadow-copying the parent.

---

## 3. Design: memory layers

### 3.1 The layer model

- The arena is a **stack of bump layers**: the base layer (non-region memory,
  RC-managed) plus one layer per active ST region, pushed LIFO.
- A region's allocations go **above its mark** (bump). Registers during the
  region hold `Option<ArenaRef>` exactly as today; a ref is *in-layer* iff
  `ref >= region_base_mark`.
- **RC exists only in the base.** A ref `r` is RC-managed iff `r < global_base`.
  Refs `>= global_base` are layer-owned and never RC'd by anyone.
  - Region-internal refs: no RC (the whole layer dies together).
  - Region → base refs: **recorded** on the layer's external list at store time
    (a cheap `r < global_base` compare + conditional push), released once at
    exit.
  - Region → region refs (nested layers): no recording, no release — the outer
    layer outlives the inner, and the outer's own exit handles everything.

### 3.2 Why "just hand over the pages" is not enough

Allocation order **interleaves** temps and result-reachable values, so a page at
region exit almost never contains only survivors. Handing a page to the parent
as-is drags garbage with it; and garbage cannot be reclaimed without knowing
what is live. The only ways to get survivor-pure pages are **evacuation**
(copy survivors onto clean pages — a copy again, just organized as a copying
collector) or **reclaiming the dead in place**. This design takes the latter:
no copying at all.

### 3.3 The collapse: mark-region + chunk ownership handoff

At region exit, `region_collapse(result)`:

1. **Trace** the result (already forced at the strict boundary), marking live
   slots with a **mark bit**, and bump a **per-chunk live counter** (so
   "chunk has zero live slots" is an O(1) counter check). The trace handles
   sharing via the mark bit.
2. **Bulk-recycle** every chunk with `live counter == 0` — the chunk is
   returned wholesale to the free pool. This is the bulk-release half.
3. **Hand over** every chunk with `live counter > 0` to the parent layer **by
   ownership**: the chunk stays allocated, now parent-owned; all refs into it
   remain valid (arena indices, no relocation).
4. **Sweep the survivor chunks in place**: for each dead slot on them, release
   its base child refs (RC--) and recycle the slot. (Region-owned child refs
   die with the chunk; they are not touched.)
5. **Release cell base-refs**: each cell's value that is a base ref (`newSTRef
   parentVal`) is RC--'d; cells die with the region. Region-owned cell values
   are either survivors (traced through the result, if the result reads them)
   or dead (swept with their chunk).
6. **Reset**: pop the layer; the region's mark is retired.

The result value never moves and is never copied: its slots are now simply
parent-owned. The region's memory literally **collapses into the final value** —
the survivor chunks are all the region leaves behind.

**Teardown cost:** O(live) trace (unavoidable — *any* scheme must identify the
result subtree) + O(dead slots on survivor chunks) + O(1) per pure-dead chunk.
No deep copy, no new slots, no 2× transient.

### 3.4 Why the result is already safe to leave in place

The strict exit boundary (`interp.rs:3572`) forces the result before teardown,
so no unforced thunk that could allocate region cells escapes. Escaping
closures (a `Value::Callable` in the result) are WHNF by construction; their env
records are part of the traced result and stay in place with the survivor
chunks. The rank-2 ST typing independently guarantees no STRef (cell) can
appear in the result type. Together with in-place collapse, lazy-escape is
closed without any copying.

### 3.5 Base values never reference region values

During a region, allocations go to the region layer, so **no new base values are
created mid-region**; pre-existing base values predate the region. Hence tracing
from the result covers every region survivor, and base child refs are always
leaf refs (RC-managed, never layer-owned). This is the invariant that makes
cross-layer accounting sound.

### 3.6 Nested regions

An inner region's collapse hands its survivor chunks to the **outer layer** (the
parent layer, not the base). When the outer exits, its sweep covers the
handed-over chunks as well. The single rule — *RC exists only below the global
base; refs above it are never RC'd or released by any layer* — extends
directly. Bottom results collapse trivially (Bottom is copied out; the layer
dies).

---

## 4. Interpreter interaction points

| Site | Today | With layers |
|---|---|---|
| `exec_runst` (`interp.rs:3531`) | force + apply + force pair + force result + `copy_owned` + `finish_region` | force + apply + force pair + force result + **`region_collapse(result)`** (trace → chunk recycle → survivor handoff → dead-slot sweep → cell base-ref release) |
| `finish_region` (`interp.rs:3596`) | per-cell `take` + `drop_ref` + truncate | absorbed into `region_collapse`; cell *base* refs only are released |
| Register drain (`run_call` epilogue), `drops_scratch` | unconditional RC drops | layer-aware: skip RC for in-layer refs (`r >= active mark`). Must compare against the pre-reset mark (drain before the mark is retired, or capture the mark first) |
| `newSTRef` / `writeSTRef` cell stores | cell holds a ref (RC-managed via `finish_region`) | cell base refs recorded/released; region-owned cell values need no RC |
| `ValueUpdateField` (COW) | COW copies to a fresh slot | unchanged; a region COW-updating a base record allocates the new value into the layer; its base children are handled by the sweep |
| Registers at region exit | caller registers never hold in-layer refs (region fragments run in their own scratch range) | same, provided the drain is layer-aware |

---

## 5. Allocator requirements (`rill_core::memory`)

Post-SP-4 the arena is `Arena<V>` over a paged `ChunkedVec` slot store — the
natural substrate for layers:

- **Bump region mode**: a region mark (`usize`) above which allocations bump;
  the base layer keeps the embedded RC free list.
- **Mark bits** per slot (a reused `Vec<bool>` sized to arena capacity, or a
  side bit-set) for the exit trace.
- **Per-chunk live counters** for O(1) pure-dead chunk detection.
- **Chunk-level recycling**: returning a whole chunk to the free pool (link its
  slots' free list), and slot-level recycling within survivor chunks.

All RT-safe: layers are a compile-time discipline; nothing allocates on the
signal path beyond the pre-sized arena; the region push/pop and the
`region_collapse` pass are deterministic, single-threaded work.

---

## 6. Benefits

1. **Zero RC inside ST regions** — eliminates the dominant value-track cost in
   the only loop-shaped, imperative part of the track (arrays, ix, tape,
   streams, DSP setup).
2. **Teardown O(live + dead-on-survivor-chunks + pure-dead-chunks)** instead of
   O(cells) per-cell drops; pure-dead pages are O(1).
3. **Capacity recycling**: region temp churn no longer accumulates against the
   fixed capacity; the bump mark is retired wholesale.
4. **No result copy**: no deep-copy, no new slots, no 2× transient of the result
   tree; the current `copy_owned` (shallow share-copy) is not just removed — it
   is rendered unnecessary because children stay in place.
5. **JIT-friendly boundary**: the region body compiles to a tight loop with no
   RC ops (only the store-boundary compare for base refs); exit is one runtime
   helper `region_collapse`. Compounds with unboxed scalar registers.

---

## 7. Costs and risks

1. **Allocator refactor** in `rill_core::memory`: bump mode + mark bits +
   per-chunk counters + chunk recycling — a real change to the arena, gated on
   the SP-4 paged store landing first.
2. **Exit trace** is O(result reachable set) — small in practice (results are
   usually small), but it is new work at every region exit.
3. **Bookkeeping on base-ref stores** — one compare + conditional push per store
   of a base ref into a layer value.
4. **Correctness surface** — the base/layer RC boundary must be pinned by tests:
   nested regions, base refs shared into results, bottom results, escaping
   closures with region envs.
5. **Benchmark bar**: per the SP-4 safety policy precedent, adoption should be
   gated on a measured win (`lang_bench` with an ST/array workload, before vs
   after) — not adopted speculatively.

---

## 8. Alternatives considered

| Alternative | Verdict |
|---|---|
| **Deep-copy collapse** (copy result subtree into the base at exit) | Rejected: copies the result, allocates O(result) new slots, 2× transient; the current `copy_owned` is already insufficient (shallow) and would need a full deep copy. |
| **Pure page ownership handoff** (no copy, no mark) | Rejected: pages are not survivor-pure (allocation interleaves temps and result values); handing them over drags garbage that can't be reclaimed without liveness knowledge. |
| **Evacuation / copying GC at exit** | Equivalent to deep-copy cost (O(survivors) copy + fresh pages); nothing gained over mark-region here. |
| **Transfer whole region, let parent RC reclaim** | Rejected: unreferenced dead values are never dropped by a pure RC arena → leak against fixed capacity; would need a sweep anyway (i.e., mark-region). |
| **Mark-region with chunk handoff (this design)** | Accepted: no copy, no new slots, O(1) bulk for pure-dead chunks, in-place reclaim on survivor chunks. |

Real-system analogues: **mark-region** / **Immix** (line-level reclaim inside a
region, survivors stay put), **tenuring** / **page promotion** in object heaps
(the evacuation variant — a copy). This design is the zero-copy member of that
family.

---

## 9. Open questions

1. Per-chunk live counter vs. a pure marking pass (the counter makes pure-dead
   detection O(1); whether the overhead is worth it on the trace is a
   benchmark question).
2. Mark-bit storage: side bit-set vs. slot padding in the paged store.
3. Whether the layer boundary should be visible to the register model
   (`run_call` drain) or encapsulated entirely in the arena + `region_collapse`.
4. Profile first: measure the actual share of RC traffic in ST-heavy code
   before committing to the refactor (a dedicated `lang_bench` ST workload).

---

## 10. Status

**Design record only.** Not a plan; no implementation is scheduled. Explicitly
**out of scope** for the current branch (`feature/rill-lang-arrays-fp`) and for
the collections/arrays value-track work in general. Revisit when (a) SP-4's
paged arena has landed and (b) a JIT effort makes the value-track simplification
list actionable.
