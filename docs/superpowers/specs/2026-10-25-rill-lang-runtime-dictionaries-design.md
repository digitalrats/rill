# rill-lang: Runtime Typeclass Dictionaries (vtable model)

> **Status:** Design approved-in-conversation (2026-10-25). Supersedes the
> call-site monomorphisation model in
> `2026-10-25-rill-lang-typeclass-layer-design.md` §3.1c and pauses Task P-A.
> **Date:** 2026-10-25
> **North star:** Haskell 2010 semantics, GHC's *implementation strategy*
> (System FC dictionaries). Files stay in rill; the mechanism follows GHC.

---

## 0. Executive summary

rill currently implements typeclass dispatch by **monomorphisation**: a
constrained definition (`ConstrainedDef`) is specialised and β-reduced at every
call site. That model cannot give a class method a **value form**, so the
lowerer grew **five parallel paths** for one operation:

| Path | Purpose |
|---|---|
| `instantiate_method` | inline an instance body |
| `lower_primitive_method` | emit a kernel for a primitive instance |
| `lower_method_value` | a method used as a value (synthesises `\a -> \b -> (+) a b`) |
| `lower_operator_value` | a bare `(+)` |
| `lower_constrained_apply_site` | specialise a `ConstrainedDef` |

Each synthesises AST by hand (`$op_a`/`$prim_a` names), and the model outright
forbids recursive polymorphic functions. Task P-A cannot proceed on this base.

This design replaces monomorphisation with **runtime dictionaries**, exactly as
GHC does it (verified against GHC 9.10.3 Core output):

```haskell
$fMyNumInt = C:MyNum $cmyAdd $cmyZero        -- a dictionary IS a data value
use = \@a $dMyNum x -> myAdd $dMyNum x ...   -- a constraint IS a parameter
myAdd = \@a v -> case v of { C:MyNum m _ -> m }   -- a method IS a field selector
```

**The five paths collapse into one.** A polymorphic function takes an extra
dictionary parameter; a method call projects a slot; a method used as a value is
an ordinary closure over `(dictionary ref, slot index)`.

---

## 1. Decisions (user)

| Decision | Value |
|---|---|
| Model | **GHC-style dictionaries** (System FC), not monomorphisation |
| Dictionary storage | **a first-class runtime value**, not a compile-time table index (chosen for JIT adaptability) |
| Runtime variant | **a dedicated `Value::Dictionary`**, not a reused `Record`/`Callable` (so codegen can lower it directly) |
| Superclasses | **fields of the same dictionary** (vtable layout: methods + base-vtable refs), as GHC and as C++ vtables |
| Names | **no strings in the runtime dictionary**: slots are positional; class/type/method names stay in `TypeEnv` metadata, like GHC's `TyCon`/`TrName` |
| Scope | **one general mechanism for ALL classes**, not only the operator ones |
| Rationale (user) | a general system *simplifies* the compiler overall |

---

## 2. Goals, non-goals, anti-goals

### Goals

- **G1** A class method is a **value**. `fold (+)`, `map ((+) 1.0)`, `mappend`
  passed around, sections — all ordinary closures, no AST synthesis.
- **G2** A constraint is a **parameter**. `f :: Num a => a -> a` has one body and
  takes a dictionary argument.
- **G3** **Recursive polymorphic functions work** (removes the documented
  `recursive class-constrained definition` limitation).
- **G4** **One dispatch path.** `instantiate_method`, `lower_method_value`,
  `lower_operator_value`, `lower_primitive_method` and
  `lower_constrained_apply_site` are deleted; a single "pass/project a
  dictionary" mechanism replaces them.
- **G5** **Superclass access is a slot projection** (base-dictionary field), not
  a search.
- **G6** Numeric literals are `fromInteger dict 1` / `fromRational dict …`, which
  makes literal polymorphism and `(1::Int) + 1.0` correctness follow from the
  model rather than from special cases.
- **G7** No `unsafe`, no allocation on the RT path beyond the pre-sized arena.

### Non-goals

- **Not** a JIT implementation. This design makes the representation JIT-ready
  (a dictionary is a fixed-layout const-like structure) but does not build codegen.
- **Not** GHC's optimizer. Specialisation/inlining stays a future backend concern.
- **Not** new classes beyond Haskell 2010's set.
- **Not** changing the arena's RC/lifetime model.

### Anti-goals

- **Keeping monomorphisation as a fallback** — that would leave two models.
- **Strings in the dispatch path** — names are metadata, never a lookup key at runtime.
- **A special case per class or per method** — one layout, one mechanism.
- **A dictionary that allocates per call** — dictionaries are built once at seat.
- **Silently degrading to the old model** when a case looks hard.

