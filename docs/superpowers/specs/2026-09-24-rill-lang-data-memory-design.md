# rill-lang: Data Types, Type Classes, and Arena+RC Memory Model — Design

> **Status:** Approved — 2026-09-24.
> **Date:** 2026-09-24
> **Branch:** `feature/rill-lang-memory-management`
> **Scope:** Add first-class runtime data values (`data` records and sums),
> Haskell-style `type` / `newtype` synonyms and wrappers, `typeclass`/`instance`
> with compile-time resolution, and an arena + reference-counting (RC) memory
> model with a runtime stack of cells for local variables. The value channel is
> a per-block control/value channel that coexists with the block-rate SIMD
> signal path. The whole program remains a single lambda process — there are no
> graph nodes in the execution system.

## 1. Problem statement

rill-lang is a Faust-style block-diagram DSL compiled to a single signal arrow.
Four gaps motivate this change:

1. **No first-class data.** Records exist only as compile-time configuration
   literals for built-ins (`Expr::Record`, folded to `f64` params). There are no
   user-defined types, no records as runtime values, no sums, no pattern
   matching, no structural state that can flow alongside signals.
2. **No user-facing type abstraction.** `type` (synonyms), `newtype`
   (distinct wrappers), and `typeclass` (ad-hoc polymorphism) are absent. The
   type system is `Scalar { Int, Float, Var }` + arities.
3. **No dynamic memory.** Local variables are compile-time only: `let`/`where`
   bindings are β-reduced away (`reduce.rs`), function parameters are bound to
   register indices in a compile-time scope stack (`lower.rs::locals`), and
   `main`'s λ-parameters are interned as named runtime params
   (`intern_param` → `ReadParam`). There is no notion of a heap value, a
   mutable cell, or a value with a runtime lifetime.
4. **The value model is monomorphic to samples.** Every wire is a block of
   samples. There is no way to carry "one structured value per tick" alongside
   the scalar signal path.

## 2. Semantic model

### 2.1 A program is a single lambda process

The execution system is **one lambda process**: a program is a function
(arrow) `(Block..) → (Block..)` compiled to a single flat IR and executed by a
single interpreter. There are **no separate graph nodes** in the execution
system. This is already the case today (`graph_compiler`/`CompiledGraph`/
`GraphIr` were removed in the unify step — the graph layer compiles a
`GraphSpec` into a single `ProgramEngine` wrapping one `RillProgram`).

Consequences:

- `rill-graph` remains a **pure frontend**: `GraphBuilder::to_graph_spec()` →
  `GraphSpec` → `rill_lang::graph::compile` → one `ProgramEngine`. Zero
  execution logic in `rill-graph`.
- The value channel is part of the single lambda process, not a "node
  boundary". Graph topology compiles to the same one `RillProgram`, so value
  channels flow through the whole program automatically.
- The arena is owned by the single `RillProgram` (like `block_state`,
  `delays`). No arena sharing between "nodes" exists.
- Typeclass resolution and COW happen inside the compiler (`lower.rs`) and the
  interpreter respectively, at the level of the unified IR.

### 2.2 Four type levels

| Level | Type | Meaning |
|---|---|---|
| sample | `Scalar` | one element inside a block (`Int` / `Float` / `Var`) |
| channel | `Channel` | one wire: either a block-rate signal or a per-block value |
| arrow | `ArrowTy` | a block transform `(I₁:Channel..Iₙ) → (O₁:Channel..Oₘ)` |
| value | `ValueTy` | the type of a heap value in the arena |

### 2.3 A channel has a rate

```rust
enum Rate { Signal, Value }

struct Channel {
    rate: Rate,
    // Signal-rate: the per-sample scalar type (as today).
    elem: Scalar,
    // Value-rate: the type of the arena value carried per tick.
    vty: ValueTy,
}
```

The `Value` rate is "one value per tick" — a single `ArenaRef` flows on the
wire, evaluated once per tick in the value-track phase of the interpreter. The
`Signal` rate is the existing block of samples processed whole-buffer via the
SIMD vector eDSL.

```rust
enum ValueTy {
    Int, Float,                        // scalar values stored in arena slots
    Data(TypeName),                    // record or sum type (resolved to a declaration)
    Newtype(NewtypeName),              // distinct wrapper
    Func(FnName),                      // reference to a named definition
    Var(TypeVarId),                    // HM type variable
}
```

