# Haskell 2010 Typeclass Layer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Express every binary operator as sugar over a class method and fill the Haskell 2010 class layer (`Eq`, `Ord`, `Show`, `Num`, `Fractional`, `Integral`, `RealFrac`, `Floating`, `Enum`, `Bounded`, `Semigroup`) so comparison, arithmetic, rendering and enumeration dispatch through instances — including the `Buffer` class for element-wise buffer math.

**Architecture:** Five phases. **P-2 adds general typeclass towers** (superclass constraints + transitive entailment) — every operator scheme needs them and the language could not express a superclass at all. **P-A is the operator layer**: polymorphic numeric literals + numeric defaulting, operators as class-method sugar with the operator constructs deleted, and the `Eq`/`Ord` semantics (`compare`, `min`, `max`, `sort`) — one task because they are one concept and any split produces an invalid tree. P2 adds `Show`. P3 moves arithmetic to `Num`/`Fractional` and adds the `Buffer` arithmetic methods, deleting the scalar-vs-buffer fork. P4 adds `Integral`/`RealFrac`/`Enum`/`Bounded`. P5 adds `Semigroup`/`Foldable` and the `Ord`-based Prelude functions.

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

### Task P-2 (P₋₂): General typeclass towers (superclass constraints and entailment)

> **Why this comes before P-A:** every operator scheme is a class with
> superclasses — `Ord a =>` requires `Eq a`, `Fractional a =>` requires `Num a`,
> `Integral a =>` requires `Real a` and `Enum a`, `RealFrac a =>` requires
> `Real a` and `Fractional a`, `Floating a =>` requires `Fractional a`. Today the
> language cannot express a superclass at all:
>
> - `TypeclassInfo` has **no** `superclasses` field (only `var`, `arity`,
>   `methods`, `defaults`);
> - `Def::Typeclass` (ast.rs) has no superclass field;
> - `parse_typeclass_def` starts at `name var where` — no `(C a) =>` form;
> - `derive_superclass_instances` is **hard-coded to `Monad → Applicative →
>   Functor`**, not a general mechanism.
>
> Without entailment, a context `Ord a` cannot satisfy `==` (an `Eq` method), and
> P-A's constraint solving has nothing to solve against.

**Files:**
- Modify: `rill-lang/src/ast.rs` (`Def::Typeclass` gains `supers: Vec<(String, TypeExpr)>` or equivalent)
- Modify: `rill-lang/src/parser.rs` (`typeclass (C1 a, C2 a) => D a where { … }`; also the parenthesised-operator method-name form `(==) : a -> a -> Bool;`)
- Modify: `rill-lang/src/types/ty.rs` (`TypeclassInfo.superclasses`; replace the hard-coded `derive_superclass_instances` with a general walk over declared superclasses; entailment helper)
- Modify: `rill-lang/src/types/infer.rs` (constraint discharge walks superclasses transitively; a superclass method resolves when the subclass is in context)
- Modify: `rill-lang/src/lower.rs` (method dispatch through the entailed instance)
- Test: `rill-lang/tests/typeclass_towers.rs` (new)

**Interfaces:**
- Consumes: existing `TypeclassInfo`, `InstanceInfo`, `resolve_method`, `class_of_method`, `infer_constrained_apply`.
- Produces: `TypeclassInfo.superclasses: Vec<(String, String)>` (class, its variable); `fn entail(&self, class, ty) -> Vec<(String, ty)>` returning the transitive superclass contexts; general `derive_superclass_instances`.

- [ ] **Step 1: Prove the gap with failing tests**

```rust
#[test] fn superclass_declaration_parses()        { /* typeclass (Eq a) => Ord a where { … } parses and registers */ }
#[test] fn ord_implies_eq()                       { /* a definition with `Ord a =>` can call (==) */ }
#[test] fn fractional_implies_num()               { /* `Fractional a =>` can call (+) */ }
#[test] fn integral_implies_real_and_enum()       { /* `Integral a =>` can call toInteger and succ */ }
#[test] fn realfrac_implies_real_and_fractional() { /* floor on `RealFrac a =>` */ }
#[test] fn floating_implies_fractional()          { /* pi and (/) together */ }
#[test] fn a_value_without_the_superclass_is_rejected() { /* a type with Ord but no Eq is a clean error */ }
#[test] fn existing_monad_superclass_derivation_still_works() { /* Functor/Applicative from Monad unchanged */ }
```

- [ ] **Step 2: Implement the declaration and the type-level data**

Parse `typeclass (C1 a, C2 a) => D a where { … }`; store the supers in
`Def::Typeclass` and `TypeclassInfo`. Keep the existing single-variable form
working unchanged.

