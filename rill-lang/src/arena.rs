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

/// Constructor tag of `False` in the Prelude's `data Bool = False | True`.
///
/// The tag is the constructor's DECLARATION position (Haskell 2010 §4.1.1:
/// `False` is the first alternative), so `False` is `0` and `True` is `1`.
/// Nothing in the value model treats a `Bool` specially: these constants exist
/// only so the few compiler primitives that produce a boolean (`ValueCompare`,
/// `member`, `atEnd`, `show_bool`) build the right `Value::Sum`.
pub const BOOL_FALSE_TAG: u32 = 0;
/// Constructor tag of `True` in the Prelude's `data Bool = False | True`.
pub const BOOL_TRUE_TAG: u32 = 1;

/// The kind of a [`Value`], used for runtime dispatch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueKind {
    /// Signed integer value.
    Int,
    /// Floating-point value.
    Double,
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
    /// First-class immutable array: bounds + flat elements.
    Array,
    /// The unit value.
    Void,
    /// A first-class signal buffer (see [`Value::Buffer`]).
    Buffer,
    /// Divergent computation (bottom); propagates through strict ops.
    Bottom,
    /// A lazy cons cell (head + lazy tail).
    Cons,
    /// The empty lazy list.
    Nil,
    /// An opaque primitive handle (`World`, `STState`, `STRef`). Source code
    /// can neither construct nor inspect one; only compiler primitives do.
    Opaque,
    /// A runtime class dictionary — the vtable of a type (see
    /// [`Value::Dictionary`]).
    Dictionary,
}

/// One method slot of a [`Value::Dictionary`].
///
/// A slot is **positional**: its index is the method's index in
/// [`TypeclassInfo::methods`](crate::types::ty::TypeclassInfo::methods), decided
/// statically by the compiler. There are no names here — names live in the
/// compiler, never in the runtime value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DictSlot {
    /// A compiled method fragment (a user instance equation, a class default
    /// body, or a selector). Indexes the existing `Ir::fragments` table,
    /// exactly like [`Value::Callable`]'s `fragment` field.
    Fragment {
        /// Index into [`Ir::fragments`](crate::ir::Ir::fragments).
        fragment: u32,
    },
    /// A primitive kernel: comparison, scalar arithmetic, buffer element-wise.
    /// A primitive method has no source body, so its slot names the operation
    /// instead of a fragment.
    Kernel(KernelId),
}

/// A primitive **operation** the interpreter can execute as a dictionary
/// method (see [`DictSlot::Kernel`]).
///
/// This is deliberately one variant per primitive operation, **not** one per
/// class method: `(+)` for `Int`, for `Double` and for a buffer all select
/// [`KernelId::Add`], and a new class that reuses existing operations does not
/// grow the enum. The compiler maps `(class, method)` to a kernel through
/// `TypeEnv`, so the runtime value stays a positional vtable.
///
/// The variant names mirror the operation, not the class: the whole
/// `Eq`/`Ord` comparison family (which the interpreter already runs through
/// `ValueCompare`/`CmpOp`) is here as the six predicates plus the three-way
/// [`KernelId::Compare`], and the numeric family covers both the scalar
/// instructions and the whole-buffer kernels — a kernel implementation
/// dispatches on its operand kinds, so `Add` serves `Num Double` and
/// `Num (FixedBuffer a)` alike.
///
/// **Implementation status.** `Eq`/`Ne`/`Lt`/`Gt`/`Le`/`Ge` are executable today
/// (they map to the existing `ValueCompare` instruction), and `Add`/`Sub`/`Mul`/
/// `Div` map to the existing `ValueAdd` family and `ValueBufferBin`. `Abs` exists
/// for buffers only (`ValueBufferUn`). `Compare`, `Negate`, `Signum`,
/// `FromInteger` and `Recip` have **no IR instruction yet** — design §3.9 puts
/// those instructions in scope for the dictionary model, and the task that
/// implements the kernels adds them; declaring the variants here does not
/// pretend they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KernelId {
    /// `(==)`: structural equality (`ValueCompare`/`CmpOp::Eq`).
    Eq,
    /// `(/=)`: structural inequality (`ValueCompare`/`CmpOp::Ne`).
    Ne,
    /// `(<)`: ordering predicate (`ValueCompare`/`CmpOp::Lt`).
    Lt,
    /// `(>)`: ordering predicate (`ValueCompare`/`CmpOp::Gt`).
    Gt,
    /// `(<=)`: ordering predicate (`ValueCompare`/`CmpOp::Le`).
    Le,
    /// `(>=)`: ordering predicate (`ValueCompare`/`CmpOp::Ge`).
    Ge,
    /// `compare`: the three-way order as an `Int` (`-1`/`0`/`1`) over the same
    /// total order the predicates use. **No IR instruction yet** (design §3.9
    /// `ValueCompare3`).
    Compare,
    /// `(+)`: addition, scalar or whole-buffer element-wise.
    Add,
    /// `(-)`: subtraction, scalar or whole-buffer element-wise.
    Sub,
    /// `(*)`: multiplication, scalar or whole-buffer element-wise.
    Mul,
    /// `(/)`: division, scalar or whole-buffer element-wise.
    Div,
    /// `negate`: unary minus. **No scalar IR instruction yet** (design §3.9);
    /// buffers already have `UnOp::Neg`.
    Negate,
    /// `abs`: absolute value. Buffers already have `UnOp::Abs`; the **scalar**
    /// form has no instruction yet (design §3.9).
    Abs,
    /// `signum`: sign as `-1`/`0`/`1` of the same type. **No IR instruction
    /// yet** (design §3.9).
    Signum,
    /// `fromInteger`: widen an `Int` literal/operand to the instance's type.
    /// **No IR instruction yet** (design §3.9) — literal elaboration needs it.
    FromInteger,
    /// `recip`: `1 / x`. **No IR instruction yet** (design §3.9).
    Recip,
}

