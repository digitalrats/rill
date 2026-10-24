# rill-lang: the World as One Real State Object — IO = ST RealWorld, One Substrate, Two Applications — Design

> **Stage:** C refinement — supersedes §4 (`IO as State + RealWorld`, `Full Haskell ST`)
> of `2026-10-17-rill-lang-io-state-world-streams-design.md`, folding the two
> mechanism descriptions into one unified state model.
> **Status:** Design (not yet implemented; design questions resolved 2026-10-24).
> **Prerequisites:** Stage A value track; the `Monad`/`IO`/`ST` typeclass machinery;
> the rank-2 `runST` special case; SP-4 memoryregion (implemented P1+P2);
> `2026-10-19-rill-lang-memory-layers-design.md` (the layered-memory mechanism).

## 0. Executive summary

The rule **"all state lives in RealWorld"** is taken literally. The World is
**one real runtime object**: the executor owns it and threads it (a unique `&mut`)
through every bind of the program and into the signal tick; the language only
sees an opaque, non-constructible, non-duplicable token. **IO is a form of ST**
(`type IO a = ST RealWorld a`): one token kind, one State-threading mechanism;
rank-2 `s` keeps `runST` regions isolated at the type level while all physical
state lives in the one store.

Inside the store there is **one low-level substrate** (`ChunkedVec`) applied
**two ways**:

1. **Mechanism 1 — carve a block and use it directly as a buffer.** Laws:
   non-moving, fixed block size, strict contiguity, (when necessary) alignment.
   This is the SIMD path; it is enforced by construction in today's code.
2. **Mechanism 2 — build complex structures on the substrate.** Arena slots,
   `ArenaRef`s, ST cells, region layers — the general heap.

The two mechanisms are shaped by the **evaluation mode**: in **Strict** mode
mechanism 2 (value-heap structure-building) is **forbidden entirely** — with no
laziness, lambdas normalize immediately to values or builtin buffer calls, so
strict programs are buffer-only, allocate nothing for structures at runtime, and
are therefore the RT-safe mode. **Complete** mode has both. `runST` is a separate
mechanism, orthogonal to the mode (§4.3).

## 1. The rule and its runtime meaning

- **For the program**, the `World` token (the GHC `State# RealWorld` analog) is
  phantom: it has no constructors, cannot be observed or duplicated, only the
  runtime creates `realWorld` and discards the final token
  (io-state-world-streams §4.1).
- **For the runtime**, the World is a **real existing object**. The executor owns
  it and threads it: binds move it (`IO a = World -> Pair World a`), the tick
  receives it as a unique `&mut`. There is no separate "stable handle" — the
  threaded object *is* the state.
- **Consequence for today's code:** `RillProgram.world: Rc<RefCell<WorldState>>`
  (`program.rs:263–266`) is the wrong representation — an aliased, shared,
  runtime-checked cell. The corrected model: the executor owns `WorldState` and
  passes a unique `&mut` through the bind chain and into the tick. One top-level
  borrow; zero per-access checks.

## 2. IO is a form of ST

The current spec defines two shapes:
`data IO a = { unIO: World -> Pair World a }` (§4.1) and
`data ST s a = { unST: STState -> Pair STState a }` (§4.2).

**Unified shape** — one State-threading mechanism:

```
data STState;                        -- primitive opaque token (the State# s analog)
data ST s a = { unST: STState -> Pair STState a }
type IO a = ST RealWorld a           -- IO is ST at the world's token
```

- One token kind; `World` is `STState RealWorld`. The program-level prelude names
  `World` and `STState` as opaque primitives; runtime builtins create their values.
- `runST` opens a **fresh** `STState s` (a fresh layer); the rank-2 `forall s`
  makes escape inexpressible — exactly GHC's `runST`.
- **Runtime:** one physical store (the `WorldState`). Regions allocate in it as
  layers; isolation is type-level — exactly GHC's physical-sharing model, where
  `STRef` and `IORef` ride the same `MutableRef#` machinery.
- The `Monad (ST s)` instance serves both `ST s` and `IO`; `return`/`bind` thread
  the token as in §4.1 (data-dependency ordering; no linear-type subsystem).

## 3. WorldState — the single real mutable object

The executor-owned store contains the persistent runtime state and both memory
subsystems:

