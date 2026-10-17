# rill-lang: Haskell Data.Array — immutable & mutable (STArray), Ix, bang patterns, buffer bridge — Design

> **Stage:** new capability layer on top of the io-streams-core re-scope (real
> `IO`/`ST`/`STRef`, `STD_PRELUDE`). Branch: `feature/rill-lang-arrays`.
> **Prerequisites:** the lazy value track (`Value::List`/`Map`/`Set`/`Buffer`/
> `Opaque`/`BuiltinCallable`, `Arena`), the ST region machinery with `runST`
> and `STRef` (region-phantom escape = compile-time error), the typeclass
> system (`CATEGORY_PRELUDE` / `STD_PRELUDE`, `builtin_sigs`, `ctor_kinds`),
> the lowering op wiring (`ir.rs ValueOp` / `ValueEffectOp`), the interpreter
> arena with `ValueForce`.
> **Scope:** Data.Array as an **independent analog** (not a Buffer wrapper) +
> mutable STArray + the `Ix` class + bang patterns + the Buffer bridge — plus
> the compiler machinery that makes them first-class: `=>` constrained sigs,
> unification-based typeclass method dispatch, head-arg-aware instance
> matching. Matrix math (dot/mult) and deeper conveniences are follow-on work.

## Intent and why

The user wants the Data.Array discipline in rill-lang: immutable, index-typed,
bounded arrays as the language's first-class data structure for

- interacting with signal **buffers** (create / extract / math over buffer
  contents),
- **static data** for filters, effects and generators (coefficients, wavetables,
  LUTs),
- the foundation for **pure math**, including matrices.

The adjective **independent** is deliberate: arrays are not a thin wrapper over
`Buffer`; they are their own value type that *bridges* to buffers. Arrays are
strict (the interpreter is strict/call-by-need with WHNF only — GHC-style
lazy array elements do not carry over; this is a design call).

## Goals / Non-goals / Anti-goals

**Goals**
- `Ix` typeclass + `Array i e` immutable value type, full `Data.Array` surface.
- Mutable `STArray s i e` in ST regions, frozen to `Array i e` via `runSTArray`.
- Infix `!` indexing with tight precedence, and `!` bang patterns (strictness).
- Buffer bridge: `Array Int Float` ⇄ `FixedBuffer f32` by copy.
- Design extensible to more index types and higher dimensions without
  rearchitecting (only new `Ix` instances are added).

**Non-goals (this stage)**
- `unsafeFreeze` / `unsafeThaw` (freeze is **only** via `runSTArray`).
- `Data.Array.Unboxed`/`UArray`, the `MArray` typeclass (STArray ops are
  concrete, not class-dispatched).
- Matrix algebra (dot, mult, inverse) — build *on* arrays later.
- `show` for `Array`, or `Array` in serialization/JSON.

**Anti-goals**
- Arrays must never be confused with mutable `Buffer` semantics: an immutable
  `Array` is never mutated in place; every update produces a new array.
- No panic / UB on out-of-range access: a clean runtime error, consistent with
  the arena capacity fault policy (`ArenaError::CapacityExceeded`). (Haskell
  `error`/bottom is deliberately not reproduced; see Fault policy.)

## Frozen decisions (user)

1. **Full `Data.Array`** as an independent analog.
2. **`STArray` in scope** (mutable arrays in ST).
3. **Freeze only via `runSTArray`** — no `unsafeFreeze`/`unsafeThaw`.
4. **`Ix` instances: `Int` and `Pair Int Int` now**; the design must anticipate
   other index types and higher dimensions (e.g. `Pair Int (Pair Int Int)`,
   `Integer`, `Char`) with **no architecture change** — only new instances.
5. **Infix `!`** is the indexing operator (`a ! i`), and **`!` is also the bang
   pattern** marker; the dead `Tok::Cut` token is renamed/reused or replaced.
6. (provisional, pending user check) Matrix math is a follow-on; first iteration
   is the Data.Array + STArray + bridge surface.
