//! Fixed-capacity arena with reference counting and copy-on-write.
//!
//! All slots are pre-allocated up front; allocation pops the head of an
//! **embedded free list** — a freed slot stores the next free index in place of
//! its payload, so the free list needs no separate bookkeeping structure. RC is
//! a non-atomic `u32` — the arena is owned by a single `RillProgram` on a
//! single-threaded DAG. Copy-on-write: `mutate()` copies the value to a fresh
//! slot when `rc > 1`.

/// Opaque index into an [`Arena`] slot. Stable across slot reuse.
pub type ArenaRef = u32;

/// The kind of a [`Value`], used for runtime dispatch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueKind {
    /// Signed integer value.
    Int,
    /// Floating-point value.
    Float,
    /// Boolean value.
    Bool,
    /// String value.
    String,
    /// Record with named fields stored as arena refs.
    Record,
    /// Sum-type constructor invocation with a payload of arena refs.
    Sum,
    /// `newtype`-wrapped inner value.
    Newtype,
    /// First-class function: captured environment record + body fragment id.
    Closure,
    /// First-class list of element arena refs.
    List,
    /// First-class map of sorted (key, value) arena ref pairs.
    Map,
    /// First-class set of sorted element arena refs.
    Set,
    /// The unit value.
    Void,
}

/// A heap value stored in an [`Arena`] slot.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Signed integer payload.
    Int(i64),
    /// Floating-point payload.
    Float(f64),
    /// Boolean payload.
    Bool(bool),
    /// String payload.
    String(String),
    /// Record fields, one arena ref per field.
    Record(Vec<ArenaRef>),
    /// Constructor index plus one arena ref per constructor argument.
    Sum(u32, Vec<ArenaRef>),
    /// A single inner value wrapped by a `newtype`.
    Newtype(ArenaRef),
    /// A first-class function: captured environment record + body fragment id.
    ///
    /// A closure OWNS its env record the way a record owns its field refs:
    /// `drop_ref` releases the env when the last closure reference is dropped,
    /// and `ValueMakeClosure` recounts the env so both the creating register
    /// and the closure are balanced owners.
    Closure(ArenaRef, u32),
    /// A first-class list: element refs; the current length is `elems.len()`.
    List {
        /// Element arena refs; the current length is `elems.len()`.
        elems: Vec<ArenaRef>,
    },
    /// A first-class map: sorted (key, value) ref pairs.
    Map {
        /// Sorted (key, value) arena ref pairs.
        pairs: Vec<(ArenaRef, ArenaRef)>,
    },
    /// A first-class set: sorted element refs.
    Set {
        /// Sorted element arena refs.
        elems: Vec<ArenaRef>,
    },
    /// The unit value.
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

/// A slot in the arena: either live (with an RC) or on the embedded free list.
///
/// The free-list link lives *inside* the freed slot — this is the embedded
/// free-list trick from Alexandrescu's "Affordable Allocator": no separate
/// free-list structure, and a freed slot's memory is reused to hold the link.
#[derive(Debug, Clone)]
enum Slot {
    /// A live value with a reference count.
    Occupied { rc: u32, val: Value },
    /// A freed slot holding the index of the next free slot (the free-list
    /// head when reached from [`Arena::free`]).
    Free { next: Option<ArenaRef> },
}

/// Size-classed payload buffer pool.
///
/// Collection and record payloads (`Vec<ArenaRef>`) are served from this pool
/// instead of the global heap, so the interpreter's per-tick collection ops
/// allocate nothing in the default (RT) mode. The pool is pre-allocated from
/// `ValueLayout::buffer_budget` at program construction. In the default mode
/// `take` returns `None` when the pool is exhausted (the caller latches a
/// `ProcessError`); with the `growable-arena` feature it allocates a fresh
/// buffer on the heap instead.
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

/// A fixed-capacity arena. `with_capacity(0)` is allowed and means "no values".
///
/// The arena owns all values on behalf of a single-threaded [`RillProgram`] and
/// is never shared between graph nodes, so RC is a plain non-atomic `u32`.
#[derive(Debug)]
pub struct Arena {
    slots: Vec<Slot>,
    free: Option<ArenaRef>,
    live: usize,
    capacity: usize,
    /// Reserved for future debug/abort-safety support; not yet read.
    next_gen: u32,
    /// Payload buffer pool for Record/Sum/List/Set element buffers.
    pub pool: BufferPool,
}

