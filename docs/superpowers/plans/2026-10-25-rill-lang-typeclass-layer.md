# Haskell 2010 Typeclass Layer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Express every binary operator as sugar over a class method and fill the Haskell 2010 class layer (`Eq`, `Ord`, `Show`, `Num`, `Fractional`, `Integral`, `RealFrac`, `Floating`, `Enum`, `Bounded`, `Semigroup`) so comparison, arithmetic, rendering and enumeration dispatch through instances — including the `Buffer` class for element-wise buffer math.

**Architecture:** Five phases, each independently shippable and green. P0 makes the operator desugaring path exist without changing behaviour (operators keep their current opcodes, reached through primitive instances). P1 lands `Eq`/`Ord` and retires the comparison fork. P2 adds `Show`. P3 moves arithmetic to `Num`/`Fractional` and adds the `Buffer` arithmetic methods, deleting the scalar-vs-buffer fork. P4 adds `Integral`/`RealFrac`/`Enum`/`Bounded`. P5 adds `Semigroup`/`Foldable` and the `Ord`-based Prelude functions.

**Tech Stack:** Rust, `rill-lang`; tests via `cargo test -p rill-lang --features dsp`.

**Spec:** `docs/superpowers/specs/2026-10-25-rill-lang-typeclass-layer-design.md`

## Global Constraints

- **Baseline compatibility:** `cargo test -p rill-lang --features dsp` is **903 passing, 0 failing** at the plan base. Every one must still pass.
- **North star:** Haskell 2010 + GHC Prelude. A divergence is a defect; **no "pre-existing" exemption** (`rill/AGENTS.md`).
- **No residual operator fork.** After a phase lands, no lowering branch may key on the operator itself; dispatch decides inside instances. A residual scalar-vs-buffer fork is a defect.
- **`&&` / `||` are not desugared** — Haskell 2010 §3.4 short-circuit. They stay nodes and gain strict `Bool` typing.
- `#![deny(unsafe_code)]`; English comments; single-quoted commit messages.
- Gates per task: test + clippy + fmt (no `--all-targets`).
- Every deliberate encoding deviation (e.g. `compare :: a -> a -> Int`) goes in `docs/superpowers/DOCS-GAPS.md`.

## Review Focus

1. **A silent semantic change under the desugaring** — `1 + 2` must still be `Int(3)`, not `Float(3.0)`; `1 + 1.0` must still unify as today. The Int track is the most likely casualty.
2. **`&&`/`||` losing short-circuit** — desugaring them into functions would evaluate the right operand unconditionally. A test must pin that `false && (bottom)` is `False`, not a fault.
3. **Comparison losing the total order** — `value_cmp_ref` is a *total* order (NaN sorts greatest and equals itself). A naive `Ord` instance using `partial_cmp` with an `Equal` fallback becomes intransitive and breaks `Map`/`Set` binary search.
4. **A class method and its old opcode both live** — the phase that adds a class must delete the fork it replaces; otherwise the two drift and one is dead but tested.
5. **Buffer arithmetic unreachable or unpinned** — no current test exercises it (`sine` is unregistered without the `dsp` feature). The phase that class-ifies it must add a runnable `dsp` test, not just a type-level one.
6. **`Ord ⊑ Eq` not enforced** — a value with `Eq` but no `Ord` must be rejected at a `<` call site, with a clean message.
7. **Derived instances invisible to dispatch** — `derive_eq_ord` markers (`methods: HashMap::new()`, `from_prelude: true`) must be understood by the resolver, or every record comparison silently stops working.

---

## File Structure

