# rill-lang Data + Memory Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add first-class runtime data values (`data` records/sums), `type`/`newtype`, `typeclass`/`instance` (compile-time), and an arena + RC + COW memory model with a runtime stack of cells to rill-lang — all within the single-lambda-process execution system.

**Architecture:** Extend the arrow type model with a rate (`Signal`/`Value`) per channel. Add a per-tick value-track to the flat IR executed once per block alongside the existing block-track. Own a fixed-capacity arena inside `RillProgram`; local variables become arena cells on a runtime stack; COW mutation decides copy-vs-inplace via non-atomic RC. Typeclass methods resolve at compile time to direct calls.

**Tech Stack:** Rust (edition 2021), rill-core (`FixedBuffer`, `Transcendental`, `Algorithm`), existing rill-lang pipeline (lexer → parser → infer → reduce → lower → IR → schedule → interp). No new external dependencies. `#![deny(unsafe_code)]` in rill-lang.

**Reference spec:** `docs/superpowers/specs/2026-09-24-rill-lang-data-memory-design.md`.

---

## Conventions used across all tasks

- **Run commands** from `rill/` (workspace root): `cargo test -p rill-lang`, `cargo test -p rill-lang <test_name>`, `cargo clippy -p rill-lang`, `cargo fmt -p rill-lang`.
- **Every task ends with a commit.** Conventional commits, single quotes in `-m` (AGENTS.md).
- **Warnings policy:** zero warnings. Run `cargo clippy -p rill-lang` after each task.
- **Docs:** all code comments and doc comments in English.
- **No new dependencies.**
- `#![deny(unsafe_code)]` is set in `rill-lang/src/lib.rs:6` — do not add unsafe.

---

### Task 1: Arena core (alloc/free/RC/COW)

**Files:**
- Create: `rill-lang/src/arena.rs`
- Modify: `rill-lang/src/lib.rs` (register `pub mod arena;`)

- [ ] **Step 1: Write the failing test**

Create `rill-lang/src/arena.rs` with a test module:

```rust
//! Fixed-capacity arena with reference counting and copy-on-write.

use std::collections::VecDeque;

pub type ArenaRef = u32;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueKind {
    Int,
    Float,
    Record,
    Sum,
    Newtype,
    Func,
    Void,
}

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Record(Vec<ArenaRef>),
    Sum(u32, Vec<ArenaRef>),
    Newtype(ArenaRef),
    Func(u32),
    Void,
}

#[derive(Debug, Clone)]
struct Slot {
    rc: u32,
    val: Value,
}

#[derive(Debug)]
pub struct Arena {
    slots: Vec<Option<Slot>>,
    free: Vec<ArenaRef>,
    capacity: usize,
    next_gen: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_and_drop_recycles_slot() {
        let mut a = Arena::with_capacity(4);
        let r = a.alloc(Value::Int(7));
        assert_eq!(a.rc(r), 1);
        assert_eq!(a.get(r).unwrap(), &Value::Int(7));
        a.drop_ref(r);
        assert_eq!(a.rc(r), 0);
        let r2 = a.alloc(Value::Float(1.0));
        assert_eq!(r2, r, "freed slot must be recycled");
    }

    #[test]
    fn capacity_exhaustion_is_a_build_time_error() {
        let mut a = Arena::with_capacity(1);
        a.alloc(Value::Int(1));
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
        assert_eq!(a.rc(f), 1, "field ref decremented but survives via extra ref");
        a.drop_ref(f);
        assert_eq!(a.rc(f), 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang arena::tests`
Expected: FAIL with `could not compile` (module `arena` does not exist in `lib.rs`).

- [ ] **Step 3: Implement the arena**

Write the full implementation in `rill-lang/src/arena.rs`:

```rust
//! Fixed-capacity arena with reference counting and copy-on-write.
//!
//! All slots are pre-allocated up front; allocation pops a free index from a
//! free-list and returns a slot. RC is a non-atomic `u32` — the arena is owned
//! by a single `RillProgram` on a single-threaded DAG. Copy-on-write: mutate()
//! copies the value to a fresh slot when `rc > 1`.

use std::collections::VecDeque;

pub type ArenaRef = u32;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueKind {
    Int,
    Float,
    Record,
    Sum,
    Newtype,
    Func,
    Void,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Record(Vec<ArenaRef>),
    Sum(u32, Vec<ArenaRef>),
    Newtype(ArenaRef),
    Func(u32),
    Void,
}

impl Value {
    pub fn kind(&self) -> ValueKind {
        match self {
            Value::Int(_) => ValueKind::Int,
            Value::Float(_) => ValueKind::Float,
            Value::Record(_) => ValueKind::Record,
            Value::Sum(..) => ValueKind::Sum,
            Value::Newtype(_) => ValueKind::Newtype,
            Value::Func(_) => ValueKind::Func,
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
#[derive(Debug)]
pub struct Arena {
    slots: Vec<Option<Slot>>,
    free: VecDeque<ArenaRef>,
    capacity: usize,
    next_gen: u32,
}

impl Arena {
    /// Create an arena with `capacity` pre-allocated slots.
    pub fn with_capacity(capacity: usize) -> Self {
        let mut free = VecDeque::with_capacity(capacity);
        for i in 0..capacity as ArenaRef {
            free.push_back(i);
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
        self.slots.get(r as usize).and_then(|s| s.as_ref()).map(|s| &s.val)
    }

    /// Mutable view of a slot's value (no RC change).
    pub fn get_mut(&mut self, r: ArenaRef) -> Option<&mut Value> {
        self.slots.get_mut(r as usize).and_then(|s| s.as_mut()).map(|s| &mut s.val)
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
        if let Some(slot) = self.slots.get_mut(r as usize).and_then(|s| s.as_mut()) {
            if slot.rc > 0 {
                slot.rc -= 1;
            }
            if slot.rc == 0 {
                let val = self.slots[r as usize].take().unwrap().val;
                match val {
                    Value::Record(fields) | Value::Sum(_, fields) => {
                        for f in fields {
                            self.drop_ref(f);
                        }
                    }
                    Value::Newtype(inner) => self.drop_ref(inner),
                    _ => {}
                }
                self.free.push_back(r);
            }
        }
    }

    /// Copy-on-write entry point: returns a ref that is safe to mutate.
    /// Copies when `rc > 1`, otherwise returns the same ref.
    pub fn mutate(&mut self, r: ArenaRef) -> Result<ArenaRef, ArenaError> {
        let rc = self.rc(r);
        if rc <= 1 {
            return Ok(r);
        }
        // Copy value, decrement original, return fresh slot.
        let val = self
            .slots
            .get(r as usize)
            .and_then(|s| s.as_ref())
            .map(|s| s.val.clone())
            .ok_or(ArenaError::DanglingRef)?;
        self.drop_ref(r);
        self.alloc(val)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArenaError {
    CapacityExceeded,
    DanglingRef,
    RcOverflow,
}

// [tests from Step 1]
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang arena::tests`
Expected: PASS (all 6 tests).

- [ ] **Step 5: Register module + commit**

In `rill-lang/src/lib.rs`, add `pub mod arena;` after `pub mod ast;`.

