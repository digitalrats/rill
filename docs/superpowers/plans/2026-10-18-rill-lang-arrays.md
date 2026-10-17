# rill-lang: Data.Array, STArray, Ix, bang patterns, buffer bridge — Implementation Plan

**Spec:** `docs/superpowers/specs/2026-10-17-rill-lang-arrays-design.md` (final, user-approved).
**Branch:** `feature/rill-lang-arrays` (already off `develop`).
**Method:** TDD, one failing-first test per sub-step, gate after every task.
**Gates after every task:** `cargo test -p rill-lang --features dsp` green,
`cargo clippy -p rill-lang --features dsp --all-targets` zero warnings,
`cargo fmt -- --check`. Code comments and docs in **English**. Do **not** touch
`src/arrow.rs` or `src/graph/reconstruct.rs` (standing ruling).

---

## Task 0 — commit the staged cleanup (prep, no code)

The branch already carries a staged cleanup from the previous work:

- `M .gitignore` (adds `.superpowers/`)
- `D  rill-lang/tests/graph_reconstruct.rs` (removed probe test)

Commit it now so later commits are clean:

```
git add .gitignore rill-lang/tests/graph_reconstruct.rs
git commit -m 'chore(rill-lang): drop probe/reconstruct tests; ignore .superpowers/'
```

Optional (ask user): commit the design doc + this plan together:
`docs/superpowers/specs/…-arrays-design.md` and `docs/superpowers/plans/…-arrays.md`
(`docs/superpowers/` is **not** gitignored; only the root `.superpowers/` is).

---

## Task 1 — Split `->` / `=>` lexer tokens (`Tok::ThinArrow`)

**Goal.** `=>` becomes lexically distinct from `->` so a constrained signature
can be parsed without ambiguity (design §Constrained signatures, builder-check Q1).
Today both map to one `Tok::FatArrow` (`lexer.rs` lines 267–282).

**User directive (rules this task):** prioritize **Haskell 2010** syntax in all
cases; **backward compatibility is not required**. So: `->` is the function type
arrow, the lambda arrow, AND the case-alternative arrow; `=>` is reserved for
class constraints (instance heads and the new `=>` signature contexts).

**Files.** `src/lexer.rs`, `src/parser.rs`, `src/types/ty.rs`,
`tests/io_streams_memory.rs`.

**Interfaces.**
- `lexer.rs`: add `Tok::ThinArrow`. The `->` branch (currently emitting
  `FatArrow` at lines 275–282) emits `Tok::ThinArrow`; the `=>` branch
  (lines 267–274) keeps emitting `Tok::FatArrow`.
- `parser.rs` — switch these `FatArrow` sites to `ThinArrow` (all of them mean
  `->` under Haskell 2010):
  - `parse_type_expr` function-type arrows (lines ~633, 635).
  - the lambda arrow (`\x -> body`, line ~1022).
  - the match-arm and guard arrows (lines ~975, 982, 988) — the parser no
    longer accepts `=>` in match-arm position.
  Keep `FatArrow` ONLY for the class-constraint context:
  `parse_instance_def` constraint list (line ~743) and (in later tasks) the
  `=>` signature context.
- **Backward-compat conversions** (user: not required — but the fixed preludes
  must keep parsing, so do them): convert the legacy `=>` match-arm spellings to
  `->`:
  - `src/types/ty.rs` `STD_PRELUDE` instance bodies (lines ~382, 387):
    `case m of { Nothing => Nothing; Just x => … }` → `->`.
  - `tests/io_streams_memory.rs` (lines ~55, 148–150, 173–175, 213–215, 235,
    249, 266–268, 290–292): match arms `=>` → `->`.
  - Grep the whole crate for `=>` in match-arm position to catch any missed
    site. Do NOT touch `instance … => …` constraint contexts (those stay `=>`).
- Update the lexer test vectors that mention `FatArrow` (lines ~700, 731, 814)
  to the correct arrow token.
- The renderer already emits `->` for function types, lambdas and match arms
  (`render.rs`) — no change needed there; round-trips confirm.

**TDD.**
1. Add a lexer unit test: `kinds("a -> b")` yields `Tok::ThinArrow` at the arrow
   position; `kinds("a => b")` yields `Tok::FatArrow`. Red (both are FatArrow).
