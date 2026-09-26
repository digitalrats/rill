# rill-lang: Higher-Kinded Types and First-Class Haskell-Style Collections — Design

> **Status:** Draft — awaiting review.
> **Date:** 2026-09-26
> **Branch:** `feature/rill-lang-hkt`
> **Scope:** Add higher-kinded types to rill-lang — parameterized data types and
> kind polymorphism (type variables ranging over type constructors) — and make
> six Haskell-style collections first-class arena values: `List`, `Map`, `Set`,
> `Maybe`, `Pair`, `Either`. New scalar value types `Bool` and `String`. Map/Set
> keys are generalized to any acyclic data type via `Eq`/`Ord` typeclasses with
> compiler-derived instances. Collection capacities are part of the type and are
> **strict bounds**: exceeding one is a runtime `ProcessError`. Typeclass method
> resolution stays compile-time with zero runtime dispatch.

## 1. Problem statement

The current value track (`ValueTy`, `arena.rs`) supports named data types that
are **unparameterized**: `data Point = { x: Float, y: Float }`. `ValueTy::Data`
carries only a name (`Data(String)`); there is no way to express `List Float`,
`Maybe Int`, `Map String Float`, or a user `data Box a`. Consequently:

1. **No type constructors** — a value type cannot be parameterized by other
   types (kind `* → *` / `* → * → *`).
2. **No kind polymorphism** — a type variable cannot range over a type
   constructor, so `typeclass Functor f` / `instance Functor List` is
   impossible.
3. **No first-class collections** — there are no `List`/`Map`/`Set`/`Maybe`/
   `Pair`/`Either` arena values, and no operations over them (`cons`, `map`,
   `fold`, `insert`, `lookup`, …).
4. **No `Bool`/`String`** value types — predicates (`filter`, `member`,
   comparisons) and map keys have no representation.
5. **Nullary constructors are rejected** — `data Color = Red | Green` cannot be
   constructed; `Nothing` (for `Maybe`) needs nullary support.

The signal track is **out of scope**: `Scalar` stays `Int | Float | Var`, and
`Bool`/`String`/collections live only on the value track (per-tick arena
values).

## 2. Semantic model

### 2.1 Kinds

Three kinds in v1, **declared/checked, not inferred**:

| Kind | Types |
|---|---|
| `*` | `Int`, `Float`, `Bool`, `String`, data-type names, `Func`, `Cap` |
| `* → *` | unary constructors: `List a n`, `Maybe a`, `Set a n` |
| `* → * → *` | binary constructors: `Map k v n`, `Pair a b`, `Either a b` |

`Cap` (capacity) is a natural-number pseudo-type (kind `Nat`) used as a
constructor argument: `List Float 16` = `App("List", [Float, Cap(16)])`.

### 2.2 `ValueTy` representation

```rust
// types/ty.rs
enum ValueTy {
    Int,
    Float,
    Bool,                        // NEW
    String,                      // NEW
    Data(String, Vec<ValueTy>),  // user data type + arguments
    Newtype(String, Vec<ValueTy>),
    App(String, Vec<ValueTy>),   // builtin constructor application: List Float 16
    Cap(usize),                  // capacity literal (Nat argument)
    Func(Vec<ValueTy>, Vec<ValueTy>),
    Var(TypeVarId),              // kind *
    TyConVar(TypeVarId),         // NEW — kind * → * (or * → * → *)
}
```

- `Bool`/`String` are leaf value types. `Scalar` (signal element type) is
  untouched.
- `DataInfo::Record`/`Sum` gain type parameters. Each `data` declaration's
  tyvars get fresh `TypeVarId`s at declaration time; field/payload types are
  stored as `ValueTy` over those ids and **instantiated (fresh-substituted) at
  every use** — unification stays entirely in `ValueTy`-land.

### 2.3 Capacity is a strict bound

`List a n`, `Map k v n`, `Set a n` carry a **strict capacity** `n`. The
operations that grow a container preserve the type's capacity:

```
cons   : a → List a n → List a n        // len < n required at runtime
insert : k → v → Map k v n → Map k v n  // new key: len < n required at runtime
```

