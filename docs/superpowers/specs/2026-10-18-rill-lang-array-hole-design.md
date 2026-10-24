# rill-lang array: polymorphic construction via a shared lazy hole (M8)

Status: design (approved shape, v2)
Date: 2026-10-18
Branch: `feature/rill-lang-arrays`
Related: `docs/superpowers/specs/2026-10-17-rill-lang-arrays-design.md` (the parent
array design this amends), review item **PARK M8** of the whole-branch review.

## Intent

`array` is currently Float-only because it fills unlisted positions with `0.0` —
the language has no way to conjure a value of an arbitrary element type
(`ty.rs:566`). This design makes `array` polymorphic over the element type
(`Array i e`) by adopting the GHC model for unlisted positions: a **shared lazy
hole** whose *forcing* is a clean runtime error, not a construction-time value.

The construction moves wholesale into new/reshaped interpreter primitives. This
is deliberate and load-bearing: it removes Ix method dispatch and offset
plumbing from the language prelude construction paths, simplifies the `index`
dual-duty node, and shrinks the surface a future JIT must compile.

## Decisions (confirmed with the user, 2026-10-18)

- **Q1 — index-shape policy:** `Ix` is an open class and rill supports
  user-declared instances (`instance Ix Foo`). The primitives implement offset
  arithmetic only for the two prelude instances (`Int`, `Pair Int Int`).
  Anything else is a **clean runtime fault at construction**
  (`"array*: unsupported index type"`), never silent mis-flattening. The
  language signatures stay open (`Ix i => …`); the runtime restriction is a
  documented limitation.
- **Q2 — scope of the takeover:** full — all four construction helpers
  (`array`, `accumArray`, `update`, `accum`) stop computing offsets in the
  language. `arrayUpdate`/`arrayAccum` change signature from flat
  `List (Pair Int e)` to index-typed `List (Pair i e)` and resolve offsets
  against the stored bounds internally.
- **Q3 — exactness contract:** the primitives reproduce the `Ix` instance
  bodies exactly: row-major order, wrapping i64 arithmetic (matching
  `exec_value_arith`), `inRange` check *before* offset arithmetic (an
  out-of-rectangle cell with an accidentally in-range flat offset must be
  rejected), and inverted bounds produce a valid **empty** array rather than a
  fault. Explicit tests for `(0,-1)` and `(5,3)` bounds.

## Goals

- `array :: Ix i => Pair i i -> List (Pair i e) -> Array i e` (polymorphic element).
- Unlisted positions hold **one shared lazy object** per array (GHC
  `error "undefined array element"` model): forcing it is a clean
  `ProcessError::processing`, at read time, not build time.
- Construction moves from the prelude into primitives that do all bounds and
  offset work internally — no `checkedOffset`, no runtime `index` method
  dispatch, no `range`/`map` scaffolding in any construction def.
- ST form (`newArray`/`newListArray`/`writeArray`/`readArrayFlat`) is untouched:
  flat cells, no holes (GHC standard, `newArray` takes an explicit init).

## Non-goals / anti-goals

- **No user-visible hole term.** No `hole`/`undefined` identifier, no parser or
  syntax change. The hole exists only as a runtime artifact of lazy
  construction (each `array` build allocates one shared fault object; a
  `Value::Callable` / lowered fragment whose body latches the fault).
- **No per-position diagnostics.** The fault message is the generic
  "array: undefined array element" with no coordinate. Position-tagged
  diagnostics are a separate larger body of work (error/observability
  machinery).
- **No change to `listArray`/`newListArray` strictness.** They stay on
  `arrayFromListChecked` / `newArrayFromListFlat` with their clean length fault.
- **No lazy elements in STArray.** ST cells remain flat values.
- **No bottom-like value semantics change beyond the array hole.** The existing
  `Value::Bottom` propagation stays as-is; the hole is a *callable* whose force
  latches a fault, not `Bottom` (see Background).

## Background: what already exists

- The value runtime is lazy at the representation level: every unevaluated
  expression is a `Value::Callable { fragment, env, cell }` in the arena
  (`interp.rs:4945`), forced by `force()` to WHNF (`interp.rs:4932`). A plain
  value `init` *is* a (nullary-in-practice) callable; a function is a callable
  with more applied arguments — the "value vs function" distinction is a
  property of *when/how they are forced*, not of the heap object.
- `arrayFromList` builds a `Value::Array { bounds, elems }` where
  `elems: Vec<ArenaRef>` are slots; it recounts each element ref but does
  **not** force them (`interp.rs:2684-2725`) — an immutable array already stores
  lazy thunk refs.
