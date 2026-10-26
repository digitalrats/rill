# Runtime Typeclass Dictionaries — Stage 1 (type groundwork + dictionary value)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lay the two foundations the dictionary model needs — (a) the type system can express a constraint and a dictionary type and type a constrained body under a *given* constraint, and (b) a `Value::Dictionary` exists, is walkable by every RC path, and is observable — with **no behaviour change** to any working program.

**Architecture:** Two independent stages, each shippable on its own. Stage 1 adds `Expr::Annot`, `ValueTy::Dict`, `Scheme.constraints` and a selection table on `TypedProgram`, and makes inference type a constrained definition's body once (under a given constraint) instead of leaving it uninferred. Stage 2 adds the runtime value: `Value::Dictionary { supers, methods }` with `DictSlot`, wired into all four RC walkers plus the comparison/kind paths, built nowhere yet (stage 3 builds them at seat).

**Tech Stack:** Rust, `rill-lang` crate; tests via `cargo test -p rill-lang --features dsp`.

**Spec:** `docs/superpowers/specs/2026-10-25-rill-lang-runtime-dictionaries-design.md` (§3.0, §3.6, §3.1, §3.2, and the migration-order item 0–1)

## Global Constraints

- **No behaviour change.** Every currently passing test must stay green (baseline: **974 passing, 0 failing**). Stage 1 and 2 add capability, they do not switch dispatch.
- **Synthesised code is never a substitute for a type.** A dictionary carries no runtime type information; slots are positional and the type evidence stays in the compiler.
- **Static typing is preserved** (spec §3.0, §3.12): an ill-typed constrained body must be a compile error; `no instance of …` stays a compile error.
- **One RC walker rule:** a new `Value` variant must be handled by `arena::visit_refs`, `interp::recount_children`, `interp::drop_value_children`, `interp::pool_clone_value`, and by `PartialEq`, `value_cmp_ref`, `kind_rank`, `ValueKind`. Missing one leaks or double-frees.
- **`Expr::Annot` erases at lowering** — it is type-level only.
- `#![deny(unsafe_code)]`; no new dependencies; English comments; single-quoted commit messages.
- Gates: `cargo test -p rill-lang --features dsp`, `cargo clippy -p rill-lang --features dsp`, `cargo fmt --check` (no `--all-targets`).

## Review Focus

1. **A constrained body that used to be un-inferred now being inferred** — the risk is a body that only worked because each call site specialised it. Test: an `Ix`-constrained body keeps compiling, and an ill-typed one is rejected.
2. **`Expr::Annot` precedence** — `e :: t` must bind looser than application and tighter than arithmetic (Haskell 2010 §3.12). Test: `f x :: Int` parses as `(f x) :: Int`.
3. **A literal in an `Int` position staying `Int`** once literals become polymorphic-capable (spec §3.6, criterion 15). Test: `readArrayFlat arr 1` still resolves the index to `Int`.
4. **A missing RC walker arm** — adding `Dictionary` without all four walkers compiles (they use `match` with a wildcard) or panics. Test: a dictionary with a superclass is dropped and the superclass survives.
5. **`ValueTy::Dict` leaking into paths that assume data** — `kind_rank`, `value_type_key`, `type_name_of_vty` must give it a defined answer rather than `unreachable!()`.

---

## File Structure

