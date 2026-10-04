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
        assert!(
            chunk_cap.is_power_of_two(),
            "chunk_cap must be a power of two"
        );
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
            let chunk: Vec<T> = (base..base + n).map(&mut make).collect();
            s.chunks.push(chunk.into_boxed_slice());
        }
        s.len = capacity;
        s
    }

    /// Append a chunk of `chunk_cap` elements produced by `make(base_index)`.
    /// Existing elements keep their addresses.
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

    /// Immutable access by logical index (panics when out of bounds).
    #[inline]
    pub fn at(&self, i: usize) -> &T {
        let (c, o) = self.split(i);
        &self.chunks[c][o]
    }

    /// Mutable access by logical index (panics when out of bounds).
    #[inline]
    pub fn at_mut(&mut self, i: usize) -> &mut T {
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

    /// Contiguous slice over the logical range `start..end`.
    ///
    /// # Panics
    /// Panics if the range spans a chunk boundary (the caller must allocate
    /// ranges that never cross a chunk).
    #[inline]
    pub fn slab_slice(&self, start: usize, end: usize) -> &[T] {
        debug_assert!(end <= self.len, "range end {end} exceeds len {}", self.len);
        let (c, o) = self.split(start);
        let (c2, _) = self.split(end - 1);
        assert_eq!(c, c2, "range spans a chunk boundary");
        &self.chunks[c][o..o + (end - start)]
    }

    /// Mutable contiguous slice over the logical range `start..end`.
    ///
    /// # Panics
    /// Panics if the range spans a chunk boundary.
    #[inline]
    pub fn slab_slice_mut(&mut self, start: usize, end: usize) -> &mut [T] {
        debug_assert!(end <= self.len, "range end {end} exceeds len {}", self.len);
        let (c, o) = self.split(start);
        let (c2, _) = self.split(end - 1);
        assert_eq!(c, c2, "range spans a chunk boundary");
        &mut self.chunks[c][o..o + (end - start)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> ChunkedVec<u64> {
        ChunkedVec::with_capacity(5, 4, |i| i as u64)
    }

    #[test]
    fn logical_indices_are_contiguous_across_chunks() {
        let s = store();
        assert_eq!(s.len(), 5);
        assert_eq!(s.at(0), &0u64);
        assert_eq!(s.at(4), &4u64);
        assert_eq!(s.num_chunks(), 2);
    }

    #[test]
    fn push_chunk_grows_without_moving_existing() {
        let mut s = ChunkedVec::<u64>::with_capacity(0, 4, |_| 0);
        s.push_chunk(|base| base as u64);
        s.push_chunk(|base| base as u64);
        assert_eq!(s.at(0), &0u64);
        assert_eq!(s.at(4), &4u64);
        assert_eq!(s.at(7), &7u64);
        assert_eq!(s.len(), 8);
        *s.at_mut(3) = 99;
        assert_eq!(s.at(3), &99u64);
        assert_eq!(s.at(4), &4u64, "existing data stable across growth");
    }

    #[test]
    fn slab_slices_are_contiguous_within_a_chunk() {
        let s = ChunkedVec::<u64>::with_capacity(8, 4, |i| i as u64);
        assert_eq!(s.slab_slice(1, 4), &[1, 2, 3]);
        assert_eq!(s.slab_slice(4, 7), &[4, 5, 6]);
    }

    #[test]
    #[should_panic(expected = "index 5 out of bounds")]
    fn index_out_of_bounds_panics() {
        let s = store();
        let _ = s.at(5);
    }

    #[test]
    #[should_panic(expected = "spans a chunk boundary")]
    fn spanning_slice_panics() {
        let s = store(); // len 5, chunk_cap 4: index 4 lives in chunk 1
        let _ = s.slab_slice(3, 5);
    }
}
