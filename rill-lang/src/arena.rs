//! Value arena — rill-lang's value heap, backed by the generic allocator in
//! [`rill_core::memory`].
//!
//! The arena machinery ([`Arena`], [`ArenaRef`], [`ArenaError`], [`BufferPool`])
//! now lives in `rill_core::memory`; this module keeps the language's value
//! model ([`Value`], [`ValueKind`]) and its [`ArenaValue`] implementation, and
//! re-exports the allocator types so existing `rill_lang::arena::*` paths keep
//! working.

pub use rill_core::memory::{Arena, ArenaError, ArenaRef, ArenaValue, BufferPool};

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