2. Implement the split; fix the parser sites; run the `=>` → `->` conversions;
   existing parser/integration tests stay green (function types, lambdas, match
   arms, instances all still parse; the preludes still load).
3. Gate + commit: `feat(rill-lang): split '->'/'=>' into ThinArrow/FatArrow tokens`

---

## Task 2 — `!`: bang patterns and infix indexing (Tok::Bang, adjacency, `index` apply, ValueForce)

**Goal.** `!` plays both roles per the design (§`!`): infix `a ! i` desugars to
`index`; attached `!x` is a bang marker in pattern/param/binding position. The
rule is the unary-minus span-adjacency model (`next.span.start == tok.span.end`).

**Files.** `src/lexer.rs`, `src/parser.rs`, `src/ast.rs`, `src/lower.rs`.

**Interfaces.**
- `lexer.rs`: rename `Tok::Cut` → `Tok::Bang` (the `b'!'` branch, line ~464);
  update test vectors that name `Cut`. `!=` already lexes as `Tok::NotEq`
  (line ~291) — untouched.
- `parser.rs`:
  - New `InfixOp::Index`. In `infix_binding_power`, `Tok::Bang` maps to
    `(InfixOp::Index, 15, 16)` — tight, just below application, above `*`.
  - The operator loop (line ~853) must consult token adjacency before treating
    `Tok::Bang` as an operator. New helper mirroring `peek_is_negative_literal`
    (line ~136): `fn is_attached_bang(toks: &[Token], pos: usize) -> bool`
    (`next.span.start == toks[pos].span.end && is_pattern_start(&next.tok)`).
    An **attached** `!` in expression/operator position → parse error
    (`a!i`, `a !i`); a **separated** `!` → `InfixOp::Index`.
  - Operator-loop arm for `InfixOp::Index`:
    `Expr::Apply { name: "index".into(), args: vec![lhs, rhs], span }`
    (same shape as `InfixOp::Cons` → `cons`, line ~888).
  - `!` stays out of `is_atom_start` / `is_juxtaposition_arg_start` (it begins
    no expression).
  - Bang marker parsing (prefix, **attached**): in pattern position
    (`parse_pattern`/case arms), `!` + pattern → `Pattern::Bang(..)`; in param
    position (`f !x = …`, `\ !x -> …`) → `Param { bang: true }`; in let-binding
    (`let !x = e`) → `Def::Eval { bang: true }`.
- `ast.rs`:
  - `Pattern::Bang(Box<Pattern>)` (variant on the existing `Pattern` enum).
  - `Param { name, span, bang: bool }` (update all constructors/matchers).
  - `Def::Eval { …, bang: bool }`.
- `lower.rs`: a bang param/pattern/binding emits the existing
  `ValueInstr::ValueForce { dst, src }` on the incoming value before binding
  (the same instruction already emitted for case scrutinees / `if` conditions).
  Semantics = WHNF forcing, no deep strictness.

**TDD.**
1. Parser: `a ! i` → `Expr::Apply{name:"index", args:[a,i]}`; `a ! b ! c`
   left-assoc; `a ! i + 1` ≡ `(a ! i) + 1`; `a!i` and `a !i` are parse errors.
2. Parser: `f !x = …` → `Param.bang == true`; `\ !x -> …`; `let !x = e`;
   `case e of { !x -> … }` → `Pattern::Bang(Var x)`.
3. Lowering/interp: a program with `f !x`, `\ !x`, `let !x`, `case … of { !x }`
   forces to WHNF — observable through a lazy/shared thunk scenario.
4. Gate + commit:
   `feat(rill-lang): bang patterns and infix '!' indexing (Tok::Bang, adjacency disambiguation)`

---

## Task 3 — `TypeExpr::TConstrained` + `=>` signature contexts

**Goal.** `Def::Eval` signatures can carry a Haskell-style class context:
`listArray : Ix i => …`, `ixmap : (Ix i, Ix j) => …` (design §Constrained
signatures). The `=>` token already exists (Task 1 made it distinct).

**Files.** `src/ast.rs`, `src/parser.rs`, `src/render.rs`,
`src/types/infer.rs`, `src/types/ty.rs`.

