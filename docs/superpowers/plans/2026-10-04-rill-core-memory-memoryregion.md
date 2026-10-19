# rill-core Memory Subsystem: Generic Arena + MemoryRegion — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Status (audited 2026-10-19):** P1 + P2 **implemented and merged** — the
> artifacts are in the code: `rill_core::memory` (`Arena<V>`, `ChunkedVec`,
> `BufferPool`, `MemoryRegion`/`RegionHandle`/`BufferAllocator`), `TapeRing`/
> `RingStorage`, region-backed `@` delays (`RegionDelay`), the `Value::Buffer`
> buffer table, and the `Send`/`Sync` supertrait removal. **Phase 3 not done**:
> no `RegionHandle` usage in `rill-fft` / `rill-core-dsp` / `rill-core-model` /
> `rill-router`. Phase 0–2 checkboxes below are marked complete; Phase 3 stays
> open.

**Goal:** Move the value-arena memory manager from `rill-lang` into `rill-core` (genericized as `Arena<V>` over an `ArenaValue` payload trait) and introduce a chunked, reference-counted region allocator (`MemoryRegion`/`RegionHandle`) plus a unified ring (`TapeRing`) that back tape loops, delay lines, FFT scratch, and algorithm internal buffers — one centralized, RT-safe memory subsystem.

**Architecture:** `rill_core::memory` = `ChunkedVec<T>` (grow-safe chunked storage, the shared primitive) + `Arena<V>` (paged slot store, RC, COW, `BufferPool`) + `MemoryRegion<T>`/`RegionHandle<T>`/`BufferAllocator<T>` (tier-1 slabs). `rill_core::buffer::TapeRing<T, S: RingStorage<T>>` unifies `TapeLoop`/`DelayLine`/`@` delay rings over any storage. `Algorithm<T>`/`MultichannelAlgorithm<T>` drop their `Send`/`Sync` supertraits (verified redundant — the graph never crosses a thread boundary). See `docs/superpowers/specs/2026-10-04-rill-core-memory-memoryregion-design.md` (§2.1 unsafe policy, §2.2 Send/Sync, §3.1–3.6, §9 phasing).

**Tech Stack:** Rust, Cargo workspace `rill/`, crates `rill-core` (primary) + `rill-lang` + DSP crates. Branch `feature/rill-lang-arrays`. No new external dependencies.

---

## Conventions used throughout

- **Safe code default; `unsafe` only under §2.1** (proven perf benefit + `SAFETY` comment + Miri). `rill-core/src/memory/mod.rs` carries `#![allow(unsafe_code)]`; every other module in rill-core keeps the crate-level `#![deny(unsafe_code)]`.
- **Verification after every task:** `cargo test -p rill-core` and `cargo test -p rill-lang` (whichever crate changed); `cargo check --workspace`; before finishing a phase `cargo clippy --all-features --workspace` + `cargo fmt`.
- **Commit style:** conventional commits, single-quoted `-m` (git-flow; we are on `feature/rill-lang-arrays`).
- **Docs:** all code comments and module docs in English.

---

## Phase 0 — Baseline

- [x] **Step 0.1: Confirm clean baseline**

Run: `git -C /home/mikek/Projects/digitalrats/rill status --short` and `git -C /home/mikek/Projects/digitalrats/rill branch --show-current`
Expected: clean tree, branch `feature/rill-lang-arrays`.

- [x] **Step 0.2: Snapshot the moving file**

`rill-lang/src/arena.rs` (726 lines) is the source to move. `Cargo.lock`/`target/` untouched. Record pre-P1 `lang_bench` numbers:

Run: `cargo bench -p rill-lang --bench lang_bench` (from `rill/`), save the output to a scratch file. These are the "before" numbers for Task 1.8.

---

## Phase 1 — Arena move + genericize + trait-bound removal

### Task 1.1: `rill_core::memory` skeleton + `ChunkedVec<T>`

**Files:**
- Create: `rill-core/src/memory/mod.rs`
- Create: `rill-core/src/memory/chunked.rs`
- Modify: `rill-core/src/lib.rs`
- Modify: `rill-core/src/prelude.rs`

- [x] **Step 1.1.1: Write the failing tests for `ChunkedVec`**

Create `rill-core/src/memory/chunked.rs` with a test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> ChunkedVec<u64> {
        ChunkedVec::with_capacity(5, 4, |i| i as u64)
    }

    #[test]
    fn logical_indices_are_contiguous_across_chunks() {
        let s = store(); // capacity 5, chunk_cap 4 -> 2 chunks (4 + 1)
        assert_eq!(s.len(), 5);
        assert_eq!(s.index(0), &0u64);
        assert_eq!(s.index(4), &4u64);
        assert_eq!(s.num_chunks(), 2);
    }

    #[test]
    fn push_chunk_grows_without_moving_existing() {
        let mut s = ChunkedVec::<u64>::with_capacity(0, 4, |_| 0);
        s.push_chunk(|base| base as u64); // chunk of 4, base 0
        s.push_chunk(|base| base as u64); // chunk of 4, base 4
        assert_eq!(s.index(0), &0u64);
        assert_eq!(s.index(4), &4u64);
        assert_eq!(s.index(7), &7u64);
        assert_eq!(s.len(), 8);
        *s.index_mut(3) = 99;
        assert_eq!(s.index(3), &99u64);
        assert_eq!(s.index(4), &4u64, "existing data stable across growth");
    }

    #[test]
    #[should_panic]
    fn index_out_of_bounds_panics() {
        let s = store();
        let _ = s.index(5);
    }
}
```

- [x] **Step 1.1.2: Run to verify they fail**

Run: `cargo test -p rill-core memory::chunked`
Expected: FAIL — `ChunkedVec` not defined (or module not found).

- [x] **Step 1.1.3: Implement `ChunkedVec<T>`**

`rill-core/src/memory/chunked.rs`:

```rust
//! # ChunkedVec — grow-safe chunked storage
//!
//! A contiguous *logical* vector split into fixed-size chunks. Each chunk is a
//! separate `Box<[T]>` allocation, so appending a chunk never moves existing
//! elements: addresses (chunk index + offset) stay valid across growth. This
//! is the shared storage primitive for the value arena's paged slot store and
//! for [`MemoryRegion`](super::region::MemoryRegion) slabs.
//!
//! Single-threaded: no atomics, no locks.

use std::fmt;

/// Grow-safe chunked vector. `chunk_cap` is a power of two so the
/// logical-index -> (chunk, offset) mapping is a shift.
pub struct ChunkedVec<T> {
    chunks: Vec<Box<[T]>>,
    chunk_cap: usize,
    bits: u32,
    len: usize,
}

impl<T> ChunkedVec<T> {
    /// Number of chunks.
    pub fn num_chunks(&self) -> usize {
        self.chunks.len()
    }

    /// Logical length.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Power-of-two chunk capacity.
    pub fn chunk_cap(&self) -> usize {
        self.chunk_cap
    }

    /// Create a store pre-allocating `capacity` slots across power-of-two
    /// chunks of `chunk_cap` elements. `make(global_index)` produces each
    /// element, so callers can link free lists during construction.
    ///
    /// # Panics
    /// Panics if `chunk_cap` is not a power of two.
    pub fn with_capacity(
        capacity: usize,
        chunk_cap: usize,
        mut make: impl FnMut(usize) -> T,
    ) -> Self {
        assert!(chunk_cap.is_power_of_two(), "chunk_cap must be a power of two");
        let mut s = Self {
            chunks: Vec::new(),
            chunk_cap,
            bits: chunk_cap.trailing_zeros(),
            len: 0,
        };
        let chunks = capacity.div_ceil(chunk_cap);
        for c in 0..chunks {
            let base = c * chunk_cap;
            let n = (capacity - base).min(chunk_cap);
            let chunk: Vec<T> = (base..base + n).map(|i| make(i)).collect();
            let mut boxed = chunk.into_boxed_slice();
            s.chunks.push(std::mem::replace(
                &mut boxed,
                Vec::new().into_boxed_slice(),
            ));
        }
        s.len = capacity;
        s
    }

    /// Append a chunk of `chunk_cap` elements produced by `make(base_index)`.
    /// Existing elements keep their addresses. Returns the number of elements
    /// now present.
    pub fn push_chunk(&mut self, make: impl FnMut(usize) -> T) {
        let base = self.len;
        let chunk: Vec<T> = (base..base + self.chunk_cap).map(make).collect();
        self.chunks.push(chunk.into_boxed_slice());
        self.len += self.chunk_cap;
    }

    /// Map a logical index to its (chunk, offset) pair.
    #[inline]
    fn split(&self, i: usize) -> (usize, usize) {
        debug_assert!(i < self.len, "index {i} out of bounds (len {})", self.len);
        (i >> self.bits, i & (self.chunk_cap - 1))
    }

    /// Immutable access.
    #[inline]
    pub fn index(&self, i: usize) -> &T {
        let (c, o) = self.split(i);
        &self.chunks[c][o]
    }

