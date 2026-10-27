# rill-lang: dictionary stages — stage 2 (dictionary construction at seat)

> **Status:** **DONE** — commit `105bf22` (1087 tests).
> **Design:** `2026-10-25-rill-lang-runtime-dictionaries-design.md` §3.2 (where
> dictionaries live), §3.7 step 2 (this stage), §3.9 (missing kernels).
> **Predecessors, all landed:** §3.7 step 0 (`f439106`, `03bd4ee`, `4d7ff99`) and
> step 1 (`caf8ca7`).

## Where we are

| §3.7 step | State |
|---|---|
| 0 — `Expr::Annot`, `ValueTy::Dict`, `Scheme.constraints`, constrained bodies typed | **done** |
| 1 — `Value::Dictionary` + `DictSlot` + all four RC walkers | **done** (`caf8ca7`) |
| **2 — dictionary construction at seat** | **this plan** |
| 3–8 — pass a dictionary, project a slot, delete the five specialisation paths | later |

Also landed and relevant: `TypeEnv::type_entry` (one type-name registry),
`Kind` projection, type-level `[a]`/`(a, b)` syntax.

## What exists today, and what is missing

Measured on `9472bda`:

- `Value::Dictionary { supers: Vec<ArenaRef>, methods: Vec<DictSlot> }` and
  `DictSlot::{Fragment{fragment}, Kernel(KernelId)}` exist (`arena.rs:78`, `:260`).
- `KernelId` has 16 variants; **six have no IR instruction** and are documented as
  such: `Compare`, `Negate`, `Signum`, `Abs` (scalar), `FromInteger`, `Recip`.
- `TypeEnv` holds everything a dictionary needs: `typeclasses` (with
  `superclasses`), `instances` (`InstanceInfo` with `methods`, `primitive`,
  `constraints`, `head_args`), and the single name registry.
- Inference fills `TypedProgram::dict_selections: HashMap<Span, (class, ValueTy)>`
  and lowering threads it, **but nothing reads it** — stage 2 is its first
  consumer.
- `WorldState` has **no** dictionary table, and `Ir` carries **no** instance or
  class information at all. So seat cannot build a dictionary today: it has
  nothing to build from.
- The `(class, type, method) → kernel` decision exists only **inline** inside
  `lower_primitive_method` (`lower.rs:5403`), as `match (class_name, name)`.

## Scope

Stage 2 delivers **the table and the proof it is complete**, and nothing else:
no call site changes, no dispatch changes, no behaviour change.

### One kernel table, two consumers

The `(class, method) → KernelId` decision moves into **one** function. Two
consumers must use it: `lower_primitive_method` (refactored to consult it instead
of its inline `match`) and seat (which needs the same answer to fill
`DictSlot::Kernel`). Two copies of that mapping is exactly the defect class this
work exists to remove.

### The descriptor that crosses into the IR

Lowering already has the whole compile-time picture (`TypedProgram`: `type_env`,
`dict_selections`). It emits a compact descriptor list into the IR:

```rust
// ir.rs
pub struct DictDesc {
    pub class: String,
    pub ty_key: String,                 // matches value_type_key / instance keys
    pub supers: Vec<usize>,             // indices into Ir::dicts
    pub methods: Vec<DictSlot>,         // Ir::fragments index, or KernelId
}
// Ir gains: pub dicts: Vec<DictDesc>,
```

- The descriptor set is the **transitive closure** of every `(class, type)` the
  program mentions (from `dict_selections` plus the superclasses each class
  declares), so a dictionary never references a superclass that has no slot.
- **Order is topological**: a superclass descriptor precedes the descriptor that
  references it, so seat can build in one forward pass and never needs a
  fixpoint. If the closure is not acyclic, that is a hard error naming the cycle.
- `ty_key` uses the same identity as `value_type_key`/instance keys, so a
  dictionary and its instance can never disagree. `render_type_key` stays the
  nominal spelling for that reason.

### Seat builds them

`WorldState` gains `dicts: Vec<ArenaRef>` — one arena slot per descriptor, in
`Ir::dicts` order — plus a debug accessor that proves the table matches the
descriptors one-to-one (index → `Value::Dictionary`, supers pointing at the
expected slots).

### The six unimplemented kernels must be visible, not silent

Building a dictionary whose slot names a kernel with no IR instruction is
unavoidable today (`Num Int` legitimately declares `signum`). The stage must
therefore **report** them, not hide them: a machine-readable accessor listing
every `(class, type, method)` whose slot is a kernel with no instruction, and a
test pinning that list to design §3.9's six names. Stage 4 (slot projection) can
then never return a silent hole, and stage 7 has an exact checklist.

## Steps