| File | Responsibility |
|---|---|
| `rill-lang/src/ast.rs` | `Expr::Annot` node; `Def::Typeclass` already carries supers |
| `rill-lang/src/parser.rs` | postfix `:: type` on an expression |
| `rill-lang/src/desugar.rs` | erase `Annot` (and keep the inner expression's desugaring) |
| `rill-lang/src/types/ty.rs` | `ValueTy::Dict`, `Scheme.constraints`, `TypeclassInfo` slot ordering doc |
| `rill-lang/src/types/infer.rs` | type a constrained body under given constraints; record selections in `TypedProgram` |
| `rill-lang/src/types/unify.rs` | unify `Dict` (identity on the class id) |
| `rill-lang/src/arena.rs` | `Value::Dictionary`, `DictSlot`, `ValueKind::Dictionary`, `visit_refs` |
| `rill-lang/src/ir.rs` | reuse `Ir::fragments` for `DictSlot::Fragment` (no new table) |
| `rill-lang/src/backend/interp.rs` | the other three walkers + `value_cmp_ref`/`kind_rank` arms |
| `rill-lang/tests/type_annotations.rs` | new: `Expr::Annot` behaviour |
| `rill-lang/tests/constrained_bodies.rs` | new: a constrained body is typed once |
| `rill-lang/tests/dictionary_value.rs` | new: the value, its RC behaviour, its kind |

---

### Task 1 (Stage 1a): Expression annotations `e :: t`

**Files:**
- Modify: `rill-lang/src/ast.rs` (`Expr::Annot`), `rill-lang/src/parser.rs` (postfix `::`),
  `rill-lang/src/desugar.rs` (erase), `rill-lang/src/types/infer.rs` (pin the inner type),
  `rill-lang/src/lower.rs`, `rill-lang/src/reduce.rs`, `rill-lang/src/render.rs` (pass through)
- Test: `rill-lang/tests/type_annotations.rs` (new)

**Interfaces:**
- Consumes: `Tok::DoubleColon` (already lexed), `type_expr_to_vty`, `infer_expr_expected`.
- Produces: `Expr::Annot { expr: Box<Expr>, ty: TypeExpr, span: Span }`; parser support; erasure at desugar/lower.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn annotation_pins_a_literal()        { /* (1::Int) is Int: show_int works */ }
#[test] fn annotation_binds_looser_than_application() { /* f x :: Int is (f x) :: Int */ }
#[test] fn annotation_tighter_than_arithmetic()       { /* x + (y :: Int) parses */ }
#[test] fn mismatched_annotation_is_a_type_error()    { /* (1::Float) where Int is required */ }
#[test] fn annotation_erases_at_lowering()            { /* (1.5::Float) evaluates to 1.5 */ }
#[test] fn int_index_position_stays_int()             { /* readArrayFlat arr 1 resolves the index to Int */ }
```

- [ ] **Step 2: Run to verify they fail** — parse error on postfix `::`.

- [ ] **Step 3: Add the AST node and parse it**

`Expr::Annot { expr, ty, span }`. Parser: in the expression Pratt loop, after an expression at a binding power below application, accept `Tok::DoubleColon` and parse a `TypeExpr`. Follow Haskell 2010 §3.12 (`exp₁ :: type`) for the precedence relative to application and arithmetic.

- [ ] **Step 4: Infer, desugar, lower**

Inference: `infer_expr_expected(inner, Some(type_expr_to_vty(ty)))`; the result type is the annotation's type. Desugar and lower: erase the node (lower the inner expression). Reduce/render: transparent pass-through (render prints `e :: t`).

- [ ] **Step 5: Verify and commit**

```bash
cargo test -p rill-lang --features dsp
git commit -m 'feat(lang): expression type annotations'
```

---

### Task 1.5 (conformance): check arguments at a user-definition call

> **Why this is in this plan:** it was found while reviewing Task 1 and it is a
> Haskell 2010 violation, not a scope boundary. Per `AGENTS.md` there is no
> "pre-existing" exemption: a defect that predates the branch is still a defect.
> Verified on `91db138`:
>
> ```
> f x = x + 1.0;
> main = do { write (Just (show_float (f "str"))) stdout; return () };
> ```
> **compiles today.** A `String` argument to a `Float` parameter is accepted.
> Builtin calls do check (`sine "a" …` is rejected), so the gap is specific to
> user definitions.
>
> It also blocks Task 3: typing a constrained body is meaningless if calls into
> plain definitions never check their arguments.

**Files:**
- Modify: `rill-lang/src/types/infer.rs` (`infer_apply_impl` — unify each argument
  against `fn_param_tys[name][i]`; `TypedProgram.fn_param_tys` already exists)
- Test: `rill-lang/tests/arg_checking.rs` (new)

**Interfaces:**
- Consumes: `TypedProgram.fn_param_tys: HashMap<String, Vec<ValueTy>>` (already
  built in `infer_program_with`, ~`infer.rs:2170`), `def_schemes`.
- Produces: user-def call sites reject a mismatched argument type.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_string_argument_to_a_float_parameter_is_rejected() { /* f x = x + 1.0; f "str" */ }
#[test] fn a_float_argument_to_an_int_parameter_is_rejected()   { /* f x :: Int -> Int; f 1.5 */ }
#[test] fn a_matching_argument_is_accepted()                    { /* f x = x + 1.0; f 1.0 runs */ }
#[test] fn an_unresolved_argument_defers()                      { /* a polymorphic arg in a constrained body still works */ }
#[test] fn higher_order_argument_is_checked()                   { /* f g = g 1.0; f (\s -> s) is an error */ }
```

- [ ] **Step 2: Run to verify they fail** — the first two compile today.

- [ ] **Step 3: Unify arguments against the definition's parameter types**

At a named user-def call, look up `fn_param_tys[name]` and `unify_value` each
actual argument type against the corresponding formal. A formal that is still a
`Var` defers (do not over-constrain polymorphism); a concrete mismatch is a type
error naming the function and position. Keep the existing arity handling.

- [ ] **Step 4: Verify and commit**

```bash
git commit -m 'fix(lang): check argument types at user-definition calls'
```

---

### Task 2 (Stage 1b): `ValueTy::Dict` and `Scheme.constraints`

**Files:**
- Modify: `rill-lang/src/types/ty.rs` (`ValueTy::Dict(ClassId)`, `Scheme.constraints`),
  `rill-lang/src/types/unify.rs` (unify `Dict` by class id), `rill-lang/src/types/infer.rs` (construct schemes)
- Test: `rill-lang/tests/constrained_bodies.rs` (new)

**Interfaces:**
- Consumes: Task 1 (nothing directly; parallel work).
- Produces: `ValueTy::Dict(String)` (the class name is the id — names live in the compiler, never at runtime); `Scheme { lam_count, vars, constraints: Vec<(String, TypeVarId)>, ty }`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_constrained_scheme_carries_its_constraint() { /* `mk :: Ix i => …` has constraints == [("Ix","i")] */ }
#[test] fn dict_unifies_only_with_the_same_class()       { /* Dict("Num") ~ Dict("Num") ok; Dict("Num") ~ Dict("Eq") error */ }
#[test] fn dict_has_no_data_interpretation()            { /* type_name_of_vty(Dict) is None; kind_rank defined */ }
```

- [ ] **Step 2: Implement**

Add the variant and the field; teach `unify_value` that `Dict(a) ~ Dict(b)` iff `a == b` (never unifiable with a data type); give `type_name_of_vty`, `kind_rank`, `value_type_key` a defined answer (`None` / a stable rank / no key) rather than `unreachable!()`. Populate `Scheme.constraints` where schemes are constructed from a `TConstrained` signature.

- [ ] **Step 3: Verify and commit**

```bash
git commit -m 'feat(lang): dictionary type and scheme constraints'
```

---

### Task 3 (Stage 1c): Type a constrained body under a given constraint

**Files:**
- Modify: `rill-lang/src/types/infer.rs` (infer constrained bodies; record selections),
  `rill-lang/src/types/ty.rs` (the `ConstrainedDef` doc comment — it says the body is never inferred)
- Test: `rill-lang/tests/constrained_bodies.rs`

**Interfaces:**
- Consumes: Task 2's `Scheme.constraints` / `ValueTy::Dict`.
- Produces: constrained bodies are inferred once with the class variable bound to an abstract `Dict`; `TypedProgram` gains `dict_selections: HashMap<Span, (String, ValueTy)>` (call site → chosen `(class, concrete type)`).

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_constrained_body_is_type_checked()      { /* ill-typed body `mk :: Ix i => …` with a String error is rejected */ }
#[test] fn a_superclass_method_resolves_in_a_body()  { /* `f :: Ord a => a -> Bool` may call `==` */ }
#[test] fn no_instance_is_still_a_compile_error()    { /* a concrete type with no instance fails at compile time */ }
#[test] fn an_ix_body_still_compiles()               { /* the existing array helpers keep working */ }
```

- [ ] **Step 2: Implement**

Inferring a constrained definition: bind its class variable to `ValueTy::Dict(class)` for the body's scope, infer the body, and check the result against the declared result type. Method calls inside the body resolve against the **given** constraint (the abstract dictionary), not against a call-site type. Record each solved `(class, concrete type)` in `dict_selections` (this is what will later become a dictionary argument). Keep the existing per-call-site specialisation *working* — this task adds the typing, it does not remove the specialisation.

- [ ] **Step 3: Verify and commit**

```bash
git commit -m 'feat(lang): type constrained bodies under given constraints'
```

---

### Task 4 (Stage 2): `Value::Dictionary` and its RC handling

**Files:**
- Modify: `rill-lang/src/arena.rs` (`Value::Dictionary`, `DictSlot`, `ValueKind::Dictionary`, `visit_refs`),
  `rill-lang/src/backend/interp.rs` (`recount_children`, `drop_value_children`, `pool_clone_value`,
  `value_cmp_ref`, `kind_rank`)
- Test: `rill-lang/tests/dictionary_value.rs` (new)

**Interfaces:**
- Consumes: `Ir::fragments` (existing code table) for `DictSlot::Fragment`.
- Produces:
  ```rust
  pub enum DictSlot { Fragment { fragment: u32 }, Kernel(KernelId) }
  pub enum KernelId { /* one variant per primitive OPERATION, not per method */ }
  Value::Dictionary { supers: Vec<ArenaRef>, methods: Vec<DictSlot> }
  ```

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_dictionary_is_a_value()            { /* alloc a Dictionary, read it back */ }
#[test] fn its_supers_are_rc_children()         { /* drop the dictionary → the super's rc decrements */ }
#[test] fn it_recounts_on_copy()                { /* copy the dict → the super's rc increments */ }
#[test] fn it_clones_deeply_for_the_pool()      { /* pool_clone_value keeps supers reachable */ }
#[test] fn kind_is_dictionary()                 { /* ValueKind::Dictionary */ }
#[test] fn comparison_is_defined()              { /* value_cmp_ref on two dictionaries does not panic */ }
```

- [ ] **Step 2: Run to verify they fail** — no such variant.

- [ ] **Step 3: Implement the value and every RC path**

Add the variant, `DictSlot`, `KernelId` (one variant per primitive operation: comparison, the arithmetic family, buffer element-wise — **not** one per class method). Wire `visit_refs`, the three `interp.rs` walkers, `PartialEq`, `value_cmp_ref`, `kind_rank`, `ValueKind`. Nothing constructs a dictionary yet.

- [ ] **Step 4: Verify and commit**

```bash
git commit -m 'feat(lang): runtime dictionary value with RC support'
```

---

### Task 5.5 (conformance): Haskell type names — registry, `Char`, and the shared-type rename

> **Why:** a type that exists in both Haskell 2010 and rill must be spelled the
> Haskell way, and a name must denote a type only if that type exists. Measured
> defects: undeclared names are accepted as phantom types, and `Pair`/`List` are
> rill spellings of Haskell concepts.
>
> **Backward compatibility is explicitly NOT required** (user decision): old
> spellings are removed, not aliased.

**No sub-task of 5.5 is currently startable.** Two are done, three are blocked
on work outside this plan — measured, not assumed:

| Sub-task | Status | Blocked on |
|---|---|---|
| **5.5a** — one type-name registry; `Void` → `()` | **DONE** `59ffecd` | — |
| **5.5pre-0** — `Float` → `Double` (the name was wrong, not the precision) | **DONE** `0e78439` | — |
| **5.5pre-1** — `FixedBuffer`/`Tape` element type | **DEFERRED** | per-buffer f32/f64 pools: the element is a host monomorphisation parameter (`WorldState<T, BUF>`, `compile::<f32, BUF>`), so `FixedBuffer a` cannot honestly constrain `a` |
| **5.5b** — `Char`, `String = [Char]` | **DEFERRED** | type-level list syntax + dictionary builders: `String` is a primitive, and `String = [Char]` makes every string instance a conditional head (`instance Monoid [Char]`) |
| **5.5c** — `[a]`, `(a, b)` | **DEFERRED** | type-level list syntax + dictionary builders: `f :: [Int] -> [Int]` is not parsable today, and `instance Monoid [a]` needs a builder |
| ~~5.5pre-2~~ — `Floating`/`Integral` | **DELETED** | duplicated P3/P4 of `2026-10-25-rill-lang-typeclass-layer.md`; its unique requirement moved to P3 Step 4 |

**Consequence: the next unit of work is the dictionary stages, not 5.5.**
Everything left in 5.5 needs either per-buffer pools or dictionary builders —
that is, design `§3.7` steps 2–4 (build dictionaries at seat; pass a dictionary
as a parameter; project a slot). Doing 5.5 first would mean renaming a nominal
`List` to look like `[]` while behaving like `List`, which is worse than the
current honest `List`.

> **Ordering rule:** anything that adds a type name must land *after* 5.5a, or
> it will add a seventh `matches!` list — the exact defect 5.5a removes.

---

#### Task 5.5pre-0 (revised): `Float` → `Double` — an incorrect NAME, not an incorrect precision

> **Why:** Report §6.4.1 says `Float` is single precision and `Double` is double.
> rill's `Value::Float` is `f64`, and its measured arithmetic is exactly GHC's
> **`Double`**: `0.1 + 0.2` → `0.30000000000000004` and `1/3` →
> `0.3333333333333333` (GHC verified for `Double`; `:: Float` gives `0.3` and
> `0.33333334`). So the defect is not that `Float` is too precise — it is that the
> `Double` type is called `Float`. The fix is a **rename**, and it changes no
> numerical behaviour at all.
>
> An earlier version of this task said "demote `Float` to `f32` and add
> `Double`". That was wrong twice over: it would have changed observable results
> (breaking tests for no conformance gain) and it would have forced numeric
> defaulting to be redesigned inside a naming task. The rename is the correct
> and much smaller operation. **`Float` single precision is a separate, later
> task** (see below).

**Files:** mechanical, compiler-driven — `rill-lang/src/types/{ty.rs, infer.rs,
unify.rs}`, `arena.rs`, `ast.rs`, `lexer.rs`, `parser.rs`, `lower.rs`,
`ir.rs`, `backend/interp.rs`, `program.rs`, `builtins/*`, `types/ffi.rs`,
`render.rs`, plus every test program.

**Measured size (on `59ffecd`):** `ValueTy::Float` 221, `Value::Float` 245,
`Expr::Float`/`Tok::Float`/`LitFloat` 73, the registry string `"Float"` 57,
`Float` in test programs 647 — ~1240 mechanical replacements. The compiler finds
every site; no decision is involved at any of them.

- [ ] **Step 1: Rename the type.** `ValueTy::Float` → `ValueTy::Double`,
  `Value::Float` → `Value::Double`, `ValueKind::Float` → `ValueKind::Double`,
  `Expr::Float`/`LitFloat`/`Tok::Float` → the `Double` spellings.
- [ ] **Step 2: Rename the source-level name.** The registry entry `"Float"`
  becomes `"Double"`; `Float` then resolves to nothing and is an
  `unknown type name` error (exactly like `Char` today). No alias.
- [ ] **Step 3: `show_float` → `show_double`.** The name must not lie about its
  type. (It is rill-specific — Haskell has `show` — so this is an internal
  rename, not a conformance question.)

- [ ] **Step 4: Update every test program** that writes `Float`.
- [ ] **Step 5: Prove nothing moved.** The numeric results must be **identical**
  to before: `show_double (0.1 + 0.2)` → `0.30000000000000004`,
  `show_double (1.0 / 3.0)` → `0.3333333333333333`, both equal to GHC's
  `Double`. A changed numeric value is a bug in this task, not a fix.
- [ ] **Step 6: Commit** — `git commit -m 'refactor(lang): the double-precision type is Double, not Float'`

**Explicitly out of scope:** numeric defaulting (GHC defaults to `Double`; rill's
`Fractional` defaulting target is a separately recorded divergence),
`Floating`/`Integral`, and `Float` single precision.

**Naming note for `FixedBuffer`:** after this rename the buffer spelling is
`FixedBuffer Double`. That is correct — `FixedBuffer` is a container of
double-precision samples — and it lets 5.5pre-1 delete the host-spelling `f32`
without losing anything.

---

#### Task 5.5pre-1 (ready): `FixedBuffer` keeps its parameter; the parameter becomes real

> **Ruling (user, 2026-10-26):** `FixedBuffer` must stay polymorphic — removing
> the parameter now would be duplicate work when it returns. And there is exactly
> **one** possible instance today: **`FixedBuffer Double`**. So the defect is not
> "there is a parameter"; it is that the parameter is **discarded**. Keep the
> polymorphism, stop discarding.

**Why `Double` is the one element (measured on `2b00a05`):**

- A buffer's samples enter the language as `Value::Double` — the read path
  allocates `Value::Double(sample.to_f64())` (`interp.rs:3939`).
- `Float` does not exist in the language at all (5.5pre-0 removed the name; the
  only host sample type in use is f32: **zero** `compile::<f64>` call sites).
- Therefore `Double` is the only element a program can name or observe.

**Known and accepted:** the store is f32, so a `Double` element is a **widened**
f32. The existing test states it explicitly —
`array_buffer.rs:157`: `assert_eq!(first, Value::Double(f64::from(direct[0])))`.
This is not a conformance divergence: `FixedBuffer` has no Haskell counterpart,
so no Report rule governs its internal precision. It is also exactly why the
parameter must stay — when f64 pools (or a real `Float`) arrive, the same
`FixedBuffer Double` becomes literally exact with **no type change**.

**Measured defects to close:**

| Check | Today |
|---|---|
| `g :: FixedBuffer Int -> FixedBuffer Bool; g x = x` | **compiles and runs** — element discarded |
| `ValueTy::Buffer` | carries **no** element (89 sites in `src`) |
| `unify.rs:157` | matches `App("FixedBuffer", args)` only when `args.is_empty()` |
| `instance Num (FixedBuffer a)` | a variable-headed head; `a` means nothing while the element is discarded |
| FFI `Signal`/`Resource` | element discarded — `f32`/`Double`/`Int`/`Zork` classify identically |
| rill source | still spells the element `f32` (64 in `src`, 62 in tests) |

- [ ] **Step 1: Carry the element.** `ValueTy::Buffer` →
      `ValueTy::Buffer(Box<ValueTy>)`. **One** representation — do not also keep
      `App("FixedBuffer", [elem])` as a second shape.
- [ ] **Step 2: Check it.** `FixedBuffer a` unifies with `FixedBuffer b` iff `a`
      unifies with `b`; a buffer never unifies with a non-buffer. **Replace** the
      `args.is_empty()` arm; do not add beside it.
- [ ] **Step 3: The one legal element is `Double`.** Any other element is a type
      error naming the requirement. Keep the check data-driven (a property of the
      registry entry), so a second element is a one-line change, not a redesign —
      do not special-case `Double` in a `matches!` list.
- [ ] **Step 4: Monomorphic instance heads.** Declare
      `instance Num (FixedBuffer Double)` and
      `instance Buffer (FixedBuffer Double)` (likewise `Tape Double`) — legal
      Report §4.3.1 heads, no dictionary builder needed. The variable-headed
      `instance (Floating a) => Num (FixedBuffer a)` arrives later, when a second
      element exists, and is **additive**.
- [ ] **Step 5: The FFI layer carries the element.** `FfiParam::Signal` /
      `Resource` record it; reject a signature whose element disagrees.
- [ ] **Step 6: Migrate the source.** Every `FixedBuffer f32` / `Tape f32`
      becomes `FixedBuffer Double` / `Tape Double` (src 64, tests 62), including
      the prelude's instances and the `foreign` declarations at `ty.rs:938-939`.
- [ ] **Step 7: Delete the `data_field_vty` nominal fallback** 5.5a left so
      `FixedBuffer f32` could keep a placeholder shape; with a real element it is
      dead.
- [ ] **Step 8: Prove it.** `FixedBuffer Int` vs `FixedBuffer Bool` rejected;
      `FixedBuffer Double` works end to end (build, read the block back,
      elementwise arithmetic); FFI distinguishes elements; a wrong element is
      rejected.
- [ ] **Step 9: Commit** —
      `git commit -m 'feat(lang): the buffer element type is real and checked'`

**Why the parameter is not removed:** per-buffer f32/f64 pools make the element a
property of a buffer rather than of the program, and a JIT can later instantiate a
shape per program (a *golden gate*). Both are additive **only** if the type
already carries a checked parameter. Removing it now would be undone immediately
(user ruling).

**Unblocks:** OD-18's `FixedBuffer` half closes honestly. `Array`/`STArray`
remain open under OD-18.

---

#### Task 5.5b (DEFERRED): `Char` and `String = [Char]`

> **Deferred.** `Char` looks like "one registry entry", but `String = [Char]`
> turns `String` from a primitive into a **list of characters**, which changes
> the representation of every string in the language and every instance over it.

**Measured on `0d18b60`:**

- `Char` is a single scalar, so adding it is genuinely one registry entry plus a
  `Value` payload with no RC children. That part is cheap.
- But the Report defines `String = [Char]`, and rill's `String` is a **primitive**
  (`Value::String`, `ValueTy::String`, `TyCon`-level). Making the synonym real
  means `String` becomes `[Char]`, i.e. a list value — a representation change,
  not a name change.
- The prelude's instances immediately become conditional heads:
  `instance Monoid String` → `instance Monoid [Char]`. GHC accepts
  `instance Monoid [a]` (verified) but **rejects `instance Monoid List`**
  (`Not in scope: type constructor or class ‘List’`, verified).
- `[a]` is not even a parsable type in rill today:
  `f :: [Int] -> [Int]` → `parse error: expected type expression, found LBracket`,
  and so is `[] a`. `(Int, Double)` and `List Int` do parse.

So 5.5b cannot land before list syntax-as-type exists, and the instance heads it
implies need dictionary builders. **It belongs with 5.5c, after the same
precondition.**

---

#### Task 5.5c (DEFERRED): `[a]` and `(a, b)` — built-in type syntax, not a rename

> **Deferred.** This is not a rename. `List`/`Pair` are *nominal* constructors in
> rill; `[]` and `(,)` are **built-in type syntax** in Haskell, and the
> difference is structural.

**Measured on `0d18b60`:**

| Check | Result |
|---|---|
| `f :: [Int] -> [Int]` (type syntax) | **parse error**, `expected type expression, found LBracket` |
| `f :: [] a -> [] a` | **parse error** |
| `f :: List Int -> List Int` | parses |
| `f :: (Int, Double) -> …` | parses (tuple syntax exists) |
| `List Int` ↔ `List Bool` unification | **accepted** — element not checked (OD-18) |
| `GHC: f :: List Int -> Int` | `Not in scope: type constructor or class ‘List’` |
| `GHC: instance Monoid List` | same error |
| `GHC: instance Monoid [a]` / `Functor []` | accepted |

The real work:

1. **Type-level list syntax.** The parser must accept `[t]` as a type and
   `(t1, t2)` as a type (the latter partly exists), producing the built-in
   constructors rather than a `TApp("List", …)`.
2. **`Pair` → tuple.** `Pair a b` becomes `(a, b)`; `Pair { first, second }`
   construction/`fst`/`snd` follow. `Pair` is a nominal record today.
3. **Instance heads with a variable.** `instance Monoid [a]`, `Functor [a]`,
   `Monad [a]` replace `… List`. A head whose argument is a *variable* is a
   **conditional instance**, which needs a dictionary builder (design §3.10) —
   nothing else can express `$fMonoidList :: Monoid [a]`.
4. **Element checking.** After the change `List Int` and `List Bool` must be
   different types. Today they unify (OD-18), so the rename alone would move the
   defect rather than fix it.

**Precondition:** dictionary builders (design §3.10) and type-level list/tuple
syntax. Both must exist first; otherwise the result is a nominal `List` renamed
to look like `[]` while behaving like `List` — the worst outcome.

---

### Task 6 (close-out): Detailed Haskell 2010 conformance audit

> **User requirement (2026-10-25): after the stages are done, perform a detailed
> audit of Haskell 2010 conformance. Divergences are inadmissible — they are
> fixed, not documented.** This is the audit task: it is allowed to open new work
> items when it finds one, and it does not close until every finding is either
> fixed or promoted to a named task.

**Files:**
- Create: `docs/superpowers/audits/2026-10-25-haskell2010-conformance-audit.md`
- Modify: whatever the findings require

**Interfaces:**
- Consumes: the whole branch.
- Produces: an audit report with a verdict per area and a list of fixed items.

#### Audit areas (each gets a verdict + evidence)

| # | Area | What to check |
|---|---|---|
| A1 | **Lexical** | `--` comments, nested `{- -}`, precedence of pragmas, identifiers, literals, string escapes |
| A2 | **Syntax** | layout/offside, `where`, guards (definitions and case), `let`/`in`, `do`, `case`, patterns, sections, backticks, comprehensions, `::` on expressions, fixities §4.4.2 |
| A3 | **Declarations** | `data`/`newtype`/`type`, `typeclass` with superclasses, `instance` (incl. constraint heads), derived `Eq`/`Ord`, standalone signatures |
| A4 | **Types** | HM inference, class constraints and entailment, defaulting, the numeric tower, `Int`-vs-`Float` numeral mapping, `Ix` |
| A5 | **Expressions** | application/currying, `if`/`case` typing, `bottom` propagation, laziness in `MODE complete`, short-circuit `&&`/`||` |
| A6 | **Prelude surface** | every function the design claims: `+ - * /`, `==`, `/=`, `<`, `<=`, `>`, `>=`, `compare`, `min`, `max`, `sort`, `maximum`, `minimum`, `show`, `negate`, `abs`, `signum`, `fromInteger`, `div`, `mod`, `quot`, `rem`, `fromIntegral`, `toInteger`, `floor`, `ceiling`, `round`, `truncate`, `succ`, `pred`, `enumFromTo`, `minBound`, `maxBound`, `foldr`, `foldl`, `concat`, `sum`, `product`, `elem`, `null`, `map`, `filter`, `length`, `take`, `from` |
| A7 | **Typeclass semantics** | default methods, superclass projection, no-instance errors, recursive constrained definitions |
| A8 | **Error behaviour** | every rejection is a compile error with an actionable message; no silent wrong answers anywhere |

#### Method

- For each area, **run programs**, not just read code. The reference is
  `runghc` on GHC 9.10.3: where a program is valid Haskell 2010 it must run with
  the same value; where it is invalid, rill must reject it.
- **A silent wrong answer is the worst finding** and is fixed first.
- Every finding goes into the audit document as `FOUND → FIXED (commit)` or
  `FOUND → TASK (name)`. Nothing stays as "documented divergence".

- [ ] **Step 1: Run the audit and write the document**
- [ ] **Step 2: Fix every finding, or promote it to a named task in this plan**
- [ ] **Step 3: Re-run the full gates and the audit matrix**
- [ ] **Step 4: Commit** — `docs: Haskell 2010 conformance audit`

---

### Task 5 (Stage 2 close-out): Static-typing and no-behaviour-change audit

**Files:** tests only; `docs/superpowers/DOCS-GAPS.md`

- [ ] **Step 1: Prove no behaviour change**

Run the full suite. Expected: **974 + the new tests, 0 failures**; every pre-existing test unchanged. Any change is a regression — revert it.

- [ ] **Step 2: Prove the Review Focus items**

For each of the five Review Focus lines, name the test that pins it; add any missing one.

- [ ] **Step 3: Record the remaining stages**

Append to `DOCS-GAPS.md` a note that stages 3–8 (dictionary construction at seat, dictionary parameters, slot projection, method-as-value selectors, superclass projection, literals, deletion of the five paths) are planned but not implemented, with a pointer to the design's §3.7.

- [ ] **Step 4: Commit**

```bash
git commit -m 'test(lang): dictionary groundwork verification'
```