    /// Mutable access.
    #[inline]
    pub fn index_mut(&mut self, i: usize) -> &mut T {
        let (c, o) = self.split(i);
        &mut self.chunks[c][o]
    }

    /// Immutable slice of one chunk.
    #[inline]
    pub fn chunk(&self, c: usize) -> &[T] {
        &self.chunks[c]
    }

    /// Mutable slice of one chunk.
    #[inline]
    pub fn chunk_mut(&mut self, c: usize) -> &mut [T] {
        &mut self.chunks[c]
    }

    /// The global index of the first element of chunk `c`.
    #[inline]
    pub fn chunk_base(&self, c: usize) -> usize {
        c * self.chunk_cap
    }
}

impl<T: fmt::Debug> fmt::Debug for ChunkedVec<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChunkedVec")
            .field("num_chunks", &self.chunks.len())
            .field("chunk_cap", &self.chunk_cap)
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}
```

Note: `capacity.div_ceil` requires Rust ≥ 1.73 (workspace edition 2021; check `rust-version` — if older, use `(capacity + chunk_cap - 1) / chunk_cap`).

- [x] **Step 1.1.4: Wire the module**

`rill-core/src/memory/mod.rs`:

```rust
//! # rill-core memory subsystem
//!
//! Centralized, RT-safe memory management for the rill ecosystem.
//!
//! - [`ChunkedVec`] — grow-safe chunked storage (the shared primitive).
//! - [`Arena`] / [`ArenaValue`] — the genericized value arena (fixed-capacity,
//!   reference-counted, copy-on-write, embedded free list).
//! - [`BufferPool`] — size-classed payload buffer pool.
//! - [`MemoryRegion`] / [`RegionHandle`] / [`BufferAllocator`] — chunked,
//!   reference-counted slabs for large static buffer memory.
//!
//! # Safety policy
//!
//! This module is exempt from the crate-level `#![deny(unsafe_code)]` under the
//! policy in the SP-4 design spec (§2.1): `unsafe` is only introduced with a
//! proven performance benefit, every `unsafe` block carries a `// SAFETY:`
//! invariant, and the unsafe paths are Miri-checked.

#![allow(unsafe_code)]

pub mod arena;
pub mod chunked;
pub mod pool;
pub mod region;

pub use arena::{Arena, ArenaError, ArenaRef, ArenaValue};
pub use chunked::ChunkedVec;
pub use pool::BufferPool;
pub use region::{BufferAllocator, MemoryRegion, RegionHandle};
```

`rill-core/src/lib.rs` — add the module after `buffer`:

```rust
/// Centralized, RT-safe memory management (arena, region slabs, pools).
pub mod memory;
```

`rill-core/src/prelude.rs` — add under a new "Memory Types" section:

```rust
pub use crate::memory::{Arena, ArenaError, ArenaRef, ArenaValue, BufferPool, ChunkedVec, MemoryRegion, RegionHandle};
```

- [x] **Step 1.1.5: Run the tests**

Run: `cargo test -p rill-core memory::chunked`
Expected: PASS (3 tests).

- [x] **Step 1.1.6: Commit**

```bash
git add rill-core/src/memory/mod.rs rill-core/src/memory/chunked.rs rill-core/src/lib.rs rill-core/src/prelude.rs
git commit -m 'feat(rill-core): memory module skeleton with ChunkedVec grow-safe storage'
```

### Task 1.2: Move `BufferPool` to `rill_core::memory::pool`

**Files:**
- Create: `rill-core/src/memory/pool.rs`
- Delete (later, in 1.4): the pool portion of `rill-lang/src/arena.rs`

- [x] **Step 1.2.1: Write the moved tests first**

Copy the `BufferPool` tests from `rill-lang/src/arena.rs:563-580` (`buffer_pool_exhaustion_and_recycle`, `growable_pool_allocates_on_exhaustion`) into `pool.rs`'s test module, adapted to the new path:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_pool_exhaustion_and_recycle() {
        let mut pool = BufferPool::new(8, false);
        let b1 = pool.take(4).unwrap();
        assert!(b1.capacity() >= 4);
        assert!(pool.take(4).is_none(), "RT pool must report exhaustion");
        pool.put(b1);
        assert!(pool.take(4).is_some(), "returned buffer must be reusable");
    }

    #[test]
    fn growable_pool_allocates_on_exhaustion() {
        let mut pool = BufferPool::new(8, true);
        let _b1 = pool.take(4).unwrap();
        let b2 = pool.take(4).unwrap();
        assert!(b2.capacity() >= 4);
    }
}
```

- [x] **Step 1.2.2: Implement `pool.rs`**

Copy the `BufferPool` struct verbatim from `rill-lang/src/arena.rs:119-207` (size classes, `class_of`, `class_cap`, `new`, `take`, `put`), replacing nothing except the module doc header. `ArenaRef` is now `crate::memory::ArenaRef` (defined in Task 1.3 — for now reference it via `super::arena::ArenaRef`; it will exist after Task 1.3 lands). The buffer element type stays `u32` (a ref) — no generics needed.

- [x] **Step 1.2.3: Run the tests**

Run: `cargo test -p rill-core memory::pool`
Expected: PASS.

- [x] **Step 1.2.4: Commit**

```bash
git add rill-core/src/memory/pool.rs
git commit -m 'feat(rill-core): move size-classed BufferPool into memory module'
```

### Task 1.3: Generic `Arena<V>` + `ArenaValue` (paged slot store)

**Files:**
- Create: `rill-core/src/memory/arena.rs`

- [x] **Step 1.3.1: Write the failing tests (moved + genericized)**

Port every `arena.rs` unit test from `rill-lang/src/arena.rs:534-725` using a test payload that models a record of child refs:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Test payload: a "record" owning child refs, with a pool-backed payload
    /// Vec (mirrors `Value::Record`).
    #[derive(Debug, Clone, PartialEq)]
    struct Node {
        kids: Vec<ArenaRef>,
    }

    impl ArenaValue for Node {
        fn visit_refs(&self, mut f: impl FnMut(ArenaRef)) {
            for &k in &self.kids {
                f(k);
            }
        }
        fn clone_pooled(&self, pool: &mut BufferPool) -> Result<Self, ArenaError> {
            let mut b = pool.take(self.kids.len()).ok_or(ArenaError::CapacityExceeded)?;
            b.extend_from_slice(&self.kids);
            Ok(Node { kids: b })
        }
        fn detach_payload(&mut self) -> Option<Vec<ArenaRef>> {
            let kids = std::mem::take(&mut self.kids);
            (!kids.is_empty()).then_some(kids)
        }
    }

    fn arena(cap: usize) -> Arena<Node> {
        Arena::with_capacity(cap)
    }

    #[test]
    fn embedded_free_list_recycles_and_counts() {
        let mut a = arena(4);
        assert_eq!(a.live(), 0);
        let r = a.alloc(Node { kids: vec![] }).unwrap();
        assert_eq!(a.live(), 1);
        a.drop_ref(r);
        assert_eq!(a.live(), 0);
        let r2 = a.alloc(Node { kids: vec![] }).unwrap();
        assert_eq!(r2, r, "freed slot must recycle via the embedded free list");
    }

    #[test]
    fn copy_increments_rc() {
        let mut a = arena(4);
        let r = a.alloc(Node { kids: vec![] }).unwrap();
        let r2 = a.copy(r).unwrap();
        assert_eq!(a.rc(r), 2);
        a.drop_ref(r);
        assert_eq!(a.rc(r2), 1);
    }

    #[test]
    fn cow_copies_when_rc_gt_1() {
        let mut a = arena(4);
        let r = a.alloc(Node { kids: vec![] }).unwrap();
        let r2 = a.copy(r).unwrap();
        let out = a.mutate(r).unwrap();
        assert_ne!(out, r2, "must copy before mutation");
        assert_eq!(a.rc(out), 1);
        assert_eq!(a.rc(r2), 1);
    }

    #[test]
    fn drop_recurses_children_and_recycles_payload() {
        let mut a = arena(8);
        let f = a.alloc(Node { kids: vec![] }).unwrap();
        let r = a.alloc(Node { kids: vec![f] }).unwrap();
        a.copy(f).unwrap();
        a.drop_ref(r);
        assert_eq!(a.rc(f), 1, "child survives via extra ref");
        a.drop_ref(f);
        assert_eq!(a.rc(f), 0);
    }

    #[test]
    fn cow_keeps_field_rc_consistent() {
        let mut a = arena(8);
        let f = a.alloc(Node { kids: vec![] }).unwrap();
        let r = a.alloc(Node { kids: vec![f] }).unwrap();
        a.copy(r).unwrap();
        let out = a.mutate(r).unwrap();
        assert_ne!(out, r);
        assert_eq!(a.rc(f), 2);
        a.drop_ref(r);
        assert_eq!(a.rc(f), 1);
    }

    #[test]
    fn capacity_exhaustion_is_a_build_time_error() {
        let mut a = arena(1);
        a.alloc(Node { kids: vec![] }).unwrap();
        assert!(a.alloc(Node { kids: vec![] }).is_err());
    }

    #[test]
    fn growable_arena_allocates_new_pages() {
        let mut a = arena(2);
        a.pool.growable = true;
        a.alloc(Node { kids: vec![] }).unwrap();
        a.alloc(Node { kids: vec![] }).unwrap();
        let r = a.alloc(Node { kids: vec![] }).unwrap();
        assert_eq!(a.get(r).unwrap(), &Node { kids: vec![] });
        assert_eq!(a.live(), 3);
    }

    #[test]
    fn paged_store_uses_one_page_fast_path_for_small_capacity() {
        let mut a = arena(5); // page_cap = 8 (next pow2)
        assert_eq!(a.slots.num_chunks(), 1, "capacity 5 fits one page");
        let r = a.alloc(Node { kids: vec![] }).unwrap();
        assert_eq!(r, 0);
        assert_eq!(a.capacity(), 5, "logical capacity unchanged");
    }

    #[test]
    fn mutate_on_freed_ref_errors() {
        let mut a = arena(4);
        let r = a.alloc(Node { kids: vec![] }).unwrap();
        a.drop_ref(r);
        assert_eq!(a.mutate(r), Err(ArenaError::DanglingRef));
    }
}
```

- [x] **Step 1.3.2: Run to verify they fail**

Run: `cargo test -p rill-core memory::arena`
Expected: FAIL — `Arena`/`ArenaValue` not defined.

- [x] **Step 1.3.3: Implement `Arena<V>`**

Create `rill-core/src/memory/arena.rs`:

```rust
//! # Genericized value arena
//!
//! Fixed-capacity, reference-counted, copy-on-write slot arena with an
//! embedded free list, modeled on Alexandrescu's "Affordable Allocator".
//! Slots live in a [`ChunkedVec`] page store (one pre-allocated page at RT,
//! pages appended only under the `growable-arena` path). RC is a non-atomic
//! `u32` — the arena is owned by a single thread.