`list n` produces an empty `List a n`; `empty_map n` / `empty_set n` produce an
empty `Map k v n` / `Set a n` (the `map` name is reserved for the higher-order
builtin, so the empty-map constructor is `empty_map`); `[e1, …, en]` is a
`List T n`; `{ "k": v, … }` is a `Map String T n`. Exceeding a capacity is a
**runtime user error** (`ProcessError::Processing("list capacity exceeded")`,
`"map capacity exceeded"`, `"set capacity exceeded"`), surfaced through a
value-error latch (see §6.1). `map`, `filter`, `tail` keep the capacity
unchanged; `length` needs none.

### 2.4 Arena values

```rust
// arena.rs
enum Value {
    // existing: Int, Float, Record(Vec<ArenaRef>), Sum(u32, Vec<ArenaRef>),
    //           Newtype(ArenaRef), Closure(ArenaRef, u32), Void
    Bool(bool),                                   // NEW
    String(String),                               // NEW
    List  { elems: Vec<ArenaRef>, cap: usize },   // NEW
    Map   { pairs: Vec<(ArenaRef, ArenaRef)>, cap: usize }, // NEW
    Set   { elems: Vec<ArenaRef>, cap: usize },   // NEW
}
```

- `Maybe`, `Pair`, `Either` are **not** dedicated variants. They are builtin
  `data` declarations injected into the `TypeEnv` at compile time, reusing the
  existing `Sum`/`Record`/`Match` machinery:

  ```
  data Maybe a  = Just a | Nothing;
  data Pair a b = { first: a, second: b };
  data Either a b = Left a | Right b;
  ```

  This requires **nullary-constructor support** (v1 currently rejects
  `data Color = Red | Green`): `Value::Sum(ctor, [])` already works; construction
  via a bare `Ref` and a `match` arm with no bindings are the new pieces.
- Arena RC/COW: `drop_ref` and `mutate` recurse into the new variants'
  child refs exactly like `Record`/`Sum`. `ArenaError` unchanged. Arena
  exhaustion stays a **build-time bug** (`debug_assert!` + detectable `None`,
  as today) — the capacity heuristic guarantees it never fires.

### 2.5 Eq/Ord constraints with derived instances

Map keys and Set elements generalize to **any acyclic data type** (not just
scalars), constrained by two builtin typeclasses:

```
typeclass Eq  a where { eq: a -> a -> Bool; }
typeclass Ord a where { lt: a -> a -> Bool; }
```

- The compiler **auto-derives** instances for all concrete value types
  (Haskell `deriving` equivalent), **except `Func`** (no meaningful order on
  closures). Derived order is structural: leaves by value; `Sum` by constructor
  index then payload; `Record` by field order; `Newtype` by inner; `List`
  lexicographic; `Pair`/`Either` by components; `Map`/`Set` lexicographic over
  their (k, v) pairs / elements.
- Operation signatures carry the constraint implicitly: `insert`/`lookup`/
  `member` on `Map k v n` / `Set a n` resolve `Ord k` / `Ord a` at the call
  site; a `Func` key is a compile error ("no Ord instance for function type").
- **Variant A (chosen)** — derived-only. No user-written `instance Ord`.
  Runtime comparison is one interpreter builtin `value_cmp(a, b) → Ordering`;
  no runtime typeclass dispatch, preserving the current property that typeclass
  resolution is compile-time.
- **Ordering storage**: entries are kept sorted by the derived order →
  `lookup`/`member` are binary search O(log n); `insert` is COW O(n).

### 2.6 Kind polymorphism (HKT)

Method signatures become full type expressions:

```rust
// ast.rs
enum TypeExpr {
    TName(String),                 // concrete type or type variable
    TApp(String, Vec<TypeExpr>),   // constructor application: f a, List Float 16
    TFunc(Vec<TypeExpr>, TypeExpr),// a -> b (curried)
    TCap(usize),                   // capacity literal
}
```

```rust
// types/ty.rs
struct TypeclassInfo {
    var: String,
    arity: usize,                       // NEW — inferred from method sigs
    methods: Vec<(String, TypeExpr)>,   // NEW — sigs become TypeExpr
}
```