**Interfaces.**
- `ast.rs`: new variant
  `TypeExpr::TConstrained(Vec<(String, String)>, Box<TypeExpr>)` — each
  `(class_name, type_var)` constraint, then the body type. Delegate
  `arg_count` / `arg_types` / `sig_arg_count` to the inner type (add arms to
  the existing impl at lines 28–55).
- `parser.rs`: in signature parsing (the `:: TypeExpr` path in `parse_top_def`,
  line ~444), parse an optional leading constraint context:
  - `Ix i => T` → `TConstrained([("Ix","i")], T)`
  - `(Ix i, Ix j) => T` → `TConstrained([("Ix","i"),("Ix","j")], T)`
  Mirror the constraint-list parsing already in `parse_instance_def`
  (lines ~725–743, which produces `Vec<(String,String)>`).
- Every other `match` on `TypeExpr` gains a `TConstrained` arm delegating to
  the inner type. Find them with `grep -rn "TypeExpr::" src/` (render,
  signature conversion `type_expr_to_vty`, `signature_param_tys_conv`,
  `class_var_arity`, `class_var_arg_index`, `arg_count` impl, …).

**TDD.**
1. AST unit: `TConstrained([..], inner).arg_count()` delegates to `inner`.
2. Parser: `f : Ix i => Pair i i -> List e -> Array i e` → sig is
   `TConstrained([("Ix","i")], TFunc(..))`; the `(Ix i, Ix j) =>` form parses
   to two constraints.
3. Render round-trips a constrained sig.
4. All existing (unconstrained) sigs still parse/type-check.
5. Gate + commit: `feat(rill-lang): TypeExpr::TConstrained with '=>' signature contexts`

---

## Task 4 — Head-arg-aware instance matching (full-type instance keys + dispatch recovery)

**Goal.** `Ix Int`, `Ix (Pair Int Int)` and future `Ix (Pair Int (Pair Int Int))`
coexist and dispatch distinctly — the registry keys by the **full applied index
type**, and the arity-0 method dispatch recovers the class var by **unifying the
method's signature parameter pattern against the concrete argument type**
(design §Constrained signatures Q2/Q4; frozen decision 8).

**Facts already verified (read the code before starting).**
- `InstanceInfo.ty` is the instance head's full type string as parsed
  (`ty.rs` `Def::Instance` arm, lines 981–994) — `Ix (Pair Int Int)` → `"Pair Int Int"`,
  so the registry **already** distinguishes full types; the collision is only in
  the **dispatch key**.
- The arity-0 method dispatch (lower.rs lines 1964–2066) selects the instance by
  `type_name_of_vty(selector)` — the selector's **top-level head name**
  (`Pair`), which cannot tell `Ix Int` from `Ix (Pair Int Int)`. This is the bug.
- `class_var_arg_index` (`ty.rs` lines 1147–1158) returns `None` for a bare
  class var (`Ix i`, like `Eq a`): no method argument is `TApp("i", _)`.

**Files.** `src/types/ty.rs`, `src/lower.rs`, `src/types/infer.rs`.

**Interfaces.**
- `ty.rs`: add `pub(crate) fn class_var_param_pattern(sig, class_var) -> Option<usize>`
  — the index of the first method argument whose type **mentions** the class var
  (for `index : Pair i i -> i -> Int` with `class_var = "i"` → `0`). This is the
  argument to unify against.
- `lower.rs`: in the arity-0 dispatch path (lines 1964–2066), replace the
  top-level-name key with a **full-type** key:
  1. find the method's sig for the class (`TypeEnv` has it),
  2. take the `class_var_param_pattern` argument,
  3. unify that argument's **signature pattern** (`Pair i i`) against the
     selector's concrete value type (`Pair Int Int` → `i := Int`) to solve the
     class var, reusing the existing unification machinery
     (`signature_param_tys_conv` / the same substitution path constraint
     discharge uses),
  4. render the solved class var's **full** type (`"Int"` / `"Pair Int Int"` /
     `"Pair Int (Pair Int Int)"`) and call `resolve_method(method, full_key)`.
