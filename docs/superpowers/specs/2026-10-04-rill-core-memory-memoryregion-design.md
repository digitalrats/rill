# rill-core Memory Subsystem: Generic Arena + MemoryRegion (SP-4)

> **Status:** Implemented (P1+P2) — P3 (algorithm internal buffer migrations)
> pending. Audited 2026-10-19: `rill_core::memory` (`Arena<V>`, `ChunkedVec`,
> `BufferPool`, `MemoryRegion`/`RegionHandle`/`BufferAllocator`), `TapeRing`/
> `RingStorage`, region-backed `@` delays, and the `Value::Buffer` buffer table
> are in the code; `Algorithm`/`MultichannelAlgorithm` dropped their `Send`/
> `Sync` supertraits. No `RegionHandle` usage yet in `rill-fft`/`rill-core-dsp`/
> `rill-core-model`/`rill-router` (P3 open).
> **Date:** 2026-10-04 (audited 2026-10-19)
> **Branch:** `feature/rill-lang-arrays`
> **Scope:** SP-4 of the rill-lang / rill-core work. Move the value-arena memory
> manager from `rill-lang` into `rill-core` (genericized over the payload type),
> and introduce a region-based allocator for large static buffer memory
> (`MemoryRegion`) with long lifetimes and reference counting. Together they
> form a centralized, RT-safe memory subsystem in `rill-core`.

---

## 0. Executive summary

Rill has two unrelated memory pools today:

1. **The value arena** (`rill-lang/src/arena.rs`) — a fixed-capacity, reference
   counted, copy-on-write slot arena with an embedded free list and a
   size-classed payload `BufferPool`. It is the only live runtime allocator in
   the workspace, but it lives in `rill-lang` and is hardwired to the language's
   `Value` enum.
2. **Ad-hoc buffer memory** — `TapeLoop` (`Box<[T]>`), `DelayRing`
   (`FixedBuffer<T, 65536>`), FFT scratch, wavetables, physical-model grids, all
   pre-allocated independently in each algorithm's constructor with no shared
   accounting. `rill-core/src/buffer/pool.rs` is dead code.

SP-4 unifies them: `rill-core` gains a `memory` module containing (a) the
value arena genericized to `Arena<V>` with an `ArenaValue` payload trait, and
(b) a chunked, reference-counted region allocator `MemoryRegion<T>` with
`RegionHandle<T>` handles that become the backing memory for tapes, delay
lines, FFT scratch, wavetables, and algorithm internal buffers.

The two-tier memory model that results:

| Tier | Mechanism | Owns |
|---|---|---|
| **Tier 1 — region slabs** | `MemoryRegion<T>` + `RegionHandle<T>` | Tapes, all `@` delay lines, FFT scratch, wavetables, algorithm internal buffers, future `Array`/`Vector` big objects |
| **Tier 2 — inline state** | `FixedBuffer<T,N>`, `DelayLine<T,N>`, `ScalarVector1` fields | Port buffers, small fixed state, filter/oscillator coefficients and history |

The arena and the region system share a single low-level grow-safe storage
primitive (`ChunkedVec<T>`), so arena *extension* (`growable-arena`) and buffer
*allocation* reuse the same page/chunk machinery — one memory subsystem, one
accounting model.

All ring-shaped buffers — `TapeLoop`, `DelayLine`, and the `@` delay rings —
fold onto one unified `TapeRing<T, S: RingStorage<T>>` core, so SP-4 replaces
three ad-hoc ring implementations with a single ring type over region-backed
(or inline) storage.

**Non-goal:** changing the `Algorithm` trait. Algorithms stay stateful
(`process(&mut self, …)`); region-backed internal buffers are the *enabling*
step for a future pure-function refactor, not the refactor itself.

**`unsafe` allowance (scoped):** the new `rill_core::memory` module is exempt
from the crate-level `#![deny(unsafe_code)]` (`#![allow(unsafe_code)]` at
module level), but **only** under the policy in §2.1. Everywhere else in
`rill-core`, `deny(unsafe_code)` stays.

---

## 1. Context and current state

### 1.1 The value arena (`rill-lang/src/arena.rs`, 726 lines)

- `ArenaRef = u32` — an **index** into `Arena.slots`, stable across `Vec`
  reallocation.
- `Slot { Occupied { rc: u32, val }, Free { next: Option<ArenaRef> } }` —
  embedded free list (Alexandrescu "Affordable Allocator").