- **Class-var arity** is inferred from its use in method signatures: `f a` →
  1, `f a b` → 2, bare `a` → 0 (as today). `instance Functor Pair` (arity 2)
  against an arity-1 class is a **kind error**.
- **Cap-aware kind-var matching**: a kind variable matches the **head
  constructor**, ignoring `Cap` arguments. Unifying `f a` with
  `App("List", [Int, Cap 4])` binds `f := List`, `a := Int`, and the `Cap` slot
  of the pattern unifies with `Cap 4`. In the instance body, `f b` expands to
  `App("List", [b, Cap 4])` — **capacity flows from argument to result**.
- **Instances bind constructor names**: `instance Functor List`. The body is
  type-checked in the context `f := List` with `a`/`b` free.
- **Resolution is compile-time inline**: `fmap g xs` unifies the signature
  `f a` with the argument type, instantiates the instance body, β-substitutes
  the arguments, and inlines it. `fmap` over `List` compiles directly to the
  `map` builtin call. **No runtime dispatch, no dictionaries, no fragments for
  the method itself** — the existing typeclass property is preserved.

## 3. Syntax

```faust
// types (juxta application, consistent with the DSL)
data Box a = { value: a };                 // user parameterized type
typeclass Functor f where { fmap: (a -> b) -> f a -> f b; }
instance Functor List where { fmap g xs = map g xs; }
instance Functor Maybe where { fmap g m = match m of { Nothing => Nothing; Just x => Just (g x); }; }

// collections
xs = [1.0, 2.0, 3.0];       // List Float 3 (capacity from literal length)
e  = list 4;                // empty List a 4
ys = cons 10.0 e;           // List Float 4 (valid: 1 element ≤ 4)
h  = head xs;               // Maybe Float: Just 1.0 / Nothing
t  = tail xs;               // List Float 3 (capacity preserved)
n  = length xs;             // Int
z  = map (fn x -> x * 2.0) xs;   // List Float 3
s  = fold (fn a b -> a + b) 0.0 xs;  // Float
f  = filter (fn x -> x > 1.0) xs;    // List Float 3
bad = cons 9.0 xs;          // runtime error: len 3 ≥ cap 3

m  = { "a": 1.0, "b": 2.0 };         // Map String Float 2
m1 = insert "a" 9.0 m;               // replace-on-duplicate, len stays 2
m2 = empty_map 8;                    // empty Map k v 8 (no name collision with `map`)
v  = lookup "a" m;                   // Maybe Float
b  = member "a" m;                   // Bool

st = empty_set 8;                    // empty Set a 8
s1 = insert 1 st;                    // Set Int 8
b2 = member 1 s1;                    // Bool

// scalars / logic / comparisons (value track only)
tru  = true;                         // Bool
cond = fn x -> x > 1.0 && x < 4.0;   // Bool (comparisons only in value position)
notx = not cond;                     // Bool (`not` is a prefix builtin; `!` stays wire-cut)

// nullary constructors + match
data Color = Red | Green;
c = Red;                             // Sum(0, [])
```

- Precedence (loosest → tightest): `&& ||` < `== != < > <= >=` < `+ -` <
  `* / %` < `@` < unary `-` < atoms.
- Empty-container names (`list`/`empty_map`/`empty_set`), `not`, and `insert`
  arity overloading are settled in §8.

## 4. Pipeline changes

```
source → lexer (new tokens: [ ] true false == != < > <= >= && ||)
       → parser (TypeExpr in declarations; Expr::ListLit/MapLit/Bool/Cmp/Logic)
       → infer (ValueTy extension, kind checking, Eq/Ord instances,
         kind-var unification, nullary ctors, builtin type shapes)
       → reduce (unchanged; declarations skipped)
       → lower (collection ops as ValueCallBuiltin; capacity-bound accounting;
         constructor instances inlined)
       → RillProgram (value_error latch; value_cmp; ValueBuiltinOp dispatcher)
```

## 5. IR changes

