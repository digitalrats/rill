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
    /// Every size class gets at least one buffer (so a request of any size has a
    /// candidate), then the remainder fills classes proportionally — more small
    /// buffers, since small payloads (records, sums, closures, short lists)
    /// dominate. `take` searches the request's class upward, so a small request
    /// prefers a small buffer and a large request falls through to a larger one.
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
        // Fill the remainder proportionally to inverse capacity (bounded passes).
        let mut passes = 0;
        while remaining >= 8 && passes < 64 {
            for (class, bucket) in classes.iter_mut().enumerate() {
                let cap = Self::class_cap(class);
                if remaining >= cap {
                    bucket.push(Vec::with_capacity(cap));
                    remaining -= cap;
                }
            }
            passes += 1;
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