| File | Responsibility |
|---|---|
| `rill-lang/src/types/ty.rs` | The single `PRELUDE` text, `TypeclassInfo`, `derive_eq_ord`, `resolve_method`, superclass derivation |
| `rill-lang/src/parser.rs` | `typeclass` superclass syntax, operator method names, `[a..b]` desugaring (P4) |
| `rill-lang/src/ast.rs` | `Def::Typeclass` superclass field if needed; no new expression nodes (desugaring reuses `Apply`) |
| `rill-lang/src/types/infer.rs` | Operator→method resolution, constraint discharge, strict `&&`/`||` typing |
| `rill-lang/src/lower.rs` | Operator desugaring path, method-call emission, deletion of operator forks |
| `rill-lang/src/backend/interp.rs` | Primitive kernels (unchanged, reached through instances) |
| `rill-lang/tests/typeclass_layer.rs` | New: per-phase behavioural tests |
| `rill-lang/tests/conformance_gaps.rs` | Existing: extend where a gap closes |

---

### Task 0 (P0): Operator constructs replaced by class-method dispatch

> **Ruling (user, 2026-10-25): full replacement, no reuse.**
>
> **Ruling (user, 2026-10-25, supersedes the per-step preservation rule):**
> **Do not test behaviour between steps.** The only behavioural checkpoint is the
> **finished task** — the state you commit when every step below is done.
> Intermediate states are not checkpoints and are not required to work; a step
> that deletes a construct before its replacement exists is *supposed* to be
> inconsistent while it is being written.
>
> Concretely:
> - Do **not** run the probe matrix, the test suite, or any behavioural check
>   part-way through the migration to "see whether it still works".
> - Do **not** add a compatibility shim, a dual path, a temporary bridge, or an
>   `#[allow(dead_code)]` placeholder to keep a half-migrated tree working.
> - Implement all steps, then run the gates **once** on the finished task.
> - If a gate fails then, fix the finished tree — not by resurrecting the old
>   construct.

The existing operator-specific constructs are **deleted**, not kept alongside the
class path. When the task is finished there is no `Expr::Arith`, no `Expr::Cmp`,
no `ArithOp`/`CmpOp` operator node, and no operator-keyed lowering arm. The
runtime kernels (`ValueAdd`-family, `ValueCompare`, `ValueBufferBin`,
`ValueBufferUn`, `BinArith`) survive **only as bodies of primitive instances** —
they are primitives, not operator constructs.

**Files:**
- Modify: `rill-lang/src/ast.rs` (delete `Expr::Arith`, `Expr::Cmp`, `ArithOp`, `CmpOp`; keep `Expr::Logic`/`LogicOp`)
- Modify: `rill-lang/src/parser.rs` (emit method application for `+ - * / % == /= < <= > >=` instead of operator nodes; keep infix parsing and the §4.4.2 fixity table)
- Modify: `rill-lang/src/desugar.rs` (the single place operator syntax becomes an application)
- Modify: `rill-lang/src/types/infer.rs` (type operator refs through the class methods; `Ord ⊑ Eq`)
- Modify: `rill-lang/src/lower.rs` (delete `lower_arith`, the `Expr::Arith` arm, the `Expr::Cmp` arm and the buffer fork; the method path handles them)
- Modify: `rill-lang/src/reduce.rs`, `render.rs`, `program_serde.rs` (drop the deleted nodes)
- Modify: `rill-lang/src/types/ty.rs` (declare `Num`/`Fractional` in `PRELUDE`; extend `Eq`/`Ord` with operator spellings)
- Test: `rill-lang/tests/typeclass_layer.rs`