- the streams table + captured console (today's `WorldState`, `program.rs:139–172`);
- `params` / `params_dirty` (`program.rs:202–206`);
- `value_state` — per-tick value feedback slots (`program.rs:227–228`);
- `main_cells` — persistent main λ-parameter cells (`program.rs:209–214`);
- `regions` / `st_cells` — the region stack and the STRef cell table
  (`program.rs:253–262`);
- **the buffer store** — `buffers: Vec<RegionHandle<T>>` and
  `delays: Vec<RegionDelay<T>>` (`program.rs:189–195`);
- **the value arena** — `arena: Arena<Value>` (`program.rs:207–208`).

**Stays with `RillProgram`** (my call — flagged for review): the immutable
program parts (`ir`, `params_meta`, `builtins`) and the per-tick execution
scratch (`value_regs`, `value_regs_top`, `cell_stack`, `frag_cells`,
`frag_cells_base`, `drops_scratch`, `value_outputs`, `value_error`, `block_len`).
GHC analogy: the world is the heap — persistent mutable data; machine registers
and the evaluation stack are not part of the world.

## 4. One substrate, two applications

**Substrate:** `ChunkedVec` (`chunked.rs`) — a logical-contiguous vector over
`Box<[T]>` chunks. Growth appends a chunk and never moves existing elements;
`chunk_cap` is a power of two; `slab_slice(_mut)` **panics** on a range that
spans a chunk boundary (`chunked.rs:123–142`). Both the value arena's paged
slot store and `MemoryRegion` slabs sit on it (`chunked.rs:6–7`). One substrate;
the difference is only the **application**.

### 4.1 Mechanism 1 — carve a block and use it directly as a buffer

`MemoryRegion` / `BufferAllocator` carve `RegionHandle`s at exact length at
build time (`region.rs`). The laws of this memory (important — this is the SIMD
path):

- **Non-moving** — chunk growth never relocates existing elements
  (`chunked.rs:4–7`).
- **Fixed block size** — a carve is exactly `len`, never resized (`region.rs:170`).
- **Strict contiguity** — `bump` never spans a chunk (`region.rs:47–65`, test
  `non_spanning_allocation_skips_chunks`), and `slab_slice` panics on a spanning
  range (`chunked.rs:123–142`, test `spanning_slice_panics`). Every handle is a
  contiguous slice within one chunk.
- **Alignment** — today only default `align_of::<T>()` (`Box<[T]>`). SIMD-width
  alignment is a documented *strengthening*, applied when a backend requires it
  (my call: keep the default; record the guarantee).

Access is interior-mutable (`Rc<UnsafeCell>`), single-threaded, no locks
(`region.rs:11–14`, `91–118`).

### 4.2 Mechanism 2 — complex structures on the substrate

The value arena (`Arena<Value>`, fixed capacity from IR) gives `ArenaRef`-indexed
slots; the value track and ST regions build structures on top. ST cells are arena
slots (`st_cells: Vec<Option<ArenaRef>>`); a region records `cell_base`, allocates
in the arena, and truncates back on exit (`program.rs:258–280`). This is where
general data structures, closures/thunks, and region-local cells live. Note:
`runST` *uses* this substrate for its cells, but ST itself is a separate
mechanism orthogonal to the evaluation mode (§4.3).

### 4.3 The mode split (important)

> **The mode axis in Haskell and rill-lang.** Haskell *does* have a mode switch,
> per module: `{-# LANGUAGE Strict #-}` flips the default from lazy to strict
> (eager) evaluation of bindings; without it, laziness is the default. rill-lang
> keeps the same switch but per **program** (`Mode::Strict`/`Mode::Complete`),
> and its Strict mode is deeper than GHC's `-XStrict`: it forbids laziness
> entirely (no thunks, recursion banned, mechanism 2 banned), whereas GHC's
> Strict still allows lazy patterns (`~`) and recursion. The mode is rill-lang's
> own surface; the orthogonality argument below is a rill-lang design decision,
> not a Haskell theorem.

- **Strict mode** (eager, no thunks, recursion banned — io-state-world-streams
  §4.1): mechanism 2 — complex value-heap structure-building (closures, thunks,
  collections) — is **forbidden entirely**, and it is *unnecessary*: with no
  laziness, any lambda normalizes immediately (eager β-reduction) to either a
  **pure value** or a **call of a builtin function over a buffer**. There is no
  suspended computation to store, so there is no value heap. A strict program is
  buffer-only: all memory is carved at build (mechanism 1); the value arena is
  **pre-filled at build** with the fixed register/main-cell slots and is **never
  grown for structures at runtime**. This is the **RT-safe mode** — it satisfies
  the RT rules (no heap allocation, no locks, no syscalls on the signal path) by
  construction.
- **`runST` works exactly as in GHC**, orthogonal to the mode. ST (isolated
  region state) is a *separate mechanism* from `Mode::Strict`/`Mode::Complete`:
  `runST` opens a fresh, isolated region (rank-2 `forall s`); `STRef s a` cells
  are allocated and discarded with the region — no state escapes. The mode only
  decides evaluation strategy; it does not restrict ST. Full GHC semantics:
  fresh state per region, rank-2 isolation, final state discarded. The
  no-allocation RT rule concerns the signal hot path; a program that opens
  `runST` regions on that path is authoring an RT violation — as in any RT host
  — not a mode violation.
- **Complete mode** (lazy / call-by-need): both mechanisms. The arena and
  structures are available, runtime allocation allowed; recursion and laziness
  are permitted.
- **Enforcement (decided):** the mode system rejects mechanism-2
  structure-building (closures, thunks, collections) in Strict programs at
  compile/inference time; **and** at runtime, exceeding the pre-allocated
  structure capacity is an **OOM panic** (the arena is fixed-capacity from IR;
  overflow is the existing `value_error`/`ProcessError` path).

## 5. Regions as layers

- **The value store is a layered bump memory.** The base is the program-static
  arena; each `runST` opens a fresh layer, allocates cells in the arena, and
  collapses on exit (recycle pure-dead slots, hand survivor slots to the parent
  layer) — the mechanism of `2026-10-19-rill-lang-memory-layers-design.md`.
- **The buffer store is layered, bounded by the memory manager's pages.**
  Buffers/delays are carved from the memory manager: build-time carves form the
  static base; a `runST` region may allocate big signal buffers from the
  manager's **available pages** (like GHC's region-mutable arrays — rank-2
  prevents them escaping the region). The manager's page budget is the hard
  limit; exceeding it is an OOM condition.