```rust
// ir.rs — new ValueInstr variants
ValueBool { dst: usize, value: bool }
ValueConstString { dst: usize, value: String }
ValueListLit  { dst: usize, elems: Vec<usize>, cap: usize }
ValueMapLit   { dst: usize, keys: Vec<usize>, vals: Vec<usize>, cap: usize }
ValueCompare  { dst: usize, op: CmpOp, a: usize, b: usize }   // == != < > <= >=
ValueLogic    { dst: usize, op: LogicOp, a: usize, b: usize } // && ||
ValueCallBuiltin { dst: usize, op: ValueBuiltinOp, args: Vec<usize> }

enum ValueBuiltinOp {
    Cons, Head, Tail, Length, Map, Fold, Filter, ListEmpty,
    InsertMap, Lookup, Member, InsertSet, MapEmpty, SetEmpty,
}
```

- `insert` is overloaded by arity: 3-arg `insert k v m` (Map) vs 2-arg
  `insert k s` (Set); inference disambiguates.
- `ValueConstructSum` is reused for `Just`/`Left`/`Right`/`Nothing` (empty
  payload already supported).
- The interpreter dispatches `ValueBuiltinOp`; `map`/`fold` call their function
  argument through `ValueCallFunc` per element (pre-allocated frames);
  `insert`/`lookup`/`member` use `value_cmp` over sorted entries.

## 6. Runtime

### 6.1 Value-error latch

`RillProgram` gains `value_error: Option<ProcessError>` — a single `Option`,
RT-safe (no allocation). Collection ops set it on capacity overflow; the value
phase wrapper checks and clears it at the end of the tick and returns
`Err(ProcessError::Processing(..))` from `process()`. Arena exhaustion remains a
build-time invariant (`debug_assert!` + detectable no-op), not a runtime error.

### 6.2 `value_cmp`

One interpreter builtin implements the derived `Eq`/`Ord` total order over
acyclic values: `value_cmp(a, b) → Ordering` (returns `-1/0/1` or an `Ordering`
enum). Leaves compare by value; sums by ctor index then payload; records by
field order; newtypes by inner; lists lexicographic; maps/sets over their
entries. `Func` is not ordered — `eq`/`lt` over a `Closure` is a compile error.

## 7. Capacity-bound accounting

The arena capacity heuristic (lower.rs §"capacity heuristic") extends with:

```
+ Σ subtree_size(static type) over every value subexpression whose static type
  is a container (List/Map/Set) — collection ops allocate whole containers
  inside the interpreter without emitting alloc-producing instructions.
```

`subtree_size` extension (exact, strict-cap):

```
List a n    → 1 + n · size(a)
Set a n     → 1 + n · size(a)
Map k v n   → 1 + n · (size(k) + size(v))
Maybe a     → 1 + size(a)
Pair a b    → 1 + size(a) + size(b)
Either a b  → 1 + max(size(a), size(b))
```

## 8. Resolved decisions

1. Empty containers: `list n`, `empty_map n`, `empty_set n`. `map` the
   higher-order builtin keeps its name; the empty-map constructor is
   `empty_map` to avoid collision.
2. `not` is a **prefix builtin function** (`not b`); the `!` token remains the
   wire-cut combinator.
3. `String` values are constructed from the existing `Str` literal in value
   position; compile-time param-name strings (`param("gain", …)`,
   `?name=default`) are unaffected.
4. Capacity overflow error messages: `"list capacity exceeded"`,
   `"map capacity exceeded"`, `"set capacity exceeded"`.
5. `insert` is overloaded by arity (Map 3-arg, Set 2-arg).

## 9. Out of scope (deferred)

- User-written `Eq`/`Ord` instances (Variant B) and runtime method dispatch.
- `Hashable` / hash-based `HashMap` / `HashSet`.
- Length-indexed lists (track length in the type to eliminate the runtime
  capacity check).
- String operations beyond construction and equality (`concat`, formatting,
  `show`).
- Explicit type annotations in value expressions.
- List `concat`/`append`/`reverse`; Set `union`/`intersection`.
- Signal-track `Bool` (comparisons on signal wires) — `Scalar` untouched.

## 10. Implementation phases

Each phase ends with `cargo test -p rill-lang` green and a review checkpoint.