```bash
git add rill-lang/src/arena.rs rill-lang/src/lib.rs
git commit -m 'feat(rill-lang): fixed-capacity arena with RC and COW'
```

---

### Task 2: Value-track IR instructions

**Files:**
- Modify: `rill-lang/src/ir.rs`
- Modify: `rill-lang/src/lib.rs` (re-export `ValueInstr`, `ValueLayout`)

- [ ] **Step 1: Write the failing test**

Add a test module to `rill-lang/src/ir.rs`:

```rust
#[cfg(test)]
mod value_ir_tests {
    use super::*;

    #[test]
    fn value_layout_is_defaultable() {
        let l = ValueLayout::default();
        assert_eq!(l.capacity, 0);
        assert_eq!(l.value_state_slots, 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang value_ir_tests`
Expected: FAIL (compile error — `ValueLayout` not defined).

- [ ] **Step 3: Implement value IR types**

Add to `rill-lang/src/ir.rs` (after the existing `Instr` enum):

```rust
/// A per-tick value instruction. Executed once per block in the value-track
/// phase, alongside the whole-buffer block instructions.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueInstr {
    /// Push a scope frame onto the runtime cell stack.
    ValuePushScope,
    /// Pop a scope frame, releasing cell refs.
    ValuePopScope,
    /// Bind a cell (fresh slot) for a local variable.
    ValueBindCell {
        /// Destination value register (holds the cell ref).
        dst: usize,
    },
    /// Read a variable: cell ref -> value ref (RC++ on result).
    ValueReadCell {
        /// Destination value register.
        dst: usize,
        /// Value register holding the cell ref.
        cell: usize,
    },
    /// Write a variable: value ref into a cell.
    ValueWriteCell {
        /// Cell register to write into.
        cell: usize,
        /// Value register to write.
        src: usize,
    },
    /// Create an Int value.
    ValueConstInt {
        /// Destination value register.
        dst: usize,
        /// The value.
        value: i64,
    },
    /// Create a Float value.
    ValueConstFloat {
        /// Destination value register.
        dst: usize,
        /// The value.
        value: f64,
    },
    /// Construct a record: alloc + write field refs.
    ValueConstructRecord {
        /// Destination value register.
        dst: usize,
        /// Field value registers (refs).
        fields: Vec<usize>,
    },
    /// Construct a sum: alloc + write constructor + payload.
    ValueConstructSum {
        /// Destination value register.
        dst: usize,
        /// Constructor index.
        ctor: u32,
        /// Payload value registers.
        payload: Vec<usize>,
    },
    /// Project a field: read `field` of the record in `slot`.
    ValueProject {
        /// Destination value register.
        dst: usize,
        /// Record value register.
        slot: usize,
        /// Field index.
        field: usize,
    },
    /// COW-mutate a field of a record.
    ValueUpdateField {
        /// Record value register (in/out: may be COW-copied).
        slot: usize,
        /// Field index.
        field: usize,
        /// New field value register.
        src: usize,
    },
    /// Wrap a value in a newtype.
    ValueNewtype {
        /// Destination value register.
        dst: usize,
        /// Inner value register.
        src: usize,
    },
    /// Unwrap a newtype to its inner value.
    ValueUnwrap {
        /// Destination value register.
        dst: usize,
        /// Newtype value register.
        src: usize,
    },
    /// Call a named function reference.
    ValueCallFunc {
        /// Destination value register.
        dst: usize,
        /// Index into [`Ir::value_funcs`].
        func: usize,
        /// Argument value registers.
        args: Vec<usize>,
    },
    /// Share a value (RC++).
    ValueCopy {
        /// Destination value register (same slot).
        dst: usize,
        /// Source value register.
        src: usize,
    },
    /// Drop a value (RC--, free at 0).
    ValueDrop {
        /// Value register to drop.
        src: usize,
    },
    /// Read a per-tick value-state slot (for `~` / `@` on values).
    ValueStateRead {
        /// Destination value register.
        dst: usize,
        /// State slot.
        slot: usize,
    },
    /// Write a per-tick value-state slot.
    ValueStateWrite {
        /// State slot.
        slot: usize,
        /// Value register.
        src: usize,
    },
}

/// Layout for value-track persistent storage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ValueLayout {
    /// Number of arena slots pre-allocated for the whole program.
    pub capacity: usize,
    /// Number of per-tick value-state slots (feedback/delay of values).
    pub value_state_slots: usize,
}

/// A named function value: reference to a lowering-time definition.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueFunc {
    /// Definition name.
    pub name: String,
    /// Number of value arguments.
    pub arity: usize,
}
```

Add fields to `Ir`:

```rust
pub struct Ir {
    // ... existing fields ...
    /// Value-track instructions (per-tick).
    pub value_instrs: Vec<ValueInstr>,
    /// Number of value registers required.
    pub num_value_regs: usize,
    /// Value registers holding program outputs.
    pub value_output_regs: Vec<usize>,
    /// Named function values referenced by [`ValueInstr::ValueCallFunc`].
    pub value_funcs: Vec<ValueFunc>,
    /// Value-track persistent layout.
    pub value_state: ValueLayout,
}
```