- [x] **Step 1: One kernel table.** Extract `(class, method) → Option<KernelId>`
      into a single function; refactor `lower_primitive_method` to use it and
      delete the inline `match` arms it replaces. Behaviour identical.
- [x] **Step 2: `DictDesc` + `Ir::dicts`.** Add the descriptor type and the IR
      field (empty for programs that mention no classes).
- [x] **Step 3: Build the descriptor set in lowering.** From `dict_selections`
      plus the declared superclass closure; topological order; `ty_key` from the
      existing identity function; `DictSlot::Fragment` for source instances
      (`Ir::fragments`) and `DictSlot::Kernel` for primitives. A cyclic closure
      is a hard error.
- [x] **Step 4: Seat materialises them.** `WorldState::dicts: Vec<ArenaRef>`, one
      `Value::Dictionary` per descriptor, supers resolved by index; all four RC
      walkers already handle the value, so no new walker work.
- [x] **Step 5: Debug accessors.** `dicts_len()`, a descriptor↔slot consistency
      check, and the unimplemented-kernel report from the "visible, not silent"
      section.
- [x] **Step 6: Prove it.** A program using `Num Double`, `Ord Double`,
      `Eq Double` gets a dictionary for each **and** for the `Eq`/`Ord`
      superclasses `Num` declares; every slot is a fragment or a kernel; the
      unimplemented-kernel list equals §3.9's six names and nothing else; a
      program with no classes gets an empty table.
- [x] **Step 7: No behaviour change.** Full suite green; nothing reads the table
      yet.

## Acceptance

- Every `(class, type)` a program mentions has exactly one dictionary slot.
- Superclasses are materialised transitively (`Num a` implies `Eq a`, `Ord a`).
- Build order is topological; a superclass ref always points at an
  already-built slot.
- `dicts` is empty for a program that mentions no class.
- The unimplemented-kernel list is exactly `Compare`, `Negate`, `Signum`,
  scalar `Abs`, `FromInteger`, `Recip` — asserted, so stage 7 cannot drift.
- Exactly **one** `(class, method) → KernelId` mapping exists.
- No behaviour change; all baseline tests green.

## Explicitly out of scope

Steps 3–8: passing a dictionary parameter, projecting a slot, deleting
`instantiate_method` / `lower_method_value` / `lower_operator_value` /
`lower_primitive_method` / `lower_constrained_apply_site`, literal
`fromInteger`, defaulting. Those change dispatch; this stage only builds the
table and proves it correct.

## Outcome (`105bf22`)

Controller-verified after the fact:

| Check | Result |
|---|---|
| Dictionary table for `same`/`lt`/`add` on `Double` | `dicts=3` — `Eq Double`, `Ord Double`, `Num Double` |
| Topological order | `[0] Eq supers=[]`, `[1] Ord supers=["Eq(0)"]`, `[2] Num supers=["Eq(0)","Ord(1)"]` — every superclass ref points backwards; one forward pass suffices |
| One kernel table | `kernels::kernel_for_method` (`kernels.rs:37`); consumed by `lower_primitive_method` **and** the descriptor builder; the inline `match (class, name)` arms are gone |
| Missing kernels | exactly six and nothing else: `Compare`, `Negate`, scalar `Abs`, `Signum`, `FromInteger`, `Recip` — matches design §3.9 |
| Class-free program | `dicts=0` |
| Behaviour | unchanged; 1087 tests green |
| `Ir` serialisation | `Ir` carries no `Serialize`, so no JSON tag moved; `program_json` (serialization + dsp) stays 8/8 |

### Gaps handed to stage 3 (found, not papered over)

1. **Prelude bodies' selections are unmerged** — their spans index the prelude
   text, so a dictionary needed *only* inside a prelude body has no descriptor.
2. **Infix operators record nothing** — `1.0 < 2.0` and `1.0 == 2.0` produce
   `dicts=0`, because operator dispatch still goes through the legacy
   `Expr::Arith`/`Expr::Cmp` path (P-A's unfinished half). Method-call syntax
   (`(<) a b`) does record.
3. **`fmap` over a concrete container never calls `record_selection`.**
4. **Given-constraint witnesses have no concrete type**, so `Num a => a -> a`
   yields no dictionary — correct for now (nothing to build yet), and it becomes
   "the caller supplies it" at stage 3.
5. **Some Complete-mode bodies that capture become a precise gap.**

Also fixed on the way: a latent `subtree_size` panic on a bare `List`/`Set`/`Map`.

### Consequence

Stage 2 builds and proves the table, but **no ordinary program exercises it yet**:
operators (gap 2) and `fmap` (gap 3) are exactly the two ways a user mentions a
class, and neither records a selection. Closing gaps 2–3 belongs to stage 3–4,
where a dictionary becomes an argument and a slot is projected.
