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
///
/// The value is held in an `Option` so `drop_ref` can move it out without
/// requiring `V: Default` (the arena is generic over the payload).
#[derive(Debug, Clone)]
enum Slot<V> {
    /// A live value with a reference count.
    Occupied { rc: u32, val: Option<V> },
    /// A freed slot holding the index of the next free slot.
    Free { next: Option<ArenaRef> },
}

impl<V> Default for Slot<V> {
    fn default() -> Self {
        Slot::Free { next: None }
    }
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
        let slots = ChunkedVec::with_capacity(capacity, page_cap, |i| Slot::Free {
            next: (i + 1 < capacity).then_some((i + 1) as ArenaRef),
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
                let last = base + page_cap;
                self.slots.push_chunk(|i| Slot::Free {
                    next: (i + 1 < last).then_some((i + 1) as ArenaRef),
                });
                self.capacity += page_cap;
                base as ArenaRef
            }
            None => return Err((ArenaError::CapacityExceeded, val)),
        };
        self.free = match Self::slot_ref_in(&self.slots, idx) {
            Some(Slot::Free { next }) => *next,
            _ => unreachable!("arena free list corrupt"),
        };
        *Self::slot_mut_in(&mut self.slots, idx).expect("slot must exist") = Slot::Occupied {
            rc: 1,
            val: Some(val),
        };
        self.next_gen = self.next_gen.wrapping_add(1);
        self.live += 1;
        Ok(idx)
    }

    /// Reference count of a slot (0 if freed).
    pub fn rc(&self, r: ArenaRef) -> u32 {
        match Self::slot_ref_in(&self.slots, r) {
            Some(Slot::Occupied { rc, .. }) => *rc,
            _ => 0,
        }
    }

    /// Immutable view of a slot's value.
    pub fn get(&self, r: ArenaRef) -> Option<&V> {
        match Self::slot_ref_in(&self.slots, r) {
            Some(Slot::Occupied { val, .. }) => val.as_ref(),
            _ => None,
        }
    }

    /// Mutable view of a slot's value (no RC change). Debug builds assert the
    /// slot is exclusively owned (`rc == 1`).
    pub fn get_mut(&mut self, r: ArenaRef) -> Option<&mut V> {
        debug_assert_eq!(self.rc(r), 1);
        match Self::slot_mut_in(&mut self.slots, r) {
            Some(Slot::Occupied { val, .. }) => val.as_mut(),
            _ => None,
        }
    }

    /// Share a value: `rc++` and return the same ref.
    pub fn copy(&mut self, r: ArenaRef) -> Result<ArenaRef, ArenaError> {
        match Self::slot_mut_in(&mut self.slots, r) {
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
            if let Some(Slot::Occupied { rc, .. }) = Self::slot_mut_in(&mut self.slots, r) {
                *rc = cur - 1;
            }
            return;
        }
        let mut val = match Self::slot_mut_in(&mut self.slots, r) {
            Some(Slot::Occupied { val, .. }) => match val.take() {
                Some(v) => v,
                None => return,
            },
            _ => return,
        };
        // Drop the children first, then recycle the pooled payload buffer.
        val.visit_refs(|child| self.drop_ref(child));
        if let Some(buf) = val.detach_payload() {
            self.put_buf(buf);
        }
        *Self::slot_mut_in(&mut self.slots, r).expect("slot must exist") =
            Slot::Free { next: self.free };
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
        // Clone the value and recount its children into the copy. Uses plain
        // `Clone` (matching the original COW semantics) so the payload pool is
        // budgeted only for the collection ops, exactly as before the move.
        let val = match Self::slot_ref_in(&self.slots, r) {
            Some(Slot::Occupied { val, .. }) => match val.as_ref() {
                Some(v) => v.clone(),
                None => return Err(ArenaError::DanglingRef),
            },
            _ => return Err(ArenaError::DanglingRef),
        };
        val.visit_refs(|child| {
            if let Err(e) = self.copy(child) {
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
        let v = match Self::slot_ref_in(&self.slots, r) {
            Some(Slot::Occupied { val, .. }) => match val.as_ref() {
                Some(v) => v,
                None => return Err(ArenaError::DanglingRef),
            },
            _ => return Err(ArenaError::DanglingRef),
        };
        v.clone_pooled(&mut self.pool)
    }

    /// Fast-path / paged access to a slot by ref, borrowing only the slot store
    /// (so it can be combined with a separate `&mut self.pool` borrow).
    #[inline]
    fn slot_ref_in(slots: &ChunkedVec<Slot<V>>, r: ArenaRef) -> Option<&Slot<V>> {
        if (r as usize) >= slots.len() {
            return None;
        }
        if slots.num_chunks() == 1 {
            Some(&slots.chunk(0)[r as usize])
        } else {
            Some(slots.index(r as usize))
        }
    }

    #[inline]
    fn slot_mut_in(slots: &mut ChunkedVec<Slot<V>>, r: ArenaRef) -> Option<&mut Slot<V>> {
        if (r as usize) >= slots.len() {
            return None;
        }
        if slots.num_chunks() == 1 {
            Some(&mut slots.chunk_mut(0)[r as usize])
        } else {
            Some(slots.index_mut(r as usize))
        }
    }
}

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
            if self.kids.is_empty() {
                return Ok(self.clone());
            }
            let mut b = pool
                .take(self.kids.len())
                .ok_or(ArenaError::CapacityExceeded)?;
            b.extend_from_slice(&self.kids);
            Ok(Node { kids: b })
        }
        fn detach_payload(&mut self) -> Option<Vec<ArenaRef>> {
            // Always return the payload buffer (capacity preserved even when
            // empty) so the pool never drains on empty-payload drops.
            Some(std::mem::take(&mut self.kids))
        }
    }

    fn arena(cap: usize) -> Arena<Node> {
        Arena::with_capacity(cap)
    }

    fn leaf() -> Node {
        Node { kids: vec![] }
    }

    #[test]
    fn embedded_free_list_recycles_and_counts() {
        let mut a = arena(4);
        assert_eq!(a.live(), 0);
        let r = a.alloc(leaf()).unwrap();
        assert_eq!(a.live(), 1);
        a.drop_ref(r);
        assert_eq!(a.live(), 0);
        let r2 = a.alloc(leaf()).unwrap();
        assert_eq!(r2, r, "freed slot must recycle via the embedded free list");
    }

    #[test]
    fn copy_increments_rc() {
        let mut a = arena(4);
        let r = a.alloc(leaf()).unwrap();
        let r2 = a.copy(r).unwrap();
        assert_eq!(a.rc(r), 2);
        a.drop_ref(r);
        assert_eq!(a.rc(r2), 1);
    }

    #[test]
    fn cow_copies_when_rc_gt_1() {
        let mut a = arena(4);
        let r = a.alloc(leaf()).unwrap();
        let r2 = a.copy(r).unwrap();
        let out = a.mutate(r).unwrap();
        assert_ne!(out, r2, "must copy before mutation");
        assert_eq!(a.rc(out), 1);
        assert_eq!(a.rc(r2), 1);
    }

    #[test]
    fn drop_recurses_children_and_recycles_payload() {
        let mut a = arena(8);
        let f = a.alloc(leaf()).unwrap();
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
        a.pool = BufferPool::new(32, false);
        let f = a.alloc(leaf()).unwrap();
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
        a.alloc(leaf()).unwrap();
        assert!(a.alloc(leaf()).is_err());
    }

    #[test]
    fn growable_arena_allocates_new_pages() {
        let mut a = arena(2);
        a.pool.growable = true;
        a.alloc(leaf()).unwrap();
        a.alloc(leaf()).unwrap();
        let r = a.alloc(leaf()).unwrap();
        assert_eq!(a.get(r).unwrap(), &leaf());
        assert_eq!(a.live(), 3);
    }

    #[test]
    fn paged_store_uses_one_page_fast_path_for_small_capacity() {
        let mut a = arena(5); // page_cap = 8 (next pow2)
        assert_eq!(a.slots.num_chunks(), 1, "capacity 5 fits one page");
        let r = a.alloc(leaf()).unwrap();
        assert_eq!(r, 0);
        assert_eq!(a.capacity(), 5, "logical capacity unchanged");
    }

    #[test]
    fn mutate_on_freed_ref_errors() {
        let mut a = arena(4);
        let r = a.alloc(leaf()).unwrap();
        a.drop_ref(r);
        assert_eq!(a.mutate(r), Err(ArenaError::DanglingRef));
    }
}
