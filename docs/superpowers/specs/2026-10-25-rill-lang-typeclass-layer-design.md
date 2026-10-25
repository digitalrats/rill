# rill-lang: Haskell 2010 typeclass layer — Eq/Ord/Show/Num/Fractional/Enum/Foldable

> **Status:** Design approved-in-conversation (2026-10-25). Extends the
> conformance work (`2026-10-24-rill-lang-haskell2010-conformance-design.md`);
> replaces its Task 2.6 (`min`/`max`) with a class-based implementation.
> **Date:** 2026-10-25
> **North star:** Haskell 2010 + GHC Prelude. A divergence is a defect
> (`rill/AGENTS.md`).

---

## 0. Executive summary

rill already has a working typeclass engine: `Functor`, `Applicative`, `Monad`,
`Monoid`, `Ix`, `Arrow`, `Buffer` are declared, instances are resolved by
`TypeEnv::resolve_method`, and constrained signatures are discharged at call
sites (`infer_constrained_apply`: `mappend true false` is a clean
`no instance of Monoid for Bool`; a constrained user signature that cannot be
satisfied is rejected).

What is missing is the **standard class layer** for the operations rill already
implements ad hoc. Comparison, arithmetic, rendering, integer division and
enumeration are hard-wired into the interpreter (`ValueCompare`, `BinArith`,
`UnOp`, `show_float`/`show_int`/`show_bool`), so they are monomorphic,
duplicated, and in several cases broken (scalar `min`/`max` and scalar `abs` do
not work at all).

This design puts those operations behind the Haskell 2010 classes they belong to,
and fills the genuinely missing Prelude surface (`compare`, `sort`, `floor`,
`div`, `mod`, `enumFromTo`, `[a..b]`, `show`, …) on top of that layer.

---

## 1. Audited current state (2026-10-25)

### 1.1 Classes that already exist and dispatch

| Class | Methods | Notes |
|---|---|---|
| `Functor f` | `fmap` | `fmap` on a non-functor is a clean error |
| `Applicative f` | `pure`, `ap` | |
| `Monad m` | `return`, `bind` | |
| `Monoid m` | `mempty`, `mappend` | `Monoid List` now delegates to `++` |
| `Ix i` | `range`, `index`, `inRange`, `rangeSize` | used by the array surface |
| `Arrow a` | `arr`, `first`, `compose`, … | |
| `Buffer b` | — | signal-track marker |

### 1.2 Operations hard-wired in the interpreter (no class)

| Operation | Mechanism | Consequence |
|---|---|---|
| `==` `!=` `<` `>` `<=` `>=` | `ValueInstr::ValueCompare` → `value_cmp_ref` | works on Int/Float/Bool/String/record/list/set/array/map, but is not a class; `Eq`/`Ord` instances are *derived* by `derive_eq_ord` and never consulted |
| `+` `-` `*` `/` `%` | `BinArith` + `ValueArith` | monomorphic over the numeric representation; no `Num` |
| `abs` `sin` `cos` `sqrt` … | `UnOp` | scalar `abs` **fails**: "abs signal argument must be a Buffer" |
| `min` `max` | `BinArith::Min/Max` | scalar path missing → `unknown 'min' in value lowering` |
| `show_float` / `show_int` / `show_bool` | three separate builtins | no `Show`; no `show` |
| `fromList`/`toList`/`fold` | value builtins | `fold` is rill-specific (3 args, left fold), not `foldr`/`foldl` |

### 1.3 Classes and functions absent from the language

Verified by compiling each expression (`unknown identifier` unless noted):

