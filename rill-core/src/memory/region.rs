//! # Region slabs — large static buffer memory
//!
//! A [`MemoryRegion`] is a chunked, grow-safe slab of `T` (f32/f64). Handles
//! ([`RegionHandle`]) are reference-counted views (offset + length) over the
//! slab, carved at build time by a [`BufferAllocator`]. Regions are static with
//! program lifetime; RC exists for sharing (one write head + many read heads),
//! not early free.
//!
//! # Safety
//!
//! Handles access the slab through `Rc<UnsafeCell<…>>` (the `SharedCell`
//! precedent in `rill_core::buffer::registry`). Every `unsafe` block carries a
//! `SAFETY` invariant; the module is Miri-checked (see the SP-4 design spec
//! §2.1).

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
    /// is a stable chunked allocation, so the address is valid for the region's
    /// lifetime, which is pinned by the `Rc` in this handle. The range is
    /// within bounds by construction (`offset + len <= slab.len()`).
    #[allow(unsafe_code)]
    pub fn with_data(&self, f: impl FnOnce(&[T])) {
        let region = unsafe { &*self.region.get() };
        let end = self.offset + self.len;
        f(&region.slab.slab_slice(self.offset, end));
    }

    /// Write access to the handle's slice.
    ///
    /// # SAFETY
    /// The single-writer / many-readers convention: at most one writer per
    /// handle range at a time, never concurrent with a reader on the same
    /// range. Bounds are valid by construction (as in [`Self::with_data`]).
    #[allow(unsafe_code)]
    pub fn with_data_mut(&self, f: impl FnOnce(&mut [T])) {
        let region = unsafe { &mut *self.region.get() };
        let end = self.offset + self.len;
        f(&mut region.slab.slab_slice_mut(self.offset, end));
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

impl<T> std::fmt::Debug for RegionHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegionHandle")
            .field("offset", &self.offset)
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

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
        // SAFETY: single-threaded build; no outstanding `&mut` borrows into
        // the region.
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
        // SAFETY: `&self` read only; no mutation.
        #[allow(unsafe_code)]
        let region = unsafe { &*self.region.get() };
        region.slab.len()
    }

    /// Elements allocated so far.
    pub fn used(&self) -> usize {
        // SAFETY: `&self` read only.
        #[allow(unsafe_code)]
        let region = unsafe { &*self.region.get() };
        region.next
    }

    /// Remaining elements before exhaustion.
    pub fn remaining(&self) -> usize {
        self.capacity().saturating_sub(self.used())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allocator(initial: usize, chunk_cap: usize) -> BufferAllocator<f32> {
        BufferAllocator::new(initial, chunk_cap, 0.0)
    }

    #[test]
    fn bump_allocation_and_slices() {
        let mut alloc = allocator(16, 16);
        let h = alloc.allocate(4).unwrap();
        assert_eq!(h.len(), 4);
        h.with_data_mut(|s| s.copy_from_slice(&[1.0, 2.0, 3.0, 4.0]));
        h.with_data(|s| assert_eq!(s, &[1.0, 2.0, 3.0, 4.0]));
        assert_eq!(alloc.used(), 4);
    }

    #[test]
    fn handles_share_one_slab_and_are_cloneable() {
        let mut alloc = allocator(32, 16);
        let w = alloc.allocate(8).unwrap();
        let r = w.clone();
        w.with_data_mut(|s| s.fill(7.0));
        r.with_data(|s| assert!(s.iter().all(|&x| x == 7.0)));
    }

    #[test]
    fn exhaustion_returns_none_when_not_growable() {
        let mut alloc = allocator(4, 4);
        assert!(alloc.allocate(4).is_some());
        assert!(alloc.allocate(1).is_none());
    }

    #[test]
    fn grow_then_allocate_succeeds() {
        let mut alloc = allocator(4, 4);
        assert!(alloc.allocate(4).is_some());
        alloc.grow(0.0);
        let h = alloc.allocate(4).unwrap();
        assert_eq!(h.len(), 4);
    }

    #[test]
    fn non_spanning_allocation_skips_chunks() {
        // chunk_cap 8; allocate 5 (uses chunk 0), then 5 -> must not span.
        let mut alloc = allocator(16, 8);
        let _a = alloc.allocate(5).unwrap();
        let b = alloc.allocate(5).unwrap();
        assert_eq!(b.offset(), 8, "b starts at chunk 1, not offset 5");
        assert_eq!(b.len(), 5);
    }

    #[test]
    fn capacity_used_remaining() {
        let mut alloc = allocator(16, 16);
        assert_eq!(alloc.capacity(), 16);
        assert_eq!(alloc.used(), 0);
        assert_eq!(alloc.remaining(), 16);
        let _ = alloc.allocate(6).unwrap();
        assert_eq!(alloc.used(), 6);
        assert_eq!(alloc.remaining(), 10);
    }
}