- `force()` on `Value::Bottom` is silent: `Bottom` is a non-callable leaf
  ("already WHNF"). The hole must therefore NOT be `Bottom` — it must be a
  callable whose body latches a fault, so forcing produces a message-bearing
  `ProcessError::processing`, not a silently propagating bottom.
- RC/COW helpers (`drop_value_children` / `recount_children`, `interp.rs:426`,
  `640`) treat `Value::Callable` and `Value::ThunkCell` uniformly, so shared
  callable slots in an array need no special case.
- ST cells are `StCell::Array { bounds, elems }` with their own flat
  `Vec<ArenaRef>`; `newArrayFlat` fills every cell with an init copy
  (`interp.rs:3690-3713`); `writeArrayFlat`/`readArrayFlat` operate slot-wise.
- The `ArrayUpdate | ArrayAccum` interpreter arm (`interp.rs:2916`) reads flat
  offsets via `flat_assoc` + `flat_index_in_range` and builds a NEW array
  (source never mutated), replacing (update) or folding (accum) one element per
  association. Every failure keeps its own message (`"arrayUpdate: …"`,
  `"arrayAccum: …"`) through the clean fault channel.
- The prelude computes offsets TODAY with `checkedOffset b ix = if inRange b ix
  then index b ix else -1` (`ty.rs:636`), used by `array`, `accumArray`,
  `update`, `accum`. The read surface (`!` via `index`, `assocs`, `ixmap`,
  `indices`) also uses it; those read paths stay in the language.

## Design

### 1. Primitive surface

New/changed value builtins (names builder-decidable; family convention kept):

```
arrayFromAssocs :: Ix i => Pair i i -> List (Pair i e) -> Array i e   -- NEW
arraySeed       :: Ix i => Pair i i -> e              -> Array i e     -- NEW
arrayUpdate     :: Ix i => Array i e -> List (Pair i e) -> Array i e   -- CHANGED: flat → index-typed assocs
arrayAccum      :: Ix i => Array i e -> (e -> e -> e) -> List (Pair i e) -> Array i e  -- CHANGED: flat → index-typed
arrayFromList      :: Ix i => Pair i i -> List e -> Array i e          -- unchanged (ixmap)
arrayFromListChecked :: Ix i => Pair i i -> Int -> List e -> Array i e -- unchanged (listArray)
arrayIndexFlat     :: Array i e -> Int -> e                            -- unchanged (!)
arrayBounds, arrayElems                                                 -- unchanged
```

- `arrayFromAssocs b xs`: resolves each assoc to its flat offset **inside the
  interpreter**, last-wins on duplicates, unlisted positions get the shared
  hole. This is the new `array`.
- `arraySeed b init`: every position of a fresh array is `init` (the lazy
  callable passed as the element). This is the base for `accumArray` — the
  seeding counterpart of the hole constructor. Shared slot machinery with
  `arrayFromAssocs` (fill strategy differs: hole vs init).
- `arrayUpdate a xs` / `arrayAccum a f xs`: resolve assoc offsets against the
  **stored bounds** of `a` internally; `arrayAccum` folds `f old new` as today.
  Their flat signatures change because the language no longer produces offsets.

The builtin-sig registration table (`ty.rs:1324-1390`) currently has
`flat_assocs = List (Pair Int e)`; both `arrayUpdate` and `arrayAccum` switch to
`assocs_ie = List (Pair i e)` with the same `forall i e` scheme, and the two new
primitives are added with that shape.

### 2. The shared hole

- One `Value::Callable` (or a lowered fragment / thunk cell) whose body, when
  forced, latches `ProcessError::processing("array: undefined array element")`.
- All unlisted slots share the SAME ref — one allocation per array, exactly the
  GHC sharing model. No per-slot objects, no env payload, no position info.
- Forcing happens at read time (when an element is forced to WHNF downstream),
  never at construction. Constructing an array with holes costs one object
  allocation regardless of range size.
- It is NOT `Value::Bottom`. (The interpreter may internally reuse `Bottom`
  after latching, as existing fault paths do — implementation detail.)

### 3. Index-shape discrimination & offset arithmetic (Q1 + Q3)

The interpreter receives no per-call-site type info; it discriminates the index
type by the **value shape** of the bounds operand, which is itself `Pair i i`:

- The bounds pair's first component is `Int` → index type `Int`, 1-D:
  `index = i - lo`, size `hi - lo + 1`.
