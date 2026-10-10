//! Value arena — rill-lang's value heap, backed by the generic allocator in
//! [`rill_core::memory`].
//!
//! The arena machinery ([`Arena`], [`ArenaRef`], [`ArenaError`], [`BufferPool`])
//! now lives in `rill_core::memory`; this module keeps the language's value
//! model ([`Value`], [`ValueKind`]) and its [`ArenaValue`] implementation, and
//! re-exports the allocator types so existing `rill_lang::arena::*` paths keep
//! working.

pub use rill_core::memory::{Arena, ArenaError, ArenaRef, ArenaValue, BufferPool};

/// Call-by-need state of a thunk cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThunkState {
    /// Not yet forced.
    Unforced,
    /// Being forced right now (black hole: re-entry ⇒ bottom).
    Forcing,
    /// Forced to WHNF; the memoized result ref.
    Forced(ArenaRef),
    /// Divergent; the value is bottom.
    Bottom,
}

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
    /// A callable value (unified closure/thunk): captured environment record,
    /// body fragment id, and an optional memo cell.
    Callable,
    /// First-class list of element arena refs.
    List,
    /// First-class map of sorted (key, value) arena ref pairs.
    Map,
    /// First-class set of sorted element arena refs.
    Set,
    /// The unit value.
    Void,
    /// Divergent computation (bottom); propagates through strict ops.
    Bottom,
    /// A lazy cons cell (head + lazy tail).
    Cons,
    /// The empty lazy list.
    Nil,
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
    /// A callable value: captured env + body fragment + optional memo cell.
    /// `cell: None` = eager closure (no memo); `cell: Some(c)` = lazy thunk
    /// (memoized, call-by-need). Unifies the former `Closure` and `Thunk`.
    Callable {
        /// Body fragment to run on call (or on forcing for a lazy cell).
        fragment: u32,
        /// Captured environment record.
        env: ArenaRef,
        /// Memo/black-hole cell (ref to `Value::ThunkCell`), if lazy.
        cell: Option<ArenaRef>,
    },
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
    /// The force-state cell behind a thunk (rc 1; mutated by forcing).
    ThunkCell(ThunkState),
    /// Divergent computation (bottom); propagates through strict ops.
    Bottom,
    /// A lazy cons cell: `head` and `tail` (a thunk, another cons, or Nil).
    /// The tail is deliberately NOT forced — this is what makes infinite
    /// sequences representable.
    Cons(ArenaRef, ArenaRef),
    /// The empty lazy list.
    Nil,
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
            Value::Callable { .. } => ValueKind::Callable,
            Value::List { .. } => ValueKind::List,
            Value::Map { .. } => ValueKind::Map,
            Value::Set { .. } => ValueKind::Set,
            Value::Void => ValueKind::Void,
            Value::ThunkCell(_) => ValueKind::Callable,
            Value::Bottom => ValueKind::Bottom,
            Value::Cons(_, _) => ValueKind::Cons,
            Value::Nil => ValueKind::Nil,
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
            Value::Callable { env, cell, .. } => {
                f(*env);
                if let Some(c) = cell {
                    f(*c);
                }
            }
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
            Value::ThunkCell(ThunkState::Forced(r)) => f(*r),
            Value::ThunkCell(_) | Value::Bottom | Value::Nil => {}
            Value::Cons(h, t) => {
                f(*h);
                f(*t);
            }
            _ => {}
        }
    }

    fn clone_pooled(&self, pool: &mut BufferPool) -> Result<Self, ArenaError> {
        // Pool-aware clone: read the shape, borrow a payload buffer, copy refs.
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
        if n == 0 {
            return Ok(self.clone());
        }
        let mut buf = pool.take(n).ok_or(ArenaError::CapacityExceeded)?;
        match (kind, self) {
            (0, Value::Record(fields)) => {
                buf.extend_from_slice(fields);
                Ok(Value::Record(buf))
            }
            (1, Value::Sum(c, fields)) => {
                buf.extend_from_slice(fields);
                Ok(Value::Sum(*c, buf))
            }
            (2, Value::List { elems }) => {
                buf.extend_from_slice(elems);
                Ok(Value::List { elems: buf })
            }
            (3, Value::Set { elems }) => {
                buf.extend_from_slice(elems);
                Ok(Value::Set { elems: buf })
            }
            _ => Ok(self.clone()),
        }
    }

    fn detach_payload(&mut self) -> Option<Vec<ArenaRef>> {
        match self {
            Value::Record(fields) | Value::Sum(_, fields) => Some(std::mem::take(fields)),
            Value::List { elems } => Some(std::mem::take(elems)),
            Value::Set { elems } => Some(std::mem::take(elems)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thunk_visits_env_and_cell() {
        let mut a = Arena::with_capacity(16);
        let env = a.alloc(Value::Record(vec![])).unwrap();
        let cell = a.alloc(Value::ThunkCell(ThunkState::Unforced)).unwrap();
        let t = a
            .alloc(Value::Callable {
                fragment: 0,
                env,
                cell: Some(cell),
            })
            .unwrap();
        a.copy(cell).unwrap(); // extra ref on the cell
        a.drop_ref(t);
        // env freed (only the thunk owned it), cell survives via the extra ref.
        assert_eq!(a.rc(env), 0);
        assert_eq!(a.rc(cell), 1);
    }

    #[test]
    fn thunk_cell_forced_keeps_result_alive() {
        let mut a = Arena::with_capacity(16);
        let res = a.alloc(Value::Int(7)).unwrap();
        let cell = a.alloc(Value::ThunkCell(ThunkState::Forced(res))).unwrap();
        a.copy(res).unwrap();
        a.drop_ref(cell);
        assert_eq!(a.rc(res), 1, "forced result survives via the copy");
    }
}