7. **`=>` support is added to `TypeExpr`** (class contexts on signatures) —
   resolved from builder-check Q2. Array/STArray functions carry real
   Haskell-style constrained sigs (`listArray : Ix i => …`); constrained
   functions are **specialized per concrete instantiation at the call site**
   (compile-time β-substitution, the same model as typeclass method inlining) —
   zero runtime dispatch is preserved. See the dedicated section below.
8. **Head-arg-aware instance matching is in scope** (builder-check Q4; user:
   not deferred) — the instance registry keys by the full applied index type,
   so `Ix Int`, `Ix (Pair Int Int)` and future `Ix (Pair Int (Pair Int Int))`
   coexist without colliding on the `Pair` head.

## Constrained signatures (`=>` in `TypeExpr`)

Builder-check surfaced the load-bearing constraint: array functions need
Haskell-style constrained signatures (`listArray : Ix i => …`), but `TypeExpr`
had no representation for a class context. **Decision (user): add `=>` support
to `TypeExpr` and to signature parsing.** This is a compiler-level extension
built on machinery that already exists:

- **Lexer** (builder-check Q1) — today both `->` and `=>` map to one
  `Tok::FatArrow` (`lexer.rs` lines 267-282), so a signature `Ix i => T` is
  byte-identical to the function type `Ix i -> T`. The design **splits the
  token**: `Tok::ThinArrow` (`->`) for function types and lambdas; `Tok::FatArrow`
  (`=>`) reserved for constraints and match-arm separators. The split is
  mechanical and behavior-preserving — every existing site already uses `->` for
  types/lambdas and `=>` for match arms / instance heads.
- **Parser** — `parse_instance_def` already parses a parenthesized constraint
  list `(C a, D b) =>` into `Vec<(String, String)>` (`parser.rs`). The same
  shape is added to signature parsing: a `Def::Eval` sig may start with a
  constraint context `Ix i => T` or `(Ix i, Ix j) => T`, producing the
  `TConstrained` node directly (no reinterpretation needed once the token is
  split).
- **AST** — a new `TypeExpr::TConstrained(Vec<(String, String)>,
  Box<TypeExpr>)` variant (class name + type variable per constraint, then the
  body). Every match on `TypeExpr` (render, `arg_count`, signature conversion,
  …) delegates to the inner type.
- **Inference** — the constraints are recorded with the function's type; at
  each call site the type variable is unified and the constraints discharged
  through the existing `bind_constraint_instance` (`infer.rs`) /
  `bind_head_args_and_discharge` (`lower.rs`) path — the same one instance-head
  constraints already use. A constraint that cannot be discharged against a
  concrete type is a **compile error**, never a runtime dictionary.
- **Lowering** — a constrained function is **specialized per concrete
  instantiation at the call site**: its body is inlined with the index type
  substituted and the `Ix` instance bound, exactly like typeclass method
  inlining. Zero runtime dispatch is preserved. The feature is general (usable
  by user code, not just arrays); constrained *builtin* sigs are not needed —
  the storage primitives are unconstrained flat ops (see below).
- **Method dispatch for type-variable classes** (builder-check Q2) — the
  existing arity-0 dispatch selects the instance by the selector argument's
  TOP-LEVEL type name (`type_name_of_vty` → `Pair`), which cannot recover the
  class var from `Pair i i`-shaped arguments: every `Ix` method call would
  resolve to `Ix (Pair Int Int)` and `Ix Int` would be unreachable (1-D arrays
  would break). Method dispatch for type-variable classes (`Ix i`, like
  `Eq a`) gains **unification-based class-var recovery**: unify the method's
  parameter pattern (`Pair i i`) against the concrete argument type
  (`Pair Int Int` → `i := Int`), then resolve the instance by the bound
  variable's **full** type. This is the load-bearing extension that makes
  `Ix`-as-typeclass actually dispatch; it mirrors what `bind_constraint_instance`
  already does for constraints.
