//! Fixed-capacity arena with reference counting and copy-on-write.
//!
//! All slots are pre-allocated up front; allocation pops a free index from a
//! free-list and returns a slot. RC is a non-atomic `u32` — the arena is owned
//! by a single `RillProgram` on a single-threaded DAG. Copy-on-write: `mutate()`
//! copies the value to a fresh slot when `rc > 1`.

use std::collections::VecDeque;

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
    /// A first-class list: element refs, current length = `elems.len()`, cap.
    List {
        /// Element arena refs; the current length is `elems.len()`.
        elems: Vec<ArenaRef>,
        /// Allocated capacity for the list.
        cap: usize,
    },
    /// A first-class map: sorted (key, value) ref pairs.
    Map {
        /// Sorted (key, value) arena ref pairs.
        pairs: Vec<(ArenaRef, ArenaRef)>,
        /// Allocated capacity for the map.
        cap: usize,
    },
    /// A first-class set: sorted element refs.
    Set {
        /// Sorted element arena refs.
        elems: Vec<ArenaRef>,
        /// Allocated capacity for the set.
        cap: usize,
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

#[derive(Debug, Clone)]
struct Slot {
    rc: u32,
    val: Value,
}

/// A fixed-capacity arena. `with_capacity(0)` is allowed and means "no values".
///
/// The arena owns all values on behalf of a single-threaded [`RillProgram`] and
/// is never shared between graph nodes, so RC is a plain non-atomic `u32`.
#[derive(Debug)]
pub struct Arena {
    slots: Vec<Option<Slot>>,
    free: VecDeque<ArenaRef>,
    capacity: usize,
    /// Reserved for future debug/abort-safety support; not yet read.
    next_gen: u32,
}

impl Arena {
    /// Create an arena with `capacity` pre-allocated slots.
    pub fn with_capacity(capacity: usize) -> Self {
        let mut free = VecDeque::with_capacity(capacity);
        for i in 0..capacity {
            free.push_back(i as ArenaRef);
        }
        Self {
            slots: vec![None; capacity],
            free,
            capacity,
            next_gen: 0,
        }
    }

    /// Total capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of live (allocated) slots.
    pub fn live(&self) -> usize {
        self.capacity - self.free.len()
    }

    /// Allocate a slot holding `val`. `Err` when the arena is full.
    pub fn alloc(&mut self, val: Value) -> Result<ArenaRef, ArenaError> {
        let idx = self.free.pop_front().ok_or(ArenaError::CapacityExceeded)?;
        self.next_gen = self.next_gen.wrapping_add(1);
        self.slots[idx as usize] = Some(Slot { rc: 1, val });
        Ok(idx)
    }

    /// Reference count of a slot (0 if freed).
    pub fn rc(&self, r: ArenaRef) -> u32 {
        self.slots
            .get(r as usize)
            .and_then(|s| s.as_ref())
            .map_or(0, |s| s.rc)
    }

    /// Immutable view of a slot's value.
    pub fn get(&self, r: ArenaRef) -> Option<&Value> {
        self.slots
            .get(r as usize)
            .and_then(|s| s.as_ref())
            .map(|s| &s.val)
    }

    /// Mutable view of a slot's value (no RC change). Debug builds assert the
    /// slot is exclusively owned (`rc == 1`) so field refs cannot be rewritten
    /// without accounting; call `mutate` first when shared.
    pub fn get_mut(&mut self, r: ArenaRef) -> Option<&mut Value> {
        debug_assert_eq!(self.rc(r), 1);
        self.slots
            .get_mut(r as usize)
            .and_then(|s| s.as_mut())
            .map(|s| &mut s.val)
    }

    /// Share a value: `rc++` and return the same ref.
    pub fn copy(&mut self, r: ArenaRef) -> Result<ArenaRef, ArenaError> {
        let slot = self
            .slots
            .get_mut(r as usize)
            .and_then(|s| s.as_mut())
            .ok_or(ArenaError::DanglingRef)?;
        slot.rc = slot.rc.checked_add(1).ok_or(ArenaError::RcOverflow)?;
        Ok(r)
    }

    /// Drop one reference; frees the slot (recursively for field refs) at rc 0.
    pub fn drop_ref(&mut self, r: ArenaRef) {
        let cur = self.rc(r);
        if cur == 0 {
            return;
        }
        if cur > 1 {
            self.slots
                .get_mut(r as usize)
                .and_then(|s| s.as_mut())
                .unwrap()
                .rc = cur - 1;
            return;
        }
        let val = self
            .slots
            .get_mut(r as usize)
            .and_then(|s| s.take())
            .unwrap()
            .val;
        match val {
            Value::Record(fields) | Value::Sum(_, fields) => {
                for f in fields {
                    self.drop_ref(f);
                }
            }
            Value::Newtype(inner) => self.drop_ref(inner),
            Value::Closure(env, _) => self.drop_ref(env),
            Value::List { elems, .. } => {
                for e in elems {
                    self.drop_ref(e);
                }
            }
            Value::Map { pairs, .. } => {
                for (k, v) in pairs {
                    self.drop_ref(k);
                    self.drop_ref(v);
                }
            }
            Value::Set { elems, .. } => {
                for e in elems {
                    self.drop_ref(e);
                }
            }
            _ => {}
        }
        self.free.push_front(r);
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
        let val = self
            .slots
            .get(r as usize)
            .and_then(|s| s.as_ref())
            .map(|s| s.val.clone())
            .ok_or(ArenaError::DanglingRef)?;
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
        self.alloc(val)
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
                cap: 4,
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
                cap: 4,
            })
            .unwrap();
        let s = a
            .alloc(Value::Set {
                elems: vec![k],
                cap: 4,
            })
            .unwrap();
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
