# SP-0: Page Arena and Open Collections — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rework rill-lang's value arena into a page-based allocator (Alexandrescu "Affordable Allocator" model) so `List`/`Map`/`Set` become open collections (no type-level `Cap`), with a pre-allocated payload buffer pool eliminating per-tick heap allocation in the default (RT) mode, and a `growable-arena` feature for non-RT growth.

**Architecture:** (1) Arena slot pool with an embedded free list; (2) full removal of `ValueTy::Cap`/`TypeExpr::TCap` from the type system; (3) a size-classed `BufferPool` for collection/record payloads borrowed by the interpreter and returned on drop/COW; (4) conservative build-time pool budgeting; (5) feature-gated growth.

**Tech Stack:** Rust, no new external dependencies. Workspace `rill/`, crate `rill-lang`. Branch `feature/rill-lang-categories`.

---

## File map (SP-0)

| File | Change |
|---|---|
| `rill-lang/src/arena.rs` | `Slot` enum + embedded free list; `BufferPool`; `Value::List/Map/Set` drop `cap` |
| `rill-lang/src/ir.rs` | `ValueListLit`/`ValueMapLit` drop `cap`; `ValueLayout` gains `buffer_budget`; `ValueBuiltinOp` arity notes |
| `rill-lang/src/types/ty.rs` | Remove `ValueTy::Cap`, `ctor_has_cap`; `ctor_kinds` arities; `match_ctor_pattern` cap-skip; `vty_of_name` |
| `rill-lang/src/types/unify.rs` | Remove `Cap` unification |
| `rill-lang/src/types/infer.rs` | `TCap`/`Cap` removal; collection op typing without caps; `has_cap` removal |
| `rill-lang/src/lower.rs` | `container_cap`/`list_type_args` removal; `value_builtin_ty`; `subtree_size`; pool budget |
| `rill-lang/src/backend/interp.rs` | Pooled buffer ops; cap-check removal; `ListEmpty`/`MapEmpty`/`SetEmpty` arity |
| `rill-lang/src/parser.rs` | `TCap` parsing removal |
| `rill-lang/src/render.rs` | `TCap` render removal |
| `rill-lang/src/ast.rs` | `TypeExpr::TCap` removal |
| `rill-lang/src/program.rs` | Arena + pool construction from `ValueLayout` |
| `rill-lang/src/error.rs` | (message text tweaks) |
| `rill-lang/Cargo.toml` | `growable-arena` feature |
| `rill-lang/tests/*` | capacity tests → open-collection + exhaustion tests |
| `rill-lang/README.md`, `docs/src/guides/rill-lang.md`, `CHANGELOG.md` | docs |

Branch: `feature/rill-lang-categories`. Verify with `cargo test -p rill-lang` after every task; `cargo clippy --all-features --workspace` and `cargo fmt` before finishing SP-0.

---

## Task 1: Arena slot rework — embedded free list

**Files:**
- Modify: `rill-lang/src/arena.rs:111-302`

- [ ] **Step 1: Write the failing tests (arena behaviors that must survive the rework)**

In `arena.rs` `mod tests`, add a test that pins free-list recycling and `live()`:

```rust
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
```

- [ ] **Step 2: Run it to verify it fails (compile error — `Slot` is still a struct)**

Run: `cargo test -p rill-lang arena::tests::embedded_free_list_recycles_and_counts`
Expected: FAIL (this test is added alongside the rework; it will compile once the rework lands — the rework below is the "minimal implementation").

- [ ] **Step 3: Rework `Slot` and the free list**

Replace the `Slot` struct and the `VecDeque` free list with an enum and a linked free list stored inside freed slots:

```rust
use std::collections::VecDeque; // REMOVE this import (no longer used)

/// A slot in the arena: either live (with an RC) or on the embedded free list.
#[derive(Debug, Clone)]
enum Slot {
    Occupied { rc: u32, val: Value },
    Free { next: Option<ArenaRef> },
}

#[derive(Debug)]
pub struct Arena {
    slots: Vec<Slot>,
    free: Option<ArenaRef>,
    live: usize,
    capacity: usize,
    /// Reserved for future debug/abort-safety support; not yet read.
    next_gen: u32,
}

impl Arena {
    /// Create an arena with `capacity` pre-allocated slots linked into the
    /// embedded free list. `with_capacity(0)` is allowed and means "no values".
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

    /// Allocate a slot holding `val`. `Err` when the arena is full.
    pub fn alloc(&mut self, val: Value) -> Result<ArenaRef, ArenaError> {
        let idx = self.free.ok_or(ArenaError::CapacityExceeded)?;
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
    /// slot is exclusively owned (`rc == 1`); call `mutate` first when shared.
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
        self.alloc(val)
    }
}
```