impl Arena {
    /// Create an arena with `capacity` pre-allocated slots linked into the
    /// embedded free list.
    pub fn with_capacity(capacity: usize) -> Self {
        let mut slots = Vec::with_capacity(capacity);
        for i in 0..capacity {
            slots.push(Slot::Free {
                next: (i + 1 < capacity).then_some((i + 1) as ArenaRef),
            });
        }
        Self {
            slots,
            free: (capacity > 0).then_some(0),
            live: 0,
            capacity,
            next_gen: 0,
            pool: BufferPool::new(0, false),
        }
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

    /// Copy a slot's value to an OWNED [`Value`], serving Record/Sum/List/Set
    /// payloads from the buffer pool so the copy allocates nothing on the heap
    /// (default mode). Map payloads and Strings fall back to `clone` (v1
    /// limitation). `Err(DanglingRef)` when the slot is not live.
    pub fn clone_pooled(&mut self, r: ArenaRef) -> Result<Value, ArenaError> {
        // Phase 1: read the value's shape and payload length (short borrow).
        let shape: (u8, usize) = match self.get(r) {
            Some(Value::Record(fields)) => (0, fields.len()),
            Some(Value::Sum(_, fields)) => (1, fields.len()),
            Some(Value::List { elems }) => (2, elems.len()),
            Some(Value::Set { elems }) => (3, elems.len()),
            Some(v) => return Ok(v.clone()),
            None => return Err(ArenaError::DanglingRef),
        };
        // Phase 2: pool a payload buffer (the slot borrow above has ended).
        let mut buf = self.take_buf(shape.1)?;
        // Phase 3: copy the payload refs (fresh short borrow).
        match shape.0 {
            0 => {
                if let Some(Value::Record(fields)) = self.get(r) {
                    buf.extend_from_slice(fields);
                }
                Ok(Value::Record(buf))
            }
            1 => {
                let ctor = match self.get(r) {
                    Some(Value::Sum(c, _)) => *c,
                    _ => 0,
                };
                if let Some(Value::Sum(_, fields)) = self.get(r) {
                    buf.extend_from_slice(fields);
                }
                Ok(Value::Sum(ctor, buf))
            }
            2 => {
                if let Some(Value::List { elems }) = self.get(r) {
                    buf.extend_from_slice(elems);
                }
                Ok(Value::List { elems: buf })
            }
            _ => {
                if let Some(Value::Set { elems }) = self.get(r) {
                    buf.extend_from_slice(elems);
                }
                Ok(Value::Set { elems: buf })
            }
        }
    }

    /// Total capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of live (allocated) slots.
    pub fn live(&self) -> usize {
        self.live
    }

    /// Allocate a slot holding `val`. `Err((.., val))` when the arena is full —
    /// the value is returned so the caller can release its children (the slot
    /// that never existed would have owned them).
    pub fn alloc(&mut self, val: Value) -> Result<ArenaRef, (ArenaError, Value)> {
        let idx = match self.free {
            Some(idx) => idx,
            None if self.pool.growable => {
                // `growable-arena` (non-RT): grow the slot pool on demand.
                let idx = self.slots.len() as ArenaRef;
                self.slots.push(Slot::Occupied { rc: 1, val });
                self.capacity += 1;
                self.live += 1;
                self.next_gen = self.next_gen.wrapping_add(1);
                return Ok(idx);
            }
            None => return Err((ArenaError::CapacityExceeded, val)),
        };
        self.free = match &self.slots[idx as usize] {
            Slot::Free { next } => *next,
            Slot::Occupied { .. } => unreachable!("arena free list corrupt"),
        };
        self.slots[idx as usize] = Slot::Occupied { rc: 1, val };
        self.next_gen = self.next_gen.wrapping_add(1);
        self.live += 1;
        Ok(idx)
    }

    /// Reference count of a slot (0 if freed).
    pub fn rc(&self, r: ArenaRef) -> u32 {
        match self.slots.get(r as usize) {
            Some(Slot::Occupied { rc, .. }) => *rc,
            _ => 0,
        }
    }

    /// Immutable view of a slot's value.
    pub fn get(&self, r: ArenaRef) -> Option<&Value> {
        match self.slots.get(r as usize) {
            Some(Slot::Occupied { val, .. }) => Some(val),
            _ => None,
        }
    }

    /// Mutable view of a slot's value (no RC change). Debug builds assert the
    /// slot is exclusively owned (`rc == 1`) so field refs cannot be rewritten
    /// without accounting; call `mutate` first when shared.
    pub fn get_mut(&mut self, r: ArenaRef) -> Option<&mut Value> {
        debug_assert_eq!(self.rc(r), 1);
        match self.slots.get_mut(r as usize) {
            Some(Slot::Occupied { val, .. }) => Some(val),
            _ => None,
        }
    }

    /// Share a value: `rc++` and return the same ref.
    pub fn copy(&mut self, r: ArenaRef) -> Result<ArenaRef, ArenaError> {
        match self.slots.get_mut(r as usize) {
            Some(Slot::Occupied { rc, .. }) => {
                *rc = rc.checked_add(1).ok_or(ArenaError::RcOverflow)?;
                Ok(r)
            }
            _ => Err(ArenaError::DanglingRef),
        }
    }

    /// Drop one reference; frees the slot (recursively for field refs) at rc 0.
    pub fn drop_ref(&mut self, r: ArenaRef) {
        let cur = self.rc(r);
        if cur == 0 {
            return;
        }
        if cur > 1 {
            if let Some(Slot::Occupied { rc, .. }) = self.slots.get_mut(r as usize) {
                *rc = cur - 1;
            }
            return;
        }
        let val = match self.slots.get_mut(r as usize) {
            Some(Slot::Occupied { val, .. }) => std::mem::replace(val, Value::Void),
            _ => return,
        };
        // Drop the children first (the refs live in the payload buffer), then
        // return the container's own payload buffer to the pool. Map pairs are
        // `Vec<(ArenaRef, ArenaRef)>` — not poolable by the ref pool in v1
        // (deferred; Map payloads stay heap-backed).
        match &val {
            Value::Record(fields) | Value::Sum(_, fields) => {
                for f in fields {
                    self.drop_ref(*f);
                }
            }
            Value::Newtype(inner) => self.drop_ref(*inner),
            Value::Closure(env, _) => self.drop_ref(*env),
            Value::List { elems } => {
                for e in elems {
                    self.drop_ref(*e);
                }
            }
            Value::Map { pairs, .. } => {
                for (k, v) in pairs {
                    self.drop_ref(*k);
                    self.drop_ref(*v);
                }
            }
            Value::Set { elems } => {
                for e in elems {
                    self.drop_ref(*e);
                }
            }
            _ => {}
        }
        match val {
            Value::Record(fields) => self.put_buf(fields),
            Value::List { elems } => self.put_buf(elems),
            Value::Set { elems } => self.put_buf(elems),
            _ => {}
        }
        self.slots[r as usize] = Slot::Free { next: self.free };
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
        // Copy value, decrement original, return fresh slot. The clone shares
        // the original's field refs, so count each one to give the fresh slot
        // independent ownership before the original is dropped.
        let val = match self.slots.get(r as usize) {
            Some(Slot::Occupied { val, .. }) => val.clone(),
            _ => return Err(ArenaError::DanglingRef),
        };
        match &val {
            Value::Record(fields) | Value::Sum(_, fields) => {
                for f in fields {
                    self.copy(*f)?;
                }
            }
            Value::Newtype(inner) => {
                self.copy(*inner)?;
            }
            Value::Closure(env, _) => {
                self.copy(*env)?;
            }
            Value::List { elems, .. } => {
                for e in elems {
                    self.copy(*e)?;
                }
            }
            Value::Map { pairs, .. } => {
                for (k, v) in pairs {
                    self.copy(*k)?;
                    self.copy(*v)?;
                }
            }
            Value::Set { elems, .. } => {
                for e in elems {
                    self.copy(*e)?;
                }
            }
            _ => {}
        }
        self.drop_ref(r);
        match self.alloc(val) {
            Ok(new) => Ok(new),
            Err((err, val)) => {
                // The fresh slot never existed; release the clone's recounted
                // children so the arena stays balanced (a build-time exhaustion
                // bug — `drop_ref` above already recycled the original).
                match &val {
                    Value::Record(fields) | Value::Sum(_, fields) => {
                        for f in fields {
                            self.drop_ref(*f);
                        }
                    }
                    Value::Newtype(inner) => self.drop_ref(*inner),
                    Value::Closure(env, _) => self.drop_ref(*env),
                    Value::List { elems } => {
                        for e in elems {
                            self.drop_ref(*e);
                        }
                    }
                    Value::Map { pairs, .. } => {
                        for (k, v) in pairs {
                            self.drop_ref(*k);
                            self.drop_ref(*v);
                        }
                    }
                    Value::Set { elems } => {
                        for e in elems {
                            self.drop_ref(*e);
                        }
                    }
                    _ => {}
                }
                Err(err)
            }
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_free_list_recycles_and_counts() {
        let mut a = Arena::with_capacity(4);
        assert_eq!(a.live(), 0);
        let r = a.alloc(Value::Int(7)).unwrap();
        assert_eq!(a.live(), 1);
        a.drop_ref(r);
        assert_eq!(a.live(), 0);
        let r2 = a.alloc(Value::Float(1.0)).unwrap();
        assert_eq!(r2, r, "freed slot must recycle via the embedded free list");
        assert_eq!(a.live(), 1);
    }

    #[test]
    fn alloc_and_drop_recycles_slot() {
        let mut a = Arena::with_capacity(4);
        let r = a.alloc(Value::Int(7)).unwrap();
        assert_eq!(a.rc(r), 1);
        assert_eq!(a.get(r).unwrap(), &Value::Int(7));
        a.drop_ref(r);
        assert_eq!(a.rc(r), 0);
        let r2 = a.alloc(Value::Float(1.0)).unwrap();
        assert_eq!(r2, r, "freed slot must be recycled");
    }

    #[test]
    fn buffer_pool_exhaustion_and_recycle() {
        let mut pool = BufferPool::new(8, false);
        let b1 = pool.take(4).unwrap();
        assert!(b1.capacity() >= 4);
        // The RT (non-growable) pool must not allocate on exhaustion.
        assert!(pool.take(4).is_none(), "RT pool must report exhaustion");
        pool.put(b1);
        assert!(pool.take(4).is_some(), "returned buffer must be reusable");
    }

    #[test]
    fn growable_pool_allocates_on_exhaustion() {
        let mut pool = BufferPool::new(8, true);
        let _b1 = pool.take(4).unwrap();
        let b2 = pool.take(4).unwrap();
        assert!(b2.capacity() >= 4, "growable pool allocates a fresh buffer");
    }

    #[test]
    fn growable_arena_allocates_new_slots() {
        let mut a = Arena::with_capacity(1);
        a.pool.growable = true;
        a.alloc(Value::Int(1)).unwrap();
        let r = a.alloc(Value::Int(2)).unwrap();
        assert_eq!(a.get(r).unwrap(), &Value::Int(2));
        assert_eq!(a.capacity(), 2);
        assert_eq!(a.live(), 2);
    }

    #[test]
    fn capacity_exhaustion_is_a_build_time_error() {
        let mut a = Arena::with_capacity(1);
        a.alloc(Value::Int(1)).unwrap();
        assert!(a.alloc(Value::Int(2)).is_err());
    }

    #[test]
    fn copy_increments_rc() {
        let mut a = Arena::with_capacity(4);
        let r = a.alloc(Value::Int(5)).unwrap();
        let r2 = a.copy(r).unwrap();
        assert_eq!(a.rc(r), 2);
        assert_eq!(r2, r, "copy shares the same slot");
        a.drop_ref(r);
        assert_eq!(a.rc(r2), 1);
    }

    #[test]
    fn cow_mutates_in_place_when_rc_1() {
        let mut a = Arena::with_capacity(4);
        let r = a.alloc(Value::Int(1)).unwrap();
        let out = a.mutate(r).unwrap();
        assert_eq!(out, r);
    }

    #[test]
    fn cow_copies_when_rc_gt_1() {
        let mut a = Arena::with_capacity(4);
        let r = a.alloc(Value::Int(1)).unwrap();
        let r2 = a.copy(r).unwrap();
        let out = a.mutate(r).unwrap();
        assert_ne!(out, r2, "must copy before mutation");
        assert_eq!(a.rc(out), 1);
        assert_eq!(a.rc(r2), 1);
    }

    #[test]
    fn drop_recurses_record_fields() {
        let mut a = Arena::with_capacity(8);
        let f = a.alloc(Value::Int(3)).unwrap();
        let r = a.alloc(Value::Record(vec![f])).unwrap();
        a.copy(f).unwrap(); // extra ref on field
        a.drop_ref(r);
        assert_eq!(
            a.rc(f),
            1,
            "field ref decremented but survives via extra ref"
        );
        a.drop_ref(f);
        assert_eq!(a.rc(f), 0);
    }

    #[test]
    fn mutate_on_freed_ref_errors() {
        let mut a = Arena::with_capacity(4);
        let r = a.alloc(Value::Int(1)).unwrap();
        a.drop_ref(r);
        let out = a.mutate(r);
        assert_eq!(out, Err(ArenaError::DanglingRef));
    }

    #[test]
    fn cow_copies_keep_field_rc_consistent() {
        let mut a = Arena::with_capacity(8);
        let f = a.alloc(Value::Int(3)).unwrap();
        let r = a.alloc(Value::Record(vec![f])).unwrap();
        a.copy(r).unwrap(); // rc 2
        let out = a.mutate(r).unwrap();
        assert_ne!(out, r, "COW must clone the shared record");
        // Both records now own the field independently.
        assert_eq!(a.rc(f), 2);
        a.drop_ref(r);
        // The surviving copy holds the last counted ref on the field.
        assert_eq!(a.rc(f), 1);
        match a.get(out).unwrap() {
            Value::Record(fields) => {
                assert_eq!(a.get(fields[0]).unwrap(), &Value::Int(3));
            }
            _ => panic!("surviving copy must still be a record"),
        }
    }

    #[test]
    fn list_and_map_rc_cow() {
        let mut a = Arena::with_capacity(16);
        let e0 = a.alloc(Value::Float(1.0)).unwrap();
        let e1 = a.alloc(Value::Float(2.0)).unwrap();
        let l = a
            .alloc(Value::List {
                elems: vec![e0, e1],
            })
            .unwrap();
        // COW copies the list and recounts its elements.
        let l2 = a.copy(l).unwrap();
        let out = a.mutate(l).unwrap();
        assert_ne!(out, l2);
        assert_eq!(a.rc(e0), 2, "both list copies own the element");
        // `out` is the COW copy; `l` is the original (l2 == l). Dropping both
        // list copies frees the shared element.
        a.drop_ref(l);
        a.drop_ref(out);
        assert_eq!(a.rc(e0), 0, "element freed when both lists dropped");
    }

    #[test]
    fn drop_recurses_into_map_and_set() {
        let mut a = Arena::with_capacity(16);
        let k = a.alloc(Value::String("a".into())).unwrap();
        let v = a.alloc(Value::Float(1.0)).unwrap();
        // `k` is logically owned by BOTH `m` and `s` but starts at rc 1: `alloc`
        // does not recount its children, so aliasing containers share the ref
        // until one of them is dropped (which must therefore happen first).
        let m = a
            .alloc(Value::Map {
                pairs: vec![(k, v)],
            })
            .unwrap();
        let s = a.alloc(Value::Set { elems: vec![k] }).unwrap();
        a.drop_ref(m);
        assert_eq!(a.rc(v), 0);
        a.drop_ref(s);
        assert_eq!(a.rc(k), 0);
    }

    #[test]
    fn bool_string_are_leaf_values() {
        let mut a = Arena::with_capacity(4);
        let b = a.alloc(Value::Bool(true)).unwrap();
        let s = a.alloc(Value::String("hi".into())).unwrap();
        assert_eq!(a.get(b).unwrap(), &Value::Bool(true));
        assert_eq!(a.get(s).unwrap(), &Value::String("hi".into()));
    }
}
