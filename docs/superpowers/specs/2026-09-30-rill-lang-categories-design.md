# rill-lang: Page Arena, Open Collections, and Builtin Category Typeclasses — Design

> **Status:** Approved (user, 2026-09-30).
> **Date:** 2026-09-30
> **Branch:** `feature/rill-lang-categories`
> **Scope:** Rework the value-track arena into a page-based allocator in the style of
> Alexandrescu's "Affordable Allocator" so that `List`/`Map`/`Set` become **open**
> collections (no type-level capacity), then ship `Functor`, `Applicative`, `Monad`,
> `Monoid`, `Arrow`, and `Kleisli` as **builtin first-class typeclasses** with full
> `do`-notation, constraint-qualified instances, result-directed nullary method
> dispatch, superclass auto-derivation, and an `Arrow` instance over both the
> value track (`Kleisli m`) and the signal track (the block-diagram combinators).

---

## 0. Executive summary

Four independently shippable sub-projects, built in order:

| # | Sub-project | Deliverable |
|---|---|---|
| **SP-0** | Page arena + open collections | Alexandrescu-style allocator; `Cap(n)` removed from `List`/`Map`/`Set`; RT-safe pre-allocated pool (+ `growable-arena` feature); pooled payload buffers kill per-tick heap allocation |
| **SP-1** | Value-track category core | Constraint-qualified instances, default methods, result-directed `mempty`, superclass auto-derivation; builtin `Functor`/`Applicative`/`Monad`/`Monoid` + instances; IR ops `ConcatMap`/`AppendList`/`ConcatString`; full `do`-notation |
| **SP-2** | Kleisli + value-track Arrow | `data Kleisli m a b`; builtin `Arrow` class; `instance Monad m => Arrow (Kleisli m)` |
| **SP-3** | Signal-track Arrow | Block-diagram combinators as the concrete `Arrow` instance; method dispatch by operand track |

This design document covers all four. SP-0 is planned and built first; SP-1 → SP-3
are planned each when the previous sub-project ships.

---

## SP-0. Page arena and open collections

### 0.1 Problem

The current arena (`arena.rs`) pre-allocates a fixed number of value *slots*
(`Vec<Option<Slot>>` + `VecDeque<ArenaRef>` free list). Collection payloads —
`Value::Record(Vec<ArenaRef>)`, `Value::List/Map/Set` element buffers, and
`Value::String` — are ordinary heap `Vec`s constructed per tick by the
interpreter (`read_field_refs`, `map`'s `Vec::with_capacity`, `cons`'s
`insert(0, ..)`, `filter`, `insert`, …). Two consequences:

1. **The "no heap allocation on the RT path" guarantee is already soft for the
   value track** — payload buffers allocate per tick inside the I/O callback.
2. **Type-level capacities (`Cap(n)`) break category laws**: `mappend` of
   `List a n`/`List a m` needs `max(n,m)`; `mempty` needs an `n`; `concat_map`
   result capacity is data-dependent; `pure`/`return` is a capacity-1 singleton.
   `Functor`/`Applicative`/`Monad`/`Monoid` cannot hold over capacity-indexed
   lists.

### 0.2 Allocator design (Alexandrescu "Affordable Allocator" model)

**Slot pool with an embedded free list.** Replace the `VecDeque` with a
free-list *stored inside the freed slots themselves*:

```rust
enum Slot {
    Occupied { rc: u32, val: Value },
    Free { next: Option<ArenaRef> },   // next free index lives in the freed block
}
```

- `alloc` pops the free-list head; `drop_ref` (at rc 0) stores `Free { next }` at
  the freed index. O(1), no separate bookkeeping structure.
- Slots are pre-allocated in **pages** at build time (`Vec<Option<Slot>>` laid
  out contiguously, or page-vectors). The default mode never grows at runtime.

**Payload buffer pool.** Collection payloads (`Vec<ArenaRef>`, `Vec<(ArenaRef,
ArenaRef)>`, `String`) are served from a pre-allocated **buffer pool** instead of
the global heap:

- `Pool::take(n)` returns a pooled buffer with capacity ≥ n; `Pool::put(b)`
  returns it. Ops borrow a buffer, fill it, hand it to the arena slot; `drop_ref`
  and COW return the buffer to the pool.
- Size classes (`0..8`, `9..16`, `17..32`, `33..64`, `65..128`, …) keep `take`
  amortized O(1) with acceptable slack.