- [ ] **Step 3: Implement transitive entailment**

`Ord a` entails `Eq a`; `Integral a` entails `Real a` and `Enum a` (and therefore
whatever those entail); `RealFrac a` entails `Real a` + `Fractional a`;
`Floating a` entails `Fractional a`. Entailment must be **transitive and
cycle-safe**.

- [ ] **Step 4: Generalise superclass instance derivation**

Replace the hard-coded `Monad → Applicative → Functor` walk with a general pass
over every declared superclass. Default method bodies for the derived
superclasses stay as templates (`fmap` via `bind`/`pure`), but they are now
selected through the general mechanism.

- [ ] **Step 5: Dispatch through the entailment**

`resolve_method` / method dispatch must find a superclass method given a
subclass context: with `Ord a` in scope, `==` resolves to `Eq`'s instance for the
actual type.

- [ ] **Step 6: Checkpoint and commit**

Run the gates once. The eight tests above must pass; every existing test stays
green (the `Monad`/`Applicative`/`Functor` behaviour is unchanged because the
general mechanism must reproduce it).
```bash
git commit -m 'feat(lang): general typeclass towers with superclass entailment'
```

---

### Task P-A (P-A): The operator layer — polymorphic literals, class methods, and `Eq`/`Ord`

> **Ruling (user, 2026-10-25): P-1, P0 and P1 are ONE task.** They touch the same
> files, change one concept (how an operator gets its meaning), and every
> intermediate split produces an invalid tree: literals cannot be polymorphic
> without class-typed operators, operators cannot be class methods without
> class-typed literals, and `Ord`/`min`/`max`/`sort` are simply the first
> consumers of that machinery. Splitting them forced half-migrations and shims;
> merging them lets the work be done once, coherently.
>
> **Behaviour expectation.** Results change, and the new values are the correct
> ones (all measured against `runghc` 9.10.3):
> - `1 == 1.0` → **`True`** (today `False`);
> - `let add x y = x + y in add 1 2` → **`3`** (today a type error);
> - `(1::Int) + 1.0` → **type error** `Fractional Int` (today silent `2.0`);
> - `min`/`max`/`compare`/`sort` work on `Float`, `Int`, `String` and records.
>
> **No checkpoint between steps; one checkpoint when the task is done.** No shims,
> dual paths, or temporary bridges. Backward compatibility is explicitly not a
> requirement.

**Files:**
- Modify: `rill-lang/src/ast.rs` (delete `Expr::Arith`/`Expr::Cmp` and `ArithOp`/`CmpOp`; a numeric literal must be able to carry an unresolved numeric type)
- Modify: `rill-lang/src/parser.rs` (keep infix syntax and the §4.4.2 fixity; stop constructing operator nodes)
- Modify: `rill-lang/src/desugar.rs` (THE one place infix becomes `ApplyExpr { callee: Ref(op), args }`)
- Modify: `rill-lang/src/types/ty.rs` (`Eq`/`Ord` method sets incl. operators; `Num`/`Fractional` declarations)
- Modify: `rill-lang/src/types/infer.rs` (literal polymorphism; class-constraint solving; end-of-inference numeric defaulting; delete the forced-`Float` pin)
- Modify: `rill-lang/src/lower.rs` (primitive-instance bodies emitting the existing kernels; delete `lower_arith`, the `Expr::Arith`/`Expr::Cmp` arms and the buffer fork)
- Modify: `rill-lang/src/reduce.rs`, `render.rs`, `program_serde.rs` (drop the deleted nodes)
- Modify: `rill-lang/src/backend/interp.rs` (expose `value_cmp_ref` for `Ord`; kernels unchanged)
- Test: `rill-lang/tests/typeclass_layer.rs` (new), `rill-lang/tests/numeric_literals.rs` (new)

**Interfaces:**
- Consumes: P-2's `entails`/`entailed_classes` and superclass declarations; `infer_constrained_apply`.
- Produces: literal typing `Num a => a`; numeric defaulting; `fn operator_method(op: &str) -> Option<&'static str>`; `compare :: a -> a -> Int`, `min`, `max`, `(/=)`, `sort`, `sortBy`, `maximum`, `minimum`.