**Interfaces:**
- Consumes: existing `Expr::Ref("+")` → callable machinery.
- Produces: `fn operator_method(op: &str) -> Option<&'static str>`; primitive instances for `Int`/`Float`/`Bool`/`String`/`Buffer` whose bodies emit the existing kernels.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn arithmetic_works_through_methods() { /* 1 + 2 == 3 (Int); 1.0 + 2.0 == 3.0 */ }
#[test] fn comparison_works_through_methods() { /* "a" < "b"; record == record */ }
#[test] fn operator_reference_is_a_method()   { /* let plus = (+); plus 1.0 2.0 == 3.0 */ }
#[test] fn int_track_preserved()              { /* 1 + 2 is Int: show_int works, no Float widening */ }
#[test] fn logic_stays_short_circuit()         { /* false && <fault> is False */ }
```

- [ ] **Step 2: Run to verify they fail.**

- [ ] **Step 3: Delete the operator constructs**

Remove `Expr::Arith`/`Expr::Cmp` and `ArithOp`/`CmpOp`; update every match arm (ast, parser, desugar, infer, reduce, lower, render, serde). `Expr::Logic`/`LogicOp` stay (Haskell 2010 §3.4 short-circuit is not expressible as a plain function).

- [ ] **Step 4: Emit method application instead**

The parser keeps infix syntax and the §4.4.2 fixity; `desugar` is the ONE place that turns an infix operator into `Expr::ApplyExpr { callee: Ref(op), args: [lhs, rhs] }`. Declare `Num`/`Fractional` in the single `PRELUDE` constant; extend the built-in `Eq`/`Ord` `TypeclassInfo` with the operator spellings.

- [ ] **Step 5: Behavioural checkpoint — run this ONCE, after Steps 3–4 are complete**

Do not run it earlier. Primitive instances (`Int`, `Float`, `Bool`, `String`,
`Buffer`) emit exactly the kernels that ran before, so the probe matrix matches
the starting one: `1 + 2` → `Int(3)`, `1.0 + 2.0` → `Float(3.0)`, `1 + 1.0` →
its current result, comparisons → the same booleans, buffer arithmetic
unchanged. Semantic corrections (`1 + 1.0` becoming a type error, `Num`
polymorphism) belong to P1/P3.

If a probe mismatches, fix the finished tree. Do not restore the deleted
construct to make it match.

- [ ] **Step 6: Verify and commit**

```bash
cargo test -p rill-lang --features dsp
cargo clippy -p rill-lang --features dsp
cargo fmt --check
git commit -m 'refactor(lang): replace operator constructs with class-method dispatch'
```

---

### Task 1 (P1): `Eq` / `Ord` — comparison, `min`/`max`, `sort`, total order

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`Ord` superclass `Eq`; method defaults for `min`/`max`/`<`/`<=`/`>`/`>=`/`/=`; `compare`), `rill-lang/src/lower.rs` (delete the `Expr::Cmp` operator fork), `rill-lang/src/backend/interp.rs` (expose `value_cmp_ref` through the primitive instance)
- Test: `rill-lang/tests/typeclass_layer.rs`, `rill-lang/tests/conformance_gaps.rs`

**Interfaces:**
- Consumes: Task 0's dispatch.
- Produces: `compare :: a -> a -> Int` (-1/0/1), `min`, `max`, `(/=)`, `sort`, `sortBy`, `maximum`, `minimum`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn compare_scalars()          { /* compare 1 2 == -1; compare "b" "a" == 1 */ }
#[test] fn ord_min_max()              { /* min/max on Float, Int, String, record, first-class: fold max */ }
#[test] fn not_equal_operator()       { /* 1 /= 2 */ }
#[test] fn sort_and_maximum()         { /* sort [3,1,2]; maximum */ }
#[test] fn ord_requires_eq()          { /* a type with Eq but not Ord is rejected at (<) */ }
#[test] fn nan_total_order_preserved(){ /* NaN compares greatest and equals itself (Map/Set keys stay searchable) */ }
```

- [ ] **Step 2: Run to verify they fail.**

- [ ] **Step 3: Implement `Ord` with `Eq` superclass**

Declare `typeclass (Eq a) => Ord a` (P1 needs parser support for superclass constraints; `TypeclassInfo` gains a `superclasses: Vec<(String, String)>` field). Defaults: `min x y = if x <= y then x else y`, `max` the dual, `(/=)` the `Eq` dual, `<`/`<=`/`>`/`>=` from `compare`.