- Backward-compat check: for classes whose class var **is** the selector's
  top-level type (`Eq a`, `Ord a` — `eq x y`, `x : Float`), the unification
  gives the same key as today (`"Float"`); existing Eq/Ord/Monoid dispatch stays
  green. If the class var is bare in the selector (`eq : a -> a -> Bool`),
  unify the bare var directly.

**TDD** (probe class — no real `Ix` yet; define a throwaway prelude/`data`
  fragment in a test file):
1. Registry: instances `C Int`, `C (Pair Int Int)`, `C (Pair Int (Pair Int Int))`
   all register under distinct full-type keys; `resolve_method("m", "Pair Int (Pair Int Int)")`
   returns the right body.
2. Dispatch: a probe class `C a { size : Pair a a -> Int }` with instances for
   `Int` and `Pair Int Int`; `size (0,9)` → `C Int` body; `size ((0,0),(2,2))`
   → `C (Pair Int Int)` body.
3. Existing `Eq`/`Ord`/`Monoid` dispatch unaffected (suite green).
4. Gate + commit:
   `feat(rill-lang): head-arg-aware instance dispatch via class-var unification`

---

## Task 5 — Constrained (`=>`) defs: sig-driven typing, deferred bodies, call-site specialization

**Goal.** `listArray : Ix i => …`-style functions actually work: the signature
drives parameter typing, the body is **not** eagerly inferred, and each call
site **specializes** the body with the concrete index type and the bound `Ix`
instance (design §Constrained signatures Q3; frozen decision 7).

**Facts already verified (read the code before starting).**
- `Def::Eval.sig` is currently decorative: `infer_def_body` (infer.rs line 1436)
  never reads it; params default to `Float` (line ~1481–1484).
- `prelude_defs` are inferred **eagerly** at load (`infer.rs` lines 1149–1165).
- `validate_instances` (line 989) **skips** instance bodies that carry
  `head_args`/`constraints` — the exact deferral pattern to mirror.

**Files.** `src/types/ty.rs`, `src/types/infer.rs`, `src/lower.rs`.

**Interfaces.**
- `ty.rs`: on `TypeEnv`, add a constrained-def registry:
  ```rust
  pub constrained_defs: HashMap<String, ConstrainedDef>,
  pub struct ConstrainedDef {
      pub sig: TypeExpr,            // a `TConstrained` node
      pub params: Vec<Param>,
      pub body: Expr,
      pub where_defs: Vec<Def>,
  }
  ```
  Populated during prelude registration and `register_decls` for any `Def::Eval`
  whose `sig` is a `TConstrained`.
- `infer.rs`:
  - `infer_def_body`: when `def.sig()` exists, type params from the sig's arg
    types (function-type body) instead of defaulting to `Float`; when the sig is
    `TConstrained`, register the def in `constrained_defs` and **skip** eager
    body inference (return a placeholder) — same spirit as the `validate_instances`
    skip. Keep the existing retry/rerate machinery working for non-constrained
    defs (do not disturb `fibs`-style recursion).
  - At a call site to a constrained def, discharge `Ix i` against the concrete
    type through the existing `bind_constraint_instance` (infer.rs line 760) /
    `bind_head_args_and_discharge` (lower.rs) path — the same one instance-head
    constraints use. A constraint that cannot be discharged is a **compile
    error** (never a runtime dictionary).
- `lower.rs`: call-site specialization — when lowering `Apply(name, args)` where
  `name` is in `constrained_defs`, inline the body with the class var
  substituted to the concrete index type and the `Ix` instance bound (the same
  β-substitution model typeclass method inlining uses). Inside the body, Ix
  method calls (`rangeSize b`) dispatch via Task 4's full-type recovery.

**TDD** (probe constrained defs before real arrays exist; a minimal probe class
  + defs in a test prelude fragment):
1. A constrained def `mk : C i => Pair i i -> e -> …` registers without eager
   body inference (no error at load).
2. `mk (0,9) v` (Int) and `mk ((0,0),(2,2)) v` (Pair) each specialize to the
   correct instance; the body's method calls resolve by Task 4.
3. `mk` called with a non-instanced index type → compile error.
4. Existing unconstrained prelude defs still infer eagerly (suite green).
5. Gate + commit:
   `feat(rill-lang): constrained '=>' defs — sig-driven typing, deferred bodies, call-site specialization`

---