- This eliminates per-tick heap allocation for all collection and record
  payloads and for strings.

**RT policy (feature-gated).**

| Mode | Exhaustion behavior |
|---|---|
| default (RT-safe) | Pre-allocated pool (slots + buffers); exhaustion → `ProcessError::Processing("arena exhausted")` |
| `growable-arena` (non-RT) | On exhaustion, allocate a fresh page / buffer from the heap |

The default preserves the hard-RT contract; the feature matches the pre-existing
(soft) behavior for non-RT use. `Value::List/Map/Set` **drop the `cap` field**
entirely — a collection grows as needed up to the pool budget.

### 0.3 Type-system change: full `Cap` removal

`List a`, `Map k v`, `Set a` become open (kind `* → *`, `* → * → *`, `* → *`):

| Change | Site |
|---|---|
| Drop `ValueTy::Cap`, `TypeExpr::TCap` | `types/ty.rs`, `ast.rs`, `parser.rs`, `render.rs` |
| `ctor_kinds`: `List`→(1,false), `Set`→(1,false), `Map`→(2,false); remove `ctor_has_cap` | `types/ty.rs` |
| Remove `Cap` unification and the cap-skip in `match_ctor_pattern` | `types/unify.rs`, `types/ty.rs` |
| Collection op result types lose the `Cap` slot; empty constructors become `list`, `empty_map`, `empty_set` (no capacity arg) | `lower.rs`, `types/infer.rs` |
| `subtree_size`/capacity heuristic → conservative **pool budget** (see 0.4) | `lower.rs` |
| IR: `ValueListLit`/`ValueMapLit` drop `cap`; `ValueBuiltinOp::ListEmpty/MapEmpty/SetEmpty` drop the capacity argument; interpreter drops the "…capacity exceeded" checks | `ir.rs`, `backend/interp.rs` |
| `ValueLayout.capacity` → pooled slot budget; new `ValueLayout.buffer_budget` | `ir.rs`, `program.rs` |

**Semantics shift (documented):** the type-level *strict bound* becomes a
runtime *pool budget*. Per-op capacity overflow errors ("list capacity exceeded",
"map capacity exceeded", "set capacity exceeded") are replaced by a single
"arena exhausted" `ProcessError` when the pre-allocated pool is full.

### 0.4 Pool-budget accounting

`subtree_size` currently returns exact bounds because capacities are in the type.
With open collections, element counts are data-dependent. The v1 budget is
**conservative**:

```
slot_budget   = Σ subtree_size(static type) over all value subexprs   (containers: 1 + ELEM_EST × size(elem))
                + 1 per alloc-producing value instruction
                + Σ worst-case growth of each collection op
buffer_budget = Σ (ELEM_EST × size(elem)) over collection ops + record literal field counts
```

`ELEM_EST` estimates worst-case element counts per op:
- `map`/`filter`/`tail` → source length (≤ source est)
- `cons`/`insert` → source est + 1
- `concat_map`/`mappend`/`append` → sum of source estimates
- list/map/set literals → literal length

These are estimates, not proofs. The runtime safety net is the exhaustion error
(default mode) or pool growth (`growable-arena`). The plan uses a
`POOL_SAFETY_MULTIPLIER` (e.g. ×4) on the estimated budget for the default mode
so legitimate programs do not trip the error; the exact multiplier is tuned by
the collection stress tests.

### 0.5 Builtin surface

```
list         : List a                      (was `list n`)
empty_map    : Map k v                     (was `empty_map n`)
empty_set    : Set a                       (was `empty_set n`)
cons  x xs   : List a -> List a            (no cap check; grows)
insert k v m / insert k s                  (no cap check; grows)
```

`head`/`tail`/`length`/`map`/`fold`/`filter`/`lookup`/`member` unchanged apart
from dropping the capacity plumbing.

---

## SP-1. Value-track category core

### 1.1 Builtin classes

```rill
typeclass Functor f     where { fmap:  (a -> b) -> f a -> f b; }
typeclass Applicative f where { pure:  a -> f a; ap: f (a -> b) -> f a -> f b; }
typeclass Monad m       where { return: a -> m a; bind: m a -> (a -> m b) -> m b; }
typeclass Monoid m      where { mempty: m; mappend: m -> m -> m; }
```