---

## 3. Design

### 3.0 Static typing comes first; dictionaries are its elaboration

Dictionaries are the **runtime representation** of a constraint that the type
system has already proved satisfiable — they do not replace static typing. Both
reference systems separate the layers the same way:

| | Static layer | Runtime layer |
|---|---|---|
| **GHC** | infer/check `Num a =>`, solve the constraint, record the instance | dictionary parameter; method = field selector |
| **C++** | resolve overloads, check the class hierarchy, pick the override | vtable pointer |

So the pipeline is:

```
infer (prove constraints, record instance selections) → desugar (insert dictionary args) → lower
```

Three concrete consequences, and each is a required change to the current code:

1. **A constrained body IS typed, once, under an abstract dictionary.** Today
   `ConstrainedDef` is deliberately never inferred (`types/ty.rs:270-276`):
   *"the body is never inferred in isolation"*. That must change: inference types
   the body with the class variable **given** (not solved), which is exactly a
   `Num a =>` judgement. Method calls inside the body resolve against the GIVEN
   constraint, not against a call-site type.
2. **Constraints and dictionary types must be expressible.**
   - `Scheme` gains `constraints: Vec<(String, TypeVarId)>` (class, its variable).
   - `ValueTy` gains `Dict(ClassId)` — the type of a dictionary parameter. A
     constrained function's type carries one leading `Dict` for each of its
     constraints: `f :: Num a => a -> a` elaborates to
     `Thunk([Dict(Num)], Thunk([a], [a]))`.
   These are the System FC "dictionary argument" and "constraint" made explicit;
   without them lowering has nothing to pass and inference has nothing to record.
3. **Selections are recorded, not recomputed.** Inference already solves class
   variables (`class_var_solved_ty`, `method_dispatch_key`); under this design it
   **keeps the result**: `TypedProgram` gains a table mapping each call site /
   body position to the chosen `(class, concrete type)`. Lowering inserts the
   dictionary the type checker chose; it never re-derives it from signatures.

A body typed under an abstract dictionary proves the two properties static typing
exists for:

- **every method call is well-typed under the declared constraint** (no dynamic
  lookup can fail — an instance absence is a compile error, as today);
- **the constraint is discharged at each call site by a concrete instance** —
  `f @Float` passes `dictNumFloat`, and `no instance of Num for Bool` is still a
  *compile-time* diagnostic.

The dictionaries themselves carry **no types** at runtime (they are positional
vtables, §3.1), which is what makes them JIT-friendly; the type evidence lives in
the compiler, not in the value.

### 3.1 The dictionary value

```rust
/// A runtime class dictionary — the vtable of a type.
///
/// Layout follows C++ vtables and GHC's dictionary constructors: the
/// SUPERCLASS dictionaries come first (base-class pointers), then the class's
/// own method slots in declaration order. There are NO names here: a method is
/// addressed by slot index, decided statically by the compiler.
Value::Dictionary {
    /// Superclass dictionaries, in the order the class declares them.
    /// `Ord a`'s dictionary holds `[Eq a]`; `Num a` holds `[Eq a, Ord a]`.
    supers: Vec<ArenaRef>,
    /// Method slots in `TypeclassInfo.methods` order. A slot is either a
    /// compiled fragment (a user instance equation) or a primitive kernel.
    methods: Vec<DictSlot>,
}

/// One method slot.
enum DictSlot {
    /// A user instance method: the fragment to call. `Ir::fragments` already
    /// holds the code; a dictionary slot is the same kind of index a
    /// `Value::Callable` uses, so no new call machinery is needed.
    Fragment { fragment: u32 },
    /// A primitive (kernel-backed) method: comparison, scalar arithmetic,
    /// buffer arithmetic, structural Eq/Ord. Chosen statically by the compiler.
    Kernel(KernelId),
}
```

`KernelId` is an opaque small integer the interpreter maps to an existing
operation (`ValueCompare`, the `ValueAdd` family, `ValueBufferBin`,
`ValueBufferUn`). **`KernelId` is one enum covering the primitive operations that
already exist**, not one variant per class method — otherwise it grows with every
method of every class.

### 3.2 Where dictionaries live and when they are built

- **Built once per `(class, type)` pair the program actually uses**, during
  `WorldState::seat`, into arena slots held by a `Vec<ArenaRef>` table on the
  world (one slot per instance, like `buffers`/`streams`).
- **Passed by ref** (`ArenaRef`) as the extra parameter of a polymorphic
  function; the callee never copies it.