### 2.4 Arrow combinators are rate-aware

| Combinator | Signal part | Value part |
|---|---|---|
| `_` | identity | identity |
| `!` | cut | cut |
| `A : B` | as today | value types unified (`vty` matched) |
| `A , B` | channel concat | channel concat (vty travels with elem) |
| `A <: B` | arity ×k | value channels are **shared** (RC++ per copy), not multiplied |
| `A :> B` | sum ×k | **forbidden in v1** (summing values is undefined) |
| `A ~ B` | block state | value state slot (1-tick delayed value) |
| `A @ n` | delay line | value delayed n ticks |

## 3. Syntax

### 3.1 Type declarations

```faust
// type — synonym (pure substitution)
type Angles = Float;

// newtype — distinct wrapper with explicit construct/unwrap
newtype Hz = Float;

// data — product type (record)
data Point = { x: Float, y: Float };

// data — sum type (constructors with payload)
data Shape = Circle Float | Rect Float Float;

// typeclass + instance — resolved at compile time
typeclass Show a where {
    show: a -> Str;
}
instance Show Float where {
    show f = "float";
}
```

### 3.2 Value expressions

```faust
p  = Point { x: 1.0, y: 2.0 };     // record constructor
s  = Circle 1.0;                    // sum constructor
r  = p.x;                           // field projection
p.x := 3.0;                         // COW mutation (see §5)
h  = Hz 440.0;                      // newtype constructor
f  = hz_to_f h;                     // newtype unwrap (automatic where Float expected)
v  = area s;                        // pattern matching:

area x = match x of {
    Circle r    => 3.14159 * r * r;
    Rect w h    => w * h;
};
```

## 4. Type model changes (`types/ty.rs`, `types/infer.rs`)

- `Block` is extended/replaced by `Channel { rate, elem, vty }`. The existing
  scalar unification (`unify_scalar`) is preserved for `elem`.
- `ArrowTy { ins: Vec<Channel>, outs: Vec<Channel> }` — arities remain
  `len(ins)` / `len(outs)`.
- `ValueTy` unification: `Data(TypeName)` unifies only with the same name;
  `Var` unifies structurally; `Func(FnName)` with the same name.
- `Scheme { lam_count, vars, ty }` unchanged in shape; λ-parameters
  (meta-level) remain distinct from channels (object-level).
- Arity synthesis stays bottom-up in `infer.rs`; combinator laws become
  rate-aware (§2.4).
- `TypeVarId` / `Subst` extend to `ValueTy` substitution where needed.

## 5. Memory model: arena + RC + COW + runtime stack of cells

### 5.1 Arena

```rust
type ArenaRef = u32;

struct ArenaSlot {
    rc: u32,          // non-atomic — single-threaded DAG
    gen: u32,         // generation counter (debug/abort-safety)
    val: Value,
}

enum Value {
    Int(i64),
    Float(f64),
    Record(TypeName, Box<[ArenaRef]>),       // fields = refs to slots
    Sum(TypeName, CtorIdx, Box<[ArenaRef]>), // constructor + payload
    Newtype(NewtypeName, ArenaRef),          // wrapper
    Func(FnName),                            // reference to a definition
    Void,                                   // null slot (empty arena)
}
```

- **Fixed capacity** `N`, computed at build time from the IR
  (`ValueLayout { capacity }`, alongside `StateLayout`). No growth after build.
- **Free-list**: `Vec<ArenaRef>` of free indices + `Vec<Slot>`; a slot with
  `rc == 0` is free. `alloc` = pop from free-list, `free` = push. All O(1), no
  syscalls, no locks, no heap growth.

### 5.2 RC rules

- `ValueCopy { dst, src }` → `rc[dst]++` (share).
- `ValueDrop { src }` → `rc--`; at `rc == 0`, recursively drop field refs, push
  the slot back to the free-list.
- **COW**: `ValueUpdate { slot, field, src }` — if `rc[slot] > 1`, allocate a
  fresh slot, copy the value, `rc[new] = 1`, `rc[old]--`, then mutate the copy;
  if `rc == 1`, mutate in place. This is the only place RC decides copy-vs-mutate.

### 5.3 Runtime stack of cells (local variables)