- [ ] **Step 4: Make derived instances dispatchable (Review Focus 7)**

`derive_eq_ord` writes instances with `methods: HashMap::new()` and `from_prelude: true`. Teach `resolve_method` that this marker means "primitive structural comparison" and have it emit `ValueCompare` against `value_cmp_ref`. **Do not** replace `value_cmp_ref` with `partial_cmp` (Review Focus 3).

- [ ] **Step 5: Delete the comparison fork**

Remove the `Expr::Cmp` → `ValueCompare` direct lowering; `==`/`<`/… now reach the same instruction through the primitive instance. `lower.rs` must contain no operator-keyed branch for comparisons.

- [ ] **Step 6: Verify and commit**

Gate + `git commit -m 'feat(lang): Ord/Eq class layer with compare, min, max and sort'`.

---

### Task 2 (P2): `Show` — one `show`

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`Show` class + instances), `rill-lang/src/lower.rs` (route `show_*` through the class), `rill-lang/src/backend/interp.rs` (shared renderer)
- Test: `rill-lang/tests/typeclass_layer.rs`

**Interfaces:**
- Consumes: Task 1.
- Produces: `show :: a -> String`; `show_float`/`show_int`/`show_bool` remain as aliases.

- [ ] **Step 1: Write failing tests** — `show` for Float/Int/Bool/String/List/record/Sum; the three legacy names still work.

- [ ] **Step 2–3: Implement** — `Show` class declared; instances delegate to the existing renderers (`show_float` etc. become `instance Show Float where { show = show_float }`-shaped aliases). Lists render as Haskell-2010 `[a,b,c]`.

- [ ] **Step 4: Verify and commit** — `git commit -m 'feat(lang): Show class and generic show'`.

---

### Task 3 (P3): `Num` / `Fractional` + `Buffer` arithmetic — delete the scalar-vs-buffer fork

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`Num`/`Fractional` classes, `Int`/`Float` instances, `Buffer` arithmetic methods on `FixedBuffer a`/`Tape f32`), `rill-lang/src/lower.rs` (delete the `Expr::Arith` buffer/scalar fork at ~3356 and the `ValueBufferUn`/`ValueUnOp` fork), `rill-lang/src/backend/interp.rs` (unchanged kernels)
- Test: `rill-lang/tests/typeclass_layer.rs`, plus a `#[cfg(feature = "dsp")]` buffer test