- **`visit_refs` walks `supers`** so RC/`drop_ref`/trace handle a dictionary like
  any other aggregate. `methods` are code indices, not refs, so they are ignored.
- **Static budget**: each dictionary costs one arena slot per method-fragment
  aggregate plus its `supers` vector; the accounting adds a constant per
  *used* instance, not per call site.

### 3.3 Dispatch: what the compiler emits

| Source | Emitted shape |
|---|---|
| `f :: Num a => a -> a` | `f = \dict -> \x -> …` — a plain extra parameter |
| `f @Float` at a call site | `f dictNumFloat` — the dictionary for `(Num, Float)` |
| `x + y` inside `f` | project slot `(+)` from `f`'s `Num` dictionary, call it with `x`,`y` |
| `(+)` used as a value | a closure capturing `(dict_ref, slot)`, i.e. the **same** `Callable` machinery with one captured env field |
| `min` from `Ord`'s default | a slot whose fragment is the class default body, instantiated once per dictionary |
| `1` (a literal) | `fromInteger dictNumT 1` where `T` is solved, or defaulted at the top |
| Superclass method via `Ord` | project the `Eq` dictionary from `supers[0]`, then its slot |

Consequences:

- a method is a **value** (G1) because a projected slot plus its dictionary IS a
  closure;
- a constraint is a **parameter** (G2);
- recursion is fine (G3) — there is one body;
- the five paths become **one**: "project a slot / pass a dictionary".

### 3.4 What is deleted

| Removed | Why |
|---|---|
| `ConstrainedDef` + `lower_constrained_apply_site` | no call-site specialisation |
| `instantiate_method` | no body inlining |
| `lower_method_value`, `lower_operator_value` | a method value is a closure, not synthesised AST |
| `lower_primitive_method` | a primitive method is a `DictSlot::Kernel` |
| `class_var_solved_ty` / `class_var_param_pattern` / `class_var_selector_index` / `class_var_arg_index` | dispatch no longer *solves* the class variable at the call site by pattern-matching signatures; it passes the dictionary the type checker already determined |
| the `$op_a`/`$prim_a`/`$prim_x` synthetic names | no synthesised lambdas |

`InstanceInfo.primitive` becomes `DictSlot::Kernel` at construction time
(it already distinguishes kernel-backed instances).

### 3.5 Type inference's remaining job

Inference keeps solving the class variable at the call site — that is
unavoidable and is exactly what GHC's constraint solver does. What changes is the
*destination* of the result: instead of β-reducing a body, it **selects a
dictionary** and records that selection for lowering.

- `Var` that solves to a concrete type → look up the instance → the dictionary
  ref for `(class, concrete type)`.