## Task 6 — `Value::Array` + ArenaValue arms + ctor_kinds + `STArray` abstract decl

**Goal.** The immutable `Array i e` value type exists in the arena; `Array` and
`STArray` are registered type constructors (design §Value-level design, §Array
and STArray types).

**Files.** `src/arena.rs`, `src/types/ty.rs`, `src/lexer.rs`/prelude sources in
`ty.rs` (STD_PRELUDE string).

**Interfaces.**
- `arena.rs`: new variant on `Value`:
  ```rust
  Value::Array {
      bounds: ArenaRef,        // an ordinary value: `Pair i i` (Int bounds or a Pair bounds)
      elems:  Vec<ArenaRef>,   // flat, row-major for multi-dim, lo..=hi order
  }
  ```
  `ArenaValue` impl: `visit_refs` walks `bounds` then each `elems`; `clone_pooled`
  and `detach_payload` get an `Array` arm alongside `List`/`Set`/`Map`.
- `ty.rs`: `ctor_kinds` (lines 613–623) += `("Array", 2)` and `("STArray", 3)`.
- `STD_PRELUDE` (ty.rs line ~353): add the abstract declaration
  `data STArray s i e;` (like `STRef`). `Array` is a builtin collection type
  (no prelude `data` decl), exactly like `List`.

**TDD.**
1. Unit: hand-build a `Value::Array`, exercise `visit_refs` (both bounds and
   elems visited), `clone_pooled`, `detach_payload`.
2. `with_builtins` registers `Array` (kind 2) / `STArray` (kind 3); `STD_PRELUDE`
   still parses with the `STArray` abstract decl.
3. Gate + commit:
   `feat(rill-lang): Value::Array arena variant and Array/STArray type constructors`

---

## Task 7 — Immutable array storage primitives (ValueBuiltinOp + builtin_sigs + interp)

**Goal.** The flat, type-agnostic immutable-array ops run in the interpreter
(design §Storage primitives, immutable list).

**Files.** `src/ir.rs`, `src/types/ty.rs`, `src/lower.rs`, `src/backend/interp.rs`.

**Interfaces.**
- `ir.rs`: `ValueBuiltinOp` +=
  - `ArrayFromList` — `(Pair i i, List e) → Array i e` (store bounds + elems)
  - `ArrayBounds` — `Array i e → Pair i i`
  - `ArrayIndexFlat` — `(Array i e, Int) → e` (bounds-checked; clean fault)
  - `ArrayUpdate` — `(Array i e, List (Pair Int e)) → Array i e` (new array)
  - `ArrayAccum` — `(Array i e, (e→e→e), List (Pair Int e)) → Array i e` (new)
  - Buffer bridge: `ArrayFromBuffer` — `FixedBuffer f32 → Array Int Float`
    (copy, lo=0, hi=len−1); `BufferFromArray` — `Array Int Float → FixedBuffer f32`
    (copy) — both interpreter-level (program buffer table access).
- `ty.rs`: `builtin_sigs` entries for the user-facing names:
  `arrayFromList`, `arrayBounds`, `arrayIndexFlat`, `arrayUpdate`,
  `arrayAccum`, `arrayFromBuffer`, `bufferFromArray` — polymorphic over
  `i`/`e` with `TForall` (the same helper style as the `st`/`tv`/`forall_s`
  closures, lines 745–783); **unconstrained** (flat ops, no Ix math).
- `lower.rs`: wire each name → `ValueBuiltinOp` (mirror the existing
  `fromList`/`toList` name→op wiring around lines 1322–1500).
- `interp.rs`: implement the ops in the arena; `ArrayIndexFlat` out-of-range →
  the existing latched fault channel (`latch_fault` → `ProcessError`), never a
  panic (design §Fault policy).

**TDD.**
1. `arrayFromList b xs` builds an array; `arrayBounds`/`arrayElems` read it back.
2. `arrayIndexFlat` in-range returns the element; out-of-range is a clean error
   (no panic), asserted via the fault channel.
3. `arrayUpdate`/`arrayAccum` return **new** arrays; the original is unchanged
   (immutability).
4. Buffer bridge round-trip: `bufferFromArray (arrayFromBuffer b)` equals `b`
   contents.
