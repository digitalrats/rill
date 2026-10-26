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

### Task 5.5 (conformance): Rename types shared with Haskell to their Haskell names

> **Why:** a type that exists in both Haskell 2010 and rill must be spelled the
> Haskell way, otherwise the same program text means something different from
> what a Haskell programmer expects. This is a naming conformance requirement,
> not cosmetics — the inventory (`docs/superpowers/audits/2026-10-26-haskell2010-conformance-inventory.md`)
> lists `Pair`/`List` as shared concepts spelled rill's way.
>
> **Backward compatibility is explicitly NOT required** (user decision), so the
> old spellings are removed, not aliased.

**Files:** wherever the names occur — `rill-lang/src/types/ty.rs` (arithmetic of
type constructors), `ast.rs`, `parser.rs`, `lower.rs`, `types/infer.rs`,
`render.rs`, `program_serde.rs`, the prelude text, and every test/program that
uses them. This is a mechanical rename with a large blast radius; the compiler
finds every site.

**Interfaces:**
- Consumes: nothing.
- Produces: Haskell spelling for shared types; rill-only types keep their names.

#### The mapping (decided here, applied by the task)

| rill today | Haskell 2010 | Action |
|---|---|---|
| `Pair a b` | `(a, b)` | **rename to the tuple type**; `Pair { first, second }` literals/projections become `( , )` and `fst`/`snd` |
| `List a` | `[a]` | **rename**; list literal `[1,2,3]` and `:` already exist |
| `String` | `[Char]` (`String` is a synonym) | keep `String` as the synonym, add `Char` |
| `Maybe`, `Either` | same | keep |
| `Set`, `Map`, `Array`, `STArray`, `ST`, `IO`, `Kleisli` | same (from `Data.*`/`Control.*`) | keep |
| `FixedBuffer a`, `Tape a`, `Buffer b`, `InputStream a`, `OutputStream a` | **no Haskell equivalent** | keep (rill-specific) |
| `Int`, `Float`, `Bool`, `Void` | same (`Void` is `()`) | keep; `()` as the unit type |

- The **constructors** follow: `Pair { first, second }` → tuple syntax; a
  two-field record that is genuinely a named record (`EqBand`) is NOT a tuple
  and keeps its record syntax.
- `show_float`/`show_int`/`show_bool` are rill-specific and stay; `show` is a
  separate gap (inventory A1).

#### Scalar type names — measured, and two defects to fix

rill has exactly four scalar type names in `ValueTy` (`Int`, `Float`, `Bool`,
`String`) plus `Void`. Probed on `b2ba351`:

| Name | `vty_of_name` | Haskell 2010 | Verdict |
|---|---|---|---|
| `Int` | `Int` | `Int` | OK |
| `Float` | `Float` | `Float` (single precision) | OK |
| `Bool` | `Bool` | `Bool` | OK |
| `String` | `String` | `[Char]` synonym | OK (kept as a synonym) |
| `Void` | `Data("Void", [])` | `()` | **WRONG** — rill spells unit `Void`; Haskell uses `()`, and rill ALSO parses `()`, so two names mean the same thing while `Void` is a phantom |
| `Char` | `Data("Char", [])` | `Char` | **MISSING** — no `Char` value exists, yet the name type-checks |
| `Double` | `Data("Double", [])` | `Double` | **EXTRA** — accepted, types a program, but no value can ever have it |
| `Integer`, `Word`, `Ordering` | `Data(…)` | real types | **EXTRA** — same phantom acceptance |

The two defects:

1. **Phantom type names.** Any identifier parses as a type at a signature, and an
   undeclared name becomes `Data(name, [])`. So `f :: Integer -> Integer; f x = x`
   **compiles and runs** although no `Integer` value exists. This is a hole in
   static typing: it must be a compile error, or the type must exist for real.
2. **`Void` vs `()` for unit.** Haskell 2010 spells the unit type `()`. rill
   accepts both, and `Void` is a phantom while `()` resolves correctly.

Decisions for the task:

- `()` is the unit type; `Void` is removed (no alias — backward compatibility is
  not required).
- `Char` becomes a **real** type with character literals `'a'`, and `String`
  stays `[Char]`.
- **Every other name that is not declared must be a compile error**, not a
  phantom `Data`. That is the general fix, and it also closes the `Double` /
  `Integer` / `Word` / `Ordering` acceptance. `Double` may later be added as a
  real type; it is *not* silently accepted in the meantime.

#### Staging (measured on `5421744`)

The rename is far larger than one dispatch: `Pair` 284 in `src` + 271 in tests,
`List` 262 in `src`, `Void` 73. It is split into three independently green
sub-tasks, in this order:

| Sub-task | Scope | Why this order |
|---|---|---|
| **5.5a** | phantom type names → compile error; `Void` → `()` | closes a static-typing hole; tiny blast radius |
| **5.5b** | `Char` as a real type + character literals; `String = [Char]` | needs 5.5a first (otherwise `Char` is just another phantom) |
| **5.5c** | `List a` → `[a]`, `Pair a b` → `(a, b)` | purely mechanical, largest radius, independent of a/b |

- [ ] **Step 1: Inventory the exact sites**

```bash
grep -rn 'Pair' --include='*.rs' rill-lang/src rill-lang/tests | wc -l   # ~271 uses in tests
grep -rn '"Pair"' --include='*.rs' rill-lang/src | wc -l
grep -rn 'List' --include='*.rs' rill-lang/src | wc -l
```
Record the counts in the report before touching anything.

- [ ] **Step 2: Rename `Pair` → tuple, `List` → `[]`**

Compile-driven: change the type-constructor registry and the AST/type
representation, then fix each compile error. The prelude and every test program
are updated. No aliases left behind.

- [ ] **Step 3: Add `Char` and keep `String` as its synonym**

`String = [Char]` per the Report; a character literal `'a'` is part of this (the
inventory notes rill has no `Char` at all).

- [ ] **Step 4: Prove nothing else moved**

Run the full suite; every remaining failure must be a *deliberate* part of the
rename, listed in the report. Anything else is a regression.

- [ ] **Step 5: Commit**

```bash
git commit -m 'refactor(lang): spell shared types the Haskell 2010 way'
```

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