- Non-atomic RC — the arena is owned by one single-threaded `RillProgram`.
- COW: `mutate()` copies to a fresh slot when `rc > 1`, recounting children.
- `BufferPool` — six size classes (8/16/32/64/128/256 refs) of `Vec<ArenaRef>`
  payload buffers; `take` returns `None` on exhaustion in RT mode (caller
  latches a `ProcessError`), `growable-arena` feature allocates fresh.
- Owned by `RillProgram` (`program.rs:57`), used by the interpreter
  (`backend/interp.rs`, ~130 `prog.arena.*` call sites) and by `set_param` /
  value outputs.
- The arena's `Value`-specific logic: recursive drop over child refs, COW
  child recounting, pooled payload clone (`clone_pooled`), pooled payload
  return on drop.

### 1.2 Buffer memory today

| Buffer | Backing | Where |
|---|---|---|
| `FixedBuffer<T, SIZE>` | inline `[T; SIZE]` | `rill-core/src/buffer/buffer_trait.rs` |
| `DelayLine<T, MAX_DELAY>` | inline `[T; MAX_DELAY]` | `rill-core/src/buffer/delay.rs` |
| `TapeLoop<T>` | `Box<[T]>` | `rill-core/src/buffer/tape.rs` |
| `DelayRing<T, 65536>` (one per `@` site) | `FixedBuffer` (inline, in a `Vec`) | `rill-lang/src/program.rs:108` |
| FFT scratch / convolver | `Vec` fields | `rill-fft/src/{overlap_add,real_fft}.rs` |
| Wavetable / sample tables | `Box<[T]>` | `rill-core-dsp/src/generators/reader.rs` |
| Physical models | `Vec<T>` grids | `rill-core-model/src/{string,plate}/` |

`SharedCell<T>` / `ResourceRegistry<T>` (`rill-core/src/buffer/registry.rs`)
provide Rc-based sharing of `Box<dyn DelayBuffer<T>>` for tape heads
(single writer + many readers) — a capability split that is orthogonal to where
the memory lives.

`rill-core/src/buffer/pool.rs` is **dead code**: it references a nonexistent
`super::aligned::AlignedBuffer`, is not declared in `buffer/mod.rs`, and nothing
uses it. It is not revived; the live `BufferPool` is the arena's.

### 1.3 Prior art

- SP-0 (`docs/superpowers/specs/2026-09-30-rill-lang-categories-design.md`)
  planned a "page arena" and the embedded free list + `BufferPool`; SP-4
  realizes the page part via the shared chunked primitive.
- SP-3 (`docs/superpowers/specs/2026-10-02-rill-lang-signal-buffer-design.md`)
  explicitly deferred the "big objects" memory subsystem and `Array`/`Vector`
  collection types to a later stage; SP-4 lays the foundation (§7).

---

## 2. Goals and non-goals

### Goals

1. Move the memory manager (arena + payload pool) from `rill-lang` to
   `rill-core`, genericized over the payload type, with **zero behavior change**
   and a mechanical rill-lang migration.
2. Introduce `MemoryRegion<T>` — a chunked, grow-safe, reference-counted slab
   for large static buffer memory — with `RegionHandle<T>` handles.
3. Back tapes, all `@` delay lines, FFT scratch, wavetables, and algorithm
   internal buffers with regions (tier 1); keep small fixed state inline
   (tier 2); unify all ring-shaped buffers (tape, delay, `@` rings) on one
   `TapeRing<T, S>` core.
4. Unify arena extension and buffer allocation on one shared `ChunkedVec<T>`
   primitive, so `growable-arena` growth carves pages through the same
   machinery.
5. Preserve RT-safety: zero allocation on the signal path; all carving happens
   at build time.
6. Design `RegionHandle` to be embeddable in future value-track
   `Array`/`Vector` big objects (no language types shipped).

### Non-goals

- No `Algorithm` trait change; algorithms remain stateful (pure functions are
  *enabled*, not implemented).
- No `Array`/`Vector` language types (P4, future).
- No new external dependencies.

### 2.1 Safety policy for the memory module

`unsafe` is permitted inside `rill_core::memory` (and the ring/region types it
feeds) only when **all** three conditions hold:

1. **Proven performance benefit.** A criterion benchmark (`lang_bench` for the
   arena, a ring/region micro-benchmark for `TapeRing`/`RegionHandle`) must show
   a measured gain from the `unsafe` path over its safe fallback (e.g. the
   `RefCell`-guarded variant) on an RT path. Unsafe is never introduced
   speculatively.
2. **Documented invariant — the "formal proof" bar.** Every `unsafe` block
   carries a `// SAFETY:` comment stating the invariant that makes it sound:
   the single-threaded DAG, disjoint or read-shared handle ranges, `offset+len`
   within the slab, write-exclusive access. The invariant must be mechanically
   checkable from the surrounding code (assertions/bounds at the boundary).
   Precedent: `registry.rs` (`SharedCell`), `write_head.rs`/`read_head.rs`
   (`unsafe impl Send/Sync` for single-threaded heads).
3. **Elevated test coverage.** Unsafe paths get unit tests that exercise the
   boundary conditions of the invariant, plus **Miri**
   (`cargo +nightly miri test -p rill-core`) to catch UB. Miri runs are part of
   the phase's verification (documented; required when nightly is available).

The safe fallbacks remain the default anywhere the benchmark does not justify
`unsafe` (e.g. `ChunkedVec` stays safe code).

### 2.2 `Algorithm: Send + Sync` is redundant (verified)