**Interfaces:**
- Consumes: Task 0's desugaring, Task 1's dispatch.
- Produces: `negate`, `signum`, `abs`, `fromInteger`, `pi`, `recip`, `**`, `log`, and element-wise `Buffer` arithmetic.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn num_scalar_ops()          { /* + - * on Int and Float; negate; signum; abs scalar */ }
#[test] fn fractional_ops()          { /* / on Float; recip */ }
#[test] fn floating_ops()            { /* pi; log; **; sin/cos */ }
#[test] fn abs_scalar_works()        { /* abs (-1.0) == 1.0 — currently a Buffer-argument error */ }
#[cfg(feature = "dsp")]
#[test] fn buffer_arithmetic_works() { /* (bufA + bufB) element-wise; buf * 0.5 — currently unreachable in tests */ }
```

- [ ] **Step 2: Run — expect failures** including the current `abs signal argument must be a Buffer`.

- [ ] **Step 3: Implement `Num`/`Fractional`/`Floating`** with `Int` (Num, Integral later) and `Float` (Num, Fractional, Floating) instances. Scalar bodies emit the existing `ValueArith`/`UnOp`.

- [ ] **Step 4: Implement `Buffer` arithmetic** — `(+)`/`(-)`/`(*)`/`(/)`/`min`/`max`/`abs` on `FixedBuffer a` and `Tape f32`, bodies emitting `ValueBufferBin`/`ValueBufferUn`. Register the `dsp` test so the path is actually reachable (Review Focus 5).

- [ ] **Step 5: Delete both forks** — after this task `lower.rs` has **no** `matches!(a_ty, ValueTy::Buffer)` branch in operator lowering; the instance body decides.

- [ ] **Step 6: Verify and commit** — `git commit -m 'feat(lang): Num, Fractional and Buffer arithmetic through instances'`.

---

### Task 4 (P4): `Integral` / `RealFrac` / `Enum` / `Bounded`

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (classes + instances), `rill-lang/src/parser.rs` (`[a..b]` desugaring), `rill-lang/src/lower.rs` (`%` moves to `rem`)
- Test: `rill-lang/tests/typeclass_layer.rs`

- [ ] **Step 1: Write failing tests** — `div`/`mod`/`quot`/`rem`; `floor`/`ceiling`/`round`/`truncate`; `fromIntegral`/`toInteger`; `succ`/`pred`; `enumFromTo`; `[1..5]`; `minBound`/`maxBound`.
- [ ] **Step 2: Run — expect `unknown identifier`.**
- [ ] **Step 3: Implement the four classes** (`Integral Int`, `RealFrac Float`, `Enum Int`, `Bounded Int`), with `%` desugaring to `rem`.
- [ ] **Step 4: Implement `[a..b]`** as `enumFromTo a b` in the parser (Haskell 2010 §3.10); `[a,b..c]` is out of scope → gap row.
- [ ] **Step 5: Verify and commit** — `git commit -m 'feat(lang): Integral, RealFrac, Enum and Bounded'`.

---

### Task 5 (P5): `Semigroup` / `Foldable` and `Ord`-based Prelude functions

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`Semigroup`, `Foldable`, `foldr`/`foldl`/`concat`/`sum`/`product`/`elem`/`null`), `rill-lang/src/backend/interp.rs` (fold kernels over `List` and finite `Cons`)
- Test: `rill-lang/tests/typeclass_layer.rs`

- [ ] **Step 1: Write failing tests** — `foldr`/`foldl` left/right association pinned by a non-commutative function; `concat`; `sum`/`product`; `elem`; `null`.
- [ ] **Step 2: Run — expect `unknown identifier`.**
- [ ] **Step 3: Implement `Semigroup`** (`<>` on `List`/`String`/`Float`), keep `Monoid` as the superclass relationship (`Monoid ⊑ Semigroup`, Haskell 2010 §).
- [ ] **Step 4: Implement `Foldable`** over `List` and finite `Cons` (infinite `Cons` faults loudly — Task 5 of the conformance plan owns laziness).
- [ ] **Step 5: Verify and commit** — `git commit -m 'feat(lang): Semigroup and Foldable with foldr, foldl, concat, sum'`.

---

### Task 6 (final): Fork audit, gap register, full gates

**Files:**
- Test: whole crate
- Modify: `docs/superpowers/DOCS-GAPS.md`

- [ ] **Step 1: Audit for residual operator forks**

Run: `grep -n 'Expr::Arith\|Expr::Cmp' rill-lang/src/lower.rs`
Expected: no operator-keyed branch that inspects operand types; the only remaining use is the desugaring entry point.

Run: `grep -n 'ValueTy::Buffer' rill-lang/src/lower.rs`
Expected: no buffer-vs-scalar fork in operator lowering.

- [ ] **Step 2: Closed gaps**

Delete every `DOCS-GAPS.md` row this work closed (MC-1's `param()` is separate; close `min`/`max`, `stdio`, `div`, … as applicable) and add the encoding deviations (`compare :: Int`) as `open` with justification.

- [ ] **Step 3: Full gates**

```bash
cargo test -p rill-lang --features dsp
cargo clippy -p rill-lang --features dsp
cargo fmt --check
```
Expected: green; test count ≥ the 903 baseline.

- [ ] **Step 4: Review Focus check**

For each of the seven Review Focus lines, name the test that pins it in the final report.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m 'docs: close the typeclass gaps and audit operator dispatch'
```