Registered in `TypeEnv::with_builtins()` so programs never redeclare them. Method
resolution stays compile-time inline; instances produce no runtime dispatch.

### 1.2 Superclass auto-derivation

On instance registration (inference phase 1):

- `instance Monad T` → synthesize `Applicative T` (`pure = return`,
  `ap mf mx = bind mf (fn f -> bind mx (fn x -> return (f x)))`) and
  `Functor T` (`fmap g x = bind x (fn y -> return (g y))`).
- `instance Applicative T` → synthesize `Functor T` (`fmap g x = ap (pure g) x`).
- **Explicit instances always win** — derivation fills gaps only. Builtin
  instances for `List`/`Maybe` are still written out in full (a `bind`-based
  `fmap` over `List` would be O(n·cap) vs the direct `map`).

### 1.3 Builtin instances (open collections)

| Class | Type | Body |
|---|---|---|
| Functor | `List` | `fmap g xs = map g xs;` |
| Functor | `Maybe` | `match m of { Nothing => Nothing; Just x => Just (g x); }` |
| Functor | `Either a` | `match e of { Left x => Left x; Right y => Right (g y); }` |
| Monad | `Maybe` | `bind m f = match m of { Nothing => Nothing; Just x => f x; }` |
| Monad | `List` | `bind xs f = concat_map f xs;` (new IR op) |
| Monad | `Either a` | `bind e f = match e of { Left x => Left x; Right y => f y; }` |
| Monoid | `List a` | `mappend xs ys = append_list xs ys;` (new IR op) |
| Monoid | `String` | `mappend a b = concat_string a b;` (new IR op) |
| Monoid | `Float` | `mempty` → `0.0`; `mappend a b = a + b;` |
| Monoid | `Int` | `mempty` → `0`; `mappend a b = a + b;` |

(`Applicative`/`Functor` for the Monad instances derive automatically.)

### 1.4 Instance-system machinery

**Constraint-qualified instances.** `instance Monad m => Arrow (Kleisli m)`:

- Parser: optional `(Class Var)` constraint list before the class name; instance
  head may be a parenthesized partial application `(Kleisli m)`.
- AST: `Def::Instance { constraints: Vec<(String, String)>, head: (String, Vec<String>), … }`.
- `InstanceInfo` gains `head_args: Vec<String>` and `constraints: Vec<(String, String)>`.
- Resolution (infer + lower): at a call site the head args are bound from the
  call-site argument type (`Kleisli Maybe Float Float` → `m := Maybe`), then each
  constraint `Monad m` is discharged by instance lookup of the bound type.
  A head arg still abstract at the call site → compile error (monomorphization;
  no runtime dictionaries).
- Kind check: a partial application (`Kleisli m`, arity 3 total, 1 bound) against
  an arity-2 class variable is valid; arity accounting is `total - bound`.

**Default methods.** `TypeclassInfo::methods` entries carry an optional default
body; an instance that omits a method uses the default. Used by `Arrow`'s
`second`/`both`/`fan` (SP-2). Precedence: instance body > default > error.

**Result-directed dispatch (`mempty`).** Nullary class methods resolve by
*expected type*:

- `lower_value(e, expected: Option<ValueTy>)` — a nullary method call
  (`mempty`) resolves the instance from `expected` instead of an argument.
- Inference threads a light expected-type hook; the context passes it where
  statically known: `mappend xs mempty` lowers `mempty` with
  `expected = type(xs)`. Unknown expected type → compile error.
- Non-nullary method calls keep the existing argument-directed path unchanged.

### 1.5 IR additions

```rust
enum ValueBuiltinOp {
    // …
    ConcatMap,      // bind for List: concat_map f xs
    AppendList,     // Monoid mappend for List (capacity-free, pooled buffers)
    ConcatString,   // Monoid mappend for String
}
```

`ConcatMap` dispatches `f` per element (pre-allocated frames, as `map`/`fold`)
and splices the resulting lists together via pooled buffers. Capacity checks are
gone; growth is bounded by the pool.

### 1.6 do-notation

Lexer: `KwDo`, `<-` token. Parser: `Expr::Do { statements }`. Desugar (in
`reduce`, before lowering):

```
do { x <- mx; let y = e; rest }  →  bind mx (fn x -> <desugar rest> with y := e)
do { e; rest }                   →  bind e (fn _ -> <desugar rest>)
do { e }                         →  e
```