/// A heap value stored in an [`Arena`] slot.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Signed integer payload.
    Int(i64),
    /// Floating-point payload.
    Double(f64),
    /// String payload.
    String(String),
    /// Record fields, one arena ref per field.
    Record(Vec<ArenaRef>),
    /// Constructor index plus one arena ref per constructor argument.
    ///
    /// Every Haskell 2010 data value is this shape, `Bool` included: a
    /// boolean is `Sum(0, [])` (`False`) or `Sum(1, [])` (`True`). Nullary
    /// constructors own no children, so the payload vector is empty.
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
    /// A first-class immutable array: stored bounds as an ordinary value (a
    /// `Pair i i` record where each index is an `Int`; a matrix's index type
    /// is itself `Pair Int Int`, so its bounds nest as
    /// `Pair (Pair Int Int) (Pair Int Int)`) plus flat elements in `lo..=hi`
    /// order, row-major for multi-dim arrays. Boxed and strict: the elements
    /// are arena refs (like [`Value::List`]).
    Array {
        /// Stored bounds as an ordinary value (`Pair i i`).
        bounds: ArenaRef,
        /// Flat elements, row-major for multi-dim, in `lo..=hi` order.
        elems: Vec<ArenaRef>,
    },
    /// The unit value.
    Void,
    /// A first-class signal buffer: an index into the program-level buffer
    /// table (a `Vec<RegionHandle<T>>` carved from the program's
    /// [`BufferAllocator`](rill_core::memory::BufferAllocator) at build). The
    /// `Value` enum is not `T`-generic, so the handle is represented as an
    /// opaque region id resolved against the program's allocator (SP-4 §7).
    /// Leaf value: owns no arena children; the `RegionHandle`'s RC is the
    /// buffer lifetime.
    Buffer(usize),
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
    /// An opaque primitive handle: a runtime token whose payload is meaningful
    /// only to the compiler-provided primitive that produced it. `World` is
    /// the real-world token (id `0`), `STState` a region token (the region id),
    /// and `STRef` a cell handle (a global cell-table index). Source programs
    /// can name opaque types but never construct, project, or match a value of
    /// one — so an `Opaque` payload never escapes into user-visible structure.
    Opaque(u32),
    /// A first-class EFFECT closure: a primitive effect (`newSTRef`, …) applied
    /// to its captured arguments. `env` is the arena record holding those
    /// arguments in application order (like [`Value::Callable`]'s env, the
    /// value owns its env ref). Applying it to its final argument — the STState
    /// token threading the region — runs the effect in the interpreter.
    BuiltinCallable {
        /// The primitive effect to run when applied.
        op: crate::ir::ValueEffectOp,
        /// Captured argument record (owns its field refs).
        env: ArenaRef,
    },
    /// A runtime class dictionary — the vtable of a type.
    ///
    /// Layout follows C++ vtables and GHC's dictionary constructors: the
    /// SUPERCLASS dictionaries come first (base-class pointers), then the
    /// class's own method slots in declaration order. There are no names here:
    /// a method is addressed by slot index, decided statically by the compiler.
    /// A dictionary is an aggregate value: it owns its `supers` refs (RC
    /// children) and its `methods` are code indices, not refs.
    Dictionary {
        /// Superclass dictionaries, in the order the class declares them.
        /// `Ord a`'s dictionary holds `[Eq a]`; `Num a` holds `[Eq a, Ord a]`.
        supers: Vec<ArenaRef>,
        /// Method slots in `TypeclassInfo::methods` order. A slot is either a
        /// compiled fragment (a user instance equation) or a primitive kernel.
        methods: Vec<DictSlot>,
    },
}