- The bounds pair's first component is a 2-field record (`Pair Int Int`) →
  index type `Pair Int Int`, 2-D, row-major (last coordinate fastest):
  `index = (i1 - lo1) * (hi2 - lo2 + 1) + (i2 - lo2)`,
  size `(hi1 - lo1 + 1) * (hi2 - lo2 + 1)`. The 2-D arithmetic is inlined (the
  instance is deliberately self-contained — no dispatch to the `Ix Int`
  sibling — `ty.rs:594`); the Rust helper mirrors that.
- Any other shape → clean fault at construction:
  `"arrayFromAssocs: unsupported index type"` (and the matching op name for
  `arrayUpdate`/`arrayAccum`/`arraySeed`).
- Any other shape → clean fault at construction:
  `"arrayFromAssocs: unsupported index type"` (and the matching op name for
  `arrayUpdate`/`arrayAccum`/`arraySeed`).
- **Exactness contract (Q3), mirroring the language instance bodies**
  (`ty.rs:585-605`) and `checkedOffset`:
  - All arithmetic is **wrapping i64**, identical to `exec_value_arith`
    (`wrapping_*`) — the primitives must not use checked/overflowing ops in a
    way that diverges from the language.
  - `inRange` check runs BEFORE offset arithmetic: an out-of-rectangle cell
    (e.g. `(0, 3)` in a 3×3 grid — flat offset 3 collides with the next row's
    first element) is rejected via the negative-offset sentinel, never silently
    read. This preserves the current "wrapped read the storage layer cannot
    detect" protection.
  - Inverted bounds: a negative computed size means a **valid empty array**
    (0 slots, bounds stored verbatim), matching today's `array (0,-1) []` /
    `array (5,3) []` behavior (empty range → empty array, any read faults).
  - Consistency: because construction offsets and read offsets (`!` via
    `checkedOffset` in the language) follow the same formulas, an array built by
    `arrayFromAssocs` reads correctly element-wise through `!`.
- **Caveat (accepted):** shape discrimination cannot distinguish a `newtype`
  that wraps `Int`/`Pair Int Int` from the prelude types themselves; such an
  instance flattening with the standard row-major arithmetic reads correctly.
  A pathological custom Ix on the same *value* shape that computes a *different*
  offset would build with prelude arithmetic — a doc'd limitation of the
  shape-based runtime, unavoidable without runtime type info, and out of scope
  (would require type-erasure-lifting that neither the current `Ix` nor the
  value runtime provides).

### 4. Prelude rewrite (`types/ty.rs`)

```haskell
-- array assocs: unlisted positions are one shared lazy hole (error on force).
array b xs :: Ix i => Pair i i -> List (Pair i e) -> Array i e = arrayFromAssocs b xs;

-- accumArray: seed every position with `init`, then fold associations.
accumArray f init b xs :: Ix i => (e -> (e -> e)) -> e -> Pair i i -> List (Pair i e) -> Array i e
                        = arrayAccum (arraySeed b init) f xs;

-- (//) — apply (index, value) associations, last wins, source untouched.
update a xs :: Ix i => Array i e -> List (Pair i e) -> Array i e = arrayUpdate a xs;

-- accum f — fold associations into an existing array.
accum f a xs :: Ix i => (e -> (e -> e)) -> Array i e -> List (Pair i e) -> Array i e = arrayAccum a f xs;
```

- `array`'s element type widens `Float → e`; its Float-only fallback scaffolding
  (`map (\k -> 0.0) (range b)`, `checkedOffset` wrapping) disappears.
- `accumArray` seeds with the lazy `init` value (already a callable in the value
  runtime — the "value is a lambda" observation) via `arraySeed`, then folds via
  `arrayAccum`. No `range`, no `checkedOffset` in construction.
- `update`/`accum` become one-line wrappers; their association lists are passed
  through **unwrapped** (index-typed), offsets resolved against the stored
  bounds inside the primitives.

### 5. What disappears from the language

- All four construction paths stop calling `checkedOffset` and `range`.
- The flat `List (Pair Int e)` contract for `arrayUpdate`/`arrayAccum` is
  replaced by index-typed `List (Pair i e)`.