| Class | Missing names |
|---|---|
| `Eq` | `(/=)` |
| `Ord` | `compare`, class-based `<` `<=` `>` `>=` `min` `max` |
| `Show` | `show` (generic), `showsPrec` family not required |
| `Read` | whole class — **out of scope** (no parser at runtime) |
| `Num` | `fromInteger`, `negate`, `signum`, unary `(-)`, scalar `abs`, `pi` is `Floating` |
| `Fractional` | `recip`, `fromRational`, Int-track `/` semantics |
| `Real` | `toRational` — **out of scope** |
| `Integral` | `quot`, `rem`, `div`, `mod`, `toInteger`, `fromIntegral` |
| `RealFrac` | `floor`, `ceiling`, `round`, `truncate` |
| `Floating` | `log`, `asin`, `acos`, `atan`, `sinh`, `cosh`, `tanh`, `**`, `pi` |
| `Enum` | `succ`, `pred`, `enumFromTo`, `enumFrom`, `[a..b]` syntax |
| `Bounded` | `minBound`, `maxBound` |
| `Semigroup` | whole class |
| `Foldable` | `foldr`, `foldl`, `concat`, `sum`, `product`, `elem`, `null`, `length` as a method |
| — | `sort`, `sortBy`, `maximum`, `minimum` (Prelude functions over `Ord`) |

### 1.4 What already works and must keep working

Verified: `==`/`<` on `String` and on records, `mappend` on `Float`/`String`/`List`
(backtick and prefix), `(++)`, `(+)/(*)/(/)` as first-class values, `fold (+) …`,
sections, `fmap` on `List`/`Maybe`, array surface via `Ix`, and the `Monoid`/
`Functor` instances.

---

## 2. Scope

### In scope (phases)