The final statement must be a value expression of type `m b`; `<-` bindings and
`let` statements inline into the surrounding lambda. Reuses the existing
`fn`/lambda value-track machinery — no new IR.

### 1.7 Compile errors

New messages: `no instance of Monad for type T` (constraint discharge),
`cannot resolve method 'mempty': expected type unknown`, `recursive typeclass
method` (existing), do-block type errors (`do` block body must be monadic).

---

## SP-2. Kleisli and value-track Arrow

```rill
data Kleisli m a b = { unKleisli: a -> m b };

typeclass Arrow a where {
    arr:      (b -> c) -> a b c;
    first:    a b c -> a (b, d) (c, d);
    compose:  a b c -> a c d -> a b d;          // >>>
    // defaults:
    second:   a b c -> a (d, b) (d, c);
    both:     a b c -> a d e -> a (b, d) (c, e);  // ***
    fan:      a b c -> a b d -> a b (c, d);        // &&&
}

instance Monad m => Arrow (Kleisli m) where {
    arr f      = Kleisli (fn x -> return (f x));
    first k    = Kleisli (fn p -> bind (unKleisli k p.first) (fn z -> return (Pair { first: z, second: p.second })));
    compose k1 k2 = Kleisli (fn x -> bind (unKleisli k1 x) (fn y -> unKleisli k2 y));
}
```

The `Kleisli` newtype/data is a parameterized record over a function type; its
field projection `unKleisli` extracts the underlying monadic function. The
constraint machinery from SP-1 resolves `return`/`bind` inside the instance body
against the concrete monad at each inlining site.

---

## SP-3. Signal-track Arrow

The block-diagram combinators become the **concrete `Arrow` instance** over block
transforms (`ArrowTy`):

| Method | Block lowering |
|---|---|
| `arr f` | lift a pure λ to a 1→1 block transform: `f _` |
| `first f` | `f , _` |
| `second f` | `_ , f` |
| `compose f g` (`>>>`) | `f : g` |
| `both f g` (`***`) | `f , g` |
| `fan f g` (`&&&`) | `f <: g` |
| `loop f` | `f ~ …` |

**Mechanics:** a new lowering path for *arrow methods* whose operands are
signal channels. `arr`/`first`/`second`/`compose`/`both`/`fan`/`loop` resolve by
operand **track**: signal channels → the builtin signal instance (emit
`Seq`/`Par`/`Split`/`Loop`), value channels → the value-track instance path.
Signal-track method calls type over `ArrowTy` (arities flow through the existing
channel-count synthesis). This is the largest single piece; it is planned in a
dedicated plan when SP-1 and SP-2 land.

---

## Files (all sub-projects)

| Area | Files |
|---|---|
| Arena / allocator | `arena.rs`, `ir.rs` (`ValueLayout`), `program.rs`, `backend/interp.rs` |
| Type system | `types/ty.rs`, `types/unify.rs`, `types/infer.rs`, `ast.rs`, `parser.rs`, `render.rs`, `lower.rs` |
| IR | `ir.rs` (`ValueBuiltinOp`, `ValueListLit`/`ValueMapLit`), `backend/interp.rs` |
| Syntax | `lexer.rs`, `parser.rs`, `ast.rs`, `reduce.rs` |
| Docs | `README.md`, `docs/src/guides/rill-lang.md`, `CHANGELOG.md`, `docs/superpowers/specs+plans` |
| Tests | `tests/*` (typeclass, hkt_typeclass, collections, data_sums, arena, …) |

## Verification

- `cargo test -p rill-lang` after each task; `cargo test --workspace`,
  `cargo clippy --all-features --workspace`, `cargo fmt` before finishing a
  sub-project.
- Zero new external dependencies. No `unsafe`. RT-path collection ops must not
  hit the heap in default mode (verified by an allocation-count test helper
  under `debug_assert`).
- Branch: `feature/rill-lang-categories`; conventional commits, single-quoted
  `-m`.

## Out of scope (deferred)

- Runtime typeclass dispatch / dictionaries (resolution stays compile-time).
- `Hashable` and hash-based containers.
- Length-indexed / verified-bounded lists.
- String ops beyond equality and monoid concat.
- `ArrowLoop`/`ArrowApply`/`ArrowChoice`; signal-track `arr`/`first` beyond the
  v1 mapping above.