#### Part 1 — Polymorphic literals and defaulting

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn mixed_literals_adopt_the_solved_type() { /* 1 + 1.0 == 2.0; 1.0 + 1 == 2.0 */ }
#[test] fn mixed_literal_comparison_is_true()     { /* 1 == 1.0 is True */ }
#[test] fn numeric_function_is_polymorphic()      { /* add 1 2 == 3; add 1.0 2.0 == 3.0 */ }
#[test] fn bound_int_cannot_take_a_float_literal(){ /* (1::Int) + 1.0 is an error */ }
#[test] fn num_only_defaults_to_integer()         { /* 1 + 2 is Int */ }
#[test] fn fractional_defaults_to_double()        { /* 1 / 2 == 0.5 */ }
```

- [ ] **Step 2: Polymorphic literals** — an `Int`/`Float` literal infers as a numeric literal with a fresh variable, not a concrete type; it adopts whatever the solved class variable requires. Add the `ValueTy` representation if none exists.
- [ ] **Step 3: Solve, do not pin** — remove `infer_arith`'s forced `Float` binding; resolve through constrained schemes. A conflicting literal against an already-bound type is a type error, never a cast.
- [ ] **Step 4: Numeric defaulting** — at the end of inference, an unsolved numeric variable becomes `Integer` (`Num`-only) or `Double` (with `Fractional`).

#### Part 2 — Operators are class methods

- [ ] **Step 5: Delete the operator constructs** — remove `Expr::Arith`/`Expr::Cmp`, `ArithOp`/`CmpOp`, and every match arm using them. `Expr::Logic`/`LogicOp` **stay** (Haskell 2010 §3.4 short-circuit is not a plain function).
- [ ] **Step 6: One desugaring point** — `desugar.rs` turns infix into `ApplyExpr { callee: Ref(op), args: [lhs, rhs] }`, preserving §4.4.2 fixity and non-associativity. Declare `Num`/`Fractional`; extend `Eq`/`Ord` with operator spellings.
- [ ] **Step 7: Primitive instances** — `Int`, `Float`, `Bool`, `String`, `Buffer`, plus the structural/derived `Eq`/`Ord` instances, emit the kernels that ran before (`ValueAdd` family, `ValueCompare`, `ValueBufferBin`, `ValueBufferUn`, `BinArith`). Kernels are primitives, not operator constructs.

#### Part 3 — `Eq`/`Ord` semantics

- [ ] **Step 8: `compare`, `min`, `max`, `(/=)`, `sort`, `sortBy`, `maximum`, `minimum`** — `compare :: a -> a -> Int` (-1/0/1, recorded as an encoding deviation in `DOCS-GAPS.md`); `min`/`max` derived from `compare`; `sort` a stable sort over `Ord`.
- [ ] **Step 9: Total order preserved** — `value_cmp_ref` is a genuine total order (NaN sorts greatest and equals itself). Do not replace it with `partial_cmp` + `Equal` fallback: that is intransitive and breaks `Map`/`Set` search.

- [ ] **Step 10: Tests for the whole task**

```rust
#[test] fn comparison_works_through_methods() { /* "a" < "b"; record == record */ }
#[test] fn operator_reference_is_a_method()   { /* let plus = (+); plus 1.0 2.0 == 3.0 */ }
#[test] fn int_track_preserved()              { /* 1 + 2 is Int: show_int works */ }
#[test] fn logic_stays_short_circuit()        { /* false && <fault> is False */ }
#[test] fn compare_scalars()                  { /* compare 1 2 == -1; compare "b" "a" == 1 */ }
#[test] fn ord_min_max()                      { /* min/max on Float, Int, String, record; fold max */ }
#[test] fn not_equal_operator()               { /* 1 /= 2 */ }
#[test] fn sort_and_maximum()                 { /* sort [3,1,2]; maximum */ }
#[test] fn ord_requires_eq()                  { /* a type with Eq but not Ord is rejected at (<) */ }
#[test] fn nan_total_order_preserved()        { /* NaN compares greatest and equals itself */ }
```

- [ ] **Step 11: The single checkpoint** — run the gates and the ten tests above, plus the six literal tests. Confirm no `Expr::Arith`/`Expr::Cmp`/`ArithOp`/`CmpOp` remains and no operator-keyed lowering arm exists.
- [ ] **Step 12: Commit**

```bash
git commit -m 'feat(lang): one operator layer — polymorphic literals, class methods, Eq/Ord'
```

### Task 2 (P2): `Show` — one `show`

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`Show` class + instances), `rill-lang/src/lower.rs` (route `show_*` through the class), `rill-lang/src/backend/interp.rs` (shared renderer)
- Test: `rill-lang/tests/typeclass_layer.rs`

**Interfaces:**
- Consumes: P-A's `Show`-independent dispatch and method resolution.
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
- Consumes: P-A's desugaring and instance dispatch.
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