Local variables are **arena cells**, not registers or compile-time names:

```
binding: Map<String, ArenaRef>   // variable = ref to a cell slot
```

- **Bind** a variable = allocate a cell; the cell holds a ref to the value.
- **Read** `x` → `ValueReadCell { cell }` (ref from the cell, RC++).
- **Mutate** `p.x := 3.0` → COW on `rc[p]`, write the new ref back into the
  **cell** `p` — the cell is re-assignable, SSA is not violated.
- **Enter scope / leave scope** → `ValuePushScope` / `ValuePopScope` — a
  runtime stack of cells in the arena (a runtime activation record).
- **Shadowing** → push a new cell on the stack; pop releases the old one.
- **`main` λ-parameters** → cells in the runtime stack (replacing
  `intern_param`/`ReadParam`). `SetParameter` from the actor system routes to
  the same cells.
- This is the foundation for future closures (a cell is a variable with stable
  identity that a closure can capture).

This replaces both the compile-time `locals: Vec<HashMap<String, Vec<usize>>>`
in `lower.rs` and the `intern_param` mechanism for `main` parameters. The
lowering `locals` stack becomes a compile-time mirror that emits
`ValuePushScope`/`ValuePopScope`/`ValueReadCell`/`ValueWriteCell` instructions.

## 6. Value-track in IR (`ir.rs`, `schedule.rs`, `backend/interp.rs`)

New instruction family, executed **once per tick** (not per sample), as a
separate schedule phase:

| Instruction | Meaning |
|---|---|
| `ValueConstInt` / `ValueConstFloat` | create an Int/Float slot |
| `ValueConstructRecord { ty, fields }` | alloc + write fields |
| `ValueConstructSum { ty, ctor, payload }` | alloc + sum constructor |
| `ValueNewtype { ty, src }` / `ValueUnwrap { src }` | wrap / unwrap |
| `ValueProject { dst, slot, field }` | read a field (ref alias, RC++) |
| `ValueUpdate { slot, field, src }` | COW mutation of a field |
| `ValueMatch { dst, slot, ty }` | branch on a sum constructor |
| `ValueCallFunc { dst, fn_id, args }` | call a function reference |
| `ValueCopy` / `ValueDrop` | RC manipulation |
| `ValueStateRead` / `ValueStateWrite { slot }` | value state for `~` / `@` (per-tick) |
| `ValuePushScope` / `ValuePopScope` | runtime stack of cells |
| `ValueReadCell { cell }` / `ValueWriteCell { cell, src }` | variable access |

**Value registers**: `value_regs: Vec<Option<ArenaRef>>` — like `block_regs`
but one value per register per tick (not `BUF`). `ValueState` — per-tick slots
for `~` and `@ n`.

### 6.1 Tick order (interpreter)

```
1. push_builtin_params (as today)
2. block-track:  Step::Block / Step::ForeignBlock (as today, SIMD whole-buffer)
3. value-track:  ValueInstr... (one pass per tick)
4. swap_block_state + swap_value_state
5. copy outputs (scalar + value)
```

### 6.2 RT-path reentrancy

- Arena pre-allocated → no heap allocation.
- Free-list — `Vec::pop`/`push` on pre-reserved memory → O(1), no syscalls.
- RC — non-atomic `u32` (single-threaded DAG, guaranteed by the architecture).
- Everything stays within `#![deny(unsafe_code)]` — arena on `Vec` + indices,
  no raw pointers.

## 7. Typeclass resolution (compile time)

```faust
typeclass Show a where { show: a -> Str; }
instance Show Float where { show f = "float"; }
```

- `infer.rs`: typeclass declarations register method dictionaries; constraints
  `Constraint { class, type_var, span }` are collected during inference.
- `lower.rs`: a method call `show x` over a concrete type resolves to the
  specific instance's implementation and compiles to a direct
  `ValueCallFunc` / block call. **Zero runtime dispatch.**
- Classes and instances produce **no arena values** — purely compile-time.

## 8. Pipeline

```
source → lexer → parser → AST (data/type/newtype/typeclass/instance/method)
  → infer (HM + arity + rate + typeclass constraints) → reduce (β-reduction)
  → lower (block-track + value-track IR) → RillProgram (arena + blocks + value-state)
  → ProgramEngine (mailbox + SetParameter via cells)
```