impl Value {
    /// The [`ValueKind`] of this value.
    pub fn kind(&self) -> ValueKind {
        match self {
            Value::Int(_) => ValueKind::Int,
            Value::Double(_) => ValueKind::Double,
            Value::String(_) => ValueKind::String,
            Value::Record(_) => ValueKind::Record,
            Value::Sum(_, _) => ValueKind::Sum,
            Value::Newtype(_) => ValueKind::Newtype,
            Value::Callable { .. } => ValueKind::Callable,
            Value::List { .. } => ValueKind::List,
            Value::Map { .. } => ValueKind::Map,
            Value::Set { .. } => ValueKind::Set,
            Value::Array { .. } => ValueKind::Array,
            Value::Void => ValueKind::Void,
            Value::Buffer(_) => ValueKind::Buffer,
            Value::ThunkCell(_) => ValueKind::Callable,
            Value::Bottom => ValueKind::Bottom,
            Value::Cons(_, _) => ValueKind::Cons,
            Value::Nil => ValueKind::Nil,
            Value::Opaque(_) => ValueKind::Opaque,
            Value::BuiltinCallable { .. } => ValueKind::Callable,
            Value::Dictionary { .. } => ValueKind::Dictionary,
        }
    }

    /// The `Bool` value for a host boolean: the nullary `Sum` constructor of
    /// the Prelude's `data Bool = False | True`.
    ///
    /// This is the ONE place the compiler's own primitives build a boolean, so
    /// no producer can disagree about the tag assignment. It is a value-level
    /// helper only — nothing in the type system, the arena, the RC walkers or
    /// the renderer knows that `Sum(1, [])` is `True`.
    pub fn bool(b: bool) -> Value {
        Value::Sum(if b { BOOL_TRUE_TAG } else { BOOL_FALSE_TAG }, Vec::new())
    }

