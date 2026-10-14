//! # Size-classed payload buffer pool
//!
//! Collection and record payloads (`Vec<ArenaRef>`) are served from this pool
//! instead of the global heap, so per-tick collection ops allocate nothing in
//! the default (RT) mode. The pool is pre-allocated at program construction
//! from a build-time budget. In the default mode `take` returns `None` when the
//! pool is exhausted (the caller latches a `ProcessError`); with the
//! `growable-arena` feature it allocates a fresh buffer on the heap instead.

use super::arena::ArenaRef;

/// Size-classed payload buffer pool.
#[derive(Debug, Clone)]
pub struct BufferPool {
    /// Free buffers per size class (index = class id).
    classes: Vec<Vec<Vec<ArenaRef>>>,
    /// Whether `take` may allocate a fresh buffer on exhaustion.
    pub growable: bool,
}

impl BufferPool {
    /// The size-class bucket for a request of `n` refs.
    fn class_of(n: usize) -> usize {
        match n {
            0..=8 => 0,
            9..=16 => 1,
            17..=32 => 2,
            33..=64 => 3,
            65..=128 => 4,
            _ => 5,
        }
    }
    fn class_cap(class: usize) -> usize {
        [8, 16, 32, 64, 128, 256][class.min(5)]
    }

    /// Pre-allocate `budget` refs of pooled buffers.
    ///
    /// The binding constraint is buffer **count**, not total refs: every live
    /// payload borrows exactly one buffer, so a program whose peak live payloads
    /// are all small needs many small buffers. A cap-256 buffer consumes the refs
    /// of 32 cap-8 buffers yet serves at most one payload, so the remainder is
    /// split with a share per class in inverse proportion to its capacity — small
    /// payloads (records, sums, closures, short lists) dominate live slots, so
    /// small classes get many buffers and large classes a few. Every size class
    /// still gets at least one buffer (guaranteed smallest-first) so a request of
    /// any size has a candidate. `take` searches the request's class upward, so a
    /// small request prefers a small buffer and a large request falls through to
    /// a larger one.
    pub fn new(budget: usize, growable: bool) -> Self {
        let mut classes: Vec<Vec<Vec<ArenaRef>>> = vec![Vec::new(); 6];
        let mut remaining = budget;
        // One buffer per class, smallest first.
        for (class, bucket) in classes.iter_mut().enumerate() {
            let cap = Self::class_cap(class);
            if remaining >= cap {
                bucket.push(Vec::with_capacity(cap));
                remaining -= cap;
            }
        }
        // Fill each class up to its inverse-capacity share of the budget.
        for (class, bucket) in classes.iter_mut().enumerate() {
            let cap = Self::class_cap(class);
            let want = budget / (cap * 6);
            while bucket.len() < want && remaining >= cap {
                bucket.push(Vec::with_capacity(cap));
                remaining -= cap;
            }
        }
        // Any leftover refs go to the smallest class: a partial large buffer
        // would only serve one payload, whereas the leftovers make several more
        // small buffers.
        while remaining >= Self::class_cap(0) {
            classes[0].push(Vec::with_capacity(Self::class_cap(0)));
            remaining -= Self::class_cap(0);
        }
        Self { classes, growable }
    }

    /// Borrow a buffer with capacity ≥ n. `None` when the pool is exhausted and
    /// growth is disabled (RT mode: the caller latches a `ProcessError`).
    pub fn take(&mut self, n: usize) -> Option<Vec<ArenaRef>> {
        let class = Self::class_of(n);
        for c in class..6 {
            if let Some(mut b) = self.classes[c].pop() {
                b.clear();
                return Some(b);
            }
        }
        if self.growable {
            return Some(Vec::with_capacity(n.max(1)));
        }
        None
    }

    /// Return a buffer to the pool.
    pub fn put(&mut self, mut b: Vec<ArenaRef>) {
        b.clear();
        let class = Self::class_of(b.capacity());
        self.classes[class].push(b);
    }
}

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