| # | Phase | Scope |
|---|---|---|
| 1 | TypeExpr AST + parser | `TypeExpr` in `data`/`sum`/`typeclass`/`instance` declarations; tyvar lists; no behavior change |
| 2 | ValueTy extension | `Bool`, `String`, `App`, `Cap`, `TyConVar`; unification + occurs-check; `subtree_size` |
| 3 | Constructors, kinds, Eq/Ord | builtin constructor table with kinds; kind checking; `Eq`/`Ord` with derived instances; kind-var unification |
| 4 | Arena values | `Value::Bool/String/List/Map/Set`; RC/COW/drop_ref/mutate; arena tests |
| 5 | Parser expressions | list/map/bool literals, comparisons, logic; lexer tokens; `Expr` variants |
| 6 | Value-track ops | `ValueCallBuiltin` + interpreter dispatch + `value_cmp` + overflow latch + error propagation |
| 7 | Builtin data types | `Maybe`/`Pair`/`Either` injection; nullary constructors; `match` extension |
| 8 | Kind polymorphism | method-call lowering over constructors; `Functor`/`Foldable` tests |
| 9 | Capacity bound + error path | collection accounting in the arena heuristic; runtime error polish |
| 10 | Docs | `docs/src/guides/rill-lang.md`, README, CHANGELOG; clippy clean; workspace test |

## 11. Testing

1. Parser: TypeExpr in declarations; list/map/bool literals; comparison/logic
   precedence.
2. Unify: `App`/`Cap`/`TyConVar`; occurs-check over `App`; kind-var with
   capacity flow; kind errors.
3. Arena: subtree RC/COW for `List`/`Map`/`Set`; capacity bound with containers;
   pinning across ticks.
4. Lists: literals, `cons`/`head`/`tail`/`map`/`fold`/`filter`/`length`,
   `list n`; overflow → `ProcessError` from `process()`.
5. Map/Set: `insert`/`lookup`/`member`, replace-on-duplicate, sorted storage,
   non-scalar keys (`Pair Int Int`, `List Int 2`), `Func` key → compile error.
6. Maybe/Pair/Either: `Just`/`Nothing`/`Left`/`Right`, nullary ctors, `match`.
7. HKT: `Functor`/`Foldable` over `List`/`Maybe`/user `data Box a`; kind error
   (`instance Functor Pair`); capacity flow through `fmap`.
8. Bool/String: comparisons, logic, filter predicates, String equality.
9. Regression: all existing tests stay green; zero clippy warnings.

## 12. Files

| File | Change |
|---|---|
| `rill-lang/src/ast.rs` | `TypeExpr`, `CmpOp`/`LogicOp`, `Expr::Bool/ListLit/MapLit/Cmp/Logic`, `Def` tyvars + TypeExpr fields |
| `rill-lang/src/lexer.rs` | `[ ] true false == != < > <= >= && ||` tokens |
| `rill-lang/src/parser.rs` | TypeExpr parser; new expressions; tyvar lists |
| `rill-lang/src/types/ty.rs` | `ValueTy` extension, `TypeclassInfo.arity/sigs`, `DataInfo` params, builtin type shapes (Maybe/Pair/Either), Eq/Ord registry + derivation |
| `rill-lang/src/types/unify.rs` | `App`/`Cap`/`TyConVar` unification, kind checks, capacity flow |
| `rill-lang/src/types/infer.rs` | constructor signatures, Eq/Ord resolution, nullary ctors, kind-var inference |
| `rill-lang/src/ir.rs` | new `ValueInstr` variants, `ValueBuiltinOp` |
| `rill-lang/src/lower.rs` | collection-op lowering, capacity accounting, method inline over constructors |
| `rill-lang/src/backend/interp.rs` | `ValueBuiltinOp` dispatcher, `value_cmp`, overflow latch |
| `rill-lang/src/program.rs` | `value_error` latch |
| `rill-lang/src/error.rs` | new message categories (kind, Ord-instance, capacity) |
| `rill-lang/src/reduce.rs` | declaration handling for new AST (unchanged behavior) |
| `rill-lang/tests/*` | new integration tests (§11) |
| `docs/src/guides/rill-lang.md` | language reference updates |

Verification: `cargo test -p rill-lang`, `cargo test --workspace`,
`cargo clippy --workspace`, `cargo fmt`. Zero warnings. No new external
dependencies.