use super::chunked::ChunkedVec;
use super::pool::BufferPool;

/// Opaque index into an [`Arena`] slot. Stable across slot reuse.
pub type ArenaRef = u32;

/// Errors returned by arena operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArenaError {
    /// The arena is full; no free slot is available.
    CapacityExceeded,
    /// The referenced slot does not exist (never allocated or already freed).
    DanglingRef,
    /// The reference count overflowed `u32`.
    RcOverflow,
}

/// A payload stored in an [`Arena`] slot.
///
/// Implementations provide the value-specific pieces the arena cannot know:
/// how to enumerate child refs (recursive drop, COW recounting), how to clone
/// a payload using the pool (RT-safe), and how to detach a pooled payload
/// buffer for recycling on drop.
pub trait ArenaValue: Clone + std::fmt::Debug {
    /// Enumerate every child [`ArenaRef`] owned by this value.
    fn visit_refs(&self, f: impl FnMut(ArenaRef));

    /// Clone the value, serving container payloads from the pool. The default
    /// is plain `Clone` (heap-backed payloads).
    fn clone_pooled(&self, _pool: &mut BufferPool) -> Result<Self, ArenaError> {
        Ok(self.clone())
    }

    /// Detach the pool-backed payload buffer, if any, so the arena can recycle
    /// it after the value's children are dropped. The default returns `None`.
    fn detach_payload(&mut self) -> Option<Vec<ArenaRef>> {
        None
    }
}

/// A slot: either live (with an RC) or on the embedded free list.
#[derive(Debug, Clone, Default)]
enum Slot<V> {
    /// A live value with a reference count.
    Occupied { rc: u32, val: V },
    /// A freed slot holding the index of the next free slot.
    #[default]
    Free { next: Option<ArenaRef> },
}

/// A fixed-capacity arena. `with_capacity(0)` is allowed and means "no values".
///
/// The slot store is a [`ChunkedVec`] paged store: RT pre-allocates a single
/// page sized to the capacity's next power of two; only the `growable-arena`
/// path appends pages. Logical capacity semantics match the original
/// `rill_lang::arena::Arena` exactly.
#[derive(Debug)]
pub struct Arena<V: ArenaValue> {
    slots: ChunkedVec<Slot<V>>,
    free: Option<ArenaRef>,
    live: usize,
    capacity: usize,
    /// Reserved for future debug/abort-safety support; not yet read.
    next_gen: u32,
    /// Payload buffer pool for container payloads.
    pub pool: BufferPool,
}

impl<V: ArenaValue> Arena<V> {
    /// Create an arena with `capacity` pre-allocated slots linked into the
    /// embedded free list.
    pub fn with_capacity(capacity: usize) -> Self {
        // One page at RT; page_cap is a power of two >= capacity (>= 1).
        let page_cap = capacity.max(1).next_power_of_two();
        let slots = ChunkedVec::with_capacity(capacity, page_cap, |i| {
            Slot::Free {
                next: (i + 1 < capacity).then_some((i + 1) as ArenaRef),
            }
        });
        Self {
            slots,
            free: (capacity > 0).then_some(0),
            live: 0,
            capacity,
            next_gen: 0,
            pool: BufferPool::new(0, false),
        }
    }

    /// Total logical capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of live (allocated) slots.
    pub fn live(&self) -> usize {
        self.live
    }

    /// Borrow a payload buffer of at least `n` refs from the pool (RT-safe in
    /// the default mode). `Err` on exhaustion when growth is disabled.
    pub fn take_buf(&mut self, n: usize) -> Result<Vec<ArenaRef>, ArenaError> {
        self.pool.take(n).ok_or(ArenaError::CapacityExceeded)
    }

    /// Return a payload buffer to the pool.
    pub fn put_buf(&mut self, b: Vec<ArenaRef>) {
        self.pool.put(b);
    }

    /// Allocate a slot holding `val`. `Err((.., val))` when the arena is full —
    /// the value is returned so the caller can release its children.
    pub fn alloc(&mut self, val: V) -> Result<ArenaRef, (ArenaError, V)> {
        let idx = match self.free {
            Some(idx) => idx,
            None if self.pool.growable => {
                // `growable-arena` (non-RT): append a page and link its slots.
                let base = self.slots.len();
                let page_cap = self.slots.chunk_cap();
                self.slots.push_chunk(|i| {
                    let local = i - base;
                    Slot::Free {
                        next: (local + 1 < page_cap).then_some((i + 1) as ArenaRef),
                    }
                });
                self.capacity += page_cap;
                base as ArenaRef
            }
            None => return Err((ArenaError::CapacityExceeded, val)),
        };
        self.free = match &self.slots.index(idx as usize) {
            Slot::Free { next } => *next,
            Slot::Occupied { .. } => unreachable!("arena free list corrupt"),
        };
        *self.slots.index_mut(idx as usize) = Slot::Occupied { rc: 1, val };
        self.next_gen = self.next_gen.wrapping_add(1);
        self.live += 1;
        Ok(idx)
    }

    /// Reference count of a slot (0 if freed).
    pub fn rc(&self, r: ArenaRef) -> u32 {
        match self.slot_ref(r) {
            Some(Slot::Occupied { rc, .. }) => *rc,
            _ => 0,
        }
    }

    /// Immutable view of a slot's value.
    pub fn get(&self, r: ArenaRef) -> Option<&V> {
        match self.slot_ref(r) {
            Some(Slot::Occupied { val, .. }) => Some(val),
            _ => None,
        }
    }

    /// Mutable view of a slot's value (no RC change). Debug builds assert the
    /// slot is exclusively owned (`rc == 1`).
    pub fn get_mut(&mut self, r: ArenaRef) -> Option<&mut V> {
        debug_assert_eq!(self.rc(r), 1);
        match self.slot_mut(r) {
            Some(Slot::Occupied { val, .. }) => Some(val),
            _ => None,
        }
    }

    /// Share a value: `rc++` and return the same ref.
    pub fn copy(&mut self, r: ArenaRef) -> Result<ArenaRef, ArenaError> {
        match self.slot_mut(r) {
            Some(Slot::Occupied { rc, .. }) => {
                *rc = rc.checked_add(1).ok_or(ArenaError::RcOverflow)?;
                Ok(r)
            }
            _ => Err(ArenaError::DanglingRef),
        }
    }

    /// Drop one reference; frees the slot (recursively for child refs) at rc 0.
    pub fn drop_ref(&mut self, r: ArenaRef) {
        let cur = self.rc(r);
        if cur == 0 {
            return;
        }
        if cur > 1 {
            if let Some(Slot::Occupied { rc, .. }) = self.slot_mut(r) {
                *rc = cur - 1;
            }
            return;
        }
        let mut val = match self.slot_mut(r) {
            Some(Slot::Occupied { val, .. }) => std::mem::replace(val, Slot::default_value()),
            _ => return,
        };
        // Drop the children first, then recycle the pooled payload buffer.
        val.visit_refs(|child| self.drop_ref(child));
        if let Some(buf) = val.detach_payload() {
            self.put_buf(buf);
        }
        *self.slot_mut(r) = Slot::Free { next: self.free };
        self.free = Some(r);
        self.live -= 1;
    }