- `Var` still ambiguous at the end of inference → **defaulting** (`Num`-only →
  `Int`, with `Fractional` → `Float`; recorded in `DOCS-GAPS.md` as rill's
  numeral mapping of GHC's `Integer`/`Double`).
- No instance → the existing `no instance of …` diagnostic.

This is why the numeric-literal work (P-A Steps 1–3) is *not lost*: a literal's
type is solved by the same machinery, and the literal becomes a dictionary
application.

### 3.6 Literal typing under dictionaries

- `Int`/`Float` literals infer as a numeric literal with a fresh variable bound
  by a `Num` constraint.
- The literal lowers to `fromInteger (<Num dictionary>) n`.
- `1 + 1.0`: both operands force the same `Num a`, `1.0`'s `Float` literal
  solves `a := Float`, and `1` becomes `fromInteger dictNumFloat 1`. Result `2.0`.
- `(1::Int) + 1.0`: the annotation pins `a := Int`, so `1.0` would need
  `Fractional Int` → **type error**, as GHC reports.
- Defaulting: an unsolved numeric literal becomes `Int`/`Float` per rill's
  numeral set.

### 3.7 Migration order (each step independently green)

0. **Type-system groundwork (§3.0):** `ValueTy::Dict(ClassId)`, `Scheme.constraints`,
   a `TypedProgram` selection table, and inference typing a constrained body
   under given constraints. No behaviour change yet.
1. **`Value::Dictionary` + `DictSlot` + every RC walker** (no behaviour change;
   the type exists and is walkable).
2. **Dictionary construction at seat** for every `(class, type)` the program
   mentions; a debug accessor proves the table matches `TypeEnv`.
3. **Polymorphic functions take a dictionary parameter** (the selection table
   drives it; conversions of `ConstrainedDef` call sites to the new form).
4. **Method calls project a slot**; delete `instantiate_method` and
   `lower_primitive_method`.
5. **Method-as-value is a selector** (see §3.8); delete `lower_method_value` /
   `lower_operator_value`.
6. **Superclass projection** (`supers[i]`).
7. **Literals as `fromInteger`** + defaulting, together with the missing kernels
   (§3.9).
8. **Delete `ConstrainedDef` and the specialisation path**; grep proves the five
   paths are gone.

### 3.8 Method-as-value: a selector per method, generated once

A projected slot inside a runtime env record would only be readable at runtime,
contradicting "slots decided statically", and a `Kernel` slot cannot go through
`run_call` at all. So a method used as a value is **one selector fragment per
(class, method)** — `$c+` for `Num.(+)`, `$cmyAdd` for a user method — generated
once at lowering, never per call site:

```
$c+ = \dict -> \x -> \y -> call_slot dict 0 x y
```

`fold (+)` and `map ((+) 1.0)` therefore pass a closure over that selector with
the dictionary ref in its env (the existing `Callable { fragment, env }` shape).
This is exactly GHC's `myAdd = \@a v -> case v of { C:MyNum m _ -> m }`, and it
keeps **one** dispatch path while still making a method a first-class value
("no AST synthesis" means *no per-call-site synthesis*, not "no selector").

### 3.9 Missing primitive kernels are in scope

`KernelId` cannot be "an enum over operations that already exist", because
several required operations have **no runtime kernel at all** today:
`Ord.compare`, `Num.negate`, `Num.abs` (scalar — buffer-only today),
`Num.signum`, `Num.fromInteger`, `Fractional.recip`. `fromInteger` is mandatory
for literal elaboration, so this work must add the IR instructions:

| Kernel | New IR | Notes |
|---|---|---|
| `compare` | `ValueCompare3` (or `ValueCompare` returning -1/0/1) | must keep the **total** order (`value_cmp_ref`: NaN greatest, NaN == NaN) |
| `negate`, `signum`, `abs` (scalar) | `ValueUnOp` family | reuse the existing unary instruction shape |
| `fromInteger`, `recip` | `ValueFromInteger`, `ValueRecip` | Int→the target type; `recip x = 1/x` |

`KernelId` stays one enum over these **operations**, mapping to slots by
`(class, method)` in `TypeEnv` — so adding a class does not grow the enum.

### 3.10 Constraint-parameterised instance heads

`instance (Monad m) => Arrow (Kleisli m)` and the auto-derived
`Applicative`/`Functor` need a dictionary that is a **function of another
dictionary** (GHC: `$fArrowKleisli :: Monad m => Arrow (Kleisli m)`). A flat
`supers: Vec<ArenaRef>` cannot express that, so `DictSlot` gains a third form:

```rust
enum DictSlot {
    Fragment { fragment: u32 },
    Kernel(KernelId),
    /// A dictionary CONSTRUCTOR: `dict = builder(dictArg0, dictArg1, …)`.
    /// Emitted once; the instance's `constraints` list says which dictionaries
    /// it consumes.
    Builder { fragment: u32 },   // a fragment that takes dict refs and yields a dict
}
```

The instance's existing `constraints` field (already on `InstanceInfo`) is the
builder's parameter list. Concrete dictionaries are built at seat; a
constraint-parameterised one is built when its argument dictionaries are
available (the fixpoint `derive_superclass_instances` already orders that).

### 3.11 Recursion (G3) — precise scope

Removing `recursive class-constrained definition` is necessary but not
sufficient. Recursion is gated by three other mechanisms:

- `check_recursion` rejects every recursive user def in **Strict** mode
  (`types/infer.rs:624, 2094`) — that is a *mode* rule, not a class rule, and
  stays.
- `recursive signature-less curried definition` (`lower.rs:4746`) — a lowering
  guard for definitions without a signature; a constrained definition HAS a
  signature, so this path is already unreachable for them.
- `max_call_regs` (runtime nesting bound) — unchanged.

**So G3 means:** a recursive *constrained* (class-polymorphic) definition works in
**`MODE complete`** (where recursion is legal at all), because it is now an
ordinary fragment taking a dictionary. Strict mode keeps rejecting recursion —
Haskell 2010's typeclass semantics do not require recursion, and the existing
Strict rule is a separate design decision. Acceptance criterion 6 is worded
accordingly.

### 3.12 Static typing is preserved — the tests that prove it

- A constrained body is **typed** (criterion: inference reports an error for an
  ill-typed body under its own constraint, e.g. `f :: Num a => a -> String` with
  `x + 1`).
- **No instance** is a compile error, never a runtime lookup failure.
- A superclass method resolves **from the given constraint** (e.g. `Eq`'s `==`
  used inside a body constrained by `Ord a`).
- A dictionary's slots are addressed by index; a **mismatched** class/instance
  pair cannot be constructed because the table is built from `TypeEnv`.

---

## 4. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Dictionary allocation inflates the arena budget | One dictionary per *used* `(class, type)`, built once at seat; the accounting charges a constant, not per call |
| `visit_refs` forgets `supers` → leaked / prematurely freed dictionaries | A test drops a dictionary and asserts its superclass dictionaries survive; `trace`/RC tests cover it |
| Inference still needs signature pattern-matching to solve `a` | That is inherent (GHC does it too); the *lowering* side stops using it |
| Method-as-value closure needs an env field beyond the existing `Callable` | `Callable` already carries `env: ArenaRef`; the dictionary ref and slot live in that env record |
| Deleting five paths at once is a large diff | The migration order (§3.7) is incremental and each step is green |
| Performance regression from indirect calls | Measure with the GHC-parity harness; specialisation/inlining is a backend concern (`SPEC` equivalent), not a front-end obligation |
| `KernelId` grows into a per-method enum | One enum over *existing primitive operations*, not per class method |
| Class defaults must be instantiated per dictionary | A default body becomes a fragment once, reused by every dictionary that omits the method — no per-instance code duplication |

---

## 5. Acceptance criteria

1. `Value::Dictionary` exists with `supers` and positional `methods`; **every**
   RC walker handles it — `arena::visit_refs`, `interp::recount_children`,
   `interp::drop_value_children`, `interp::pool_clone_value` — plus
   `PartialEq`/`value_cmp_ref`/`kind_rank`/`ValueKind`.
2. Dictionaries are built once at seat for every used `(class, type)`; a
   program with no constrained function builds none.
3. A constrained function takes one extra dictionary parameter; **one body**,
   independent of call sites.
4. A method call projects a slot; a method used as a value is an ordinary
   closure (`fold (+)`, `map ((+) 1.0)`, `mappend` as an argument, sections).
5. A superclass method resolves through `supers[i]` with no signature
   pattern-matching at lowering.
6. **Recursive polymorphic functions compile and run in `MODE complete`** (the
   class-specific diagnostic is deleted). Strict mode keeps its unrelated
   recursion ban (§3.11).
7. Numeric literals lower to `fromInteger`/`fromRational` on the solved
   dictionary; `1 + 1.0 == 2.0`, `1 == 1.0` is `True`, `(1::Int) + 1.0` is a type
   error, and defaulting yields `Int`/`Float`.
8. **Zero occurrences** of `ConstrainedDef`, `instantiate_method`,
   `lower_method_value`, `lower_operator_value`, `lower_primitive_method`,
   `lower_constrained_apply_site`; grep-proven.
9. `cargo test -p rill-lang --features dsp` green (baseline 974 plus the new
   tests); `clippy` and `fmt` clean.
10. No `unsafe`; no new dependencies.
11. A test asserts a dictionary's superclass entries and a method slot are
    reachable without any name lookup.
12. **Static typing is preserved** (§3.12): a constrained body is type-checked
    under its own constraint; an ill-typed body is rejected; `no instance of …`
    is a compile error; a superclass method resolves from the given constraint.
13. Constraint-parameterised instance heads build their dictionary from argument
    dictionaries (`Builder` slot, §3.10) and the `Kleisli`/derived instances keep
    working.
14. The missing kernels (§3.9) exist: `compare`, scalar `negate`/`abs`/`signum`,
    `fromInteger`, `recip`.

---

## 6. Relationship to other work

- **Supersedes** `2026-10-25-rill-lang-typeclass-layer-design.md` §3.1c
  (monomorphisation). The class *declarations*, the single `PRELUDE` registry,
  `Buffer`-as-`Num`-subclass and the primitive kernels all remain and are reused.
- **Pauses Task P-A** of the typeclass plan; P-A is rewritten on top of this
  design (its literal/defaulting goals survive as §3.6, and its operator-deletion
  goals as §3.7 steps 4–8).
- **Unblocks** the JIT work: a dictionary is a fixed-layout structure, which is
  what a code generator needs; §0 of the OpenSpec 0.6 change
  (`rill-compiler-haskell-0.6`) already establishes that no separate graph
  engine is wanted, and this design keeps the same "front-end to one IR" rule.
- **Improves** the GHC-parity benchmark's validity: rill and GHC will use the
  same dispatch model, so the remaining difference is the allocator, not the
  polymorphism strategy.