- **Signatures become load-bearing** (builder-check Q3) — `Def::Eval.sig` is
  currently decorative (`infer_def_body` ignores it; params default to `Float`)
  and `prelude_defs` are inferred EAGERLY at program load. The constrained array
  functions therefore require: (a) the sig drives parameter typing and records
  the constraints; (b) a constrained def's body is NOT inferred eagerly — it is
  deferred and specialized per concrete call site, exactly as `validate_instances`
  skips bodies that carry `head_args`/`constraints`; (c) `runSTArray`'s rank-2
  `forall s.` goes through the existing rank-N `builtin_sigs` path — it is
  **unconstrained**, mirroring `runST`, so "no constrained builtin sigs" holds.
- **Ix instance bodies stay self-contained** (builder-check Q1): a
  `Ix (Pair Int Int)` body is written with the component arithmetic inlined
  (`inRange ((l1,l2),(u1,u2)) (i1,i2) = (l1<=i1)&&(i1<=u1)&&(l2<=i2)&&(i2<=u2)`),
  so no same-class sibling dispatch is needed — the bodies are obviously
  correct and the extensibility claim needs no special-case reasoning.

## `!` — two syntactic roles

The lexer currently maps `!` → `Tok::Cut` (line 464, `lexer.rs`). `Tok::Cut` is
**dead**: `parser.rs` never consumes it (the old signal surface is removed); the
only remaining references are `arrow.rs` (legacy docs, not to be touched per the
standing ruling) and `graph/reconstruct.rs` (out of scope). So `!` is free to
reuse.

- **Rename** `Tok::Cut` → `Tok::Bang` (lexer), and
- **Infix indexing** `a ! i` — parsed through `infix_binding_power` as a
  left-associative, tight operator (precedence just below application, Haskell
  `!` is prec 9): `a ! i + 1` ≡ `(a ! i) + 1`.
- **Bang pattern** — a **prefix** `!` at the start of a pattern / parameter /
  binding: `case e of { !x -> … }`, `f !x = …`, `\ !x -> …`, `let !x = e in …`.

**Disambiguation — the unary-minus model (user).** The lexer emits a single
token for `!`; the parser splits bang vs index by **token adjacency**, exactly
the way `-` distinguishes `-1.0` (an attached negative literal) from infix
subtraction via span equality (`peek_is_negative_literal`, `parser.rs`:
`next.span.start == tok.span.end`):

- `!` directly attached to the following symbol (`!x`, `!Just y` — no
  whitespace between the `!` and the next token) → **bang marker** in
  pattern/param/binding position.
- `!` separated (whitespace around it) → the **infix indexing operator**
  (`a ! i`).

The conventional writing falls out naturally: bang is `!x` (attached), index is
`a ! i` (spaced) — mirroring `-1.0` vs `a - b`. Orthography consequences
(builder-check Q5): `a ! i` and chained `a ! b ! c` are valid indexing; `a!i`
and `a !i` attach `!` to a following identifier and are therefore bang markers
in expression position → **parse error** (bang is always attached, index is
always spaced — that is the accepted trade-off). Infix `!` desugars in the
operator loop to `Expr::Apply { name: "index", args: [lhs, rhs] }` — `!` cannot
be a def name, so it is a reserved alias for the `index` function. `!` stays out
of `is_atom_start` / `is_juxtaposition_arg_start` (it begins no expression).
(`!=` already lexes as `Tok::NotEq` before the bare-`!` case, so it is
unaffected.)

**AST**
- `Pattern::Bang(Box<Pattern>)` — for `case`-arm patterns (`!x` forces the
  scrutinee then binds `x`; the language already forces `case` scrutinees in
  `lower.rs`, so `!x` on a `Var` is semantically idempotent there but must parse).
- `Param { name, span, bang: bool }` — for function/lambda params `f !x = …`,
  `\ !x -> …`: the argument is forced on application entry.
- `Def::Eval` gains a `bang: bool` for `let !x = e` (force the RHS at binding).

**Lowering** — reuses the existing `ValueInstr::ValueForce { dst, src }`
(already emitted for case scrutinees and `if` conditions): a bang binding/param
emits a `ValueForce` on the incoming value before binding; a bang let-binding
forces the RHS at the binding point. Semantics = Haskell strict fields / bang
patterns (force to WHNF, no deep strictness).