    /// The host boolean this value denotes, when it is a `Bool` constructor.
    ///
    /// `None` for every other value — including a `Sum` of another type, which
    /// the caller distinguishes by the constructor tag it expects.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Sum(BOOL_FALSE_TAG, payload) if payload.is_empty() => Some(false),
            Value::Sum(BOOL_TRUE_TAG, payload) if payload.is_empty() => Some(true),
            _ => None,
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
            Value::BuiltinCallable { env, .. } => f(*env),
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
            Value::Array { bounds, elems } => {
                f(*bounds);
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
            // A dictionary owns its SUPERCLASS dictionaries; `methods` are code
            // indices (`DictSlot::Fragment` indexes `Ir::fragments`), not refs,
            // so they are never visited — a `drop_ref` must not treat a
            // fragment index as an arena ref.
            Value::Dictionary { supers, .. } => {
                for &s in supers {
                    f(s);
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
            // The pooled buffer serves the element refs, exactly like the List/Set
            // arm; the bounds ref is copied by value (a plain `ArenaRef`).
            Value::Array { elems, .. } => Some((4, elems.len())),
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
            (4, Value::Array { bounds, elems }) => {
                buf.extend_from_slice(elems);
                Ok(Value::Array {
                    bounds: *bounds,
                    elems: buf,
                })
            }
            _ => Ok(self.clone()),
        }
    }

    fn detach_payload(&mut self) -> Option<Vec<ArenaRef>> {
        match self {
            Value::Record(fields) | Value::Sum(_, fields) => Some(std::mem::take(fields)),
            Value::List { elems } => Some(std::mem::take(elems)),
            Value::Set { elems } => Some(std::mem::take(elems)),
            // The pooled payload is EXACTLY the element refs: `bounds` is
            // copied by value, never pooled — symmetric with the shape arm in
            // `clone_pooled` (the `(4, …)` slot), keeping take/put paired.
            Value::Array { elems, .. } => Some(std::mem::take(elems)),
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

    #[test]
    fn array_visits_bounds_then_elems() {
        let mut a = Arena::with_capacity(24);
        let lo = a.alloc(Value::Int(0)).unwrap();
        let hi = a.alloc(Value::Int(2)).unwrap();
        let bounds = a.alloc(Value::Record(vec![lo, hi])).unwrap();
        let e0 = a.alloc(Value::Int(10)).unwrap();
        let e1 = a.alloc(Value::Int(20)).unwrap();
        let e2 = a.alloc(Value::Int(30)).unwrap();
        let arr = a
            .alloc(Value::Array {
                bounds,
                elems: vec![e0, e1, e2],
            })
            .unwrap();

        let mut seen = Vec::new();
        a.get(arr).unwrap().visit_refs(|r| seen.push(r));
        assert_eq!(seen, vec![bounds, e0, e1, e2]);
    }

    #[test]
    fn array_clone_pooled_preserves_shape() {
        let mut a = Arena::with_capacity(24);
        a.pool = BufferPool::new(8, false); // budget one class-0 buffer
        let lo = a.alloc(Value::Int(0)).unwrap();
        let hi = a.alloc(Value::Int(2)).unwrap();
        let bounds = a.alloc(Value::Record(vec![lo, hi])).unwrap();
        let e0 = a.alloc(Value::Int(10)).unwrap();
        let e1 = a.alloc(Value::Int(20)).unwrap();
        let e2 = a.alloc(Value::Int(30)).unwrap();
        let arr = a
            .alloc(Value::Array {
                bounds,
                elems: vec![e0, e1, e2],
            })
            .unwrap();

        let mut cloned = a.clone_pooled(arr).unwrap();
        match &cloned {
            Value::Array { bounds: b, elems } => {
                assert_eq!(*b, bounds);
                assert_eq!(*elems, vec![e0, e1, e2]);
            }
            other => panic!("expected Array, got {other:?}"),
        }
        // Round-trip the pooled payload: detach must return EXACTLY the three
        // element refs (an `elems.len() + 1`/bounds-in-payload regression
        // yields four refs here and fails immediately). This is the invariant
        // that keeps the pool take/put pair symmetric with `clone_pooled`.
        assert_eq!(cloned.detach_payload(), Some(vec![e0, e1, e2]));
        // The detached payload is the pooled buffer: returning it (what
        // `Arena::drop_ref` does via `put_buf`) keeps the pool reusable.
        a.put_buf(vec![e0, e1, e2]);
        assert!(a.take_buf(3).is_ok(), "pool re-take after put must succeed");
    }

    #[test]
    fn array_detach_payload_reclaims_elems() {
        let mut a = Arena::with_capacity(24);
        let lo = a.alloc(Value::Int(0)).unwrap();
        let hi = a.alloc(Value::Int(2)).unwrap();
        let bounds = a.alloc(Value::Record(vec![lo, hi])).unwrap();
        let e0 = a.alloc(Value::Int(10)).unwrap();
        let e1 = a.alloc(Value::Int(20)).unwrap();
        let e2 = a.alloc(Value::Int(30)).unwrap();
        let arr = a
            .alloc(Value::Array {
                bounds,
                elems: vec![e0, e1, e2],
            })
            .unwrap();

        let payload = a.get_mut(arr).unwrap().detach_payload();
        assert_eq!(payload, Some(vec![e0, e1, e2]));
    }
}