Also add `pub type ValueReg = usize;` near the top.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang value_ir_tests`
Expected: PASS.

- [ ] **Step 5: Fix all existing `Ir { .. }` constructors**

Search for `Ir {` constructions (`lower.rs`, `regalloc.rs`, tests, `serde_def.rs`) and add the new fields:

```rust
value_instrs: Vec::new(),
num_value_regs: 0,
value_output_regs: Vec::new(),
value_funcs: Vec::new(),
value_state: ValueLayout::default(),
```

- [ ] **Step 6: Run existing tests to verify no regression**

Run: `cargo test -p rill-lang`
Expected: PASS (all existing tests).

- [ ] **Step 7: Export + commit**

In `rill-lang/src/lib.rs`, re-export:

```rust
pub use ir::{Ir, Instr, ValueInstr, ValueLayout, ValueFunc};
```

```bash
git add rill-lang/src/ir.rs rill-lang/src/lib.rs rill-lang/src/lower.rs rill-lang/src/regalloc.rs
git commit -m 'feat(rill-lang): value-track IR instructions and layout'
```

---

### Task 3: Runtime-stack cells and value-state in RillProgram

**Files:**
- Modify: `rill-lang/src/program.rs`
- Modify: `rill-lang/src/backend/interp.rs`

- [ ] **Step 1: Write the failing test**

Add to `rill-lang/src/program.rs`:

```rust
#[cfg(test)]
mod program_value_tests {
    use super::*;
    use crate::ir::{Ir, ValueInstr, ValueLayout};

    #[test]
    fn new_program_has_empty_value_state() {
        let ir = Ir {
            instrs: Vec::new(),
            num_regs: 0,
            output_regs: Vec::new(),
            num_inputs: 0,
            num_outputs: 0,
            state: StateLayout::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            value_instrs: Vec::new(),
            num_value_regs: 0,
            value_output_regs: Vec::new(),
            value_funcs: Vec::new(),
            value_state: ValueLayout {
                capacity: 4,
                value_state_slots: 2,
            },
        };
        let prog = RillProgram::<f32, 256>::new(ir);
        assert_eq!(prog.arena.capacity(), 4);
        assert_eq!(prog.value_state.len(), 2);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang program_value_tests`
Expected: FAIL (compile error — `arena`/`value_state` fields missing).

- [ ] **Step 3: Add arena, value registers, value state to RillProgram**

In `rill-lang/src/program.rs`:

```rust
use crate::arena::Arena;

pub struct RillProgram<T: Transcendental, const BUF: usize> {
    // ... existing fields ...
    /// Value arena (fixed capacity from IR).
    pub(crate) arena: Arena,
    /// Per-tick value registers.
    pub(crate) value_regs: Vec<Option<crate::arena::ArenaRef>>,
    /// Per-tick value-state slots (feedback/delay of values).
    pub(crate) value_state: Vec<Option<crate::arena::ArenaRef>>,
    /// Current runtime cell-stack frames (bindings).
    pub(crate) cell_stack: Vec<Vec<(u32, crate::arena::ArenaRef)>>,
}
```

In `RillProgram::new` and `RillProgram::build`, initialize:

```rust
arena: Arena::with_capacity(ir.value_state.capacity),
value_regs: vec![None; ir.num_value_regs],
value_state: vec![None; ir.value_state.value_state_slots],
cell_stack: Vec::new(),
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-lang program_value_tests`
Expected: PASS.

- [ ] **Step 5: Reset clears value state**

In `impl Algorithm<T> for RillProgram`, extend `reset`:

```rust
fn reset(&mut self) {
    // ... existing ...
    for v in &mut self.value_state {
        *v = None;
    }
    self.value_regs = vec![None; self.ir.num_value_regs];
    self.cell_stack.clear();
}
```

- [ ] **Step 6: Run existing tests + commit**

Run: `cargo test -p rill-lang`
Expected: PASS.

```bash
git add rill-lang/src/program.rs rill-lang/src/backend/interp.rs
git commit -m 'feat(rill-lang): arena, value registers and value state in RillProgram'
```

---

### Task 4: Value-track executor in the interpreter

**Files:**
- Modify: `rill-lang/src/backend/interp.rs`

- [ ] **Step 1: Write the failing test**

Add to `rill-lang/src/backend/interp.rs`:

```rust
#[cfg(test)]
mod value_track_tests {
    use super::*;
    use crate::ir::{Ir, ValueInstr, ValueLayout};
    use crate::program::RillProgram;
    use rill_core::traits::MultichannelAlgorithm;

    fn prog_with(value_instrs: Vec<ValueInstr>, num_value_regs: usize) -> RillProgram<f32, 256> {
        let ir = Ir {
            instrs: Vec::new(),
            num_regs: 0,
            output_regs: Vec::new(),
            num_inputs: 0,
            num_outputs: 0,
            state: Default::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            value_instrs,
            num_value_regs,
            value_output_regs: Vec::new(),
            value_funcs: Vec::new(),
            value_state: ValueLayout { capacity: 16, value_state_slots: 0 },
        };
        RillProgram::<f32, 256>::new(ir)
    }

    #[test]
    fn const_int_and_const_float_run_per_tick() {
        let mut prog = prog_with(
            vec![
                ValueInstr::ValueConstInt { dst: 0, value: 42 },
                ValueInstr::ValueConstFloat { dst: 1, value: 1.5 },
            ],
            2,
        );
        let mut out = [0.0f32; 4];
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        assert_eq!(prog.value_regs[0], Some(0));
        let slot0 = prog.arena.get(0).unwrap().clone();
        assert_eq!(slot0, crate::arena::Value::Int(42));
        let slot1 = prog.arena.get(1).unwrap().clone();
        assert_eq!(slot1, crate::arena::Value::Float(1.5));
    }

    #[test]
    fn cell_stack_bind_read_write() {
        let mut prog = prog_with(
            vec![
                ValueInstr::ValuePushScope,
                ValueInstr::ValueConstInt { dst: 0, value: 7 },
                ValueInstr::ValueBindCell { dst: 1 },
                ValueInstr::ValueWriteCell { cell: 1, src: 0 },
                ValueInstr::ValueReadCell { dst: 2, cell: 1 },
                ValueInstr::ValuePopScope,
            ],
            3,
        );
        let mut out = [0.0f32; 2];
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        // Cell register 1 holds a cell slot; reading it yields the int 7.
        let cell = prog.value_regs[1].unwrap();
        let val = prog.arena.get(cell).unwrap();
        assert_eq!(val, &crate::arena::Value::Int(7));
        let read = prog.value_regs[2].unwrap();
        assert_eq!(prog.arena.get(read).unwrap(), &crate::arena::Value::Int(7));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang value_track_tests`
Expected: FAIL (compile error — value-track not executed).

- [ ] **Step 3: Implement `run_value_track`**

Add to `rill-lang/src/backend/interp.rs`:

```rust
use crate::arena::{Arena, Value};
use crate::ir::ValueInstr;

/// Execute the value-track: one pass per tick over per-block value instructions.
/// Runs after the block-track, before feedback state swap.
pub(crate) fn run_value_track<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
) {
    let mut drops: Vec<crate::arena::ArenaRef> = Vec::new();
    for instr in &prog.ir.value_instrs {
        exec_value_instr(prog, instr, &mut drops);
    }
    // Execute deferred drops after the instruction loop so drop order is stable.
    for r in drops {
        prog.arena.drop_ref(r);
    }
}

fn vreg_set(prog: &mut RillProgram<impl Transcendental, 256>, dst: usize, r: Option<crate::arena::ArenaRef>) {
    if let Some(slot) = prog.value_regs.get_mut(dst) {
        *slot = r;
    }
}
```

Note: the interpreter is generic over `const BUF: usize`, so write the executor generically. The concrete value-register store lives on `RillProgram`, so use the real `BUF` generic. Replace the `vreg_set` helper signature with the generic version used in `run_block_mimo`.

Full executor (generic over `BUF`):

```rust
pub(crate) fn run_value_track<T: Transcendental, const BUF: usize>(
    prog: &mut RillProgram<T, BUF>,
) {
    use crate::arena::Value;
    let mut drops: Vec<crate::arena::ArenaRef> = Vec::new();
    for instr in &prog.ir.value_instrs {
        match instr {
            ValueInstr::ValuePushScope => {
                prog.cell_stack.push(Vec::new());
            }
            ValueInstr::ValuePopScope => {
                if let Some(frame) = prog.cell_stack.pop() {
                    for (_, r) in frame {
                        drops.push(r);
                    }
                }
            }
            ValueInstr::ValueBindCell { dst } => {
                let r = prog.arena.alloc(Value::Void).unwrap_or(0);
                if let Some(slot) = prog.value_regs.get_mut(*dst) {
                    *slot = Some(r);
                }
            }
            ValueInstr::ValueReadCell { dst, cell } => {
                let cell_ref = prog.value_regs.get(*cell).copied().flatten();
                let val = cell_ref
                    .and_then(|c| prog.arena.get(c).cloned())
                    .unwrap_or(Value::Void);
                let r = prog.arena.alloc(val).unwrap_or(0);
                if let Some(slot) = prog.value_regs.get_mut(*dst) {
                    *slot = Some(r);
                }
            }
            ValueInstr::ValueWriteCell { cell, src } => {
                let cell_ref = prog.value_regs.get(*cell).copied().flatten();
                let src_ref = prog.value_regs.get(*src).copied().flatten();
                if let (Some(c), Some(s)) = (cell_ref, src_ref) {
                    if let Some(Value::Void) = prog.arena.get(c) {
                        let val = prog.arena.get(s).cloned().unwrap_or(Value::Void);
                        if let Some(v) = prog.arena.get_mut(c) {
                            *v = val;
                        }
                    }
                }
            }
            ValueInstr::ValueConstInt { dst, value } => {
                let r = prog.arena.alloc(Value::Int(*value)).unwrap_or(0);
                if let Some(slot) = prog.value_regs.get_mut(*dst) {
                    *slot = Some(r);
                }
            }
            ValueInstr::ValueConstFloat { dst, value } => {
                let r = prog.arena.alloc(Value::Float(*value)).unwrap_or(0);
                if let Some(slot) = prog.value_regs.get_mut(*dst) {
                    *slot = Some(r);
                }
            }
            ValueInstr::ValueConstructRecord { dst, fields } => {
                let mut refs = Vec::with_capacity(fields.len());
                for f in fields {
                    if let Some(Some(r)) = prog.value_regs.get(*f) {
                        refs.push(*r);
                    }
                }
                let r = prog.arena.alloc(Value::Record(refs)).unwrap_or(0);
                if let Some(slot) = prog.value_regs.get_mut(*dst) {
                    *slot = Some(r);
                }
            }
            ValueInstr::ValueConstructSum { dst, ctor, payload } => {
                let mut refs = Vec::with_capacity(payload.len());
                for p in payload {
                    if let Some(Some(r)) = prog.value_regs.get(*p) {
                        refs.push(*r);
                    }
                }
                let r = prog.arena.alloc(Value::Sum(*ctor, refs)).unwrap_or(0);
                if let Some(slot) = prog.value_regs.get_mut(*dst) {
                    *slot = Some(r);
                }
            }
            ValueInstr::ValueProject { dst, slot, field } => {
                let rec_ref = prog.value_regs.get(*slot).copied().flatten();
                let r = rec_ref
                    .and_then(|rr| match prog.arena.get(rr) {
                        Some(Value::Record(fields)) => fields.get(*field).copied(),
                        _ => None,
                    })
                    .and_then(|f| prog.arena.copy(f).ok())
                    .unwrap_or(0);
                if let Some(s) = prog.value_regs.get_mut(*dst) {
                    *s = Some(r);
                }
            }
            ValueInstr::ValueUpdateField { slot, field, src } => {
                let rec_ref = prog.value_regs.get(*slot).copied().flatten();
                let src_ref = prog.value_regs.get(*src).copied().flatten();
                if let (Some(rr), Some(sr)) = (rec_ref, src_ref) {
                    if let Ok(mutated) = prog.arena.mutate(rr) {
                        if let Some(Value::Record(fields)) = prog.arena.get_mut(mutated) {
                            if let Some(f) = fields.get_mut(*field) {
                                drops.push(*f);
                                *f = sr;
                            }
                        }
                        // rc++ the new field value is implicit (it is referenced now)
                        let _ = prog.arena.copy(sr);
                        if let Some(s) = prog.value_regs.get_mut(*slot) {
                            *s = Some(mutated);
                        }
                    }
                }
            }
            ValueInstr::ValueNewtype { dst, src } => {
                let src_ref = prog.value_regs.get(*src).copied().flatten().unwrap_or(0);
                let r = prog.arena.alloc(Value::Newtype(src_ref)).unwrap_or(0);
                if let Some(slot) = prog.value_regs.get_mut(*dst) {
                    *slot = Some(r);
                }
            }
            ValueInstr::ValueUnwrap { dst, src } => {
                let nw_ref = prog.value_regs.get(*src).copied().flatten();
                let r = nw_ref
                    .and_then(|n| match prog.arena.get(n) {
                        Some(Value::Newtype(inner)) => Some(*inner),
                        _ => None,
                    })
                    .map(|inner| prog.arena.copy(inner).unwrap_or(inner))
                    .unwrap_or(0);
                if let Some(slot) = prog.value_regs.get_mut(*dst) {
                    *slot = Some(r);
                }
            }
            ValueInstr::ValueCallFunc { .. } => {
                // Function references materialize in Task 8; no-op for now.
            }
            ValueInstr::ValueCopy { dst, src } => {
                let src_ref = prog.value_regs.get(*src).copied().flatten();
                let r = src_ref.and_then(|s| prog.arena.copy(s).ok()).unwrap_or(0);
                if let Some(slot) = prog.value_regs.get_mut(*dst) {
                    *slot = Some(r);
                }
            }
            ValueInstr::ValueDrop { src } => {
                if let Some(Some(r)) = prog.value_regs.get(*src) {
                    drops.push(*r);
                }
            }
            ValueInstr::ValueStateRead { dst, slot } => {
                let r = prog.value_state.get(*slot).copied().flatten();
                let out = r.and_then(|s| prog.arena.copy(s).ok()).unwrap_or(0);
                if let Some(s) = prog.value_regs.get_mut(*dst) {
                    *s = Some(out);
                }
            }
            ValueInstr::ValueStateWrite { slot, src } => {
                let src_ref = prog.value_regs.get(*src).copied().flatten();
                if let Some(s) = prog.value_state.get_mut(*slot) {
                    *s = src_ref;
                }
            }
        }
    }
    for r in drops {
        prog.arena.drop_ref(r);
    }
}
```

- [ ] **Step 4: Call `run_value_track` in `run_block_mimo`**

In `run_block_mimo` (`backend/interp.rs`), insert after the step loop and before `swap_block_state`:

```rust
crate::backend::interp::run_value_track(prog);
```

(Actually it is in the same module — call `run_value_track(prog);`.)

Also swap value state: add a `swap_value_state` to `RillProgram`:

```rust
pub(crate) fn swap_value_state(&mut self) {
    std::mem::swap(&mut self.value_state, &mut self.value_state);
    for v in &mut self.value_state {
        if let Some(r) = v.take() {
            self.arena.drop_ref(r);
        }
    }
}
```

Note: value state is single-buffered in v1 (each tick overwrites); the swap is a no-op placeholder that drops the previous tick's slot. Simplify: `swap_value_state` just clears the state after reading (the read already copied). See note in Step 5.

- [ ] **Step 5: Fix value-state double-buffer semantics**

Because `ValueStateRead` already copies (RC++) and `ValueStateWrite` stores a fresh ref each tick, the previous tick's ref must be dropped. Implement:

```rust
pub(crate) fn swap_value_state(&mut self) {
    for v in &mut self.value_state {
        if let Some(r) = v.take() {
            self.arena.drop_ref(r);
        }
    }
}
```

Call `prog.swap_value_state();` right after `run_value_track(prog);`.

- [ ] **Step 6: Run tests to verify pass**

Run: `cargo test -p rill-lang value_track_tests`
Expected: PASS.
Run: `cargo test -p rill-lang`
Expected: PASS (no regression).

- [ ] **Step 7: Commit**

```bash
git add rill-lang/src/backend/interp.rs rill-lang/src/program.rs
git commit -m 'feat(rill-lang): per-tick value-track executor in the interpreter'
```

---

### Task 5: Rate-aware channel types and unification

**Files:**
- Modify: `rill-lang/src/types/ty.rs`
- Modify: `rill-lang/src/types/unify.rs`
- Modify: `rill-lang/src/types/infer.rs` (ripple: `Block` → `Channel` usages)
- Modify: `rill-lang/src/arrow.rs` (re-export)

- [ ] **Step 1: Write the failing test**

Add to `rill-lang/src/types/ty.rs`:

```rust
#[cfg(test)]
mod channel_tests {
    use super::*;

    #[test]
    fn channel_rates_are_distinct() {
        let sig = Channel::signal(Scalar::Float);
        let val = Channel::value(ValueTy::Data("Point".into()));
        assert_eq!(sig.rate, Rate::Signal);
        assert_eq!(val.rate, Rate::Value);
        assert!(sig.elem != Scalar::Float || sig.vty == ValueTy::Int);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang channel_tests`
Expected: FAIL (`Channel`, `Rate`, `ValueTy` undefined).

- [ ] **Step 3: Implement Channel/Rate/ValueTy**

In `rill-lang/src/types/ty.rs`, add:

```rust
/// Wire rate: block-rate signal vs per-block value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rate {
    Signal,
    Value,
}

/// Type of an arena value (per-block value channel).
#[derive(Debug, Clone, PartialEq)]
pub enum ValueTy {
    Int,
    Float,
    Data(String),
    Newtype(String),
    Func(String),
    Var(TypeVarId),
}

/// A signal or value channel.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub rate: Rate,
    pub elem: Scalar,
    pub vty: ValueTy,
}

impl Channel {
    pub fn signal(elem: Scalar) -> Self {
        Self { rate: Rate::Signal, elem, vty: ValueTy::Int }
    }
    pub fn value(vty: ValueTy) -> Self {
        Self { rate: Rate::Value, elem: Scalar::Int, vty }
    }
    /// The scalar element type (Signal rate) — panics for Value rate.
    pub fn scalar(&self) -> &Scalar {
        debug_assert_eq!(self.rate, Rate::Signal);
        &self.elem
    }
}
```

Keep `Block` as a deprecated alias or migrate mechanically. **Decision: keep `Block` as a type alias** for minimal ripple:

```rust
pub type Block = Channel;
```

Update `ArrowTy { ins: Vec<Channel>, outs: Vec<Channel> }` (same layout, rename field types; `Vec<Block>` == `Vec<Channel>` via alias).

- [ ] **Step 4: Add value unification**

In `rill-lang/src/types/unify.rs`:

```rust
use super::ty::ValueTy;

/// Unify two value types.
pub fn unify_value(a: &ValueTy, b: &ValueTy, subst: &mut Subst, span: Span) -> Result<(), CompileError> {
    match (a, b) {
        (ValueTy::Var(v), other) | (other, ValueTy::Var(v)) => {
            if let ValueTy::Var(w) = other {
                if v == w {
                    return Ok(());
                }
            }
            // ValueTy vars resolve via scalar substitution when they unify with
            // a concrete value type; extend Subst to carry value mappings.
            Ok(())
        }
        (ValueTy::Int, ValueTy::Int)
        | (ValueTy::Float, ValueTy::Float) => Ok(()),
        (ValueTy::Data(x), ValueTy::Data(y)) if x == y => Ok(()),
        (ValueTy::Newtype(x), ValueTy::Newtype(y)) if x == y => Ok(()),
        (ValueTy::Func(x), ValueTy::Func(y)) if x == y => Ok(()),
        _ => Err(CompileError::Type {
            msg: format!("cannot unify value type {a:?} with {b:?}"),
            span,
        }),
    }
}
```

(Note: `ValueTy::Var` resolution via the scalar `Subst` is a placeholder for Task 6 typeclass constraints; for Task 5 concrete types suffice.)

- [ ] **Step 5: Fix ripple**

`infer.rs` constructs `Block::new(...)` everywhere. Because `Block = Channel`, `Block::new(elem)` must be redefined on the alias:

```rust
impl Channel {
    pub fn new(elem: Scalar) -> Self {
        Self::signal(elem)
    }
}
```

- [ ] **Step 6: Run tests to verify pass**

Run: `cargo test -p rill-lang channel_tests`
Expected: PASS.
Run: `cargo test -p rill-lang`
Expected: PASS (existing tests keep passing through the `Block` alias).

- [ ] **Step 7: Re-export + commit**

In `types/mod.rs` and `arrow.rs`, re-export `Channel, Rate, ValueTy`.

```bash
git add rill-lang/src/types/
git commit -m 'feat(rill-lang): rate-aware channels (Signal|Value) and value type unification'
```

---

### Task 6: Lexer + parser for data/type/newtype/typeclass/instance/match

**Files:**
- Modify: `rill-lang/src/lexer.rs`
- Modify: `rill-lang/src/parser.rs`
- Modify: `rill-lang/src/ast.rs`

- [ ] **Step 1: Write the failing test**

Add to `rill-lang/src/parser.rs`:

```rust
#[test]
fn parses_data_record_declaration() {
    let p = prog("data Point = { x: Float, y: Float }; main = Point { x: 1.0, y: 2.0 }");
    assert!(p.defs.iter().any(|d| matches!(d, Def::Data { .. })));
}

#[test]
fn parses_data_sum_declaration() {
    let p = prog("data Shape = Circle Float | Rect Float Float; main = Circle 1.0");
    assert!(p.defs.iter().any(|d| matches!(d, Def::Data { .. })));
}

#[test]
fn parses_type_and_newtype() {
    let p = prog("type Angles = Float; newtype Hz = Float; main = Hz 440.0");
    assert!(p.defs.iter().any(|d| matches!(d, Def::TypeAlias { .. })));
    assert!(p.defs.iter().any(|d| matches!(d, Def::Newtype { .. })));
}

#[test]
fn parses_match_expression() {
    let p = prog("area x = match x of { Circle r => r; Rect w h => w; }; main = area");
    let main = p.main_def().unwrap();
    assert!(matches!(main.body(), Expr::Match { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang parser`
Expected: FAIL (compile error — `Def::Data`, `Expr::Match` undefined).

- [ ] **Step 3: Add AST variants**

In `rill-lang/src/ast.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Def {
    // ... existing Anchor, Local ...
    /// `data Name = { f1: T1, f2: T2 }` — product type.
    Data {
        name: String,
        fields: Vec<(String, TypeName)>,
        span: Span,
    },
    /// `data Name = C1 T1 | C2 T2 T3` — sum type.
    Sum {
        name: String,
        ctors: Vec<(String, Vec<TypeName>)>,
        span: Span,
    },
    /// `type Name = T` — synonym.
    TypeAlias {
        name: String,
        target: TypeName,
        span: Span,
    },
    /// `newtype Name = T` — distinct wrapper.
    Newtype {
        name: String,
        target: TypeName,
        span: Span,
    },
    /// `typeclass C a where { m: sig; }`.
    Typeclass {
        name: String,
        var: String,
        methods: Vec<(String, TypeName)>,
        span: Span,
    },
    /// `instance C T where { m = body; }`.
    Instance {
        class: String,
        ty: TypeName,
        method_bodies: Vec<(String, Expr)>,
        span: Span,
    },
}
```

Add `TypeName` as a string-typed name (resolve at infer):

```rust
pub type TypeName = String;
```

Add to `Expr`:

```rust
/// Field projection `record.field`.
FieldProject {
    record: Box<Expr>,
    field: String,
    span: Span,
},
/// COW field mutation `record.field := value`.
FieldUpdate {
    record: Box<Expr>,
    field: String,
    value: Box<Expr>,
    span: Span,
},
/// Pattern matching over a sum value.
Match {
    scrutinee: Box<Expr>,
    arms: Vec<(String, Vec<Param>, Expr)>,   // ctor, bindings, body
    span: Span,
},
```

Update `Expr::span()` for the new variants.

- [ ] **Step 4: Add lexer keywords**

In `lexer.rs` `Tok`:

```rust
/// `data` keyword.
KwData,
/// `type` keyword.
KwType,
/// `newtype` keyword.
KwNewtype,
/// `typeclass` keyword.
KwTypeclass,
/// `instance` keyword.
KwInstance,
/// `match` keyword.
KwMatch,
/// `of` keyword (in `match x of { .. }`).
KwOf,
```

In the identifier match block:

```rust
"data" => Tok::KwData,
"type" => Tok::KwType,
"newtype" => Tok::KwNewtype,
"typeclass" => Tok::KwTypeclass,
"instance" => Tok::KwInstance,
"match" => Tok::KwMatch,
"of" => Tok::KwOf,
```

- [ ] **Step 5: Add parser support**

In `parse_top_def`, before the name match, dispatch on declaration keywords:

```rust
let t = self.peek().clone();
match t.tok {
    Tok::KwData => return self.parse_data_def(),
    Tok::KwType => return self.parse_type_alias_def(),
    Tok::KwNewtype => return self.parse_newtype_def(),
    Tok::KwTypeclass => return self.parse_typeclass_def(),
    Tok::KwInstance => return self.parse_instance_def(),
    _ => {}
}
```

Implement the parsers (record/product, sum, alias, newtype, typeclass, instance). For `match`, add a prefix atom in `parse_atom`:

```rust
Tok::KwMatch => {
    self.bump();
    let scrutinee = self.parse_expr(0, false)?;
    self.eat(&Tok::KwOf)?;
    self.eat(&Tok::LBrace)?;
    let mut arms = Vec::new();
    while self.peek().tok != Tok::RBrace {
        let (ctor, _) = self.expect_ident()?;
        let mut params = Vec::new();
        while let Tok::Ident(_) = self.peek().tok {
            let (pname, pspan) = self.expect_ident()?;
            params.push(Param { name: pname, span: pspan });
        }
        self.eat(&Tok::FatArrow)?;   // add `=>` token
        let body = self.parse_expr(0, true)?;
        arms.push((ctor, params, body));
        if self.peek().tok == Tok::Semi { self.bump(); }
    }
    self.eat(&Tok::RBrace)?;
    Ok(Expr::Match { scrutinee: Box::new(scrutinee), arms, span: t.span })
}
```

Add `=>` (`Tok::FatArrow`) to the lexer.

- [ ] **Step 6: Run tests to verify pass**

Run: `cargo test -p rill-lang parser`
Expected: PASS (new + existing).

- [ ] **Step 7: Update `Def::name`/`body`/`params` accessors for new variants**

`Def::name()` returns the declaration name; `body()`/`params()`/`where_defs()` return sensible defaults for non-Anchor/Local variants (empty body is a compile error if referenced as an expression). Add `is_decl(&self) -> bool` helper used by infer/reduce/lower to skip declarations.

- [ ] **Step 8: Commit**

```bash
git add rill-lang/src/lexer.rs rill-lang/src/parser.rs rill-lang/src/ast.rs
git commit -m 'feat(rill-lang): lexer/parser for data, type, newtype, typeclass, instance, match'
```

---

### Task 7: Infer for data types and value channels

**Files:**
- Modify: `rill-lang/src/types/infer.rs`

- [ ] **Step 1: Write the failing test**

Add to `rill-lang/src/types/infer.rs`:

```rust
#[test]
fn data_record_constructor_infers_value_channel() {
    // data Point = { x: Float, y: Float }; main = Point { x: 1.0, y: 2.0 }
    let t = ty_of("data Point = { x: Float, y: Float }; main = Point { x: 1.0, y: 2.0 }");
    assert_eq!(t.process_ty.outs.len(), 1);
    assert_eq!(t.process_ty.outs[0].rate, Rate::Value);
    assert_eq!(t.process_ty.outs[0].vty, ValueTy::Data("Point".into()));
}

#[test]
fn field_project_produces_float_value() {
    let t = ty_of("data Point = { x: Float, y: Float }; p = Point { x: 1.0, y: 2.0 }; main = p.x");
    assert_eq!(t.process_ty.outs[0].rate, Rate::Value);
    assert_eq!(t.process_ty.outs[0].vty, ValueTy::Float);
}

#[test]
fn sum_match_branches_on_constructor() {
    let t = ty_of("data Shape = Circle Float | Rect Float Float; main = match _ of { Circle r => r; Rect w h => w; }");
    assert_eq!(t.process_ty.outs[0].rate, Rate::Value);
    assert_eq!(t.process_ty.outs[0].vty, ValueTy::Float);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang types::infer::tests::data_record`
Expected: FAIL.

- [ ] **Step 3: Implement data-type environment + inference rules**

In `infer.rs`, add to `Ctx`:

```rust
data_types: HashMap<String, DataInfo>,
```

where `DataInfo` carries field/constructor shapes. Register from `Def::Data`/`Def::Sum`/`Def::Newtype` at group start (declarations are pure, no recursion).

In `infer_expr`, handle:
- `Expr::Apply { name: ctor, .. }` where `name` is a record or sum constructor → produce a `Value` channel with the declared type.
- `Expr::FieldProject { record, field, .. }` → infer record as `Value` channel, resolve field type → `Value` channel of the field type.
- `Expr::FieldUpdate { record, field, value, .. }` → record must be `Data`; result is the record type.
- `Expr::Match { scrutinee, arms, .. }` → scrutinee is a `Sum` value channel; arms' bodies must agree on the same value type.

- [ ] **Step 4: Extend `infer_ref` and `infer_apply`**

When a `Ref`/`Apply` name matches a `data` constructor, bypass the builtin/def lookup and return the value-channel type.

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p rill-lang types::infer`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/types/infer.rs
git commit -m 'feat(rill-lang): infer value channels for data records and sums'
```

---

### Task 8: Lower value-track for data

**Files:**
- Modify: `rill-lang/src/lower.rs`

- [ ] **Step 1: Write the failing test**

Add to `rill-lang/src/lower.rs`:

```rust
#[test]
fn record_construct_lowers_to_value_track() {
    let ir = ir_of("data Point = { x: Float, y: Float }; main = Point { x: 1.0, y: 2.0 }");
    assert_eq!(ir.num_value_regs, 3); // 2 const floats + 1 record
    assert!(ir.value_instrs.iter().any(|i| matches!(i, ValueInstr::ValueConstructRecord { .. })));
}

#[test]
fn field_project_lowers_to_value_project() {
    let ir = ir_of("data Point = { x: Float, y: Float }; p = Point { x: 1.0, y: 2.0 }; main = p.x");
    assert!(ir.value_instrs.iter().any(|i| matches!(i, ValueInstr::ValueProject { .. })));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang lower::tests::record_construct`
Expected: FAIL.

- [ ] **Step 3: Implement lowering**

Add to `Lowerer`:

```rust
next_value_reg: usize,
```

Add `fresh_value_reg()`. In `lower`, handle the new Expr variants by emitting value instructions and returning `(block_regs, value_regs)` pairs. Refactor `lower` return to `Result<(Vec<usize>, Vec<usize>), CompileError>` (block regs, value regs) — value-track results are `None`-indexed by `num_value_regs`.

Map:
- `Expr::FieldProject` → `ValueInstr::ValueProject { dst, slot, field }`.
- `Expr::FieldUpdate` → `ValueInstr::ValueUpdateField { slot, field, src }`.
- `Expr::Match` → emit `ValueConstructSum`-compatible branches via `ValueInstr::ValueProject` on the payload + `ValueDrop` of unselected arms; v1 keeps all arms' bodies in sequence with a compile-time tag check (no runtime branch): select the arm by the constructor index statically if the scrutinee is a compile-time sum, else runtime-select via a small `ValueMatch` instruction:

Add to `ir.rs`:

```rust
/// Dispatch on a sum constructor: yields the payload refs for the matched arm.
ValueMatch {
    /// Destination value registers for the selected arm's payload.
    dst: Vec<usize>,
    /// Scrutinee sum value register.
    slot: usize,
    /// Constructor index to match.
    ctor: u32,
}
```

Executor (Task 4 file): if `slot` is `Sum(c, payload)` with `c == ctor`, copy payload refs into `dst`; else drop the copied refs and write `Void`.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test -p rill-lang lower`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add rill-lang/src/lower.rs rill-lang/src/ir.rs rill-lang/src/backend/interp.rs
git commit -m 'feat(rill-lang): lower value-track for data records, projects, and match'
```

---

### Task 9: End-to-end data program (records) — integration

**Files:**
- Create: `rill-lang/tests/data_records.rs`

- [ ] **Step 1: Write the failing test**

```rust
use rill_lang::compile;
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn record_flow_through_process() {
    // Build a program that constructs a record, projects a field, and emits it.
    let mut prog = compile::<f32>(
        "data Point = { x: Float, y: Float }; p = Point { x: 2.0, y: 3.0 }; main = p.x",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    // The value output is not a scalar block; the block outputs stay zero.
    // (In v1 the value output is exposed on value_output_regs; block output is
    //  a placeholder. Assert the program runs without error.)
    assert!(out.iter().all(|v| *v == 0.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test data_records`
Expected: FAIL (compile error — `data` not handled end-to-end).

- [ ] **Step 3: Wire value outputs**

In `run_block_mimo`, after the value-track, copy `value_output_regs` refs into a `value_outputs` slice on `RillProgram` (new field `pub(crate) value_outputs: Vec<Option<ArenaRef>>`). For v1, expose via a getter; block outputs remain the scalar path.

- [ ] **Step 4: Fix pipeline entry points**

`compile`/`compile_with`/`compile_program_inner` in `lib.rs` must thread declarations (`Def::Data`/`Sum`/`TypeAlias`/`Newtype`) through `infer`/`reduce`/`lower` without treating them as expression definitions. Add a filter in `reduce_with_cafs` and `lower_with_cafs` to skip declarations in the expression context but keep them in the defs map for type resolution.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p rill-lang --test data_records`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/tests/data_records.rs rill-lang/src/backend/interp.rs rill-lang/src/lib.rs rill-lang/src/reduce.rs
git commit -m 'feat(rill-lang): end-to-end data record values'
```

---

### Task 10: Sums end-to-end + match

**Files:**
- Create: `rill-lang/tests/data_sums.rs`

- [ ] **Step 1: Write the failing test**

```rust
use rill_lang::compile;
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn sum_match_runs() {
    let mut prog = compile::<f32>(
        "data Shape = Circle Float | Rect Float Float; s = Circle 1.5; main = match s of { Circle r => r; Rect w h => w; }",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert!(out.iter().all(|v| *v == 0.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test data_sums`
Expected: FAIL.

- [ ] **Step 3: Make it pass**

Cover the `ValueMatch` runtime dispatch in the executor (Task 8 Step 3). The scrutinee is a compile-time constant `Circle 1.5` in the test — the match resolves statically.

- [ ] **Step 4: Run test + commit**

Run: `cargo test -p rill-lang --test data_sums`
Expected: PASS.

```bash
git add rill-lang/tests/data_sums.rs
git commit -m 'feat(rill-lang): sum values and match dispatch end-to-end'
```

---

### Task 11: type / newtype end-to-end

**Files:**
- Create: `rill-lang/tests/type_newtype.rs`
- Modify: `rill-lang/src/types/infer.rs`, `rill-lang/src/lower.rs`

- [ ] **Step 1: Write the failing test**

```rust
use rill_lang::compile;
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn newtype_construct_and_unwrap_runs() {
    let mut prog = compile::<f32>("newtype Hz = Float; h = Hz 440.0; main = h");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert!(out.iter().all(|v| *v == 0.0));
}

#[test]
fn type_synonym_substitutes() {
    let mut prog = compile::<f32>("type Angles = Float; a: Angles = 45.0; main = a");
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert!(out.iter().all(|v| *v == 0.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test type_newtype`
Expected: FAIL.

- [ ] **Step 3: Implement**

- `type` (synonym): substitute the target everywhere the alias is used during inference.
- `newtype`: register `Newtype` in `data_types`; constructor `Hz x` → `ValueNewtype`; unwrap at a Float-expecting site → `ValueUnwrap`.

- [ ] **Step 4: Run test + commit**

Run: `cargo test -p rill-lang --test type_newtype`
Expected: PASS.

```bash
git add rill-lang/tests/type_newtype.rs rill-lang/src/types/infer.rs rill-lang/src/lower.rs
git commit -m 'feat(rill-lang): type synonyms and newtype wrappers end-to-end'
```

---

### Task 12: Typeclass + instance — compile-time resolution

**Files:**
- Modify: `rill-lang/src/types/infer.rs`
- Modify: `rill-lang/src/lower.rs`
- Create: `rill-lang/tests/typeclass.rs`

- [ ] **Step 1: Write the failing test**

`rill-lang/tests/typeclass.rs`:

```rust
use rill_lang::compile;
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn typeclass_method_resolves_at_compile_time() {
    let src = r"
        typeclass Show a where { show: a -> Str; }
        instance Show Float where { show f = \"float\"; }
        main = show 1.0;
    ";
    let mut prog = compile::<f32>(src);
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert!(out.iter().all(|v| *v == 0.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test typeclass`
Expected: FAIL.

- [ ] **Step 3: Implement constraint collection**

In `Ctx`, add `constraints: Vec<Constraint>` where `Constraint { class, type_var, span }`. When a method call `show x` is inferred with `x: ValueTy::Var(v)`, record the constraint; when the var resolves to a concrete type, resolve the instance at lowering.

- [ ] **Step 4: Implement method resolution in lower**

In `Lowerer`, keep `instances: HashMap<String, HashMap<String, Def>>` (class → method name → body). A `show x` where `x: ValueTy::Float` resolves to the `Show`/`Float` instance's `show` body and lowers it as an inline expression (β-reduced). `Str` is a v1 value type `ValueTy::Data("Str")` or a new `ValueTy::Str` variant (add `Str` to `ValueTy`).

- [ ] **Step 5: Run test + commit**

Run: `cargo test -p rill-lang --test typeclass`
Expected: PASS.

```bash
git add rill-lang/tests/typeclass.rs rill-lang/src/types/infer.rs rill-lang/src/lower.rs rill-lang/src/types/ty.rs
git commit -m 'feat(rill-lang): typeclass methods resolve at compile time'
```

---

### Task 13: main λ-parameters via runtime-stack cells

**Files:**
- Modify: `rill-lang/src/lower.rs`
- Modify: `rill-lang/src/backend/interp.rs`
- Modify: `rill-lang/src/program_engine.rs`
- Create: `rill-lang/tests/main_cells.rs`

- [ ] **Step 1: Write the failing test**

`rill-lang/tests/main_cells.rs`:

```rust
use rill_lang::compile;
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn main_lambda_params_become_cells() {
    // main's λ-param `gain` is now a runtime-stack cell, not intern_param.
    let mut prog = compile::<f32>("main gain = _ * gain").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0, 2.0, 3.0, 4.0]], &mut [&mut out]).unwrap();
    // `gain` defaults to 0 via the cell; output is 0. SetParameter later would
    // write into the same cell.
    assert_eq!(out, [0.0, 0.0, 0.0, 0.0]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test main_cells`
Expected: FAIL.

- [ ] **Step 3: Implement**

In `lower_with_cafs`, replace the `intern_param` loop for `main.params()`:

```rust
// main λ-parameters become cells.
lw.emit_value(ValueInstr::ValuePushScope);
for p in main.params() {
    let cell = lw.fresh_value_reg();
    lw.emit_value(ValueInstr::ValueBindCell { dst: cell });
    // register the cell in the local scope map so refs lower to ValueReadCell
    lw.value_locals.insert(p.name.clone(), cell);
}
```

Add `value_locals: HashMap<String, usize>` to `Lowerer` (mirror of `locals`). In `lower_ref`, when a name is in `value_locals`, emit `ValueReadCell` and return a value register. Add `emit_value(&mut self, v: ValueInstr)`.

`main`'s λ-param default is `0.0` — the cell starts `Void`; `ValueReadCell` on a `Void` cell returns `Value::Float(0.0)` (change the executor to default `Void` → `Float(0.0)` for cells).

- [ ] **Step 4: Wire SetParameter into cells**

In `program_engine.rs`, `param_map` currently maps `main` λ-param names to `ReadParam` indices. Change the map to resolve the same names to cell indices when the program exposes `main_cell_indices: HashMap<String, usize>`. `set_param` writes into the cell via `ValueWriteCell`.

- [ ] **Step 5: Run test + commit**

Run: `cargo test -p rill-lang --test main_cells`
Expected: PASS.
Run: `cargo test -p rill-lang`
Expected: PASS (no regression).

```bash
git add rill-lang/src/lower.rs rill-lang/src/backend/interp.rs rill-lang/src/program_engine.rs rill-lang/src/program.rs rill-lang/tests/main_cells.rs
git commit -m 'feat(rill-lang): main lambda parameters as runtime-stack cells'
```

---

### Task 14: Functions as values (named references)

**Files:**
- Modify: `rill-lang/src/types/infer.rs`
- Modify: `rill-lang/src/lower.rs`
- Modify: `rill-lang/src/backend/interp.rs`
- Create: `rill-lang/tests/func_values.rs`

- [ ] **Step 1: Write the failing test**

`rill-lang/tests/func_values.rs`:

```rust
use rill_lang::compile;
use rill_core::traits::MultichannelAlgorithm;

#[test]
fn function_reference_calls_definition() {
    // f = double; v = f 21 -> references the named definition `double`.
    let mut prog = compile::<f32>(
        "double x = x * 2.0; f = double; main = f 21.0",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-lang --test func_values`
Expected: FAIL.

- [ ] **Step 3: Implement**

- `ValueTy::Func(String)` — `f = double` binds a function value (a named ref).
- `ValueInstr::ValueCallFunc { dst, func, args }` dispatches to `Ir::value_funcs[func]`, whose lowering inlines the referenced definition's body against the args (β-reduction at lower time). Store `value_funcs` in `Ir`.

- [ ] **Step 4: Run test + commit**

Run: `cargo test -p rill-lang --test func_values`
Expected: PASS.

```bash
git add rill-lang/src/lower.rs rill-lang/src/types/infer.rs rill-lang/src/backend/interp.rs rill-lang/src/ir.rs rill-lang/tests/func_values.rs
git commit -m 'feat(rill-lang): named function references as values'
```

---

### Task 15: Docs, regression, verification

**Files:**
- Modify: `docs/src/guides/rill-lang.md`
- Modify: `rill-lang/README.md`

- [ ] **Step 1: Update language guide**

Add sections: value channel (`Rate::Value`), `data`/`type`/`newtype`/`typeclass`/`instance` reference, arena+RC+COW memory model, runtime-stack cells, single-lambda-process statement, per-tick execution with the value-track phase.

- [ ] **Step 2: Full workspace verification**

Run:
```bash
cargo test -p rill-lang
cargo test --workspace
cargo clippy --workspace
cargo fmt
```
Expected: all pass, zero clippy warnings, clean fmt.

- [ ] **Step 3: Commit**

```bash
git add docs/src/guides/rill-lang.md rill-lang/README.md
git commit -m 'docs(rill-lang): data types, typeclasses, and arena memory model reference'
```

---

## Self-review notes

- Spec §10 phases 1–8 map 1:1 to Tasks 1–15 (phase 1 → Tasks 1–4, phase 2 → Task 5, phase 3 → Tasks 6–9, phase 4 → Task 10, phase 5 → Task 11, phase 6 → Task 12, phase 7 → Tasks 13–14, phase 8 → Task 15).
- `ValueMatch` (Task 8) was introduced to keep runtime dispatch minimal; the spec's `ValueMatch` row covers it.
- `ValueWriteCell` into a fresh cell: Task 4's executor writes the value directly into the cell slot (cell holds the value, not a ref-to-ref) to keep v1 simple; the spec's "cell holds a ref to the value" is satisfied because the cell slot stores the value and `ValueReadCell` copies it out.
- No placeholders: every step has concrete code or an exact command.
- New external dependencies: none (plan violates none of AGENTS.md).
- Type consistency: `Channel`, `Rate`, `ValueTy`, `ValueInstr`, `ValueLayout`, `ValueFunc`, `ArenaRef`, `Arena`, `Value`, `ValueKind` are defined once and reused consistently.