## Type-level design

### `Ix` typeclass — `CATEGORY_PRELUDE`

Per the standing ruling `CATEGORY_PRELUDE = only typeclasses + Kleisli`, the
`Ix` class is declared there (a typeclass, like `Eq`/`Ord`):

```haskell
typeclass Ix i where {
    range:    Pair i i -> List i;
    index:    Pair i i -> i -> Int;
    inRange:  Pair i i -> i -> Bool;
    rangeSize: Pair i i -> Int;
}
```

`rangeSize` may use the typeclass default mechanism (default = length of
`range`) or be defined per instance (implementer's choice; the TypeclassInfo
`defaults` map exists).

### `Array` and `STArray` types — array layer

Registered as **builtin collection types** (like `List`/`Map`/`Set` in
`with_builtins`, `ctor_kinds`):

- `ctor_kinds["Array"] = 2` — `Array i e`, a first-class immutable value type.
- `ctor_kinds["STArray"] = 3` — `STArray s i e`, an **abstract opaque** type
  (declared `data STArray s i e;` in the prelude like `STRef`): nameable, not
  constructible/projectable/matchable in source; only the effect ops create
  handles; the region phantom `s` prevents escape.

Placement: folded into `STD_PRELUDE` (user: no separate constant). `STD_PRELUDE`
is the standard-library prelude (`STRef`, the `ST`/`IO` monads, the stream
contract types), and Data.Array belongs there. It holds the `STArray` abstract
declaration (`data STArray s i e;`, like `STRef`), the `Ix Int` /
`Ix (Pair Int Int)` instances, and the language-level array functions. The `Ix`
typeclass itself stays in `CATEGORY_PRELUDE` (the ruling: CATEGORY = only
typeclasses + Kleisli); `with_builtins` already registers CATEGORY before STD,
so the instances see their class.

Instances:

```haskell
instance Ix Int where {
    range (l, u)     = <list l..u>;
    index (l, u) i   = i - l;
    inRange (l, u) i = (l <= i) && (i <= u);
    rangeSize (l, u) = u - l + 1;
}
instance Ix (Pair Int Int) where {
    -- row-major over ((l1,l2),(u1,u2)); self-contained bodies (no sibling
    -- instance dispatch — see the `=>` section)
    index ((l1, l2), (u1, u2)) (i1, i2) = (i1 - l1) * (u2 - l2 + 1) + (i2 - l2);
    inRange ((l1, l2), (u1, u2)) (i1, i2) =
        (l1 <= i1) && (i1 <= u1) && (l2 <= i2) && (i2 <= u2);
    rangeSize ((l1, l2), (u1, u2)) = (u1 - l1 + 1) * (u2 - l2 + 1);
    range  = <componentwise nested list>;
}
```

(`(i, i)` type sugar already desugars to `TApp("Pair", …)`; value tuples to
`Pair { first, second }`.)

### Primitives vs language-level functions (with `=>`)

The user-facing array and STArray operations are **language-level constrained
functions** in `STD_PRELUDE`: their signatures carry the real `Ix i =>`
context and their bodies do the Ix math, calling unconstrained flat storage
primitives. The primitives are the only `builtin_sigs` /
`ValueOp` / `ValueEffectOp` entries (pattern: `fromList` / `toList` in
`ty.rs`). Constrained *builtin* sigs are not needed — a primitive has no body
to specialize, and every primitive takes a flat offset / size or a whole list.

Constrained language-level functions (ST side):

```haskell
newArray    : Ix i => Pair i i -> e -> ST s (STArray s i e)
newListArray: Ix i => Pair i i -> List e -> ST s (STArray s i e)
readArray   : Ix i => STArray s i e -> i -> ST s e
writeArray  : Ix i => STArray s i e -> i -> e -> ST s ()
modifyArray : Ix i => STArray s i e -> i -> (e -> e) -> ST s ()
```