5. Gate + commit: `feat(rill-lang): immutable array storage primitives and buffer bridge`

---

## Task 8 — STArray cells + ST effects + `runSTArray` freeze

**Goal.** Mutable arrays inside `runST`, frozen to immutable `Array` only via
`runSTArray` (design §Value-level design mutable; frozen decision 3).

**Files.** `src/ir.rs`, `src/types/ty.rs`, `src/program.rs`,
`src/backend/interp.rs`, `src/lower.rs`.

**Interfaces.**
- `ir.rs`: `ValueEffectOp` +=
  - `NewArrayFlat` — `(Pair i i, Int, e) → ST s (STArray s i e)` (bounds + flat
    size + init element; the language computes the size via `rangeSize`)
  - `ReadArrayFlat`, `WriteArrayFlat`, `ModifyArrayFlat` — flat offsets
  - `GetArrayBounds`, `GetArrayElems` — whole-cell reads (no Ix math)
- `ty.rs`: `builtin_sigs` entries for `newArrayFlat`, `readArrayFlat`,
  `writeArrayFlat`, `modifyArrayFlat`, `getBounds`, `getElems` — `TForall`
  over `s`, `i`, `e`, mirroring the STRef sigs (lines 753–783); plus the rank-N
  `runSTArray : (forall s. ST s (STArray s i e)) -> Array i e` exactly like
  `runST` (lines 718–733). All unconstrained.
- `program.rs` / `interp.rs`: ST cell storage — extend the existing STRef cell
  table (or add a sibling table, constrained by the region/escape machinery
  already in place) with an array-cell variant `{ bounds: ArenaRef,
  elems: Vec<ArenaRef> }`; implement the effect ops mutating the cell in place.
- `lower.rs`: wire the names → `ValueEffectOp` (mirror STRef wiring, lines
  1241–1290); handle `runSTArray` like `runST` (rank-2 application, line ~1193).
- **Freeze** (`runSTArray`): refcount transfer — the cell table lives in the
  shared arena, so freezing bumps the refcount of each element `ArenaRef` and
  the bounds ref, then drops the region cell. **No element copies.**

**TDD.**
1. Inside `runST`: `newArrayFlat b n e` allocates; `readArrayFlat` /
   `writeArrayFlat` / `modifyArrayFlat` read-modify-write; `getBounds` /
   `getElems` read the whole cell.
2. `runSTArray (\s -> …)` freezes to an immutable `Array`; the result is usable
   after the region closes.
3. Region-escape: returning the `STArray` out of `runST`/`runSTArray` is a
   **compile-time** error (port the `STRef` escape test from `st_monad.rs`).
4. Freeze by refcount transfer: after freeze, the array reads correctly and the
   region cell is released (behavioral check via a multi-element array).
5. Gate + commit: `feat(rill-lang): STArray cells, ST effects, runSTArray freeze`

---

## Task 9 — STD_PRELUDE: `Ix` class + instances + constrained array/STArray surface

**Goal.** The full user-facing Data.Array surface, plus the `Ix` typeclass and
instances (design §API surface, §Type-level design). All in the preludes
(user: **no** separate `ARRAY_PRELUDE` — Data.Array is folded into
`STD_PRELUDE`; `Ix` the class lives in `CATEGORY_PRELUDE`).

**Files.** `src/types/ty.rs` (the `CATEGORY_PRELUDE` and `STD_PRELUDE` string
constants).

**Interfaces.**
- `CATEGORY_PRELUDE`: add
  ```haskell
  typeclass Ix i where {
      range:    Pair i i -> List i;
      index:    Pair i i -> i -> Int;
      inRange:  Pair i i -> i -> Bool;
      rangeSize: Pair i i -> Int;
  }
  ```
  (`rangeSize` via the typeclass `defaults` mechanism or per-instance — open to
  the builder; the `defaults` map exists in `TypeclassInfo`.)