## 6. Hot path / RT

- The runner threads `&mut WorldState` once; the value track reaches `buffers[i]`,
  `delays[j]`, and `arena` directly through it — a single unique borrow at the
  top, zero per-access checks.
- Buffer access keeps `with_data(_mut)` over `Rc<UnsafeCell>`: stable addresses,
  no locks, no syscalls.
- **Strict-mode guarantee:** no runtime allocation at all (mechanism 2 forbidden),
  so the RT-path "no allocation" rule holds structurally.

## 7. Changes to the current code

- `WorldState` grows to hold the buffer store, the value arena, and the
  persistent state of §3.
- `RillProgram.world: Rc<RefCell<WorldState>>` becomes an executor-owned
  `WorldState` threaded by `&mut`; the `Value::Opaque` world/region tokens remain
  the program-level names for the layers.
- The IO bind machinery and `ValueEffectOp::RunST` unify onto one
  State-threading primitive (§2).
- Value-track buffer ops (`prog.buffers[i].with_data_mut`, `interp.rs:1630–1740`)
  become `world.buffers[i]…` — the same direct access, a new owner.
- Strict mode: mechanism-2 ops rejected at compile/inference time; exceeding the
  pre-allocated structure capacity at runtime is an OOM panic (§4.3).

## 8. Decisions made in this design (my calls — reviewable; architect-resolved in §9)

1. `WorldState` holds persistent state + both memory subsystems; per-tick
   execution scratch stays with the interpreter (§3).
2. Alignment stays at default `align_of::<T>()`; SIMD-width alignment is a
   documented conditional strengthening (§4.1).
3. Strict-mode forbidding of mechanism 2 (structure-building) is enforced both at
   compile/inference time and at runtime: exceeding the pre-allocated structure
   capacity in a Strict run is an OOM panic (§4.3). `runST` availability in
   Strict is orthogonal and GHC-exact (§4.3).
4. The buffer store is layered: build-time carves form the static base, and a
   `runST` region may allocate big signal buffers within the memory manager's
   available pages (rank-2 prevents escape) (§5).
5. This spec supersedes §4 of io-state-world-streams; the layered-memory design
   becomes the runtime mechanism of the value store; the memoryregion laws
   describe the buffer store.
6. The mode axis exists in both worlds: Haskell flips per module with
   `{-# LANGUAGE Strict #-}` (default lazy, flag → strict/eager); rill-lang flips
   per program (`Mode::Strict`/`Mode::Complete`). rill-lang's Strict is deeper —
   no thunks, no recursion, mechanism 2 banned (GHC's Strict still keeps lazy
   patterns and recursion). `runST` is exactly GHC's ST — fresh isolated region,
   rank-2 `forall s`, cells discarded with the region — and is orthogonal to the
   mode (§4.3).

## 9. Resolved decisions (2026-10-24)

- **Scratch boundary:** per-tick execution scratch (`value_regs`, `cell_stack`,
  `frag_cells`, `drops_scratch`, `value_outputs`, `value_error`, `block_len`)
  stays with the interpreter, not the world; the world holds the persistent state
  and the memory (§3).
- **Strict mechanism-2 ban:** enforced both at compile/inference time **and** at
  runtime — exceeding the pre-allocated structure capacity in a Strict run is an
  **OOM panic** (§4.3).
- **Region big buffers:** a `runST` region may allocate big signal buffers within
  the memory manager's **available pages**; rank-2 prevents them escaping the
  region (§5).
- **Strict mode bans mechanism 2** (value-heap structure-building): with no
  laziness, any lambda normalizes immediately to a pure value or a builtin buffer
  call, so no value heap is needed.
- **`runST` works exactly as in GHC**, orthogonal to the mode: a fresh isolated
  region per `runST`, rank-2 `forall s`, `STRef` cells discarded with the region,
  final state dropped; the mode does not restrict ST. The no-allocation RT rule
  governs the signal hot path, not the ST mechanism itself (§4.3).