Their bodies compute offsets / sizes via `Ix` and call the flat primitives:
`readArray arr i = do { b <- getBounds arr; readArrayFlat arr (index b i) }`,
`newArray b e = newArrayFlat b (rangeSize b) e` — the language computes the
flat size, so allocation needs no interpreter-side Ix math. The language has no
runtime typeclass dictionaries; the `Ix i =>` context is discharged at each
call site by specialization (see the `=>` section above).

Unconstrained flat primitives (direct `builtin_sigs`, no Ix math):

```haskell
-- immutable
arrayFromList   : Pair i i -> List e -> Array i e
arrayBounds     : Array i e -> Pair i i
arrayElems      : Array i e -> List e
arrayIndexFlat  : Array i e -> Int -> e
arrayUpdate     : Array i e -> List (Pair Int e) -> Array i e
arrayAccum      : Array i e -> (e -> e -> e) -> List (Pair Int e) -> Array i e
-- mutable (ST effects, mirrored from STRef)
getBounds       : STArray s i e -> ST s (Pair i i)
getElems        : STArray s i e -> ST s (List e)
newArrayFlat    : Pair i i -> Int -> e -> ST s (STArray s i e)
readArrayFlat   : STArray s i e -> Int -> ST s e
writeArrayFlat  : STArray s i e -> Int -> e -> ST s ()
modifyArrayFlat : STArray s i e -> Int -> (e -> e) -> ST s ()
runSTArray      : (forall s. ST s (STArray s i e)) -> Array i e   -- rank-N builtin, like runST
-- buffer bridge
arrayFromBuffer : FixedBuffer f32 -> Array Int Float
bufferFromArray : Array Int Float -> FixedBuffer f32
```

## Value-level design

### Immutable array

New variant in `arena.rs`:

```rust
Value::Array {
    /// Stored bounds as an ordinary value (a `Pair i i` record: `Int` bounds
    /// or a `Pair Int Int` bounds pair for matrices).
    bounds: ArenaRef,
    /// Flat elements, row-major for multi-dim, in `lo..=hi` order.
    elems:  Vec<ArenaRef>,
}
```

- Boxed and strict: elements are arena refs (like `Value::List`).
- `ArenaValue`: `visit_refs` walks `bounds` + `elems`; `clone_pooled` and
  `detach_payload` get an `Array` arm alongside `List`/`Set`.
- `bounds` stored as an ordinary value means `bounds :: Array i e -> Pair i i`
  is a trivial primitive read and the same machinery serves every future index
  type with zero interpreter changes.

### Mutable STArray

Like `STRef`, a mutable cell behind an **opaque handle**:
- `Value::Opaque(idx)` (or a distinct kind tag) resolves into the ST region's
  cell table; a cell holds `{ bounds: ArenaRef, elems: Vec<ArenaRef> }`.
- The interpreter's ST cell storage is extended (the existing STRef cell table
  gets an array-cell variant, or a sibling table — implementer's choice,
  constrained by the region/escape machinery already in place).
- Effects mutate the cell in place; `runSTArray` freezes the region into an
  immutable `Value::Array` by **refcount transfer** (builder-check Q4): the cell
  table lives in the shared arena, so freezing bumps the refcount of each
  element `ArenaRef` and the bounds ref, then drops the region cell — no element
  copies.

### Storage primitives

Only **flat, type-agnostic** primitives live in the interpreter; every index
computation is done in the language via `Ix` (see Index math):