Investigated and **empirically verified** (2026-10-04): removing `Send + Sync`
from `Algorithm<T>` and `Send` from `MultichannelAlgorithm<T>` compiles the
whole workspace cleanly (`cargo check --workspace`, zero warnings/errors under
rill-core's `deny(unused)`). The signal graph never crosses a thread boundary:

- The engine is **built inside** the spawned signal-thread closure
  (`rill-adrift/src/modular/mod.rs:222`, `case.start(move |running| { …
  build engine … })`); `JoinHandle<()>` returns unit, and `case.rs:21`
  documents "Graph is !Send".
- I/O process callbacks are `Box<dyn FnMut(&ClockTick)>` **without** a `Send`
  bound (`rill-core/src/io.rs:37`); backends stash them behind raw pointers
  (`portaudio.rs:28`, `pipewire.rs:41`), so the captured program never needs
  `Send`.
- No static/global registry holds algorithms (`full_registry()`/`full_ffi()`
  build fresh per call); actor messages are `CommandEnum`, never algorithms.
- `WriteHead`/`ReadHead` already note their `unsafe impl Send/Sync` exists
  only for "the builtin registry's static bounds" (`write_head.rs:12`,
  `read_head.rs:30`).

**Plan impact (SP-4):** the bounds are removed from both traits as part of
P1. Consequences: (a) `RillProgram`/`ProgramEngine` no longer need `Send`;
(b) the `unsafe impl Send/Sync` on `WriteHead`/`ReadHead` (and any analogous
impls region-backed algorithm types would need) become unnecessary — a
`RegionHandle` (`Rc<UnsafeCell<…>>`, `!Send`) can live inside an `Algorithm`
without an unsafe `Send` impl. The `+ Send + Sync` bounds on the factory
closures (`BlockFactory`/`MultichannelBlockFactory`, `builtin.rs`/`ffi.rs`)
are **kept** — they are orthogonal (a `Send` closure may return a non-`Send`
value) and preserve the option of a shared/static registry.

---

## 3. Architecture: `rill_core::memory`

```
rill-core/src/memory/
  mod.rs      — module docs + re-exports
  chunked.rs  — ChunkedVec<T>          (grow-safe, stable-address storage)
  arena.rs    — ArenaRef, ArenaError, ArenaValue, Slot<V>, Arena<V>
  pool.rs     — BufferPool             (size-classed Vec<ArenaRef> pool, moved)
  region.rs   — MemoryRegion<T>, RegionHandle<T>, BufferAllocator<T>
```

Crate-root exports: `rill_core::memory::{Arena, ArenaValue, ArenaRef,
ArenaError, BufferPool, ChunkedVec, MemoryRegion, RegionHandle,
BufferAllocator}`; the allocator types join `rill_core::prelude`.

### 3.1 `ChunkedVec<T>` — the shared grow-safe primitive

```rust
/// Contiguous logical vector split into stable chunks. Appending a chunk never
/// moves existing elements, so addresses (chunk + index) stay valid across
/// growth. Single-threaded; no atomics.
pub struct ChunkedVec<T> {
    chunks: Vec<Box<[T]>>,   // each chunk is a separate stable allocation
    chunk_cap: usize,        // power of two; logical index -> (chunk, offset)
    len: usize,              // logical length
}
```

- Logical indexing `i` maps to `(chunk = i >> BITS, off = i & MASK)` with
  `BITS = chunk_cap.trailing_zeros()`.
- `push_chunk()` appends; addresses of existing elements never change.
- `with_capacity(cap, chunk_cap)` pre-allocates enough chunks for `cap` slots.
- Used by both `Arena<V>` (its slot store) and `MemoryRegion<T>` (its slab).

### 3.2 `Arena<V>` — the genericized value arena

```rust
pub trait ArenaValue: Clone + std::fmt::Debug {
    /// Enumerate every child `ArenaRef` owned by this value (recursive drop,
    /// COW recounting).
    fn visit_refs(&self, f: impl FnMut(ArenaRef));
    /// Clone the value, serving container payloads from the pool. Defaults to
    /// plain `Clone` (heap-backed payloads).
    fn clone_pooled(&self, _pool: &mut BufferPool) -> Result<Self, ArenaError> {
        Ok(self.clone())
    }
    /// Detach the pool-backed payload buffer, if any, so the arena can recycle
    /// it after the value's children are dropped. Defaults to `None`.
    fn detach_payload(&mut self) -> Option<Vec<ArenaRef>> {
        None
    }
}

pub struct Arena<V: ArenaValue> {
    slots: ChunkedVec<Slot<V>>, // paged store (see 3.3)
    free: Option<ArenaRef>,
    live: usize,
    capacity: usize,            // logical slot count, unchanged semantics
    next_gen: u32,
    pub pool: BufferPool,
}
```

- API preserved with the same names: `with_capacity`, `alloc`, `copy`,
  `drop_ref` (recursive via `visit_refs` + `detach_payload`), `mutate` (COW),
  `clone_pooled`, `get`/`get_mut`, `rc`, `take_buf`/`put_buf`, `capacity`,
  `live`.
- `drop_ref`: pop the value, `visit_refs(|r| self.drop_ref(r))`, then
  `detach_payload()` → `put_buf`. Order preserved (children first, then pool).
- `mutate` (COW): `clone_pooled` (pooled), recount children via `visit_refs` +
  `copy`, `drop_ref` original, `alloc` fresh.
- `growable-arena` growth: on empty free list and `pool.growable`, carve a new
  page via `ChunkedVec::push_chunk` and link its slots into the free list.

### 3.3 Paged slot store — (b)-light

The arena's slot store is a `ChunkedVec<Slot<V>>` (paged) rather than a flat
`Vec`:

- **RT mode** pre-allocates exactly **one page** at build: `page_cap =
  capacity.next_power_of_two()` (a power of two keeps the index→slot mapping a
  shift). The free list links exactly `capacity` slots (logical capacity
  semantics are unchanged — `capacity()` still reports the logical count, so
  build-time capacity bounds and existing tests hold).
- **Fast path** — `if self.slots.chunks().len() == 1 { page[0][r] } else {
  page[r >> BITS][r & MASK] }`. During RT the page count never changes, so the
  branch is perfectly predicted; in a loop over slots the single-page pointer is
  hoisted, making the cost ≈ a flat `Vec<Slot>`.
- **`growable-arena`** (non-RT) appends pages via `push_chunk`. This is the
  only path that grows, and it is never on the signal path.

Rationale: the arena never needed the page model at RT (it never grows), but
sharing the `ChunkedVec` primitive unifies accounting and realizes the
page-arena design. The fast path and the one-page default bound the RT cost;
a before/after benchmark (`rill-lang/benches/lang_bench.rs`, criterion) is a
mandatory P1 check.

**Why pages at all — Alexandrescu's "Affordable Allocator".** The paged slot
store is modeled on Alexandrescu's affordable-allocator scheme. The motivating
empirical claim: allocating large, contiguous chunks (pages) instead of many
small, scattered blocks reduces allocator churn and produces better locality —
TLB pressure is lower, and the OS memory manager on x86 backs a handful of
large mappings with far fewer page-table walks than a swarm of small `Vec`
reallocations. This is plausibly a consequence of the x86 memory manager and
DDR behavior (page granularity, prefetch, and DRAM row activation all favor
sequential large regions over fragmented small ones). The region slab (§3.5)
gets the same benefit for buffer memory. As with any performance claim, the
mandatory P1 benchmark (`lang_bench`, and a region micro-benchmark) must
confirm it on this workload before the design is kept in its final form.

### 3.4 `BufferPool` — moved as-is

`rill_lang::arena::BufferPool` moves verbatim to `rill_core::memory::pool`
(size-classed `Vec<ArenaRef>` pool, `growable` flag). It stays a separate pool:
it pools *payload Vecs* (`Vec<u32>`), not raw bytes or slots, and is already
RT-safe.

### 3.5 `MemoryRegion<T>` and `RegionHandle<T>` — tier-1 slab

```rust
pub struct MemoryRegion<T> {
    slab: ChunkedVec<T>,     // grow-safe; element T (f32, f64, …)
    next: usize,             // bump cursor (linear over chunks)
}
pub struct RegionHandle<T> {
    region: Rc<UnsafeCell<MemoryRegion<T>>>,
    offset: usize,
    len: usize,
}
```

- **Chunked, not a growing `Vec`.** A `Vec<T>` reallocates on growth and
  invalidates every outstanding `offset`; `ChunkedVec` keeps addresses stable.
  This is a hard correctness requirement, not an optimization.
- Handles are allocated **non-spanning**: the bump cursor skips to the next
  chunk when a request would cross a chunk boundary (bounded waste ≤ chunk_cap
  per skip), so `as_slice()` is always contiguous.
- `RegionHandle` is `Clone` (Rc). RC is for *sharing* (one write head + many
  read heads; several taps on one delay), not early free — regions are static
  with program lifetime.
- Access via `Rc<UnsafeCell<…>>` + audited pointer deref (the `SharedCell`
  precedent, `registry.rs`): `as_slice() -> &[T]` / `as_mut_slice() -> &mut [T]`
  dereference the slab under a documented invariant (single-threaded graph;
  disjoint or read-shared handle ranges; single-writer / many-readers
  convention). The `with_data`/`with_data_mut` closure forms encapsulate the
  `unsafe` so call sites stay safe. Each `unsafe` block carries a `SAFETY`
  comment (§2.1); the module is Miri-checked. With §2.2, `Algorithm` no longer
  requires `Send`/`Sync`, so `RegionHandle` (`Rc<UnsafeCell<…>>`, `!Send`)
  needs no `unsafe impl Send/Sync` to live in an algorithm; the
  `WriteHead`/`ReadHead` precedent is superseded and those impls are dropped
  in P1. A `RefCell`-guarded fallback is kept for paths where the benchmark
  does not justify the unsafe path.

```rust
pub struct BufferAllocator<T> {
    region: Rc<UnsafeCell<MemoryRegion<T>>>,
}
impl<T: Transcendental> BufferAllocator<T> {
    pub fn new(initial: usize) -> Self;
    pub fn allocate(&mut self, len: usize) -> Option<RegionHandle<T>>; // bump
    pub fn capacity(&self) -> usize;
    pub fn used(&self) -> usize;
    pub fn remaining(&self) -> usize;
}
```

- Growth happens only at build time (`MemoryRegion` grows on demand while
  builtins are being constructed); `process()` never touches the allocator.
  `allocate` returns `None` on exhaustion when growth is disabled — a build-time
  error, matching the arena's "capacity exhaustion is a build-time bug" policy.

### 3.6 `TapeRing<T, S>` — the unified ring

`TapeLoop`, `DelayLine`, and the `@` delay rings share the same
**non-consuming write-head ring math**: `write` advances a single head, reads
happen at a delay behind it, nothing is ever popped (unlike the consuming FIFO
`RingBuffer`). They differ only in storage. SP-4 extracts the ring into one type
over a storage abstraction:

```rust
pub trait RingStorage<T> {
    /// Usable length of the ring (tape capacity / delay length).
    fn len(&self) -> usize;
    fn with_data(&self, f: impl FnOnce(&[T]));
    fn with_data_mut(&mut self, f: impl FnOnce(&mut [T]));
}

pub struct TapeRing<T, S: RingStorage<T>> {
    storage: S,
    head: usize,
}
```

- `read(delay)` (delay 0 = most recent), `read_block(delay, out)`,
  `write_block(in)`, `read_interpolated`, `write`, `clear` — the exact math
  today in `TapeLoop`. The equivalence that makes `DelayRing` a `TapeRing`:
  `DelayRing::read_block(out)` ≡ `TapeRing::read_block(0, out)` (both read the
  whole ring oldest-first) and `write_block` is identical, so a `@ n` ring is a
  `TapeRing` of length `n` (the region handle is carved at exactly `n` samples,
  so no separate length field is needed).
- Wrappers:
  - `TapeLoop<T>` = `TapeRing<T, RegionHandle<T>>` + the `DelayBuffer<T>` impl
    (the `Reader`/`Writer` capability layer is untouched).
  - `@` delay sites = `TapeRing<T, RegionHandle<T>>` (read via `read_block(0, ·)`).
  - `DelayLine<T, N>` = `TapeRing<T, [T; N]>` **plus** its extra state —
    `delay_samples`, `sample_rate`, `set_delay`/`set_delay_samples`,
    `AtomicStats`. `DelayLine::write(input) -> T` = `storage.write(input)` +
    `read(delay_samples)`; `read()` = `read(delay_samples)`. The fold is
    additive and API-preserving, and the storage stays inline `[T; N]` (tier 2).
- `RingBuffer` (the consuming FIFO queue, `ring.rs`) is **not** part of this —
  it is a queue next to `PipeBuffer`, not a ring core.
- Lives in `rill_core::buffer` (`tape_ring.rs`); `RingStorage` impls ship for
  `RegionHandle<T>`, `Box<[T]>`, and `[T; N]`.

---

## 4. rill-lang adoption

### 4.1 Arena

- `Value` implements `ArenaValue` in `rill-lang`:
  - `visit_refs` — the child-dispatch currently inside `drop_ref`/`mutate`
    (Record/Sum fields, Newtype inner, Closure env, List/Map/Set elements).
  - `clone_pooled` — the existing phase-1/2/3 pooled clone (read shape, pool
    payload, copy refs).
  - `detach_payload` — `mem::take` of the Record/List/Set payload `Vec`.
- `RillProgram.arena: Arena<Value>` (`program.rs:57`); all `prog.arena.*` call
  sites in `interp.rs` keep their exact method names — mechanical.
- Back-compat: `rill_lang::arena` re-exports the moved types (`Arena`,
  `ArenaRef`, `ArenaError`, `BufferPool` from `rill_core::memory`) and keeps
  `Value`, `ValueKind`, `impl ArenaValue for Value`. `pub use arena::{…}` in
  `lib.rs` unchanged; `prog.arena()`, `rill_lang::arena::Value`, and the
  ~100 test call sites keep compiling.

### 4.2 Regions in `RillProgram<T, BUF>`

- `RillProgram` owns a `BufferAllocator<T>`, sized at build from IR
  (`sum(delay_lens) + sum(tape caps) + builtin buffer budget + headroom`).
- **`@` delays:** `DelayRing<T, 65536>` → `TapeRing<T, RegionHandle<T>>`
  (length = the site's compile-time delay length; read via `read_block(0, ·)`).
  Every delay site carves a slice (all delays region-backed).
- **Tapes:** `tape_loop <cap>` carves a `cap`-sample handle; `TapeLoop`
  (`rill-core`) becomes `TapeRing<T, RegionHandle<T>>` + the `DelayBuffer`
  impl (replacing `Box<[T]>` and its inline ring math). `SharedCell`/
  `ResourceRegistry`/`Reader`/`Writer` and the `shared_handles()` split stay
  exactly as-is (they are the ownership-split layer, orthogonal to backing
  memory).

---

## 5. Algorithm internal buffers

### 5.1 Factory contract

The factory closures in rill-lang gain an optional allocator so builtins can
request region memory at construction:

```rust
type BlockFactory<T> =
    Box<dyn Fn(&[f64], f32, Option<&mut BufferAllocator<T>>) -> Box<dyn BlockBuiltin<T>> + Send + Sync>;
```

Same change for `MultichannelBlockFactory`, `ResourceBlockFactory`,
`ResourceMultichannelBlockFactory` in `rill-lang/src/builtin.rs` and
`rill-lang/src/ffi.rs`. DSP crates' `register_foreign_*` closures take the extra
parameter; non-migrated factories ignore it (pass `None`). This is a mechanical
signature change across ~10 crates' `lang/register.rs` / `lang.rs` files, with
no behavior change for existing algorithms.

### 5.2 Migration list (incremental, P3)

Each algorithm's `Vec`/`Box` fields become `RegionHandle<T>` fields carved from
the allocator at construction:

| Algorithm | Today | After |
|---|---|---|
| `TapeLoop` | `Box<[T]>` + inline ring math | `TapeRing<T, RegionHandle<T>>` (P2) |
| `@` delays (`DelayRing`) | `FixedBuffer<T, 65536>` + inline ring math | `TapeRing<T, RegionHandle<T>>` (P2) |
| `DelayLine<T,N>` | inline `[T; N]` + inline ring math | `TapeRing<T, [T; N]>` + `delay_samples`/stats (optional, P3) |
| `OverlapAddConvolver`, FFT scratch | 7× `Vec` | region handles (P3) |
| `InterpolatedReader` (wavetable / sample tables) | `Box<[T]>` | region handle (P3) |
| `StringModel`, `PlateModel` | `Vec<T>` grids | region handles (P3) |
| Router mixer `bus_buffers` | `Vec<FixedBuffer>` | region handles (P3) |

Small inline state (`FixedBuffer`, `ScalarVector1` fields, `Biquad` blocks,
port `FixedBuffer`s) stays inline (tier 2).

---

## 6. RT safety

- `process()` paths never allocate: the arena's pages are pre-allocated at
  build; `MemoryRegion`/`BufferAllocator` are only touched during construction.
- No locks, no atomics: `Rc`, `UnsafeCell`, `ChunkedVec` are single-threaded.
- `growable-arena` growth and region growth are build-time / non-RT only.
- `unsafe` is isolated to audited accessor functions in `rill_core::memory`
  (pointer deref in `RegionHandle`, `unsafe impl Send/Sync` on single-threaded
  holders) under the §2.1 policy — no locks, no allocation, no syscalls on the
  RT path; every block carries a `SAFETY` invariant and is Miri-checked. The
  single-writer convention is documented, not enforced by the type system (same
  as today's `SharedCell`).

---

## 7. Forward-compat: `Array`/`Vector` big objects

A future value-track `Value::Array(RegionHandle<T>)` can embed a `RegionHandle`
directly:

- `RegionHandle<T>` is `Clone` + single-threaded + RC — the Rc is the buffer
  lifetime; the handle is the value's payload.
- `visit_refs` visits nothing (array data holds no `ArenaRef`s);
  `detach_payload` returns `None`; dropping the value just drops the handle's
  Rc.
- The arena is `Arena<Value>` (payload not `T`-generic today), so the array
  element type must live in the handle's `T` — a `RegionHandle<f32>` field in a
  non-generic `Value` is represented as an opaque region id + handle, resolved
  against the program's `BufferAllocator`. Exact representation is P4 scope;
  the constraint here is that region handles remain cheap, `Clone`, and
  embeddable.

---

## 8. Testing

- **Arena (moved, genericized):** all current `arena.rs` unit tests move to
  `rill_core::memory::arena` with a small `#[derive(Clone, Debug)]` test payload
  implementing `ArenaValue` (a `Record`-like payload over `Vec<ArenaRef>`),
  covering embedded free list, RC, COW, recursive drop, pooled payloads,
  growable growth.
- **`ChunkedVec`:** index mapping, chunk growth, stable addresses across
  `push_chunk`, power-of-two shifts.
- **`MemoryRegion`/`RegionHandle`:** bump allocation, non-spanning allocation,
  chunk-skip waste bound, grow-on-demand at build, exhaustion → `None`, handle
  sharing/clone (write + read handles over one slab); boundary tests for the
  `SAFETY` invariants (§2.1).
- **`TapeRing`:** ring read/write, `read_block`/`write_block`, wraparound,
  interpolation, clear over all three storages (region handle, `Box`, inline
  `[T; N]`); port the existing `DelayRing` and `TapeLoop` tests; the
  `DelayLine` fold is covered by the existing `DelayLine` tests staying green
  (API preserved).
- **Unsafe audit (Miri):** `cargo +nightly miri test -p rill-core` over the
  memory module (region handles, `unsafe impl Send/Sync`, arena paging fast
  path) — no UB; run as part of P1/P2 verification.
- **`TapeLoop`:** existing tests stay green against region-backed storage.
- **Benchmark:** `rill-lang/benches/lang_bench.rs` before/after P1 (arena
  paging fast path) and P2 (region delays) — no regression.
- **Regression:** full `cargo test --workspace`, `cargo clippy
  --all-features --workspace`, `cargo fmt`; rill-lang tests (~100 arena call
  sites) unchanged.

---

## 9. Phasing

1. **P1 — arena move + genericize.** `rill_core::memory` (chunked + arena +
   pool); `ArenaValue`; rill-lang `impl ArenaValue for Value`; paged slot store
   with 1-page RT default + fast path; back-compat re-exports; benchmark.
   Plus §2.2: remove `Send + Sync` from `Algorithm<T>` and `Send` from
   `MultichannelAlgorithm<T>`; drop the now-unnecessary `unsafe impl Send/Sync`
   on `WriteHead`/`ReadHead`. Workspace green.
2. **P2 — region system + rill-lang buffers.** `MemoryRegion`, `RegionHandle`,
   `BufferAllocator`, `TapeRing`/`RingStorage`; `RillProgram` owns
   `BufferAllocator`; delays and `TapeLoop` → `TapeRing<T, RegionHandle<T>>`.
   rill-lang green.
3. **P3 — algorithm internal buffers.** Factory contract gains optional
   allocator; migrate FFT/convolver, wavetable reader, models, router mixer.
4. **P4 — future (out of scope here).** `Array`/`Vector` big objects;
   pure-function `Algorithm` refactor.

---

## 10. File map (P1–P3)

| Area | Files |
|---|---|
| New `rill_core::memory` | `rill-core/src/memory/{mod,chunked,arena,pool,region}.rs` (`mod.rs` carries `#![allow(unsafe_code)]` per §2.1), `rill-core/src/lib.rs` (module + re-exports), `rill-core/src/prelude.rs` |
| Moved code | `rill-lang/src/arena.rs` → `rill-core/src/memory/{arena,pool}.rs` (genericized) |
| rill-lang value | `rill-lang/src/arena.rs` (re-export shim + `impl ArenaValue for Value`), `rill-lang/src/lib.rs`, `rill-lang/src/program.rs` (`Arena<Value>`, `BufferAllocator`, `TapeRing` delays, region tapes), `rill-lang/src/backend/interp.rs` (delay sites) |
| Trait-bound removal (§2.2) | `rill-core/src/traits/algorithm.rs`, `rill-core/src/traits/multichannel_algorithm.rs`, `rill-sampler/src/tape/{write_head,read_head}.rs` |
| Unified ring + region-backed buffers | `rill-core/src/buffer/tape_ring.rs` (new: `TapeRing<T, S>`, `RingStorage`, impls for `RegionHandle`/`Box`/`[T; N]`), `rill-core/src/buffer/tape.rs` (`TapeLoop` → `TapeRing` wrapper), `rill-core/src/buffer/delay.rs` (`DelayLine` → `TapeRing` + `delay_samples`/stats, optional) |
| Factory contract | `rill-lang/src/builtin.rs`, `rill-lang/src/ffi.rs`, DSP crates `lang/register.rs` / `lang.rs` |
| Algorithm migrations (P3) | `rill-fft/src/{overlap_add,real_fft}.rs`, `rill-core-dsp/src/generators/reader.rs`, `rill-core-model/src/{string,plate}/`, `rill-router/src/builtins/mixer.rs` |
| Docs | `docs/src/guides/rill-lang.md` (memory-model section), `rill-core` module docs, `CHANGELOG.md` |
| Tests | moved arena tests, new chunked/region/delay/tape tests, lang_bench |

---

## Verification

- `cargo test -p rill-core` / `-p rill-lang` after each task; `cargo test
  --workspace`, `cargo clippy --all-features --workspace`, `cargo fmt` before
  finishing a phase.
- Zero new external dependencies. `unsafe` confined to `rill_core::memory`
  under §2.1 (module-level `#![allow(unsafe_code)]`; the rest of rill-core
  keeps `deny`); each unsafe block has a `SAFETY` comment, boundary tests, and
  Miri coverage.
- Regression: existing rill programs (combinators, builtin calls, tape) compile
  and run unchanged; arena capacity bounds and exhaustion semantics unchanged;
  `SharedCell`/`ResourceRegistry`/`Reader`/`Writer` APIs unchanged.
- Benchmark: `lang_bench` shows no regression after P1/P2.
- Branch: `feature/rill-lang-arrays`; conventional commits, single-quoted `-m`.