## 9. Out of scope

- Instantaneous `loop` / per-sample execution (needs sample-rate data path).
- Cross-node value ports in `rill-graph` (`GraphSpec` value edges). Value flow
  is internal to one lambda process; graph value edges are a follow-on.
- Closures / lambda literals — only named function references in v1.
- Typeclass runtime dispatch (dictionaries as values).
- Sum-type `:>` merge.

## 10. Implementation phases

| # | Phase | Scope |
|---|---|---|
| 1 | Arena + runtime stack of cells | `Arena`, free-list, RC, `ValuePushScope/Pop/ReadCell/WriteCell`, `ValueState`, capacity from `StateLayout`. Replaces `intern_param`/`locals` in lowering. |
| 2 | Value-track in IR + schedule | value registers, value instructions, tick phase order, value channel in `Channel` (rate). |
| 3 | `data` records | product types, constructors, `.field` projection, COW mutation `:=`. |
| 4 | `data` sums | constructors + payload, `match` pattern matching. |
| 5 | `type` / `newtype` | synonyms + distinct wrappers (construct/unwrap). |
| 6 | `typeclass` + `instance` | compile-time method resolution. |
| 7 | Functions as values | `ValueCallFunc`, references to definitions. |
| 8 | main unification, tests, docs | λ-params of `main` via cells; integration tests; `rill-lang.md`. |

Each phase is a commit with tests; phases 1 and 2 are independently useful
(arena with Int/Float values is already usable).

## 11. Testing

1. Arena: alloc/free from free-list, RC share/drop, capacity exhaustion is a
   build-time error.
2. COW: `rc > 1` mutation copies; `rc == 1` mutates in place; original
   unchanged after copy.
3. Runtime stack: push/pop, shadowing releases old cell, `main` λ-params are
   readable cells.
4. Value-track: one value per tick; `~` and `@` on value channels.
5. Records: construct, project, COW-update; records flow on value channels.
6. Sums: constructors, `match` dispatch, payload access.
7. `type`/`newtype`: synonym substitution; newtype construct/unwrap distinctness.
8. Typeclass: `show` over `Float` resolves to the instance method; no runtime
   dispatch.
9. Functions as values: `f = some_def; v = f ...` calls the referenced
   definition.
10. Regression: all existing tests stay green; zero clippy warnings.

## 12. Documentation

Update `docs/src/guides/rill-lang.md`: value channel, `data`/`type`/`newtype`/
`typeclass` reference, arena+RC memory model, runtime stack of cells, per-tick
execution model, the "single lambda process" statement.

## 13. Files

| File | Change |
|---|---|
| `rill-lang/src/types/ty.rs` | `Channel { rate, elem, vty }`, `ValueTy` |
| `rill-lang/src/types/infer.rs` | rate-aware laws, typeclass constraints, `ValueTy` unification |
| `rill-lang/src/types/unify.rs` | value unification |
| `rill-lang/src/ast.rs` | `DataDef`, `TypeAliasDef`, `NewtypeDef`, `TypeclassDef`, `InstanceDef`, `Match`, `FieldProject`, `FieldUpdate` |
| `rill-lang/src/lexer.rs` | keywords: `data`, `type`, `newtype`, `typeclass`, `instance`, `match` |
| `rill-lang/src/parser.rs` | declaration + value expression parsing |
| `rill-lang/src/reduce.rs` | keep data/type/newtype defs; β-reduce methods |
| `rill-lang/src/lower.rs` | value-track lowering, runtime-stack cells, typeclass resolution |
| `rill-lang/src/ir.rs` | value instructions, `ValueLayout` |
| `rill-lang/src/schedule.rs` | value-track phase |
| `rill-lang/src/program.rs` | `Arena`, `value_regs`, `value_state`, `ValueState` |
| `rill-lang/src/backend/interp.rs` | value-track executor |
| `rill-lang/src/lib.rs` | `compile*` pipeline |
| `rill-lang/tests/*` | new tests (§11) |
| `docs/src/guides/rill-lang.md` | language reference updates |

Verification: `cargo test -p rill-lang`, then `cargo test --workspace`,
`cargo clippy --workspace`, `cargo fmt`. Zero warnings before merge
(`AGENTS.md` warnings policy). No new external dependencies.