`ir.rs ValueOp` (pure):
- `ArrayFromList` — `(Pair i i, List e) → Array i e` (store bounds + elems).
- `ArrayBounds` — `Array i e → Pair i i`.
- `ArrayIndexFlat` — `(Array i e, Int) → e` (bounds-checked; clean error).
- `ArrayUpdate` — `(Array i e, List (Pair Int e)) → Array i e` (new array).
- `ArrayAccum` — `(Array i e, (e→e→e), List (Pair Int e)) → Array i e` (new).
- Buffer bridge: `ArrayFromBuffer` (`FixedBuffer f32 → Array Int Float`),
  `BufferFromArray` (`Array Int Float → FixedBuffer f32`) — both copy, both
  interpreter-level (the program's buffer table / `RegionHandle` access).

`ir.rs ValueEffectOp` (ST effects, mirrored from STRef):
- `NewArrayFlat` — `(Pair i i, Int, e) → ST s (STArray s i e)` (bounds +
  flat size + init element; the language computes the size via `rangeSize`).
- `ReadArrayFlat`, `WriteArrayFlat`, `ModifyArrayFlat` — flat offsets.
- `GetArrayBounds`, `GetArrayElems` — whole-cell reads (no Ix math).

## API surface

### Immutable `Data.Array` (faithful surface; `!` = infix index)

```haskell
array      : Ix i => Pair i i -> List (Pair i e) -> Array i e
listArray  : Ix i => Pair i i -> List e -> Array i e
accumArray : Ix i => (e -> e -> e) -> e -> Pair i i -> List (Pair i e) -> Array i e
(!)        : Ix i => Array i e -> i -> e          -- infix; also available as `index`
bounds     : Array i e -> Pair i i
indices    : Ix i => Array i e -> List i
elems      : Array i e -> List e
assocs     : Ix i => Array i e -> List (Pair i e)
(//)       : Ix i => Array i e -> List (Pair i e) -> Array i e
accum      : Ix i => (e -> e -> e) -> Array i e -> List (Pair i e) -> Array i e
ixmap      : (Ix i, Ix j) => Pair i i -> (i -> j) -> Array j e -> Array i e
range      : Ix i => Pair i i -> List i
inRange    : Ix i => Pair i i -> i -> Bool
rangeSize  : Ix i => Pair i i -> Int
```

Language-level definitions (in `STD_PRELUDE`) implement the vocabulary:
`listArray` validates `length elems == rangeSize (l,u)` (clean error, strict —
deviation from GHC's lazy tolerance, by design), then calls `arrayFromList`;
`array` takes assocs, fills a default (decide: `0.0`/`0`-style or a required
initial — **open, implementer or user**), then `arrayFromList`;
`a ! i = arrayIndexFlat a (index (bounds a) i)`; `(//)` maps `index` over the
assocs to flat `(pos, v)` pairs then `arrayUpdate`; `assocs = zip (range (bounds
a)) (elems a)`; `ixmap` reindexes via the two `Ix` instances. `elems`/`bounds`
are direct primitive reads.

### STArray (mutable)

`newArray`/`newListArray` compute the flat size via `rangeSize` and call
`NewArrayFlat`; `readArray`/`writeArray`/`modifyArray` compute the flat offset
via `Ix.index` on `getBounds` and call `*ArrayFlat`; `getBounds`/`getElems` are
direct primitive reads (no Ix math); freeze is the rank-2 `runSTArray`:

```haskell
runSTArray : (forall s. ST s (STArray s i e)) -> Array i e
```

### Buffer bridge (pure, interpreter-level)

```haskell
arrayFromBuffer : FixedBuffer f32 -> Array Int Float   -- copy, lo=0, hi=len-1
bufferFromArray : Array Int Float -> FixedBuffer f32   -- copy
```

Concrete over `FixedBuffer f32` now; generic over the `Buffer` class later if
needed.

## Index math and extensibility

Requirement 4 (future index types / higher dimensions → no architecture change)
is met by keeping **every** offset/range computation in **language-level `Ix`
instance bodies**. The interpreter only ever sees flat offsets and whole arrays
(`ArrayIndexFlat`, `*ArrayFlat`, `ArrayFromList`). Adding a new index type means:

1. writing the `Ix` instance in `STD_PRELUDE` (`index`/`range`/`inRange`/
   `rangeSize`), and
2. nothing else — no new interpreter ops, no new IR instructions.

With `=>` (decision 7), the constrained prelude functions are generic over
`Ix i` and **specialize at each call site**, so a new index type needs only its
`Ix` instance — the array functions and the flat primitives are untouched.

**In scope: head-arg-aware instance matching** (builder-check Q4, user: not
deferred). The instance registry keys by `(class, full-applied-index-type)`
rather than `(class, head-name)`, so `Ix Int`, `Ix (Pair Int Int)` and
`Ix (Pair Int (Pair Int Int))` (3-D) coexist without colliding on the `Pair`
head. This pairs with the unification-based class-var recovery (Q2): the
recovery produces the class var's FULL type (`Int` / `Pair Int Int` /
`Pair Int (Pair Int Int)`), and the lookup uses it verbatim. Adding any index
type — a new head (`Integer`, `Char`) or a higher dimension via `Pair` — needs
only a new instance and no architecture change, as promised.

The storage primitives are inherently type-agnostic (bounds is an ordinary
value; elements are arena refs), so the same holds for every index type.

## Fault policy and arena

- Out-of-range `!` / `readArray`/`writeArray` → **clean runtime error** via the
  interpreter's existing latched fault channel — the same path
  `ArenaError::CapacityExceeded` ("resource exhausted") and stream faults use
  (`latch_fault` → `ProcessError`), never a panic (builder-check Q3).
- `listArray`/`array`/`newListArray` length mismatch vs `rangeSize` → clean
  error (strict construction).
- Arena capacity during array allocation → existing clean capacity error.
- STArray escaping its region (returned from `runST`/`runSTArray` without a
  frozen copy) → **compile-time error**, same region-phantom machinery as
  `STRef` (tested).

## Testing outline (TDD)

New test targets under `rill-lang/tests/` (gated `--features dsp`), one per area,
written failing-first:

- `ix.rs` — `range`/`index`/`inRange`/`rangeSize` for `Int` (incl. negative and
  zero-length bounds) and `Pair Int Int` (row-major offsets, empty dimension).
- `array_immutable.rs` — `array`/`listArray` (valid, wrong-length error, 2-D),
  `!` (index, out-of-range clean error, chained `((a ! i) + j)`), `bounds`,
  `elems`, `indices`, `assocs`; `//`, `accum`, `accumArray`; `ixmap` (matrix
  transpose).
- `st_array.rs` — `newArray`/`newListArray`/`readArray`/`writeArray`/
  `modifyArray`/`getBounds`/`getElems` inside `runST`; region-escape is a
  **compile-time** test (port of the `STRef` escape test in `st_monad.rs`);
  freeze via `runSTArray`, use the frozen array afterwards; direct array
  mutation attempts stay immutable.
- `array_buffer.rs` — `arrayFromBuffer` reads the buffer's f32 contents;
  `bufferFromArray` writes them back; round-trip `buffer → array → buffer`
  equals the original.
- `bang_patterns.rs` — strictness semantics: `f !x`, `\ !x`, `let !x`, and
  `case … of { !x -> }` force to WHNF (observable through a thunk/shared-binding
  scenario); `!` infix indexing still parses in expression position.
- `array_matrix.rs` — reference demo: a `2×2`/`3×3` `Array (Pair Int Int) Float`,
  index, `assocs`, transpose via `ixmap`, accumulation.

Gates after every task: `cargo test -p rill-lang --features dsp` green,
`cargo clippy -p rill-lang --features dsp --all-targets` zero warnings,
`cargo fmt -- --check`. Docs/comments in English. `src/arrow.rs` and
`src/graph/reconstruct.rs` are not touched.

## What is left to the builder / open questions

- `array`'s default fill value when assocs don't cover every index (0.0, or a
  required explicit initial element).
- The `=>`-in-`TypeExpr` scope: it is needed for `Def::Eval` sigs (arrays); the
  same parser/AST/infer/lowering path also serves `typeclass` method sigs and
  user code — how much surface to wire in this stage is a builder call (the
  array surface is the minimum).
- `Tok::Cut` → rename-and-reuse vs delete-and-add `Tok::Bang` (equivalent; the
  lexer line and the `arrow.rs` doc comment about `!` = discard stay untouched
  as legacy).
- `rangeSize` via typeclass default or per-instance body.
- ST cell storage shape for STArray (extend the STRef cell table vs a sibling
  table) — constrained by the existing region/escape machinery.
- Whether `Eq`/`Ord` get structural `Array` instances in this stage (cheap via
  the existing structural marker; **recommended**: include if it does not
  disturb the Eq/Ord resolution path).