- `array`'s `0.0` scaffolding and `accumArray`'s `map (\k -> init)` scaffolding
  are gone (replaced by `arrayFromAssocs`' hole and `arraySeed`).

### 6. What stays in the language (reads)

- `checkedOffset` (`ty.rs:636`) remains for read paths: `index a ix` (`!`,
  `ty.rs:619`), `assocs`, `ixmap`, `indices`. These support user-declared `Ix`
  instances via ordinary method dispatch — reads stay open-world.
- `arrayFromList` stays for `ixmap`; `arrayFromListChecked` for `listArray`;
  `arrayIndexFlat` for `!`'s storage read.
- Asymmetry (accepted with Q1): you can *read* a custom-index array (`listArray`
  + `!`) but not *construct* one with `array`/`accumArray`/`update`/`accum`
  until the interpreter grows support for the instance's shape.

### 7. ST form — unchanged

- `newArray`/`newListArray`: flat cells, explicit init/list, strict length
  check — no holes, no shared thunks.
- `writeArray`/`readArrayFlat`: slot-wise on `StCell::Array`.
- `freeze` (`runSTArray`): copies cell values; the immutable result never
  inherits an ST-side hole (there are none). If a callable ever appears in an ST
  slot (should not, by the above), RC accounting is already uniform.

### 8. JIT note

Moving construction to builtins reduces the lowering surface a JIT must compile
(one opcode instead of a fragment composition), removes runtime Ix method
dispatch from the hot construction path, and keeps shared-slot memory layout
flat — all JIT-friendly. This design does not implement a JIT; it is a stated
precondition that makes one cheaper later.

## Testing

- `array` is polymorphic: `array (Pair (0,0) (1,1)) [(Pair (0,0) 1.5), (Pair (1,1) 2.5)] :: Array (Pair Int Int) Float` and a `List`-element array (`Array i (List Float)`) build and query.
- Hole semantics: reading a listed position yields the element; reading an
  unlisted position is a clean `ProcessError::processing` (message
  "array: undefined array element") — not a silent value, not `Bottom`.
- Unsupported index type (Q1): an `array`/`accumArray`/`update`/`accum` over a
  non-`Int`/non-`Pair Int Int` index shape faults at construction with
  "unsupported index type"; reads on the same array (`listArray` + `!`) still
  work.
- Out-of-range association index faults at construction (bounds check), message
  distinct from the hole read.
- Edge bounds (Q3): `array (0,-1) []` and `array (5,3) []` build valid
  0-element arrays; any read faults; out-of-rectangle 2-D cell `(0,3)` in a 3×3
  grid is rejected (no wrapped read).
- Wrapping (Q3): construction offsets match `!` reads under i64 wraparound
  (e.g. bounds spanning `Int` min/max if such a range is constructible).
- Duplicate offsets: last wins (existing `arrayUpdate` semantics preserved).
- Large holes are cheap: constructing an array over a wide range with few assocs
  does not force/visit the unlisted cells.
- Existing suite green: `accumArray`, `update`, `accum`, `listArray`, `ixmap`,
  `indices`, `assocs`, `!` pass.
- Non-regression: `array` with `Float` elements still works.

## Migration / compatibility

- `array`'s element type widens `Float → e`. Existing tests that type
  `Array i Float` via `array` remain valid (Float unifies). Tests that relied on
  the `0.0`-fill *value* for unlisted positions (expecting a silent `0.0`) must
  be updated to expect the clean hole fault — the intended behavior change,
  matching GHC.
- `arrayUpdate`/`arrayAccum` signature change (flat → index-typed assocs):
  direct primitive tests in `tests/array_storage.rs` pass `Pair { first: N, … }`
  where `N` was a flat offset. For 1-D `Int` arrays with lower bound 0 the
  number means the same under the new contract (index `N` → offset `N`), so
  those tests keep passing unchanged; their *meaning* becomes index-based. Any
  test using non-unit lower bounds or 2-D flat offsets must be rewritten in
  index terms.
- No parser, AST, or type-system surface change beyond the prelude definitions
  above and the value builtin registration table.
- ST writers used `newArray leak init` patterns already (init explicit) — no ST
  test changes expected.

## What is left to the builder

- Exact primitive names (`arrayFromAssocs`, `arraySeed` are suggestions; the
  family is `arrayFromList`, `arrayFromListChecked`, `arrayUpdate`,
  `arrayAccum`).
- How the fault callable is constructed in the interpreter: a hand-built
  `Value::Callable` vs a lowered fragment that latches a fault (whichever is
  least special-casing; must carry no env / no position).
- Exact fault messages and where they latch.
- Whether `arrayFromAssocs`/`arraySeed` reuse `pooled_elems_of`-style slot
  machinery or allocate their own slot Vec (alignment with the existing
  pooled-slot helpers).
- How the index-shape helper is factored to serve all four primitives
  (`arrayFromAssocs`, `arraySeed`, `arrayUpdate`, `arrayAccum`) without
  duplicating the Q3 arithmetic.