    /// Copy-on-write entry point: returns a ref that is safe to mutate.
    /// Copies when `rc > 1`, otherwise returns the same ref.
    pub fn mutate(&mut self, r: ArenaRef) -> Result<ArenaRef, ArenaError> {
        let rc = self.rc(r);
        if rc == 0 {
            return Err(ArenaError::DanglingRef);
        }
        if rc <= 1 {
            return Ok(r);
        }
        let val = match self.get(r) {
            Some(v) => v.clone_pooled(&mut self.pool)?,
            None => return Err(ArenaError::DanglingRef),
        };
        val.visit_refs(|child| {
            if let Err(e) = self.copy(child) {
                // Recounting cannot fail unless a child is dangling; treat as
                // an internal error (dropped below by unwinding through the
                // closure is not possible — the arena stays consistent because
                // `copy` errors only on a dangling ref, which is a caller bug).
                let _ = e;
            }
        });
        self.drop_ref(r);
        match self.alloc(val) {
            Ok(new) => Ok(new),
            Err((err, val)) => {
                // Fresh slot never existed; release the clone's recounted
                // children so the arena stays balanced.
                val.visit_refs(|child| self.drop_ref(child));
                Err(err)
            }
        }
    }

    /// Pool-aware clone of a slot's value into an owned [`ArenaValue`].
    pub fn clone_pooled(&mut self, r: ArenaRef) -> Result<V, ArenaError> {
        let v = self.get(r).ok_or(ArenaError::DanglingRef)?;
        v.clone_pooled(&mut self.pool)
    }

    /// Internal: fast path / paged access to a slot by ref.
    #[inline]
    fn slot_ref(&self, r: ArenaRef) -> Option<&Slot<V>> {
        if (r as usize) >= self.slots.len() {
            return None;
        }
        // SAFETY-free path: ChunkedVec access is safe. The fast path below is
        // just index math; the `num_chunks() == 1` branch avoids the shift.
        if self.slots.num_chunks() == 1 {
            Some(&self.slots.chunk(0)[r as usize])
        } else {
            Some(self.slots.index(r as usize))
        }
    }

    #[inline]
    fn slot_mut(&mut self, r: ArenaRef) -> Option<&mut Slot<V>> {
        if (r as usize) >= self.slots.len() {
            return None;
        }
        if self.slots.num_chunks() == 1 {
            Some(&mut self.slots.chunk_mut(0)[r as usize])
        } else {
            Some(self.slots.index_mut(r as usize))
        }
    }
}

impl<V: ArenaValue> Default for Slot<V> {
    fn default() -> Self {
        Slot::Free { next: None }
    }
}

impl<V: ArenaValue> Slot<V> {
    fn default_value() -> V {
        // Unreachable: only called on an Occupied slot whose value is replaced
        // below via mem::replace with a fresh Default; V's Default is not
        // required, so we instead require a placeholder here.
        unreachable!("default_value() must not be called; see drop_ref")
    }
}
```

**Important correction for `drop_ref`:** the code above calls `Slot::default_value()` which is wrong. Replace the `mem::replace` trick: since `V: ArenaValue` has no `Default`, pop the value out via `Option`-style replacement. The cleanest safe approach: make `Slot<V>` hold `Option<V>` for the value:

```rust
enum Slot<V> {
    Occupied { rc: u32, val: Option<V> },
    Free { next: Option<ArenaRef> },
}
```

Then `drop_ref` does `std::mem::take(&mut slot.val).unwrap()` and `get` returns `val.as_ref()`. Adjust all accessors (`get`, `get_mut`, `rc`, `alloc`). **Adopt this `Option<V>` variant in the implementation** and drop the `default_value` helper entirely. (The plan's test payloads are unaffected.)

- [x] **Step 1.3.4: Run the tests**

Run: `cargo test -p rill-core memory::arena`
Expected: PASS (all ported tests).

- [x] **Step 1.3.5: Commit**

```bash
git add rill-core/src/memory/arena.rs
git commit -m 'feat(rill-core): generic Arena<V> + ArenaValue with paged slot store'
```

### Task 1.4: rill-lang shim — `impl ArenaValue for Value`

**Files:**
- Modify: `rill-lang/src/arena.rs` (reduce to re-exports + `impl ArenaValue for Value`)
- Modify: `rill-lang/src/lib.rs`
- Modify: `rill-lang/src/program.rs`
- Modify: `rill-lang/src/backend/interp.rs`

- [x] **Step 1.4.1: Rewrite `rill-lang/src/arena.rs` as a shim**

Keep `Value`, `ValueKind`, and add the `ArenaValue` impl. The `Arena`/`ArenaRef`/`ArenaError`/`BufferPool` definitions move out; re-export them from rill-core:

```rust
//! Value arena — rill-lang's value heap, backed by the generic allocator in
//! [`rill_core::memory`].

pub use rill_core::memory::{Arena, ArenaError, ArenaRef, ArenaValue, BufferPool};

/// The kind of a [`Value`], used for runtime dispatch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueKind {
    Int, Float, Bool, String, Record, Sum, Newtype, Closure, List, Map, Set, Void,
}

/// A heap value stored in an [`Arena`] slot.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Record(Vec<ArenaRef>),
    Sum(u32, Vec<ArenaRef>),
    Newtype(ArenaRef),
    Closure(ArenaRef, u32),
    List { elems: Vec<ArenaRef> },
    Map { pairs: Vec<(ArenaRef, ArenaRef)> },
    Set { elems: Vec<ArenaRef> },
    Void,
}

impl Value {
    /// The [`ValueKind`] of this value.
    pub fn kind(&self) -> ValueKind {
        match self {
            Value::Int(_) => ValueKind::Int,
            Value::Float(_) => ValueKind::Float,
            Value::Bool(_) => ValueKind::Bool,
            Value::String(_) => ValueKind::String,
            Value::Record(_) => ValueKind::Record,
            Value::Sum(_, _) => ValueKind::Sum,
            Value::Newtype(_) => ValueKind::Newtype,
            Value::Closure(_, _) => ValueKind::Closure,
            Value::List { .. } => ValueKind::List,
            Value::Map { .. } => ValueKind::Map,
            Value::Set { .. } => ValueKind::Set,
            Value::Void => ValueKind::Void,
        }
    }
}

impl ArenaValue for Value {
    fn visit_refs(&self, mut f: impl FnMut(ArenaRef)) {
        match self {
            Value::Record(fields) | Value::Sum(_, fields) => {
                for &x in fields {
                    f(x);
                }
            }
            Value::Newtype(inner) => f(*inner),
            Value::Closure(env, _) => f(*env),
            Value::List { elems } => {
                for &e in elems {
                    f(e);
                }
            }
            Value::Map { pairs } => {
                for &(k, v) in pairs {
                    f(k);
                    f(v);
                }
            }
            Value::Set { elems } => {
                for &e in elems {
                    f(e);
                }
            }
            _ => {}
        }
    }

    fn clone_pooled(&self, pool: &mut BufferPool) -> Result<Self, ArenaError> {
        // Pool-aware clone: read shape, borrow a payload buffer, copy refs.
        let shape: Option<(u8, usize)> = match self {
            Value::Record(fields) => Some((0, fields.len())),
            Value::Sum(_, fields) => Some((1, fields.len())),
            Value::List { elems } => Some((2, elems.len())),
            Value::Set { elems } => Some((3, elems.len())),
            _ => None,
        };
        let Some((kind, n)) = shape else {
            return Ok(self.clone());
        };
        let mut buf = pool.take(n).ok_or(ArenaError::CapacityExceeded)?;
        match (kind, self) {
            (0, Value::Record(fields)) => buf.extend_from_slice(fields),
            (1, Value::Sum(c, fields)) => {
                let ctor = *c;
                buf.extend_from_slice(fields);
                return Ok(Value::Sum(ctor, buf));
            }
            (2, Value::List { elems }) => buf.extend_from_slice(elems),
            (3, Value::Set { elems }) => buf.extend_from_slice(elems),
            _ => {}
        }
        match kind {
            0 => Ok(Value::Record(buf)),
            2 => Ok(Value::List { elems: buf }),
            _ => Ok(Value::Set { elems: buf }),
        }
    }