Note: `mutate` still `clone()`s the value payload (heap) at this stage — the buffer pool (Task 6) replaces that with a pooled copy.

- [ ] **Step 4: Run the arena tests to verify they pass**

Run: `cargo test -p rill-lang arena::`
Expected: PASS (all existing `arena::tests` plus the new recycling test).

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/arena.rs
git commit -m 'refactor(rill-lang): embedded free list in the value arena'
```

---

## Task 2: Open collections — runtime value changes

**Files:**
- Modify: `rill-lang/src/arena.rs:66-89` (`Value::List/Map/Set` drop `cap`)
- Modify: `rill-lang/src/backend/interp.rs:1060-1441` (cap checks and `cap` fields)

- [ ] **Step 1: Write the failing test (a list can exceed its former literal length)**

Add to `tests/collections.rs` (or create it if absent):

```rust
#[test]
fn list_grows_past_literal_capacity() {
    // Open collections: a `cons` past the literal's former capacity is no
    // longer a runtime error — the list grows.
    let src = r#"
        xs = [1.0, 2.0];
        main = length (cons 3.0 (cons 4.0 xs));
    "#;
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    assert_eq!(prog.arena().get(v).unwrap(), &rill_lang::arena::Value::Int(4));
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p rill-lang --test collections list_grows_past_literal_capacity`
Expected: FAIL — either a compile error (`cons 4.0 xs` where `xs : List Float 2` now that caps are gone) or a runtime `ProcessError` ("list capacity exceeded").

- [ ] **Step 3: Drop `cap` from the value variants**

In `arena.rs`, remove the `cap` field from `Value::List`/`Map`/`Set`:

```rust
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
```

- [ ] **Step 4: Remove the cap checks and `cap` fields in the interpreter**

In `backend/interp.rs`, in `exec_value_call_builtin`:
- `Cons`: delete `if elems.len() >= cap { … }`; construct `Value::List { elems }` (drop `cap`).
- `Tail`: construct `Value::List { elems }` in both branches.
- `Map`: construct `Value::List { elems: out }`.
- `Filter`: construct `Value::List { elems: kept }`.
- `ListEmpty`: construct `Value::List { elems: Vec::new() }` (ignore the capacity arg for now).
- `InsertMap`: delete the `pairs.len() >= cap` branch; construct `Value::Map { pairs: new_pairs }` in both the dup and insert branches.
- `InsertSet`: delete `elems.len() >= cap` branch; construct `Value::Set { elems: new_elems }`.
- `MapEmpty`: `Value::Map { pairs: Vec::new() }`.
- `SetEmpty`: `Value::Set { elems: Vec::new() }`.

Also fix any other `cap` construction sites in `interp.rs` (search `cap,` in `Value::List/Map/Set` literals) and the `Value::List { .. }`/`Value::Map { .. }`/`Value::Set { .. }` pattern matches (the `..` already tolerates removed fields).

- [ ] **Step 5: Fix remaining compile errors from removed `cap` fields across the crate**

Run `cargo check -p rill-lang`; fix every `Value::List { elems: .., cap }`/`Value::Map { pairs: .., cap }`/`Value::Set { elems: .., cap }` construction and the `value_cmp`/`drop_value_children` patterns (they use `..`). Sites: `backend/interp.rs`, `program.rs` tests, `arena.rs` tests.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p rill-lang`
Expected: The new growth test PASSes. Existing tests that assert capacity-exceeded errors now FAIL — collect the list (they are rewritten in Task 8). Do not proceed to commit until the *non-capacity* tests are green.

- [ ] **Step 7: Commit (only after non-capacity tests green; leave capacity tests broken, noted in the commit)**

```bash
git add rill-lang/src/arena.rs rill-lang/src/backend/interp.rs
git commit -m 'feat(rill-lang): open collections — drop cap from list/map/set values'
```

---

## Task 3: Type-system `Cap` removal

**Files:**
- Modify: `rill-lang/src/ast.rs:23` (`TypeExpr::TCap`), `parser.rs:425-470`, `render.rs:167`
- Modify: `rill-lang/src/types/ty.rs` (`ValueTy::Cap` at :69, `ctor_kinds` at :265, `ctor_has_cap` at :416, `match_ctor_pattern` at :490-539, `ctor_value_arity` at :422)
- Modify: `rill-lang/src/types/unify.rs:83,174`
- Modify: `rill-lang/src/types/infer.rs:153,559-584,1776-1810,2009,2810-2990`
- Modify: `rill-lang/src/lower.rs:55-75,1038-1266,2459-2564`
- Modify: `rill-lang/src/ir.rs` (`ValueListLit`/`ValueMapLit` `cap` fields at :510-528, `ValueLayout`)

- [ ] **Step 1: Remove `TypeExpr::TCap` and `ValueTy::Cap`**

- `ast.rs`: delete `TCap(usize)`; update the `mod type_expr_tests` at :483-493 to a non-Cap assertion (e.g. `List` over `[a]` only).
- `parser.rs`: delete the `TCap` arms in `parse_type_single` (:430) and `parse_type_atom` (:454); update the parser test at :1456.
- `render.rs`: delete the `TCap` arm at :167.
- `types/ty.rs`: delete `ValueTy::Cap`; delete `ctor_has_cap`; set `ctor_kinds` to `("List",1)`, `("Set",1)`, `("Map",2)` with the `(usize, bool)` tuple shape kept as `(usize, false)` for all; `ctor_value_arity` drops the `usize::from(*cap)` adjustment; `match_ctor_pattern` loses the `ValueTy::Cap` skip loop; `subtree_size`-related `Cap` references (in `lower.rs`, handled separately).
- `types/unify.rs`: remove `(ValueTy::Cap(x), ValueTy::Cap(y))` arm and the `Cap(_)` in `is_occurs`/other matches; update tests at :307-330, :398.

- [ ] **Step 2: Remove `has_cap`/`Cap` plumbing in inference**

- `infer.rs:559-562`: `conv` for a `TApp(ctor, _)` — drop the `has_cap` push of `ValueTy::Cap(0)`; the `ctor_kinds` lookup now reads only the arity.
- `infer.rs:153,584`: delete `TCap` conversions.
- `infer.rs:1776-1810`: list/map literal types become `App("List", [elem])` / `App("Map", [String, val])`.
- `infer.rs:2810-2990`: collection-op typing — `cons`/`map`/`filter`/`tail` return `App("List", [..])`; `insert` 3-arg → `App("Map", [k, v])`, 2-arg → `App("Set", [k])`; `list`/`empty_map`/`empty_set` return open types.
- `infer.rs:2009`: the collection-op name set — `list`/`empty_map`/`empty_set` stay reserved names.

- [ ] **Step 3: Remove capacity helpers and adjust `value_builtin_ty` in lowering**

- `lower.rs:55-75`: delete `list_type_args` and `container_cap`; update callers (`value_builtin_ty`, `static_scrutinee_ctor` helpers) to match `App("List", inner)` directly.
- `lower.rs:1038,1064`: list/map literal result types lose the `Cap` slot; `ValueListLit`/`ValueMapLit` drop `cap`.
- `lower.rs:1108-1266`: `value_builtin` table — `list`/`empty_map`/`empty_set` keep arity dispatch but the typing arms return open types (`App("List", [Float])` etc.).
- `lower.rs:2459-2564`: `subtree_size` — replace the `Cap`-based `List`/`Set`/`Map` arms with the estimate `1 + ELEM_EST × size(elem)` (ELEM_EST defined in Task 5); delete the `ValueTy::Cap(_)` arm.

- [ ] **Step 4: IR and program layout**

- `ir.rs`: `ValueListLit`/`ValueMapLit` drop `cap`; `ValueLayout` gains `pub buffer_budget: usize` (default 0). Update `mod value_ir_tests`.
- `program.rs`: no change yet (Task 6 wires the pool); `Arena::with_capacity(ir.value_state.capacity)` stays.

- [ ] **Step 5: Compile and fix**

Run: `cargo check -p rill-lang`
Expected: compile errors enumerate every remaining `Cap`/`TCap`/`has_cap`/`cap` reference — fix them all. Run `cargo test -p rill-lang` and update the typeclass/HKT tests that hardcode capacity types (`hkt_typeclass.rs` uses `fmap` over `[1.0,2.0,3.0,4.0]` — result type is now `List Float`).

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src rill-lang/tests
git commit -m 'feat(rill-lang): remove type-level capacity (Cap) from collections'
```

---

## Task 4: `list`/`empty_map`/`empty_set` lose the capacity argument

**Files:**
- Modify: `rill-lang/src/lower.rs:1105-1124,1211-1262`
- Modify: `rill-lang/src/backend/interp.rs:1178-1189,1419-1441`
- Modify: `rill-lang/src/types/infer.rs` (op arity)

- [ ] **Step 1: Update the op-arity dispatch**

- `lower.rs:value_builtin`: `list`/`empty_map`/`empty_set` now take 0 args; `("list", 0) => ListEmpty`, `("empty_map", 0) => MapEmpty`, `("empty_set", 0) => SetEmpty`.
- `lower.rs:value_builtin_ty`: `list`/`empty_map`/`empty_set` arms require `args.is_empty()`.
- `infer.rs`: the same arity tightening for these three names.
- `backend/interp.rs`: `ListEmpty`/`MapEmpty`/`SetEmpty` ignore `args` entirely (construct the empty value).

- [ ] **Step 2: Update all source usages**

Grep the crate for `list n`, `empty_map n`, `empty_set n`, `(list `, `(empty_map `, `(empty_set ` in tests and docs; change to `list`, `empty_map`, `empty_set` (no argument). This includes `tests/*.rs`, `README.md`, `docs/src/guides/rill-lang.md`, and any example strings.

- [ ] **Step 3: Compile and test**

Run: `cargo test -p rill-lang`
Expected: the remaining typeclass/HKT/collections tests that use capacity-typed collections now compile with open types.

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src rill-lang/tests
git commit -m 'feat(rill-lang): empty-container constructors take no capacity arg'
```

---

## Task 5: Pool-budget accounting

**Files:**
- Modify: `rill-lang/src/lower.rs:3760-3800` (the capacity heuristic)

- [ ] **Step 1: Define the budget estimator**

Add near `subtree_size`:

```rust
/// Worst-case element estimate for open collections. Exact counts are
/// data-dependent (a `concat_map` result length is unknown statically), so
/// this is a conservative per-op estimate; the runtime safety net is pool
/// exhaustion (default) or pool growth (`growable-arena`).
const ELEM_EST: usize = 16;
/// Multiplier on the estimated budget for the default RT mode so legitimate
/// programs do not trip the exhaustion error. Tune with the collection stress
/// tests (Task 8).
const POOL_SAFETY_MULTIPLIER: usize = 4;
```

In `subtree_size_impl`, the `List`/`Set`/`Map` arms become:

```rust
"List" | "Set" => 1 + ELEM_EST * self.subtree_size_impl(&args[0], visiting),
"Map" => 1 + ELEM_EST
    * (self.subtree_size_impl(&args[0], visiting)
        + self.subtree_size_impl(&args[1], visiting)),
```

- [ ] **Step 2: Compute slot + buffer budgets in `lower`**

Where `value_capacity` is computed (:3788), add the buffer budget. The `container_tys` tracking stays but now drives both budgets:

```rust
let slot_capacity = value_capacity * POOL_SAFETY_MULTIPLIER;
let buffer_budget = lw
    .container_tys
    .iter()
    .map(|t| lw.subtree_size(t) - 1) // element slots only
    .sum::<usize>()
    * POOL_SAFETY_MULTIPLIER;
```

Set `Ir.value_state = ValueLayout { capacity: slot_capacity, buffer_budget, value_state_slots: lw.value_state_slots }`.

- [ ] **Step 3: Test that budgets are nonzero**

Run: `cargo test -p rill-lang`
Expected: all existing tests pass; the collection tests now exercise the budget. (Exhaustion and growth are covered in Tasks 6-8.)

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/lower.rs rill-lang/src/ir.rs
git commit -m 'feat(rill-lang): conservative pool budget for open collections'
```

---

## Task 6: Payload buffer pool

**Files:**
- Modify: `rill-lang/src/arena.rs` (add `BufferPool`)
- Modify: `rill-lang/src/backend/interp.rs` (pooled ops, no value clones)
- Modify: `rill-lang/src/program.rs` (construct pool)
- Modify: `rill-lang/src/ir.rs` (`ValueLayout.buffer_budget` wired)

- [ ] **Step 1: Write the failing no-alloc test**

Add to `arena.rs` `mod tests`:

```rust
#[test]
fn pooled_buffers_recycle() {
    let mut a = Arena::with_capacity(8);
    a.buf_budget = 64;
    let b = a.take_buf(4).unwrap();
    assert!(b.capacity() >= 4);
    a.put_buf(b);
    let b2 = a.take_buf(4).unwrap();
    assert!(b2.capacity() >= 4);
}
```

- [ ] **Step 2: Implement `BufferPool` in `arena.rs`**

```rust
/// Size-class buckets for pooled payload buffers. `take` borrows a free buffer
/// with capacity >= n; `put` returns it. The pool is pre-allocated from
/// `ValueLayout.buffer_budget` at program construction; the default (RT) mode
/// never allocates on the processing path — `take` errors instead.
#[derive(Debug, Clone)]
pub struct BufferPool {
    /// Free buffers per size class (index = class id).
    classes: Vec<Vec<Vec<ArenaRef>>>,
    /// Whether `take` may allocate a fresh buffer on exhaustion.
    pub growable: bool,
    budget: usize,
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

    /// Pre-allocate `budget` refs of pooled buffers, split across classes.
    pub fn new(budget: usize, growable: bool) -> Self {
        let mut classes: Vec<Vec<Vec<ArenaRef>>> = vec![Vec::new(); 6];
        let mut remaining = budget;
        for class in (0..6).rev() {
            let cap = Self::class_cap(class);
            while remaining >= cap {
                classes[class].push(Vec::with_capacity(cap));
                remaining -= cap;
            }
        }
        Self { classes, growable, budget }
    }

    /// Borrow a buffer with capacity >= n. `None` when the pool is exhausted
    /// and growth is disabled (RT mode: the caller latches a `ProcessError`).
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
```

Add to `Arena`:

```rust
pub struct Arena {
    // …existing…
    /// Payload buffer pool for Record/List/Map/Set payloads.
    pub pool: BufferPool,
}

impl Arena {
    pub fn with_capacity(capacity: usize) -> Self {
        // pool: default budget 0 + growable false; program.rs overrides.
        Self { slots, free, live: 0, capacity, next_gen: 0, pool: BufferPool::new(0, false) }
    }
    /// Borrow a payload buffer (RT-safe in default mode).
    pub fn take_buf(&mut self, n: usize) -> Result<Vec<ArenaRef>, ArenaError> {
        self.pool.take(n).ok_or(ArenaError::CapacityExceeded)
    }
    /// Return a payload buffer to the pool.
    pub fn put_buf(&mut self, b: Vec<ArenaRef>) {
        self.pool.put(b);
    }
}
```

- [ ] **Step 3: Return buffers to the pool on drop/COW**

In `Arena::drop_ref`, when rc hits 0, after recursively dropping children, return the container's own payload buffers to the pool:

```rust
match &val {
    Value::Record(fields) | Value::Sum(_, fields) => {
        for f in fields { self.drop_ref(*f); }
        if let Value::Record(fields) = &val { self.put_buf(fields.clone()); } // handled below
    }
    // …
}
```

Because the extracted `val` is moved out, capture the buffers *before* moving:

```rust
let val = /* extracted */;
let mut return_bufs: Vec<Vec<ArenaRef>> = Vec::new();
match &val {
    Value::Record(fields) => return_bufs.push(fields.clone()),
    Value::List { elems } => return_bufs.push(elems.clone()),
    Value::Set { elems } => return_bufs.push(elems.clone()),
    Value::Map { pairs } => {
        let mut flat = Vec::with_capacity(pairs.len() * 2);
        for (k, v) in pairs { flat.push(*k); flat.push(*v); }
        return_bufs.push(flat); // map payloads share the Vec<(ArenaRef,ArenaRef)> pool
    }
    _ => {}
}
// recurse into children…
for f in &val { self.drop_ref(*f); }
for b in return_bufs { self.put_buf(b); }
```

> **Map note:** `Value::Map { pairs: Vec<(ArenaRef, ArenaRef)> }` cannot be pooled by a `Vec<ArenaRef>` pool directly. For v1, flatten map pairs into a `Vec<ArenaRef>` pooled buffer at construction (`Value::Map { pairs }` keeps its own `Vec<(ArenaRef,ArenaRef)>`; the pool covers records/lists/sets, which are the hot paths). Document map payloads as still heap-backed in v1.

- [ ] **Step 4: Rewrite the collection ops to use pooled buffers and avoid value clones**

In `backend/interp.rs`, replace the `prog.arena.get(r).cloned()` value-clone pattern with a **slice-read** helper, and build results from `take_buf`:

```rust
/// Read the element refs of a List/Set value without cloning the backing Vec.
/// Returns `None` when the register/slot is not a container of the right kind.
fn read_container_refs<'a, T, const BUF: usize>(
    prog: &'a RillProgram<T, BUF>,
    reg: usize,
) -> Option<&'a [ArenaRef]> {
    let r = prog.value_regs[reg]?;
    match prog.arena.get(r) {
        Some(Value::List { elems }) => Some(elems),
        Some(Value::Set { elems }) => Some(elems),
        _ => None,
    }
}
```

- `Map` (`:1120`): `let elems = read_container_refs(prog, args[1])?;` → `let mut out = prog.arena.take_buf(elems.len()).ok()?;` → push results → `Value::List { elems: out }`. The `call_closure_single` still needs the arena mutable — restructure so the immutable slice read happens first, collect into a temp, then allocate.
- `Cons` (`:1060`): read `elems` slice; `let mut out = take_buf(len+1)?; out.push(xr); out.extend_from_slice(elems);` recount children; `Value::List { elems: out }`.
- `Filter` (`:1222`): like `Map` with a predicate.
- `Tail` (`:1190`): `out = take_buf(elems.len())?; out.extend_from_slice(&elems[1..]);`.
- `InsertSet`/`InsertMap` (`:1369`/`:1254`): `out = take_buf(elems.len()+1)?;` splice in sorted position.
- `read_field_refs` (`:339`): use `take_buf(regs.len())`.
- `ListEmpty`/`MapEmpty`/`SetEmpty`: `take_buf(0)` (or `Vec::new()` fallback when pool empty → latching error).

Where an op still needs a full owned copy of a container to inspect it (`Head`, `Length`, `Lookup`, `Member`), keep the immutable borrow (they only read) — but they currently use `.cloned()`; switch them to read directly via `prog.arena.get(r)` without cloning (no arena mutation needed in those arms).

- [ ] **Step 5: Wire the pool into `program.rs`**

In `RillProgram::new`/`build`, after building the arena:

```rust
let mut arena = Arena::with_capacity(ir.value_state.capacity);
arena.pool = BufferPool::new(ir.value_state.buffer_budget, cfg!(feature = "growable-arena"));
```

- [ ] **Step 6: Test**

Run: `cargo test -p rill-lang`
Expected: all tests pass. The pooled no-alloc test passes. Stress the pool with a collection-heavy program (`map` over a 256-element list, repeated `cons`) and confirm no panic / no silent `None` (the arena budget from Task 5 must cover it).

- [ ] **Step 7: Commit**

```bash
git add rill-lang/src/arena.rs rill-lang/src/backend/interp.rs rill-lang/src/program.rs
git commit -m 'perf(rill-lang): pooled payload buffers eliminate per-tick heap allocation'
```

---

## Task 7: `growable-arena` feature

**Files:**
- Modify: `rill-lang/Cargo.toml` (feature), `arena.rs`, `backend/interp.rs`

- [ ] **Step 1: Add the feature**

In `rill-lang/Cargo.toml`:

```toml
[features]
default = []
growable-arena = []
```

- [ ] **Step 2: Gate growth on the feature**

`Arena::take_buf`/`alloc` (slot exhaustion) already consult `pool.growable` and return `Err` otherwise. Wire `RillProgram::new` to set `growable` from the feature:

```rust
arena.pool = BufferPool::new(ir.value_state.buffer_budget, cfg!(feature = "growable-arena"));
```

For *slot* exhaustion in growable mode, `Arena::alloc` should push a fresh slot:

```rust
pub fn alloc(&mut self, val: Value) -> Result<ArenaRef, ArenaError> {
    match self.free {
        Some(idx) => { /* existing path */ }
        None if self.pool.growable => {
            let idx = self.slots.len() as ArenaRef;
            self.slots.push(Slot::Occupied { rc: 1, val });
            self.capacity += 1;
            self.live += 1;
            Ok(idx)
        }
        None => Err(ArenaError::CapacityExceeded),
    }
}
```

- [ ] **Step 3: Test both modes**

Run: `cargo test -p rill-lang` (default mode) and `cargo test -p rill-lang --features growable-arena`
Expected: both green. Add one test that forces exhaustion in default mode and asserts `ProcessError::Processing("arena exhausted")`, and (behind `#[cfg(feature = "growable-arena")]`) the same program runs to completion.

- [ ] **Step 4: Commit**

```bash
git add rill-lang/Cargo.toml rill-lang/src/arena.rs rill-lang/src/backend/interp.rs
git commit -m 'feat(rill-lang): growable-arena feature for non-RT pool growth'
```

---

## Task 8: Tests and docs

**Files:**
- Modify: `rill-lang/tests/*` (collections, hkt_typeclass, typeclass, data_sums)
- Modify: `rill-lang/README.md`, `docs/src/guides/rill-lang.md`, `CHANGELOG.md`

- [ ] **Step 1: Rewrite capacity-exceeded tests**

Find every test asserting `"list capacity exceeded"`, `"map capacity exceeded"`, `"set capacity exceeded"` (and `data_sums.rs` capacity notes, `hkt_typeclass.rs` capacity-flow comments). Replace with:
- a growth assertion (the op succeeds and `length` reflects the grown size), and
- an **exhaustion** test that drives the pool past its budget and asserts `ProcessError::Processing` contains `"arena exhausted"`.

- [ ] **Step 2: Add a no-heap-allocation guard**

Add a `#[cfg(debug_assertions)]` allocation-count helper in the interpreter (e.g. a counter bumped by any `Vec::with_capacity`/`Vec::new`/`String::with_capacity` that survives on the processing path) and an integration test running a collection-heavy program that asserts the counter is 0. If the helper is too invasive, instead assert via a pool-exhaustion probe that `map`/`cons`/`filter` on a 256-element list never returns `None`/errors with the default budget.

- [ ] **Step 3: Update docs**

- `README.md`: replace the "strict bounds" language with "open collections bounded by a pre-allocated runtime pool"; update `list`/`empty_map`/`empty_set` examples (drop the capacity arg); document `growable-arena`.
- `docs/src/guides/rill-lang.md`: same updates; remove the capacity-exceeded error listing.
- `CHANGELOG.md`: add the SP-0 entry.

- [ ] **Step 4: Full verification**

Run: `cargo test -p rill-lang && cargo test --workspace && cargo clippy --all-features --workspace && cargo fmt`
Expected: zero failures, zero clippy warnings.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/tests rill-lang/README.md docs CHANGELOG.md
git commit -m 'test(rill-lang): open-collection + pool-exhaustion coverage; docs'
```

---

## Self-review

**Spec coverage:** All SP-0 requirements from the design doc map to tasks — embedded free list (T1), open `Value` (T2), `Cap` removal in types (T3), builtin arity change (T4), pool budget (T5), buffer pool + no-alloc (T6), feature gate (T7), tests/docs (T8).

**Placeholder scan:** `ELEM_EST`/`POOL_SAFETY_MULTIPLIER` values are named constants with a documented tuning step (Task 8); map-pair pooling is explicitly deferred in v1. No other TBDs.

**Type consistency:** `ValueLayout` gains `buffer_budget` once (T3) and is consumed once (T6); `Value::List { elems }` (no `cap`) is consistent from T2 onward; `BufferPool::new(budget, growable)` signature is stable across T6/T7.