| Phase | Content |
|---|---|
| P-A | One operator layer: polymorphic numeric literals + defaulting (`infer_arith`'s forced `Float` pin removed), operators as class-method sugar with `Expr::Arith`/`Expr::Cmp` deleted, and the `Eq`/`Ord` semantics (`compare`, `(/=)`, `min`, `max`, `sort`, `sortBy`, `maximum`, `minimum`). One task because P0 alone cannot be implemented without P1: the class semantics ARE the primitive instances' bodies. |
| P2 | `Show` — one `show`; keep `show_float`/`show_int`/`show_bool` as deprecated aliases delegating to it |
| P3 | `Num`, `Fractional` — `+ - * /`, `negate`, `signum`, `abs`, `fromInteger`, `pi`; retire `BinArith`/`UnOp` for user-visible arithmetic (keep them as the primitive implementation of the `Float`/`Int` instances) |
| P4 | `Integral`, `RealFrac`, `Enum`, `Bounded` — `div`/`mod`/`quot`/`rem`, `fromIntegral`, `toInteger`, `floor`/`ceiling`/`round`/`truncate`, `succ`/`pred`/`enumFromTo`/`enumFrom`/`[a..b]`, `minBound`/`maxBound` |
| P5 | `Semigroup`, `Foldable` — `foldr`, `foldl`, `concat`, `sum`, `product`, `elem`, `null`, and `sort`/`sortBy` over `Foldable` |

### Non-goals

- `Read`, `Real`, `Traversable`, `Bifunctor`, `MonadFail`, `Alternative` — not in
  this design; if a Prelude function needs them, it is a gap row.
- Full instance-derivation inference (GHC's `deriving` machinery). rill's
  `derive_eq_ord` already builds structural instances; this design makes them
  *visible as methods*, not smarter.
- Signal-track class instances beyond the arithmetic `Buffer` needs.
- Performance work; semantic correctness first.

### Anti-goals

- Implementing a Prelude function directly in the interpreter while leaving the
  class layer absent (that is how `min`/`max` got broken).
- Keeping two implementations of the same operation (class method *and*
  hard-wired op) once the class exists.
- **Any operator having its own lowering branch after desugaring lands.** A
  residual `Expr::Arith`-style fork (scalar vs buffer) is a defect: the fork
  belongs in class dispatch.
- Desugaring `&&`/`||` into plain functions, which would silently lose
  short-circuiting.
- Partial support without a recorded gap.
- Breaking the existing working surface listed in §1.4.

---

## 3. Design

### 3.1c Typing model for operators (the crux)

**Measured divergence from GHC (2026-10-25).** `infer_arith` (`types/infer.rs`
~6909) contains:

```rust
// A structural parameter variable in arithmetic is a numeric scalar:
// bind it to Float so the lambda's Func signature resolves concretely.
if matches!(&t.vty, ValueTy::Var(_)) {
    unify_value(&t.vty, &ValueTy::Float, &mut ctx.subst, e.span())?;
}
```

Any type variable reaching arithmetic is **forcibly pinned to `Float`** — not
defaulted, pinned inside `unify_operand`. That single line is the architectural
blocker.

**GHC's actual behaviour** (verified with `runghc`, GHC 9.10.3):

| Expression | GHC result | Why |
|---|---|---|
| `1 + 1.0` | `2.0` | the `1.0` pins `a := Double`; `1` is a *literal*, so it takes `Double` too |
| `1.0 + 1` | `2.0` | same, order-independent |
| `1 == 1.0` | `True` | same |
| `let add x y = x + y in add 1 2` | `3` | `add :: Num a => a -> a -> a`; defaulting picks `Integer` |
| `let add x y = x + y in add 1.0 2.0` | `3.0` | same scheme, `a := Double` |
| `(1::Int) + 1.0` | **error** | `No instance for 'Fractional Int' arising from the literal '1.0'` |
| `let f x = x * 2.0 in f (1::Int)` | **error** | `No instance for 'Fractional Int'` |
| `1 + 2` (unconstrained) | `3` | `Num`-only → numeric defaulting to `Integer` |
| `1 / 2` | `0.5` | `Fractional` → defaulting to `Double` |
| `min 1 2` | `1` | `Ord`+`Num` → defaulting to `Integer` |

**So the mechanism is exactly three steps, in this order:**

1. **Solve the class variable from the non-literal operands.** `1 + 1.0`: the
   `Float` literal solves `a := Double`. If both operands are solvable and
   disagree (`Int` pinned vs `Double`), it is a **type error** — there is no
   coercion between *types*.
2. **Apply the solved type to the still-polymorphic literals.** `1` is an
   `Int`-looking *literal*, not an `Int` *value*: under `a := Double` it becomes
   `Double`. This is the step that makes `1 + 1.0` work. It is **literal
   polymorphism, not a cast** — a variable already bound to `Int` (as in
   `(1::Int) + 1.0`) cannot widen, and GHC reports `Fractional Int`.
3. **Dispatch the class method at the solved type.** `Num Double`, `Num Integer`,
   etc. succeed; a missing instance is the error above.

If nothing solves the variable (`1 + 2` with no other constraint), **numeric
defaulting** applies at the end: `Integer` for `Num`-only, `Double` when
`Fractional` is involved.

**Where rill diverges today (measured):**

| Expression | rill result | Status |
|---|---|---|
| `1 + 1.0` | `2.0` | coincides, but by a *forced* `Float` pin, not by step 2 |
| `1 == 1.0` | `False` | **wrong** — `Cmp` has no such pin, so `Int` literal ≠ `Float` literal |
| `let add x y = x + y in add 1 2` | **type error** | **wrong** — step 1/3 incomplete: the scheme is not `Num`-polymorphic |
| `let add x y = x + y in add 1.0 2.0` | `3.0` | coincides |
| `(1::Int) + 1.0` | `2.0` (silent) | **wrong** — must be `Fractional Int` error (step 2 must not apply to a pinned variable) |

**Required model (System FC: dictionary-passing).**

1. An operator reference has a **constrained scheme**:
   `(+) :: Num a => a -> a -> a`, `(==) :: Eq a => a -> a -> Bool`,
   `(<) :: Ord a => a -> a -> Bool`, `(/) :: Fractional a => a -> a -> a`,
   `(rem) :: Integral a => a -> a -> a`.
2. Solve the class variable from the call-site argument types — reuse
   `infer_constrained_apply`, which already does this for `Ix`-constrained array
   helpers (unify the signature's parameters against the arguments, solve the
   class variable, discharge each constraint, report `no instance of …`).
3. **Literals are polymorphic** (step 2 above): an `Int`/`Float` literal is a
   numeric literal with a fresh variable, not a fixed `Int`/`Float`; it adopts
   whatever the solved class variable requires. This must be modelled in the
   type of literals, **not** as a coercion in the operator.
4. **No widening between solved types.** If an operand is already bound to a
   concrete type, a conflicting literal is `no instance of Fractional Int` /
   `cannot unify Int with Float` — never a silent cast.
5. **Numeric defaulting only at the end** for a still-unsolved numeric variable:
   `Integer` for `Num`-only, `Double` when `Fractional` is involved. This is
   literal defaulting, never operator coercion.
6. Remove the forced-`Float` pin in `infer_arith` entirely.

**Consequence for the migration.** The following results *change*, and the new
values are the correct ones:

- `1 == 1.0`: `False` → **`True`** (step 2 applies in `Cmp` too);
- `let add x y = x + y in add 1 2`: type error → **`3`**;
- `(1::Int) + 1.0`: silent `2.0` → **type error** (a bound `Int` cannot take a
  `Fractional` literal);
- `let f x = x * 2.0 in f (1::Int)`: silent → **type error**.

A migration step that tries to preserve today's results reintroduces the forced
`Float` pin and blocks the work. The checkpoint is "the suite is green and the
operator results are **Haskell-2010-correct**", not "identical to today".

### 3.1b Binary operators are sugar over class methods (core principle)

**All binary operators are syntactic sugar for function application of the
corresponding class method.** The parser keeps the surface syntax; desugaring
turns it into an ordinary application, and class dispatch decides the meaning.

| Surface | Desugars to | Classes that may provide it |
|---|---|---|
| `a + b`, `a - b`, `a * b` | `(+) a b`, … | `Num a` (Int, Float, Buffer) |
| `a / b` | `(/) a b` | `Fractional a` (Float), `Buffer b` |
| `a % b` | `(rem) a b` | `Integral a` (Int) |
| `a == b`, `a /= b` | `(==) a b`, `(/=) a b` | `Eq a` |
| `a < b`, `<=`, `>`, `>=` | `(<) a b`, … | `Ord a` |
| `-a` (unary) | `negate a` | `Num a` |
| `a && b`, `a \|\| b` | **not desugared** | see below |

**`&&` / `||` are the exception.** Haskell 2010 §3.4 gives them short-circuit
semantics, which a plain function application cannot express. They stay distinct
AST nodes, but gain strict typing: `1.0 && 2.0` must be a compile-time type
error, not a runtime fault (verified today: it faults in the interpreter).

**Consequence — the `Buffer` case is a class, not a special branch.** rill
already declares `typeclass Buffer b` with instances `FixedBuffer a` and
`Tape f32` (empty bodies today). Those instances gain the arithmetic methods, so
`bufferA + bufferB` resolves through `Buffer b`'s `Num`-shaped methods exactly as
`1.0 + 2.0` resolves through `Num Float`. This removes the `lower.rs`
scalar-vs-buffer fork at `Expr::Arith` (~3356: `ValueBufferBin` vs `ValueAdd`)
and the parallel `ValueBufferUn` vs `ValueUnOp` fork: both collapse into one
method dispatch.

Class layering used by the desugaring:

```haskell
typeclass Buffer b where {
    -- element-wise arithmetic over a buffer
    (+) : b -> b -> b; (-) : b -> b -> b; (*) : b -> b -> b; (/) : b -> b -> b;
    min : b -> b -> b; max : b -> b -> b;
    abs : b -> b;
}
```

Instances (`FixedBuffer a`, `Tape f32`) implement these by emitting the existing
whole-buffer instruction (`ValueBufferBin`/`ValueBufferUn`) — the interpreter's
buffer kernels are unchanged; only their *reachability* changes from a syntax
branch to an instance body.

### 3.1 Class declarations (P-A/P2/P3)

Added to `CATEGORY_PRELUDE` (pure category theory), with instances in
`STD_PRELUDE`:

```haskell
typeclass Eq a where {
    (==) : a -> a -> Bool;
    (/=) : a -> a -> Bool = \ x -> \ y -> if x == y then False else True;
}
typeclass (Eq a) => Ord a where {
    compare : a -> a -> Int;              -- -1 / 0 / 1 (GHC's Ordering collapsed)
    (<)  : a -> a -> Bool = \ x -> \ y -> compare x y < 0;
    (<=) : a -> a -> Bool = \ x -> \ y -> compare x y <= 0;
    (>)  : a -> a -> Bool = \ x -> \ y -> compare x y > 0;
    (>=) : a -> a -> Bool = \ x -> \ y -> compare x y >= 0;
    min  : a -> a -> a = \ x -> \ y -> if x <= y then x else y;
    max  : a -> a -> a = \ x -> \ y -> if x <= y then y else x;
}
typeclass Show a where { show : a -> String; }
typeclass Num a where {
    (+) : a -> a -> a; (-) : a -> a -> a; (*) : a -> a -> a;
    negate : a -> a; abs : a -> a; signum : a -> a;
    fromInteger : Int -> a;
}
typeclass (Num a) => Fractional a where { (/) : a -> a -> a; recip : a -> a; }
typeclass (Num a) => Real a where { }          -- marker; toRational out of scope
typeclass (Real a, Enum a) => Integral a where {
    quot : a -> a -> a; rem : a -> a -> a; div : a -> a -> a; mod : a -> a -> a;
    toInteger : a -> Int;
}
typeclass (Real a, Fractional a) => RealFrac a where {
    floor : a -> Int; ceiling : a -> Int; round : a -> Int; truncate : a -> Int;
}
typeclass (Fractional a) => Floating a where {
    pi : a; exp : a -> a; log : a -> a; sqrt : a -> a;
    sin : a -> a; cos : a -> a; tan : a -> a;
}
typeclass Enum a where {
    succ : a -> a; pred : a -> a;
    enumFromTo : a -> a -> List a;
}
typeclass Bounded a where { minBound : a; maxBound : a; }
typeclass Semigroup a where { (<>) : a -> a -> a; }
```

Notes:

- **`compare` returns `Int`**, not GHC's `Ordering`, because the language has no
  `Ordering` data type on the value track. This is a recorded deviation
  (`DOCS-GAPS.md`), or `Ordering` is added as a three-constructor data type —
  the plan decides after checking what the data-type surface supports. The
  *semantics* must match `Ord` (-1/0/1 or `LT`/`EQ`/`GT`), only the encoding is
  rill's.
- **`Ord ⊑ Eq`** via the `(Eq a) =>` constraint form, which
  `infer_constrained_apply` already discharges.
- Operator method names (`(==)`, `(<)`, `(+)`) are how the parser's `Expr::Ref`
  for an operator resolves; the existing operator-value machinery from the
  conformance work already produces callables for `+ - * / min max`, so the
  class methods slot into the same path.

### 3.2 Instance strategy

Two kinds of instances, consistent with what exists:

1. **Primitive instances** (`Int`, `Float`, `Bool`, `String`, `List`, `Set`,
   `Map`, `Array`, records, sums, newtypes): `compare`/`==` delegate to the
   existing `value_cmp_ref`/`value_cmp` primitive. This is the *only* place the
   structural order lives; the interpreter's `ValueCompare` becomes an
   optimisation of the primitive instance, not the definition of comparison.
2. **Derived instances** (`derive_eq_ord`): already generated for every data
   type. They must now carry method bodies (or a `derived` marker the resolver
   understands) so `compare`/`==` dispatch to the structural comparator.

`derive_eq_ord` currently creates instances with `methods: HashMap::new()` and a
`from_prelude: true` marker. The plan's first step is to make the resolver treat
that marker as "structural primitive" rather than "no methods".

### 3.3 Where the operations live after the change

| Layer | Responsibility |
|---|---|
| Parser | operator sections, `[a..b]` desugaring (P4) |
| Inference | class constraints (already works), method-name resolution, `Ord ⊑ Eq` |
| Lowering | method call → `ValueCall` of the instance body; when the instance is a *primitive marker*, emit the existing `ValueCompare`/`BinArith`/`UnOp` instruction |
| Interpreter | primitive implementations (`value_cmp_ref`, `BinArith`, `UnOp`) — unchanged, now reached through instances |

This removes the duplication: `min`/`max`/`<`/`abs` stop having their own
bespoke lowering branches and become instance-driven.

### 3.4 Compatibility with the current test suite

§1.4 is the compatibility contract. Concretely:

- `==`/`<` on records and strings keep their results (they become `Ord`/`Eq`
  primitive instances).
- `mappend`/`(++)`/`(+)/(*)` first-class values keep working.
- `show_float`/`show_int`/`show_bool` keep working as aliases (P2) so existing
  tests and programs do not break; `show` is added alongside.
- The buffer path (`min _ 0.5`) is untouched: `Buffer` is a separate class
  marker and the signal-track operations are out of this design's rewrite.

---

## 4. Risks

| Risk | Mitigation |
|---|---|
| Retiring `ValueCompare` for dispatch regresses existing comparisons | Primitive instances delegate to the same `value_cmp_ref`; the existing tests are the gate |
| `compare` as `Int` vs GHC `Ordering` | Recorded deviation, or add `Ordering` — decided in the plan with a test pinning the chosen encoding |
| `Num` moving `+` to a method breaks the first-class operator work | The operator-value path already exists; methods are resolved through it |
| `Ord ⊑ Eq` constraint form unsupported | `infer_constrained_apply` already handles `(Monad m) =>` heads; verify with `(Eq a) => Ord a` |
| Numeric-tower instances multiply (Int/Float × Num/Fractional/Real/Integral/…) | Only `Int` gets `Integral`, only `Float` gets `Floating`; keep it minimal and let a missing instance be a clean error |
| Scope explosion | Phases P-A, P2–P5 are independently shippable; each ends green |
| `derive_eq_ord` markers not understood by the resolver | P-A adds the marker handling before any dispatch change |

---

## 5. Acceptance criteria

1. `typeclass Eq`, `Ord`, `Show`, `Num`, `Fractional`, `Integral`, `RealFrac`,
   `Floating`, `Enum`, `Bounded`, `Semigroup` are declared; `Ord`'s superclass is
   `Eq`, `Integral`'s are `Real`+`Enum`, `RealFrac`'s are `Real`+`Fractional`.
   `Buffer` gains its arithmetic methods and both existing instances implement
   them.
2. **Every binary operator desugars to a class method call**: there is no
   lowering branch keyed on the operator itself. `a + b` and `f (+) a b` take the
   same path. `&&`/`||` remain nodes with short-circuit semantics and strict
   `Bool` typing.
3. `min`/`max` work on `Float`, `Int`, `String`, records, and buffers, as
   functions and as first-class values (`fold max`), defined by `Ord`/`Buffer`
   (no bespoke lowering branch).
4. `compare`, `(/=)`, `sort`, `sortBy`, `maximum`, `minimum` work.
5. `show` works for scalars, lists, records and sums; the old `show_*` names
   still work.
6. `negate`, `signum`, `abs` (scalar and buffer), `fromInteger`, `pi`, `log`,
   `**`, `recip` work.
7. `div`, `mod`, `quot`, `rem`, `floor`, `ceiling`, `round`, `truncate`,
   `fromIntegral`, `toInteger` work; `succ`, `pred`, `enumFromTo`, `[a..b]`,
   `minBound`, `maxBound` work.
8. `foldr`, `foldl`, `concat`, `sum`, `product`, `elem`, `null` work over
   `List` and finite `Cons` chains.
9. `cargo test -p rill-lang --features dsp` green, including every test that
   passes today (baseline: 903 passing); `clippy` clean; `fmt` clean.
10. Every deliberate encoding deviation (e.g. `compare :: a -> a -> Int`) is
    recorded in `docs/superpowers/DOCS-GAPS.md`.
11. No operation has two live implementations after its phase lands (class
    method plus bespoke lowering branch for the same name).