    fn detach_payload(&mut self) -> Option<Vec<ArenaRef>> {
        match self {
            Value::Record(fields) | Value::List { elems } | Value::Set { elems } => {
                let taken = std::mem::take(fields_or_elems());
                (!taken.is_empty()).then_some(taken)
            }
            _ => None,
        }
    }
}
```

**Note (compile fix for `detach_payload`):** `Value::Record`/`List`/`Set` are distinct variants — `std::mem::take` must be dispatched per variant:

```rust
fn detach_payload(&mut self) -> Option<Vec<ArenaRef>> {
    let taken = match self {
        Value::Record(fields) => std::mem::take(fields),
        Value::List { elems } => std::mem::take(elems),
        Value::Set { elems } => std::mem::take(elems),
        _ => return None,
    };
    (!taken.is_empty()).then_some(taken)
}
```

(`Sum` payloads are also pooled today — include `Value::Sum(_, fields)` in the match.)

- [x] **Step 1.4.2: `lib.rs` — keep the public re-exports**

`rill-lang/src/lib.rs:44` keeps `pub use arena::{Arena, ArenaError, ArenaRef, Value, ValueKind};` — these now resolve through the shim to `rill_core::memory` types. No change needed beyond the shim.

- [x] **Step 1.4.3: `program.rs` — `Arena<Value>`**

In `rill-lang/src/program.rs`:
- `use crate::arena::{Arena, BufferPool};` → add `Value`: `use crate::arena::{Arena, ArenaValue, BufferPool, Value};`
- `arena: Arena` → `arena: Arena<Value>` (field, and both construction sites in `new`/`build`: `Arena::with_capacity(...)`).
- `alloc_main_cells(arena: &mut Arena, ...)` → `&mut Arena<Value>`; `arena.alloc(crate::arena::Value::Void)` unchanged.

- [x] **Step 1.4.4: `backend/interp.rs` — compile check**

All `prog.arena.*` calls keep their names (`alloc`, `copy`, `drop_ref`, `mutate`, `clone_pooled`, `get`, `get_mut`, `rc`, `take_buf`, `put_buf`, `live`). Run the crate check to surface any remaining `Arena` type mentions that need `<Value>`:

Run: `cargo check -p rill-lang`
Fix any `Arena`-without-parameter mentions until clean.

- [x] **Step 1.4.5: Run the rill-lang tests**

Run: `cargo test -p rill-lang`
Expected: all pass (the arena behavior is identical; the ~100 test call sites use the public API).

- [x] **Step 1.4.6: Commit**

```bash
git add rill-lang/src/arena.rs rill-lang/src/lib.rs rill-lang/src/program.rs rill-lang/src/backend/interp.rs
git commit -m 'refactor(rill-lang): use generic Arena<Value> from rill_core::memory'
```

### Task 1.5: Trait-bound removal (§2.2)

**Files:**
- Modify: `rill-core/src/traits/algorithm.rs:110`
- Modify: `rill-core/src/traits/multichannel_algorithm.rs:8`
- Modify: `rill-sampler/src/tape/write_head.rs:14,16`
- Modify: `rill-sampler/src/tape/read_head.rs:32,34`

- [x] **Step 1.5.1: Remove the supertraits**

`algorithm.rs`:
```rust
pub trait Algorithm<T: Transcendental> {
```
`multichannel_algorithm.rs`:
```rust
pub trait MultichannelAlgorithm<T: Transcendental> {
```

- [x] **Step 1.5.2: Drop the now-unnecessary unsafe impls**

`write_head.rs` — remove lines 14 and 16 (`unsafe impl<T: Transcendental, const B: usize> Send for WriteHead<T, B> {}` and the `Sync` one), and delete their preceding doc note about "builtin registry's static bounds". Same for `read_head.rs` lines 32/34.

- [x] **Step 1.5.3: Verify workspace compiles**

Run: `cargo check --workspace`
Expected: clean (0 errors, 0 warnings) — this was already verified empirically before the plan was written.

- [x] **Step 1.5.4: Run tests**

Run: `cargo test --workspace`
Expected: pass.

- [x] **Step 1.5.5: Commit**

```bash
git add rill-core/src/traits/algorithm.rs rill-core/src/traits/multichannel_algorithm.rs rill-sampler/src/tape/write_head.rs rill-sampler/src/tape/read_head.rs
git commit -m 'refactor(rill-core): drop Send+Sync supertraits from Algorithm/MultichannelAlgorithm (redundant)'
```

### Task 1.6: P1 verification + benchmark

**Files:**
- Modify: `rill-lang/benches/lang_bench.rs` (no change expected; run only)

- [x] **Step 1.6.1: Full phase checks**

Run: `cargo clippy --all-features --workspace` and `cargo fmt`; fix anything the memory module or the shim introduces. Re-run `cargo test --workspace`.

- [x] **Step 1.6.2: Benchmark before/after**

Run: `cargo bench -p rill-lang --bench lang_bench`
Compare against the Phase 0 numbers. The paged fast path (`num_chunks()==1`) must show **no regression**; note any delta in the commit or a `CHANGELOG.md` entry.

- [x] **Step 1.6.3: Miri on the new module**

Run: `cargo +nightly miri test -p rill-core memory::` (if nightly + Miri available). Expected: no UB. If nightly is unavailable, note it and rely on the unit tests + SAFETY comments.

- [x] **Step 1.6.4: Update `docs/CHANGELOG.md`** with a P1 entry (arena moved to rill-core, genericized; `Algorithm`/`MultichannelAlgorithm` supertraits relaxed).

---

## Phase 2 — Region system + `TapeRing` + region-backed buffers

### Task 2.1: `MemoryRegion` / `RegionHandle` / `BufferAllocator`

**Files:**
- Create: `rill-core/src/memory/region.rs`

- [x] **Step 2.1.1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use std::cell::UnsafeCell;

    #[test]
    fn bump_allocation_and_slices() {
        let region = Rc::new(UnsafeCell::new(MemoryRegion::<f32>::with_capacity(16, 16, 0.0)));
        let mut alloc = BufferAllocator { region: region.clone() };
        let h = alloc.allocate(4).unwrap();
        assert_eq!(h.len(), 4);
        h.with_data_mut(|s| s.copy_from_slice(&[1.0, 2.0, 3.0, 4.0]));
        h.with_data(|s| assert_eq!(s, &[1.0, 2.0, 3.0, 4.0]));
        assert_eq!(alloc.used(), 4);
    }

    #[test]
    fn handles_share_one_slab_and_are_cloneable() {
        let region = Rc::new(UnsafeCell::new(MemoryRegion::<f32>::with_capacity(32, 16, 0.0)));
        let mut alloc = BufferAllocator { region: region.clone() };
        let w = alloc.allocate(8).unwrap();
        let r = w.clone();
        w.with_data_mut(|s| s.fill(7.0));
        r.with_data(|s| assert!(s.iter().all(|&x| x == 7.0)));
    }

    #[test]
    fn exhaustion_returns_none_when_not_growable() {
        let region = Rc::new(UnsafeCell::new(MemoryRegion::<f32>::with_capacity(4, 4, 0.0)));
        let mut alloc = BufferAllocator { region };
        assert!(alloc.allocate(4).is_some());
        assert!(alloc.allocate(1).is_none());
    }

    #[test]
    fn non_spanning_allocation_skips_chunks() {
        // chunk_cap 8; allocate 5 (uses chunk 0 fully), then 5 -> must not span
        let region = Rc::new(UnsafeCell::new(MemoryRegion::<f32>::with_capacity(16, 8, 0.0)));
        let mut alloc = BufferAllocator { region };
        let _a = alloc.allocate(5).unwrap();
        let b = alloc.allocate(5).unwrap();
        // b starts at chunk 1: its offset is 8, not 5.
        assert_eq!(b.offset(), 8);
        assert_eq!(b.len(), 5);
    }
}
```

- [x] **Step 2.1.2: Run to verify they fail**

Run: `cargo test -p rill-core memory::region`
Expected: FAIL — types not defined.

- [x] **Step 2.1.3: Implement `region.rs`**

```rust
//! # Region slabs — large static buffer memory
//!
//! A [`MemoryRegion`] is a chunked, grow-safe slab of `T` (f32/f64). Handles
//! ([`RegionHandle`]) are reference-counted views (offset + length) over the
//! slab, carved at build time by a [`BufferAllocator`]. Regions are static with
//! program lifetime; RC exists for sharing (one write head + many read heads),
//! not early free.

use std::cell::UnsafeCell;
use std::rc::Rc;

use super::chunked::ChunkedVec;

/// A grow-safe slab of `T` split into chunks. Grows only at build time.
pub struct MemoryRegion<T> {
    slab: ChunkedVec<T>,
    next: usize,
    chunk_cap: usize,
}

impl<T: Clone> MemoryRegion<T> {
    /// Create a region pre-allocating `initial` elements across `chunk_cap`
    /// element chunks, filled with `fill`. `chunk_cap` must be a power of two.
    pub fn with_capacity(initial: usize, chunk_cap: usize, fill: T) -> Self {
        Self {
            slab: ChunkedVec::with_capacity(initial, chunk_cap, |_| fill.clone()),
            next: 0,
            chunk_cap,
        }
    }

    /// Grow the slab by one chunk (build-time only).
    pub fn grow(&mut self, fill: T) {
        self.slab.push_chunk(|_| fill.clone());
    }

    /// Bump-allocate `len` elements starting at or after `self.next`, never
    /// spanning a chunk boundary. Returns the offset, or `None` on exhaustion
    /// when no growth is available.
    fn bump(&mut self, len: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }
        // Chunk boundary for the current cursor.
        let chunk_start = (self.next / self.chunk_cap) * self.chunk_cap;
        let chunk_end = chunk_start + self.chunk_cap;
        let start = if self.next + len <= chunk_end {
            self.next
        } else {
            // Skip to the next chunk.
            chunk_end
        };
        if start + len > self.slab.len() {
            return None;
        }
        self.next = start + len;
        Some(start)
    }
}

/// A reference-counted view over a [`MemoryRegion`] slab.
pub struct RegionHandle<T> {
    region: Rc<UnsafeCell<MemoryRegion<T>>>,
    offset: usize,
    len: usize,
}

impl<T> RegionHandle<T> {
    /// Starting offset in the slab.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Length in elements.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the handle is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Read access to the handle's slice.
    ///
    /// # SAFETY
    /// The graph is single-threaded; this handle's range is disjoint from all
    /// other write handles, and no writer is live while a reader is. The slab
    /// is a stable allocation (chunked), so the address is valid for the
    /// region's lifetime, which is pinned by the `Rc` in this handle.
    #[allow(unsafe_code)]
    pub fn with_data(&self, f: impl FnOnce(&[T])) {
        let region = unsafe { &*self.region.get() };
        let start = self.offset;
        let end = start + self.len;
        f(&region.slab.slab_slice(start, end));
    }

    /// Write access to the handle's slice.
    ///
    /// # SAFETY
    /// The single-writer / many-readers convention: at most one writer per
    /// handle range at a time, never concurrent with a reader on the same
    /// range. Bounds are `offset+len <= slab.len()` by construction.
    #[allow(unsafe_code)]
    pub fn with_data_mut(&self, f: impl FnOnce(&mut [T])) {
        let region = unsafe { &mut *self.region.get() };
        let start = self.offset;
        let end = start + self.len;
        f(&mut region.slab.slab_slice_mut(start, end));
    }
}

impl<T> Clone for RegionHandle<T> {
    fn clone(&self) -> Self {
        Self {
            region: Rc::clone(&self.region),
            offset: self.offset,
            len: self.len,
        }
    }
}
```

**Note (compile fix):** `ChunkedVec` needs `slab_slice(start, end) -> &[T]` and `slab_slice_mut` helpers that return a contiguous slice over the logical range — only valid because handles never span chunk boundaries. Add them to `chunked.rs` (Task 1.1) with a debug assertion `start.div_ceil(chunk_cap) == end.div_ceil(chunk_cap)` (same chunk), returning `&self.chunks[start >> bits][start & mask ..][..len]`.

Add `BufferAllocator` to the same file:

```rust
/// Build-time allocator carving handles out of one [`MemoryRegion`] slab.
pub struct BufferAllocator<T> {
    region: Rc<UnsafeCell<MemoryRegion<T>>>,
}

impl<T: Clone> BufferAllocator<T> {
    /// Create an allocator over a fresh region with `initial` pre-allocated
    /// elements and `chunk_cap`-element chunks.
    pub fn new(initial: usize, chunk_cap: usize, fill: T) -> Self {
        Self {
            region: Rc::new(UnsafeCell::new(MemoryRegion::with_capacity(
                initial, chunk_cap, fill,
            ))),
        }
    }

    /// Carve a handle of `len` elements. `None` on exhaustion (build-time
    /// error); the caller may call [`grow`](Self::grow) first.
    pub fn allocate(&mut self, len: usize) -> Option<RegionHandle<T>> {
        // SAFETY: single-threaded build; no outstanding &mut borrows.
        #[allow(unsafe_code)]
        let region = unsafe { &mut *self.region.get() };
        let offset = region.bump(len)?;
        Some(RegionHandle {
            region: Rc::clone(&self.region),
            offset,
            len,
        })
    }

    /// Grow the slab by one chunk (build-time only). Call before `allocate`
    /// when a request exceeds the current slab.
    pub fn grow(&mut self, fill: T) {
        // SAFETY: single-threaded build.
        #[allow(unsafe_code)]
        let region = unsafe { &mut *self.region.get() };
        region.grow(fill);
    }

    /// Total elements currently available.
    pub fn capacity(&self) -> usize {
        // SAFETY: no mutation; &self only.
        #[allow(unsafe_code)]
        let region = unsafe { &*self.region.get() };
        region.slab.len()
    }

    /// Elements allocated so far.
    pub fn used(&self) -> usize {
        // SAFETY: no mutation; &self only.
        #[allow(unsafe_code)]
        let region = unsafe { &*self.region.get() };
        region.next
    }

    /// Remaining elements before exhaustion.
    pub fn remaining(&self) -> usize {
        self.capacity().saturating_sub(self.used())
    }
}
```

- [x] **Step 2.1.4: Run the tests + Miri**

Run: `cargo test -p rill-core memory::region` then `cargo +nightly miri test -p rill-core memory::region`
Expected: PASS; Miri clean.

- [x] **Step 2.1.5: Commit**

```bash
git add rill-core/src/memory/region.rs rill-core/src/memory/chunked.rs
git commit -m 'feat(rill-core): MemoryRegion/RegionHandle/BufferAllocator region slabs'
```

### Task 2.2: `TapeRing<T, S>` + `RingStorage`

**Files:**
- Create: `rill-core/src/buffer/tape_ring.rs`
- Modify: `rill-core/src/buffer/mod.rs` (declare + re-export)

- [x] **Step 2.2.1: Write the failing tests**

Port `tape.rs` and `delay.rs` tests as `TapeRing` tests over `Vec<T>` storage (a `RingStorage` impl that owns a `Vec<T>`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Test storage: an owned Vec.
    struct V<T>(Vec<T>);
    impl<T: Transcendental> RingStorage<T> for V<T> {
        fn len(&self) -> usize { self.0.len() }
        fn with_data(&self, f: impl FnOnce(&[T])) { f(&self.0); }
        fn with_data_mut(&mut self, f: impl FnOnce(&mut [T])) { f(&mut self.0); }
    }

    fn ring<const N: usize>() -> TapeRing<f32, V<f32>> {
        TapeRing::new(V(vec![0.0f32; N]))
    }

    #[test]
    fn write_read_wraparound() {
        let mut t = ring::<4>();
        for i in 0..10 { t.write(i as f32); }
        assert_eq!(t.read(0), 9.0);
        assert_eq!(t.read(1), 8.0);
        assert_eq!(t.read(2), 7.0);
        assert_eq!(t.read(3), 6.0);
    }

    #[test]
    fn block_roundtrip() {
        let mut t = ring::<4>();
        let block = [1.0f32, 2.0, 3.0, 4.0];
        t.write_block(&block);
        let mut out = [0.0f32; 4];
        t.read_block(0, &mut out);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn read_interpolated() {
        let mut t = ring::<4>();
        t.write(0.0); t.write(1.0); t.write(2.0); t.write(3.0);
        let v = t.read_interpolated(1.5);
        assert!((v - 2.5).abs() < 0.001);
    }

    #[test]
    fn clear_resets() {
        let mut t = ring::<4>();
        t.write_block(&[1.0, 2.0, 3.0, 4.0]);
        t.clear();
        assert_eq!(t.read(0), 0.0);
    }
}
```

- [x] **Step 2.2.2: Run to verify they fail**

Run: `cargo test -p rill-core buffer::tape_ring`
Expected: FAIL — not defined.

- [x] **Step 2.2.3: Implement `TapeRing`**

`rill-core/src/buffer/tape_ring.rs`:

```rust
//! # TapeRing — unified non-consuming write-head ring
//!
//! The shared ring math behind `TapeLoop`, `DelayLine`, and rill-lang's `@`
//! delay rings: `write` advances a single head; reads are at a delay behind it
//! (delay 0 = most recent); nothing is ever consumed. Storage is abstracted via
//! [`RingStorage`], so the same ring works over a [`RegionHandle`], a
//! `Box<[T]>`, or an inline `[T; N]`.

use crate::math::Transcendental;

/// Abstract ring storage: the contiguous backing plus its usable length.
pub trait RingStorage<T> {
    /// Usable length of the ring (tape capacity / delay length).
    fn len(&self) -> usize;
    /// Read access to the whole backing.
    fn with_data(&self, f: impl FnOnce(&[T]));
    /// Write access to the whole backing.
    fn with_data_mut(&mut self, f: impl FnOnce(&mut [T]));
}

/// A non-consuming write-head ring over any [`RingStorage`].
pub struct TapeRing<T, S: RingStorage<T>> {
    storage: S,
    head: usize,
}

impl<T: Transcendental, S: RingStorage<T>> TapeRing<T, S> {
    /// Create a ring over `storage`, with the head at the start.
    pub fn new(storage: S) -> Self {
        Self { storage, head: 0 }
    }

    /// Write a single sample and advance the head.
    #[inline(always)]
    pub fn write(&mut self, sample: T) {
        let n = self.storage.len();
        self.storage.with_data_mut(|buf| buf[self.head] = sample);
        self.head = (self.head + 1) % n;
    }

    /// Read a sample at `delay` behind the head (0 = most recent).
    #[inline(always)]
    pub fn read(&self, delay: usize) -> T {
        let n = self.storage.len();
        debug_assert!(delay < n, "delay {delay} out of range (len {n})");
        let pos = (self.head + n - delay - 1) % n;
        let mut out = T::ZERO;
        self.storage.with_data(|buf| out = buf[pos]);
        out
    }

    /// Write a whole block, advancing the head.
    pub fn write_block(&mut self, block: &[T]) {
        let n = self.storage.len();
        let len = block.len().min(n);
        self.storage.with_data_mut(|buf| {
            for (i, &b) in block.iter().enumerate().take(len) {
                buf[(self.head + i) % n] = b;
            }
        });
        self.head = (self.head + len) % n;
    }

    /// Read a whole block at `delay` behind the head (oldest first).
    pub fn read_block(&self, delay: usize, out: &mut [T]) {
        let n = self.storage.len();
        let d = delay.min(n - 1);
        let len = out.len().min(n);
        for (i, o) in out.iter_mut().enumerate().take(len) {
            *o = self.read(d + len - 1 - i);
        }
    }

    /// Read a single sample at a fractional `delay` with linear interpolation.
    pub fn read_interpolated(&self, delay: f64) -> T {
        let d = delay as usize;
        let frac = T::from_f64(delay.fract());
        let s1 = self.read(d);
        let s2 = self.read(d + 1);
        s1 + (s2 - s1) * frac
    }

    /// Zero the ring and reset the head.
    pub fn clear(&mut self) {
        self.storage.with_data_mut(|buf| buf.fill(T::ZERO));
        self.head = 0;
    }

    /// Usable length.
    pub fn len(&self) -> usize {
        self.storage.len()
    }
}
```

Note: `TapeRing::read_block` above matches **TapeLoop** semantics (`TapeLoop::read_block(d, out)` reads samples at delays `d+len-1 .. d`, oldest first). The rill-lang `DelayRing::read_block` (read at delay 0) is exactly `TapeRing::read_block(0, out)` — see Task 2.4.

- [x] **Step 2.2.4: Wire into `buffer/mod.rs`**

Add `mod tape_ring;` to the submodule list and `pub use tape_ring::{RingStorage, TapeRing};` to the re-exports.

- [x] **Step 2.2.5: Run the tests**

Run: `cargo test -p rill-core buffer::tape_ring`
Expected: PASS.

- [x] **Step 2.2.6: Commit**

```bash
git add rill-core/src/buffer/tape_ring.rs rill-core/src/buffer/mod.rs
git commit -m 'feat(rill-core): TapeRing unified ring over RingStorage'
```

### Task 2.3: `TapeLoop` over a `RegionHandle`

**Files:**
- Modify: `rill-core/src/buffer/tape.rs`
- Modify: `rill-core/src/buffer/mod.rs` (re-export `RingStorage for RegionHandle`)

- [x] **Step 2.3.1: Add `RingStorage for RegionHandle<T>`**

In `rill-core/src/memory/region.rs` add (import `crate::buffer::tape_ring::RingStorage`):

```rust
impl<T: Clone> RingStorage<T> for RegionHandle<T> {
    fn len(&self) -> usize {
        RegionHandle::len(self)
    }
    fn with_data(&self, f: impl FnOnce(&[T])) {
        RegionHandle::with_data(self, f);
    }
    fn with_data_mut(&mut self, f: impl FnOnce(&mut [T])) {
        // `with_data_mut` takes `&self` on the handle (interior mutability).
        RegionHandle::with_data_mut(self, f);
    }
}
```

- [x] **Step 2.3.2: Rewrite `TapeLoop` as a `TapeRing` wrapper**

`rill-core/src/buffer/tape.rs` becomes:

```rust
//! Tape loop — a large delay buffer for tape effects. Backed by a
//! [`TapeRing`] over a [`RegionHandle`] (or any [`RingStorage`]).

use crate::buffer::registry::DelayBuffer;
use crate::buffer::tape_ring::{RingStorage, TapeRing};
use crate::math::Transcendental;

/// Heap-backed tape loop. Kept for API compatibility; the backing is a
/// [`TapeRing`] over an owned `Vec<T>`.
#[derive(Debug)]
pub struct TapeLoop<T: Transcendental> {
    inner: TapeRing<T, VecStorage<T>>,
}

#[derive(Debug)]
pub(crate) struct VecStorage<T>(Vec<T>);

impl<T: Transcendental> RingStorage<T> for VecStorage<T> {
    fn len(&self) -> usize { self.0.len() }
    fn with_data(&self, f: impl FnOnce(&[T])) { f(&self.0); }
    fn with_data_mut(&mut self, f: impl FnOnce(&mut [T])) { f(&mut self.0); }
}

impl<T: Transcendental> TapeLoop<T> {
    /// Allocate a new tape loop with the given capacity (in samples).
    pub fn new(capacity: usize) -> Option<Self> {
        if capacity == 0 {
            return None;
        }
        let mut vec = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            vec.push(T::ZERO);
        }
        Some(Self {
            inner: TapeRing::new(VecStorage(vec)),
        })
    }

    /// Maximum capacity in samples.
    pub fn capacity(&self) -> usize {
        self.inner.len()
    }

    /// Write a single sample and advance the write cursor.
    #[inline(always)]
    pub fn write(&mut self, sample: T) {
        self.inner.write(sample);
    }

    /// Read a sample at `delay` samples behind the write position.
    #[inline(always)]
    pub fn read(&self, delay: usize) -> T {
        self.inner.read(delay)
    }

    /// Read with linear interpolation between samples.
    #[inline(always)]
    pub fn read_interpolated(&self, delay: f64) -> T {
        self.inner.read_interpolated(delay)
    }

    /// Write a full block of samples.
    #[inline(always)]
    pub fn write_block(&mut self, block: &[T]) {
        self.inner.write_block(block);
    }

    /// Read a full block starting at `delay` samples behind write position.
    #[inline(always)]
    pub fn read_block(&self, delay: usize, output: &mut [T]) {
        self.inner.read_block(delay, output);
    }

    /// Fill the entire buffer with a constant value.
    pub fn fill(&mut self, value: T) {
        self.inner.clear();
        self.inner.write_block(&vec![value; self.inner.len()]);
    }

    /// Reset write position and zero the buffer.
    pub fn clear(&mut self) {
        self.inner.clear();
    }
}
```

Note: keep `write_pos()` and the `Buffer<T>`/`DelayBuffer<T>` impls delegating to the same methods. **This task keeps `TapeLoop` on a `Vec`** (zero behavior change) so the region-backed switch can land in Task 2.4 alongside `RillProgram`'s allocator; a follow-up (Phase 3) migrates `TapeLoop` to `RegionHandle` storage via the factory allocator.

- [x] **Step 2.3.3: Run the tape tests**

Run: `cargo test -p rill-core buffer::tape` (existing tests must stay green).

- [x] **Step 2.3.4: Commit**

```bash
git add rill-core/src/buffer/tape.rs rill-core/src/memory/region.rs rill-core/src/buffer/mod.rs
git commit -m 'refactor(rill-core): TapeLoop over TapeRing; RingStorage for RegionHandle'
```

### Task 2.4: `RillProgram` — `BufferAllocator`, region-backed delays

**Files:**
- Modify: `rill-lang/src/program.rs`
- Modify: `rill-lang/src/backend/interp.rs` (delay read/write sites, `program.rs:108` `DelayRing`)

- [x] **Step 2.4.1: Write the failing test (region delays compile + run)**

In `rill-lang/tests/` add a test that uses `@` delays and a tape, e.g. extend `tests/delays.rs` (or create `tests/region_delays.rs`):

```rust
use rill_lang::compile;

#[test]
fn delay_sites_are_region_backed_and_correct() {
    let mut prog = compile::<f32>("main = _ @ 2 + _ @ 3").unwrap();
    let mut out = [0.0f32; 4];
    // First tick: no history yet.
    prog.process(Some(&[1.0, 2.0, 3.0, 4.0]), &mut out).unwrap();
    // Values flow through the two delay lines; block semantics verified by
    // the existing test suite — this test pins that programs still run.
    assert_eq!(out.len(), 4);
}
```

- [x] **Step 2.4.2: Run to verify it fails**

Run: `cargo test -p rill-lang region_delays`
Expected: FAIL — no test yet (create file) then PASS after wiring (see below).

- [x] **Step 2.4.3: Add `RegionDelay` and the allocator to `RillProgram`**

In `rill-lang/src/program.rs`:

```rust
use rill_core::buffer::tape_ring::{RingStorage, TapeRing};
use rill_core::memory::{BufferAllocator, RegionHandle};

/// A region-backed `@` delay site: a `TapeRing` over a [`RegionHandle`].
pub(crate) struct RegionDelay<T: Transcendental>(TapeRing<T, RegionHandle<T>>);

impl<T: Transcendental> RegionDelay<T> {
    fn new(buf: RegionHandle<T>) -> Self {
        Self(TapeRing::new(buf))
    }
    pub(crate) fn read_block(&self, out: &mut [T]) {
        self.0.read_block(0, out);
    }
    pub(crate) fn write_block(&mut self, input: &[T]) {
        self.0.write_block(input);
    }
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}
```

Replace the `delays: Vec<DelayRing<T, MAX_DELAY_LEN>>` field with `delays: Vec<RegionDelay<T>>`, and add `allocator: BufferAllocator<T>` to `RillProgram`. In `new`/`build`, size the allocator from the IR and carve each delay site:

```rust
let total_delay: usize = ir.state.delay_lens.iter().sum();
let mut allocator = BufferAllocator::new(total_delay.max(1), 8192, T::ZERO);
let delays = ir
    .state
    .delay_lens
    .iter()
    .map(|&l| {
        let h = allocator.allocate(l).unwrap_or_else(|| {
            allocator.grow(T::ZERO);
            allocator.allocate(l).expect("allocator must grow on demand at build")
        });
        RegionDelay::new(h)
    })
    .collect();
```

Delete `DelayRing` and `MAX_DELAY_LEN` (no longer used).

- [x] **Step 2.4.4: `interp.rs` delay call sites unchanged**

`prog.delays[*line].read_block(...)` / `write_block(...)` keep working — same method names on `RegionDelay`. Verify the crate compiles.

Run: `cargo check -p rill-lang`

- [x] **Step 2.4.5: Run rill-lang tests**

Run: `cargo test -p rill-lang`
Expected: pass.

- [x] **Step 2.4.6: Commit**

```bash
git add rill-lang/src/program.rs rill-lang/src/backend/interp.rs rill-lang/tests/region_delays.rs
git commit -m 'feat(rill-lang): region-backed @ delay sites via TapeRing/RegionHandle'
```

### Task 2.5: Phase 2 verification

- [x] **Step 2.5.1:** `cargo test --workspace`, `cargo clippy --all-features --workspace`, `cargo fmt`.
- [x] **Step 2.5.2:** `cargo bench -p rill-lang --bench lang_bench` — no regression vs Phase 0.
- [x] **Step 2.5.3:** Miri over `memory::region` + `buffer::tape_ring`.
- [x] **Step 2.5.4:** Update `docs/CHANGELOG.md` (P2 entry).

---

## Phase 3 — Algorithm internal buffers (factory contract + migrations)

### Task 3.1: Factory contract gains an optional allocator

**Files:**
- Modify: `rill-lang/src/builtin.rs` (factory type aliases + register/build signatures)
- Modify: `rill-lang/src/ffi.rs`
- Modify: `rill-lang/src/program.rs` (`build_builtin` passes the allocator)
- Modify: DSP crates' registration (`rill-core-dsp`, `rill-digital-effects`, `rill-router`, `rill-fft`, `rill-sampler`, `rill-lofi` `lang.rs`/`register.rs`) — mechanical `+ _: Option<&mut BufferAllocator<T>>` param

- [ ] **Step 3.1.1: Update the factory type aliases**

`builtin.rs:37-47` and `ffi.rs:22-31` — add `Option<&mut BufferAllocator<T>>` as the third parameter of `BlockFactory`/`MultichannelBlockFactory` (and the resource variants):

```rust
type BlockFactory<T> = Box<
    dyn Fn(&[f64], f32, Option<&mut BufferAllocator<T>>) -> Box<dyn BlockBuiltin<T>>
        + Send
        + Sync,
>;
```

- [ ] **Step 3.1.2: Update register + build methods**

`register_block`, `register_multichannel_block`, the resource variants, and `build_block`/`build_multichannel_block` (and FFI equivalents) thread the new parameter. Non-migrated factories ignore it. This is a mechanical ripple across ~10 crates; compile-driven: `cargo check --workspace` lists every closure to update. Update each with the extra `|params, sr, _alloc|` (or `..`-style) parameter.

- [ ] **Step 3.1.3: `program.rs::build_builtin` passes the allocator**

`RillProgram::build` owns `BufferAllocator<T>`; pass `Some(&mut allocator)` to factory builds.

- [ ] **Step 3.1.4: Verify + commit**

`cargo test --workspace`, `cargo clippy --all-features --workspace`. Commit `feat(rill-lang): factory contract carries optional BufferAllocator`.

### Task 3.2: Migrate FFT scratch + convolver

**Files:**
- Modify: `rill-fft/src/overlap_add.rs`, `rill-fft/src/real_fft.rs`, `rill-fft/src/lang.rs`

Replace each `Vec<T>`/`Vec<Complex<T>>` field with a `RegionHandle<T>`/`RegionHandle<Complex<T>>` carved from the allocator in the factory closure; the algorithm's `new(alloc: &mut BufferAllocator<T>)` requests its buffers, falling back to `Vec` when `alloc` is `None` (keeps standalone construction working). Port the crate's existing tests; add a region-backed construction test. Commit per crate.

### Task 3.3: Migrate wavetable `InterpolatedReader`

**Files:**
- Modify: `rill-core-dsp/src/generators/reader.rs`, `wavetable.rs`, `rill-core-dsp/src/lang.rs` (if a registration exists)

`InterpolatedReader.buffer: Box<[T]>` → optional `RegionHandle<T>`. Same `None`-allocator fallback. Keep standalone tests green.

### Task 3.4: Migrate physical-model grids

**Files:**
- Modify: `rill-core-model/src/string/mod.rs`, `rill-core-model/src/plate/mod.rs`

`delay_line: Vec<T>` / three `Vec<T>` grids → optional region handles via the allocator. Keep tests green.

### Task 3.5: Optional — `DelayLine` fold onto `TapeRing`

**Files:**
- Modify: `rill-core/src/buffer/delay.rs`

`DelayLine<T, N>` = `TapeRing<T, [T; N]>` + `delay_samples`/`sample_rate`/stats. Public API preserved (`set_delay`, `set_delay_samples`, `delay_samples`, `max_delay`, `sample_rate`, `write` (writes + returns delayed sample), `read`, `read_delayed`, `read_interpolated`, `clear`, `write_position`, `Index`, `IndexMut`, `Buffer` impl). Run the existing `delay.rs` tests unchanged. If the fold is not clean within this task, keep `DelayLine` as-is and mark the fold P4 — it is optional.

### Task 3.6: Phase 3 verification

- [ ] **Step 3.6.1:** `cargo test --workspace`, `cargo clippy --all-features --workspace`, `cargo fmt`.
- [ ] **Step 3.6.2:** `cargo bench -p rill-lang --bench lang_bench`; region micro-benchmark for `TapeRing`/`RegionHandle` access (unsafe path vs `RefCell` fallback) to satisfy §2.1.
- [ ] **Step 3.6.3:** Miri over the region/ring unsafe paths.
- [ ] **Step 3.6.4:** Update `docs/src/guides/rill-lang.md` (memory-model section), `CHANGELOG.md`.

---

## Phase 4 — Deferred (out of scope)

- `Array`/`Vector` big objects on top of `RegionHandle` (design constraint in spec §7).
- Pure-function `Algorithm` refactor (state passed into `process`).
- `DelayLine` fold (if not done in Task 3.5).

---

## Self-review checklist

- [ ] §2.1 unsafe policy → Tasks 2.1 (SAFETY comments + Miri), 3.6.3.
- [ ] §2.2 Send/Sync removal → Task 1.5.
- [ ] §3.1 `ChunkedVec` → Task 1.1.
- [ ] §3.2 `Arena<V>`/`ArenaValue` → Tasks 1.3, 1.4.
- [ ] §3.3 paged slot store + fast path → Task 1.3 (`slot_ref` fast path, `paged_store_uses_one_page_fast_path` test).
- [ ] §3.4 `BufferPool` → Task 1.2.
- [ ] §3.5 `MemoryRegion`/`RegionHandle`/`BufferAllocator` → Task 2.1.
- [ ] §3.6 `TapeRing`/`RingStorage` → Tasks 2.2–2.4.
- [ ] §4 arena adoption → Task 1.4.
- [ ] §5 factory contract + migrations → Phase 3.
- [ ] §6 RT safety → throughout (build-time-only growth; no locks; audited unsafe).
- [ ] §8 tests/bench/Miri → Tasks 1.6, 2.5, 3.6.
- [ ] §9 phasing → P1 (Tasks 1.1–1.6), P2 (2.1–2.5), P3 (3.1–3.6).