- `STD_PRELUDE`: add
  - `instance Ix Int` — `range (l,u) = <list l..u>; index (l,u) i = i - l;
    inRange (l,u) i = (l <= i) && (i <= u); rangeSize (l,u) = u - l + 1;`
  - `instance Ix (Pair Int Int)` — **self-contained** row-major bodies
    (design §`=>`, Q1): `index ((l1,l2),(u1,u2)) (i1,i2) =
    (i1-l1)*(u2-l2+1)+(i2-l2); inRange … = (l1<=i1)&&(i1<=u1)&&(l2<=i2)&&(i2<=u2);
    rangeSize ((l1,l2),(u1,u2)) = (u1-l1+1)*(u2-l2+1); range = <componentwise
    nested list>;`
  - The constrained language-level functions (exact sigs from the design
    §API surface / §Primitives vs language-level):
    `array`, `listArray`, `accumArray`, `index`, `bounds`, `indices`, `elems`,
    `assocs`, `accum`, `ixmap`, `range`, `inRange`, `rangeSize`
    (immutable), and `newArray`, `newListArray`, `readArray`, `writeArray`,
    `modifyArray` (ST side), plus `arrayFromBuffer`, `bufferFromArray` (bridge).
    Bodies do the Ix math and call the flat primitives (Tasks 7–8).
  - Naming: def names must be `Tok::Ident` (`parse_top_def`, line ~419), so the
    operator-shaped API names are exposed as alphanumeric defs — the `:`→`cons`
    precedent. `a ! i` already desugars to `index` (Task 2). The update operator
    `(//)` is exposed as `update` (the constrained offset-computing function);
    the flat primitive keeps the name `arrayUpdate`. `listArray` validates
    `length elems == rangeSize (l,u)` (clean error, strict — design §API
    surface).

**TDD.**
1. `ix.rs`-style: `range`/`index`/`inRange`/`rangeSize` for `Int` (incl. negative
   and zero-length bounds) and `Pair Int Int` (row-major offsets, empty dim).
2. Immutable surface: `listArray` (valid + wrong-length clean error), `array`,
   `accumArray`, `!`/`index`, `bounds`, `elems`, `indices`, `assocs`, `update`
   (`//`), `accum`, `ixmap` (matrix transpose); 1-D and 2-D.
3. ST surface: `newArray`/`newListArray`/`readArray`/`writeArray`/`modifyArray`
   inside `runST`; freeze via `runSTArray`; immutability of the frozen result.
4. Bridge: `arrayFromBuffer` reads the buffer f32 contents; `bufferFromArray`
   writes them back; round-trip equals the original.
5. Gate + commit:
   `feat(rill-lang): Data.Array in STD_PRELUDE — Ix class, instances, constrained surface`

---

## Task 10 — Integration test targets + matrix demo

**Goal.** Dedicated test targets per area, written failing-first, covering the
design §Testing outline, plus the matrix demo.

**Files (new).** `rill-lang/tests/`, gated `--features dsp`:
- `ix.rs` — Ix math (Int + Pair Int Int), edge bounds.
- `array_immutable.rs` — full immutable surface incl. fault-policy errors
  (out-of-range `!` → clean error), `ixmap` transpose.
- `st_array.rs` — STArray ops in `runST`; **compile-time** region-escape test;
  `runSTArray` freeze + use-after-freeze; direct mutation of the frozen array
  stays immutable.
- `array_buffer.rs` — bridge round-trips.
- `bang_patterns.rs` — WHNF strictness semantics (`f !x`, `\ !x`, `let !x`,
  `case … of { !x }`); `!` infix indexing still parses in expression position.
- `array_matrix.rs` — reference demo: 2×2/3×3 `Array (Pair Int Int) Float`,
  index, `assocs`, transpose via `ixmap`, accumulation.

**TDD.** One test file at a time, failing-first; each new file is its own commit:
`test(rill-lang): arrays — <area> (<…>)`.

**Final gate** (full feature): `cargo test -p rill-lang --features dsp` green,
`cargo clippy -p rill-lang --features dsp --all-targets` zero warnings,
`cargo fmt -- --check`.

---

## Left to the builder (from the spec)

- `array`'s default fill value for uncovered indices (e.g. `0.0`, or a required
  explicit initial element).
- `rangeSize` via typeclass `defaults` vs per-instance body.
- ST cell storage shape (extend the STRef cell table vs a sibling table).
- Whether `Eq`/`Ord` get structural `Array` instances this stage (cheap; include
  if it does not disturb the Eq/Ord resolution path).
- Whether to commit `docs/superpowers/` (design + plan) with this feature.
