# Stage 7b — literal markers, `Ratio`, and the numeric kernels

> **Status:** design, APPROVED (2026-10-29). `Int32` deferred to benchmark results.
> **Predecessors:** stage 7a (`Integer`) — commits `5e2b082`…`498cb62`, docs
> `20d38d5`. Stage 3a (operators are class-method sugar), dictionary stages
> 1–2. Tracked defects: **OD-27** (defaulting), **OD-24** (`Integral`, `div`/
> `mod`/`rem`/`quot`).
> **Supersedes:** the never-implemented §3.6 of
> `2026-10-25-rill-lang-runtime-dictionaries-design.md` ("literal typing under
> dictionaries"), which this document replaces with a concrete design.

**Goal.** A literal is recognised as **one strict exact type** and only then
widened by a named function, exactly as Haskell 2010 §6.4.1 prescribes:

| Literal | Recognised as | Widened by |
|---|---|---|
| `42` | `Integer` | `fromInteger :: Integer -> a` |
| `0.1` | `Rational` (the exact fraction `1/10`) | `fromRational :: Rational -> a` |

The resulting type is decided by unifying that application's type variable with
the context. Where no context exists, `default (Integer, Double)` decides.

**Why this replaces 46 heuristics.** Stage 7a made literals work by threading an
"expected type" into position after position — 20 sites in `lower.rs`, 18 in
`infer.rs`, 4 in `reduce.rs`, 4 in `ast.rs`, 46 in total. Six review rounds
fixed six named sites and left the same defect at the next one, twice turning a
loud type error into a silently wrong number. The reason is structural: a literal
in `Expr::Int` had no type of its own, so every position had to guess. Once the
literal has its strict type and is widened by an ordinary method, every one of
those 46 sites becomes a place where a type variable simply unifies — or does
not. (The node is also renamed to say what it holds — §1.5.)

---

## 0. What this plan may NOT do (the user's constraint)

The user's directive for this stage: **do not recreate exact Haskell 2010
behaviour by workarounds.** Concretely, each of these is forbidden:

- **No fake class tower.** `Integral`, `Real`, `Enum`, `Bounded`, `Show` do not
  exist. Do not synthesise them, do not special-case their names, and do not
  write `Ratio`'s declarations as if `Integral a =>` were available.
- **No `matches!` list of "classes we pretend to have."** If a construct needs
  a class that is absent, the construct is absent and the absence is an **OD row
  with an owner**.
- **No silent structural instance for a type whose instances are semantic.**
  `Ratio`'s `Eq`/`Ord` are value comparisons (cross-multiplication). A derived
  structural instance is *wrong* (measured below) and must not be left in place
  "because it compiles".
- **No dual representation of one concept.** This is the B2 lesson
  (`ValueTy::App` + `ValueTy::Data`): one concept, one shape. `Rational` is
  `Ratio Integer` in the type system, or it does not exist yet.

Every departure from H2010 that remains after this stage is a row in section 6
with a **named owner**. A departure documented without an owner is a bug.

---

## 1. Measured facts the design rests on

All measured on this machine, GHC 9.10.3 unless stated.

### 1.1 Haskell 2010 semantics

| Fact | Measurement |
|---|---|
| Integer literal is `fromInteger (n :: Integer)` at `Num a => a` | `f :: Num a => a; f = 42` gives `42` at `Int`, `42` at `Integer`, `42.0` at `Double` |
| Float literal is `fromRational (r :: Rational)` at `Fractional a => a` | Report §6.4.1 |
| `default (Integer, Double)`, `Integer` tried first | `1 + 2 :: Integer` is `3`; `1 + 2 + 10^27` = `1000000000000000000000000003` (no overflow) |
| A float literal defaults to `Double` because `Integer` has no `Fractional` | `1.0 + 2.0` → `2.0` |
| `:%` is **not exported** from `Data.Ratio` | `print (1 :% 2)` → `Data constructor not in scope: (:%)` |
| `Ratio` normalizes through `%` | `numerator (3 % 6)` = `1`, `denominator` = `2` |
| Arithmetic normalizes too | `numerator (1%2 + 1%2)` = `1`, `denominator` = `1` |
| The sign lives on the numerator | `1 % (-2)` prints `(-1) % 2` |
| `Ratio`'s `Eq`/`Ord` are value comparisons | `1%2 == 2%4` → `True`; `compare (1%2) (1%3)` → `GT` |
| `Ratio Double` is rejected | `No instance for 'Integral Double' arising from a use of '%'` |

### 1.2 rill's current state (the baseline this design changes)

| Program | rill today | GHC | Note |
|---|---|---|---|
| `f :: Num a => a -> a; f n = n + 1; f 0.0` | `1.0` ✅ | `1.0` | signature polymorphism already works |
| `f n = n + 1; f 0.0` | type error | `1.0` | unannotated body defaults the parameter to `Int` |
| `show_integer (1 + 2)` | type error | `3` | integer default is `Int`, not `Integer` |
| `fromInteger 3` | type error: "expected type unknown" | `3` | `fromInteger` is registered `Int -> a`, and cannot resolve |
| `fromRational` | unknown identifier | works | absent |
| `negate 3.0`, `compare 1 2`, `recip 2.0`, `signum 3`, `abs (-3)` | type error: kernel has no IR instruction | works | design §3.9's six gaps |

### 1.3 Three Critical findings that constrain the design

These were found while preparing this document. Each is a measured defect in a
*candidate* design, and each shapes a decision below.

**(a) A derived structural instance for `Ratio` is wrong, silently.**
`data Ratio a = { num: a, den: a }` with `derive_eq_ord`'s automatic instance
gives, measured:

| Program | rill (structural) | GHC |
|---|---|---|
| `Ratio{num:1,den:2} == Ratio{num:2,den:4}` | **`False`** | `True` |
| `Ratio{num:1,den:2} > Ratio{num:1,den:3}` | **`False`** | `True` |

So `Ratio` must get explicit, value-comparing instances. Reason (c) below says
where they can be written.

**(b) A CONCRETE-head explicit instance loses to the derived one; a POLYMORPHIC
head wins.** Measured at HEAD, discriminating (the two values are equal in value
but not in representation, so a structural compare answers `False`):

| Instance written | `1/2 == 2/4` | `1/2 == 1/3` (control) |
|---|---|---|
| none (derived structural) | `False` ❌ | `False` ✅ |
| `instance Eq (Ratio Integer)` (concrete head) | **`False`** ❌ | `False` ✅ |
| `instance Eq (Ratio a)` (polymorphic head) | **`True`** ✅ | `False` ✅ |

So the concrete head is keyed as `Ratio Integer` while the derived marker is
keyed as `Ratio`, and the lookup prefers the marker. **The instances must be
written with a polymorphic head** — which is also what makes them sound (the
payload's own instances are the constraint H2010 writes as `Integral a =>`).
The `derive_eq_ord` skip rule is therefore **not** required for this; an earlier
draft of this document claimed it was, and that claim is withdrawn.

**(b2) Class DEFAULT bodies do not resolve — measured on a concrete head.**

| Program | Result |
|---|---|
| `instance Ord Q where { compare a b = … }` then `q1 < q2` | *"cannot resolve method `compare` of `Ord`: the argument type is not concrete"* |
| the same, then `min q1 q2` | *"field projection requires a record value"* |
| `min 1.0 2.0` at `Double` | *"primitive instance `Ord Double` has no kernel for method `min`"* |

So `Ord`'s `min`/`max` — class **defaults**, per the Prelude's own comment — are
unusable for every instance, primitive or explicit. Only `compare` (written
explicitly) works. Consequence for this design: `Ratio`'s `Ord` cannot rely on
defaults, and §2.5's kernel table must say what happens to `min`/`max`.
Recorded as assumption **A13** with its own step, because §3's acceptance table
otherwise implies the numeric classes are usable when they are not.

**(b3) The fractional nodes are both lossy by payload and dishonest by name.**
`Expr::Double(f64)` and `Pattern::LitDouble(f64)` each hold an `f64`, so `0.1` is
lost, and each is named after the *default type* rather than the literal's kind
(§1.5). They become `Expr::FractionalLit` / `Pattern::FractionalLit` carrying the
exact fraction. A pattern `f 3.5 = …` is legal H2010 and must match by exact value, so
the pattern node changes with the expression node (assumption A9 covers both).

**(b4) `type Rational = Ratio Integer` is an AST change, not a parser tweak.**
`Def::TypeAlias.target` is a `String` (`ast.rs:474`, `TypeName` at `ast.rs:11`)
and `TypeEnv::type_aliases` is a `HashMap<String, String>` (`ty.rs:603`), walked
as a string chain in `type_entry` (`ty.rs:2421`). Making the target a
`TypeExpr` therefore touches a **serialized** AST (`program_serde.rs` is plain
serde over `Program`), so the `program_json` round-trip contract is part of this
step's cost.

**(c) A polymorphic instance head works, and is the fix for both.** Measured:
`instance Eq (Ratio a) where { (==) x y = x.num * y.den == y.num * x.den; }`
answers `1/2 == 2/4` with **`True`**, and
`instance Num (Ratio a) where { (+) x y = Ratio { num: … }; }` answers
`(1/2 + 1/3).num` with `5`. So `Ratio`'s instances must be **polymorphic in the
payload**, which is sound because the bodies are pure field arithmetic (the
payload's own instances are the constraint, and that is exactly why H2010 writes
`Integral a =>`).

### 1.4 What GHC actually puts in the AST (measured, `-ddump-parsed-ast`)

GHC distinguishes literals by **kind**, not by type:

```
a = 42              →  HsIntegral  (IL (SourceText "42")   False 42)
b = (42 :: Int)     →  HsIntegral  (IL (SourceText "42")   False 42)
c = (42 :: Integer) →  HsIntegral  (IL (SourceText "42")   False 42)
d = 3.14            →  HsFractional(FL (SourceText "3.14") False (:% 314 1))
e = (3.14 :: Float) →  HsFractional(FL (SourceText "3.14") False (:% 314 1))
```

Three things follow, and they are the design's foundation:

1. **One node per literal KIND, not per type.** All three integer forms are one
   `HsIntegral`; the type is decided later, by the class method. So rill needs
   **one** node for integer literals and **one** for fractional literals — and
   `Int`/`Integer` are types, not literals.
2. **The source text is kept** (`SourceText "3.14"`) alongside the parsed value.
3. **The fractional payload is an exact ratio** (`:% 314 1`) — measured proof for
   A9: `fromRational` never sees an `f64`.

### 1.5 rill's AST node names are dishonest today

| Node | Holds | Name says | Verdict |
|---|---|---|---|
| `Expr::Int(BigInt, Span)` | an arbitrary-precision integer literal | `Int` — a fixed-width *type* | **wrong name**: it is not a value of type `Int` |
| `Expr::Double(f64, Span)` | an `f64` | `Double` | **wrong payload**: `0.1` is lost, and `Double` is the *default* type, not the literal's kind |
| `Pattern::LitInt` / `Pattern::LitDouble` | same | same | same two problems |

Renamed in this stage, matching GHC's kind-based split:

| New node | Holds | Meaning |
|---|---|---|
| `Expr::IntegerLit(BigInt, Span)` | the exact integer | an integer literal of kind `Integral` |
| `Expr::FractionalLit(Ratio, Span)` | the exact decimal fraction, plus the source text | a fractional literal of kind `Fractional` |
| `Pattern::IntegerLit` / `Pattern::FractionalLit` | same | same |

### 1.6 `i32` at the parameter boundary — measured defect

`i32` exists only in `rill-core`'s parameter API, not as a language type:

| Value | Width | Where |
|---|---|---|
| `ParamValue::Int(i32)` | 32 bits | `rill-core/src/traits/param.rs:174` — the host's parameter channel |
| `Value::Int(i64)` | 64 bits | `rill-lang/src/arena.rs` — the language's `Int` |
| `ValueTy::Int` | 64 bits | the language type `Int` |

They are joined **through `f64`**, which is lossy above 2^53:

```rust
ParamValue::Int(v) => *v as f64,   // backend/interp.rs:21
ParamValue::Int(i) => *i as f32,   // register.rs:42
```

> **Ruling (user, this stage): `i32` is a PARAMETER-BOUNDARY concern only.** No
> `Int32` language type is added, and `Int` stays 64-bit (Report §6.4 requires at
> least 30 bits and does not fix 64; GHC uses 64). The user's reasoning, recorded:
> *unlike floating point, integers suffer no real problem when precision is
> reduced* — so narrowing `i64 → i32` at the API is acceptable, while widening
> `i32 → i64` must be **lossless and type-preserving**, never through `f64`.
>
> **Why `Int32` is deferred, with the measurement that settled it** (user's
> question: "is i64 overkill for f32 buffers?"). Measured: `size_of::<Value>()`
> is **48 bytes and determined by the `Dictionary` variant** (two `Vec`s), not by
> `Int`; `Int(i64)` therefore costs **zero** bytes — an `i32` payload would not
> shrink the arena. The value track runs **once per program run**, not per
> sample (`exec_entry`), and buffer samples never see an integer (they are
> `T::from_f64`). So `Int32` buys no memory and no hot-path speed, and H2010 has
> no such type. **Decision (user): defer `Int32` until real benchmarks show a
> need.** If they do, it is an additive new scalar leaf — it can reuse the same
> registry, kernels, and literal machinery this stage builds, so deferring costs
> nothing. Note the same-width argument already applies to floating point and is
> equally deferred: the language's `Double` is wider than the `f32` backend
> buffer, and `Float` is P3.
>
> **Assumption A14 (owner: this stage).** `ParamValue::Int(i32)` reaches the
> language as `Value::Int(i64)` by widening, and never as a `Double`. Observable
> today: a parameter above 2^53 loses its low bits. `ValueTy::Int` remains the
> only integer *language* type; `Int32` is not introduced.

---

## 2. Design

### 2.1 A literal is a strict exact value, then `fromInteger`/`fromRational`

**This supersedes two earlier drafts of this section.** Draft 1 proposed a
"wanted constraint" store in `Ctx`; draft 2 proposed the literal *as* a lazy
thunk. Both over-built. The measured model is simpler and is exactly what the
Report says: **a literal is recognised as one strict, exact type first, and only
then widened by a named function.**

| Literal | Recognised as | Widened by | Resulting type |
|---|---|---|---|
| `42` | `Integer` | `fromInteger :: Integer -> a` | `Num a => a` |
| `0.1` | `Rational` (the exact decimal fraction `1/10`) | `fromRational :: Rational -> a` | `Fractional a => a` |

So there is **no guessing**: the literal has one strict type at parse time, and
every position it can occupy is reached by a class-polymorphic function whose
type variable unification decides — the ordinary machinery rill already has.

#### What this means concretely in rill

**Integer literals need a RENAME, not a representation change.** Stage 7a
already made the node hold `BigInt` — that *is* the strict `Integer` value — but
the node is called `Expr::Int`, which names a fixed-width *type* for a value that
is not one (§1.5). It becomes `Expr::IntegerLit`. GHC's own AST makes the same
split by kind (`HsIntegral` / `HsFractional`, measured §1.4), so `Int` and
`Integer` remain **types**, never literal nodes.

**Fractional literals do.** `Expr::Double(f64)` stores an `f64`, and `f64` cannot
represent `0.1` exactly. Measured in GHC:

```
0.1 :: Rational            → 1 % 10
toRational (0.1 :: Double) → 3602879701896397 % 36028797018963968
```

`fromRational` receives the FIRST. So the node must carry the exact decimal
fraction (`num`/`den` from the literal's digits), not an `f64`. This is a
required representation change (assumption A9), not a nicety.

#### Why the type variable unifies and does not get guessed

```
Expr::IntegerLit(42)          ⟹  fromInteger (42 :: Integer)   -- type: fresh a, Num a
Expr::FractionalLit(314/100)  ⟹  fromRational (314/100)        -- type: fresh a, Fractional a
```

Every case then follows from unification of that one variable:

| Situation | What decides the type | Mechanism already present? |
|---|---|---|
| `show_int 3` (expected `Int`) | the callee's parameter type unifies `a := Int` | yes — measured working today |
| `show_integer 3` (expected `Integer`) | `a := Integer` | yes — measured working today |
| `1 + 1.0` | `(+) :: Num a => a -> a -> a` puts BOTH literals at ONE `a`; `Fractional ⊆ Num` leaves `Fractional a`, so defaulting picks `Double` | yes — unification, not a conversion |
| `f n = n + 1` | `n`'s variable unifies with the literal's, so the scheme becomes `Num a => a -> a` | yes — this is why the unannotated case currently gets `Int` and must become polymorphic |
| `f :: Num a => a -> a; f x = x + fromInteger 1` | the GIVEN dictionary | yes — measured: this one already *resolves* and fails only on the missing kernel |
| `1 + 2`, no context | nothing unifies `a`, so **defaulting** (2.3) picks `Integer` | **no — this is the one real gap** |

#### The pieces that are genuinely missing, measured

A builder-check corrected this document's first diagnosis. The gap is **not**
"result-directed dispatch exists only for the bare class variable": that
dispatch already reaches `Int -> a` (`sig_result_is_bare_class_var` recurses
through `TFunc` at `types/infer.rs:1379`). Two **different** defects are
measured:

**(i) The primitive-instance branch returns the unapplied arrow.**
`(fromInteger 1) :: Integer` → *"cannot unify value type `Thunk([Int],
[Integer])` with `Integer`"*; `x :: Int; x = fromInteger 1` → same shape with
`Int`. The primitive path returns `nest_thunk(&param_tys, vec![ret])` — the whole
*arrow* — where the source-instance path returns the body's result type
(`types/infer.rs:6425` vs its source counterpart). So the type of an applied
primitive method is its own signature, not its result.

**(ii) A builtin's declared parameter type is not threaded into its arguments.**
`show_int (fromInteger 1)` → *"cannot resolve method `fromInteger` of `Num`:
expected type unknown"*, even though `show_int`'s parameter is concretely `Int`.
The expected type is threaded only for `IO`/`Thunk` parameters or through
`int_literal_widens_to` (`types/infer.rs:5836-5849`, `7688`).

Neither defect needs a constraint store. Both are ordinary inference fixes, and
they are what this stage actually buys: with them, `fromInteger`/`fromRational`
become normal methods and **the literal is their ordinary application**.

> **Correction recorded.** An earlier draft named a third gap ("only the bare
> class variable shape") and a fourth ("`fromInteger` mentions the class variable
> only in its result"). Both are wrong; the recursion at `infer.rs:1379` already
> handles the shape. The error was mine, not the code's.

#### What this deletes

The heuristics exist to guess a type the marker now supplies. They become
**absent**, not "simplified":

`annotate_formal_arg_types`, `scheme_pinned_params`, `declared_binding_ty`,
`closure_param_tys`'s placeholder branch, `int_literal_widens_to`,
`is_int_literal`, `numeric_leaf_of`, `def_arg_widens_to`'s literal arm, the
`Expr::Annot` literal special case, and every `matches!(expected,
Some(ValueTy::Double | ValueTy::Integer))` literal arm.

Deletion is an acceptance criterion: after this stage
`grep -c 'is_int_literal\|int_literal_widens_to\|annotate_formal_arg_types\|scheme_pinned_params\|declared_binding_ty\|numeric_leaf_of'`
over `rill-lang/src` is **0**.

#### Why not the thunk (draft 2) and why not a constraint store (draft 1)

- **No constraint store.** The literal's type variable *is* the constraint's
  carrier; the existing instance registry and `Subst` already unify and
  discharge it. A second store would be one more thing to keep in sync with
  `Subst` — the same defect class as the dual `App`/`Data` representation.
- **No thunk.** A thunk is the *runtime* realisation of laziness, not the
  typing mechanism. Making the marker a thunk would force a runtime
  representation decision into the type checker, and it buys nothing: the type
  variable unifies identically either way.

> **Assumption A1 (owner: none — genuine §6.4.1).** A literal is recognised as
> one strict exact type (`Integer`, or `Rational` from its decimal digits) and
> widened by `fromInteger`/`fromRational`; its type is decided by unifying that
> application's type variable.
>
> **Assumption A10 (owner: none — see 2.3).** With no unifying context, the type
> variable is ambiguous and defaulting chooses `Integer` (integer literal) or
> `Double` (float literal, because `Integer` has no `Fractional` instance).

### 2.2 `Ratio` and `Rational`

Declared in the `PRELUDE` as an ordinary parameterized record:

```
data Ratio a = { num: a, den: a };
type Rational = Ratio Integer;
```

with **polymorphic** instances (finding 1.3c):

```
instance Eq (Ratio a) where { (==) x y = x.num * y.den == y.num * x.den; }
instance Ord (Ratio a) where {
    compare x y = if x.num * y.den > y.num * x.den then 1
                  else if x.num * y.den == y.num * x.den then 0 else -1; }
instance Num (Ratio a) where {
    (+) x y = Ratio { num: x.num * y.den + y.num * x.den, den: x.den * y.den };
    (-) …; (*) x y = Ratio { num: x.num * y.num, den: x.den * y.den };
    negate x = Ratio { num: -x.num, den: x.den };
    abs x = …; signum x = …;
    fromInteger n = Ratio { num: n, den: 1 }; }
instance Fractional (Ratio a) where {
    (/) x y = Ratio { num: x.num * y.den, den: x.den * y.num };
    recip x = Ratio { num: x.den, den: x.num };
    fromRational r = r; }
```

Two prerequisites are measured to be missing and are **in scope**:

1. **`type Rational = Ratio Integer` does not parse.** `parse_type_alias_def`
   (`parser.rs:1565`) requires the target to be a single identifier
   (`expect_ident`). It must accept a type expression. This is a parser fix, not
   a language extension.
2. **`derive_eq_ord` must not derive an instance for a type that already has an
   explicit one** — it currently derives by bare name, which shadows the
   polymorphic head (finding 1.3b). The rule becomes: skip any name that has a
   user/Prelude-written instance for that class, including a polymorphic head.

`numerator`/`denominator` are builtins over the record fields
(`Ratio a -> a`), and `%` is a builtin smart constructor
(`a -> a -> Ratio a`) that **normalizes** (2.4).

> **Assumption A2 (owner: OD-24 / P4).** `Ratio`'s payload is unconstrained.
> H2010 writes `data (Integral a) => Ratio a`, so `Ratio Double` is ill-formed
> there (measured: `No instance for 'Integral Double'`). rill has no `Integral`,
> so our `Ratio a` accepts any `a`; a `Ratio Double` program compiles here and
> is rejected by GHC. Cost: a divergence, precisely scoped, removed when OD-24
> lands.
>
> **Assumption A3 (owner: OD-24 / P4).** `Rational` is a **synonym** of
> `Ratio Integer`, not a distinct leaf — so the B2 class of dual representation
> does not arise. The synonym needs the parser fix above; until that fix lands
> inside this stage, `Rational` may not be written by a user in a signature,
> which would be a usability regression and is therefore not acceptable — the
> parser fix is required, not optional.
>
> **Assumption A4 (owner: OD-24 / P4).** `%`'s real signature is
> `Integral a => a -> a -> Ratio a`. Ours is `Integer -> Integer -> Ratio Integer`
> (2.4 explains why `Rational`/`Ratio Integer` and not `Int`). When `Integral`
> arrives the signature generalizes — an additive change.

### 2.3 Defaulting — the one place where a type must be CHOSEN

Everything in 2.1 is unification. Defaulting is the single case where there is
nothing to unify against, and the compiler must pick.

After inference finishes a **binding**, a type variable that is still
**ambiguous** — an unconstrained variable that the binding's own type does not
mention — is resolved by Haskell 2010 §4.3.4's four conditions:

1. the variable has at least one class constraint;
2. every constraint on it is a standard numeric class (`Num`, `Fractional`, …);
3. it is ambiguous (not reachable from the binding's type);
4. the default list is tried in order and the first type satisfying **all** the
   variable's constraints wins.

The default list is **`default (Integer, Double)`** — §4.3.4's list, and exactly
what OD-27 records as missing.

| Program | The variable | Constraints | Default |
|---|---|---|---|
| `1 + 2` | ambiguous | `Num a` | `Integer` (`Integer` is `Num`) |
| `1.0 + 2.0` | ambiguous | `Fractional a` | `Double` (`Integer` is not `Fractional`) |
| `1 + 2 :: Int` | **not** ambiguous (the annotation fixes it) | — | `Int`, no defaulting |
| `f :: Num a => a -> a; f = 1 + 2` | **not** ambiguous (the signature binds it) | `Num a` | stays polymorphic — **no defaulting inside a constrained body** |

That last row is the one that makes defaulting a step of its own: the current
code applies its default at the place the class variable happens to be solved,
which is why `f :: Num a => a -> a; f = n + 1` currently *works* while an
unannotated `f n = n + 1` gets `Int`. Defaulting belongs at the binding's
generalisation point, and nowhere else.

**Measured honesty about that point.** rill has **no** generalisation-point
defaulting today, and no store of residual constraints: `def_scheme`
(`types/infer.rs:3460-3498`) builds a scheme from a *signature*
(`scheme_constraints`) and returns `Vec::new()` for an unconstrained definition;
`Ctx`'s full field list contains no wanted/pending set. The only defaulting is
the use-site `Double` unification at `types/infer.rs:6648-6660`.

So this section is **not** "move an existing default". It requires a store of
residual numeric constraints plus a pass at the binding's close. That is a real
addition, and it is the one place where this design contradicts the earlier
draft's "no constraint store" claim — the store is needed for defaulting, and for
nothing else. What it is **not** needed for is literal typing: A1's literal gets
its type from unifying `fromInteger`/`fromRational`'s variable, which needs no
store at all.

> **Correction recorded.** An earlier draft said "no constraint store". That is
> right for typing and wrong for defaulting: §4.3.4 needs to know which
> variables are still ambiguous at a binding's close, and nothing in `Ctx` can
> answer that today.

The smallest honest shape is a `Vec<(String, TypeVarId)>` of residual numeric
constraints plus their span, consulted once per binding group — not a general
solver.

**The row above is not a legal program as written.** Measured:
`f :: Num a => a -> a; f = 1 + 2` is rejected — *"signature of `f` declares 1
argument(s), but the definition binds 0"*. The row means `f :: Num a => a; f = 1 + 2`;
the acceptance test in §5 must use that spelling.

**Scope of this stage:** the default list is fixed at `(Integer, Double)`. A
user `default (...)` declaration is not read (non-goal).

> **Assumption A5 (owner: none — genuine §4.3.4).** Defaulting is applied at a
> binding's generalisation point to ambiguous numeric variables, in the order
> `Integer`, then `Double`, first satisfying type winning.
>
> **Assumption A11 (owner: a later item).** A user-written `default (...)` is
> ignored; only the fixed list is used. Observable only in a program that writes
> `default (Int, Double)`, which this stage does not support.

### 2.4 Normalization (`%`, `fromRational`, arithmetic)

H2010 normalizes: `numerator (3%6)` is `1`, and `1%2 + 1%2` has `numerator 1`.
Our arithmetic bodies above do **not** normalize (`1/2 + 1/2` → `2/4`).

Two options, and the choice is deliberate:

- **(i) Normalize in every instance body** — requires `gcd`, which is
  `Integral a => a -> a -> a`; we would need `gcd :: Integer -> Integer -> Integer`
  first, and every body would carry a normalization call.
- **(ii) Normalize only at construction** (`%`, `fromInteger`, `fromRational`,
  and — this is the divergence — **not** in `+`/`-`/`*`/`/`), and define
  `Eq`/`Ord` by cross-multiplication so equality is correct even when the value
  is unreduced.

**Decision: (ii), with `gcd` as a builtin at `Integer`.**

Rationale: (i) needs `Integral` for `gcd`, which we do not have — implementing
`gcd` only at `Integer` and calling it from every body is the same hard-coding
either way, so (i) buys nothing over (ii) for arithmetic while making every body
depend on a signature it cannot honestly have. (ii) keeps equality *correct*
(cross-multiplication is invariant to reduction) and confines the divergence to
`numerator`/`denominator` of an unreduced intermediate.

> **Ruling (revised after builder-check): A6 is withdrawn for the `Integer`
> payload.** A6 proposed leaving `+`/`-`/`*`/`/` unnormalized and recording the
> divergence. The user's constraint says a documented deviation is still an
> error, and for this payload the conformant route is available: `gcd` at
> `Integer` is a plain function, and calling it in the four bodies is *correct
> H2010 behaviour* for `Ratio Integer`, not a workaround. So the bodies
> normalize:
>
> ```
> (+) x y = let n = x.num * y.den + y.num * x.den
>               d = x.den * y.den
>               g = gcd n d
>           in Ratio { num: n / g, den: d / g };
> ```
>
> with `den` kept positive (H2010's sign convention, measured: `1 % (-2)` prints
> `(-1) % 2`). `numerator (1%2 + 1%2)` then answers `1`, as GHC does.
>
> **Assumption A6 (owner: OD-24 / P4)** now covers only what genuinely remains:
> `gcd`'s signature is `Integer -> Integer -> Integer`, not
> `Integral a => a -> a -> a`, so the bodies are written for the `Integer`
> payload. A `Ratio` over any other payload would need `Integral` — the A2
> divergence. Observable only for a non-`Integer` payload, which this stage does
> not provide.

**`Rational`'s representation.** `Ratio Integer` of `BigInt` fields: exact, and
the reason `Rational` cannot be a `Double`-backed leaf. Parsing `3.14`:
the marker converts the decimal text of `Expr::FractionalLit` to an exact
`Ratio Integer` (`314/100`, normalized to `157/50`), so `fromRational` rounds
**once** at the target type — identical to GHC. A `Double`-backed literal would
round twice and diverge.

> **Assumption A9 (owner: none — genuine).** The literal's decimal text is
> carried as `Ratio Integer`, not as an `f64`. `Expr::Double(f64)` (renamed
> `Expr::FractionalLit`, §1.5) currently
> holds an `f64`; it must hold the decimal digits or an exact ratio so no
> information is lost before `fromRational`. This is a required representation
> change, not a nicety: `0.1` at `Rational` must be `1/10`, not
> `3602879701896397/36028797018963968`.

### 2.5 The six kernels (design §3.9)

All six get IR instructions in this stage, since `Num`'s and `Ord`'s methods are
unusable without them and `fromInteger` is the marker's own target:

| Kernel | IR instruction | Semantics |
|---|---|---|
| `FromInteger` | `ValueFromInteger { dst, src, target }` | `Integer → a`. **`target` is required**: the kernel cannot infer the instance's type from an unbound `dst` register, and without it the instruction cannot tell `Integer` from `Double` from `Ratio` |
| `Negate` | `ValueNegate { dst, src }` | `x ↦ -x` for all four numeric types (scalar and buffer) |
| `Signum` | `ValueSignum { dst, src }` | `-1/0/1` per §6.4.4 |
| `Abs` (scalar) | `ValueAbs { dst, src }` | `|x|` (the buffer form already exists) |
| `Compare` | `ValueCompare3 { dst, a, b }` | three-way result: `-1/0/1` |
| `Recip` | `ValueRecip { dst, src }` | `1 / x` |

`Compare` returns `Int` in rill's `Ord.compare` (documented divergence:
H2010 returns `Ordering`; there is no `Ordering` type yet).

**Where `Int` truncation lives.** Exactly one place: lowering's
`literal_to_i64` (`lower.rs:9459-9472`, whose own doc says "Nothing below this
point sees a `BigInt` on the `Int` track"). `ValueFromInteger` therefore does
**not** truncate for an `Int` target — lowering emits `ValueConstInt` there.
The kernel serves `Integer`, `Double`, and `Ratio` only. Two places doing the
same conversion would be the dual representation §0 forbids.

**`Value` needs no new variant for `Ratio`.** A `Ratio Integer` is an ordinary
arena `Record` of two `Integer` leaves (`Value::Integer` each), so §2.2's source
instances operate on ordinary field projections — the same representation the
type says. That is why §7's `arena.rs` entry reads "no new value variant"; an
earlier draft's "(maybe a `Ratio` value)" is withdrawn.

**Recording A13.** `Ord`'s `min`/`max` are class DEFAULTS and do not resolve
today for any instance (measured, finding b2). This stage therefore leaves them
broken rather than adding two kernels that would duplicate a default body. If
`Ratio`'s `Ord` needs `min`/`max` before A13 is fixed, its instance spells them
out — recorded in the task, not silently.

> **Assumption A13 (owner: a default-body stage).** `Ord`'s `min`/`max` — and
> any other class default that calls a sibling method — do not resolve. Language
> `Ord` comparisons (`<`, `>`, `<=`, `>=`, `compare`) work when `compare` is
> written; `min`/`max` do not.

> **Assumption A8 (owner: a `Show`/`Enum` stage).** `Ord.compare` returns `Int`
> (−1/0/1) where H2010 returns `Ordering` (`LT`/`EQ`/`GT`). Pre-existing,
> documented, and unchanged by this stage; it is listed here so the new
> `Compare` instruction is not mistaken for a fix.

---

## 3. What this buys, measured as acceptance

| Program | Today | After |
|---|---|---|
| `show_integer (1 + 2)` | type error | `3` (OD-27 half b closes) |
| `1 + 2 + 10^27` | overflow / type error | exact |
| `fromInteger 3 :: Int` | type error | `3` |
| `fromRational (1/2)` | unknown identifier | `0.5` at `Double` |
| `f n = n + 1; f 0.0` | type error | `1.0` |
| `f n = n + 1; f (2::Integer)` | type error | `3` (one body, two types) |
| `negate 3.0` | type error | `-3.0` |
| `compare 1 2` | type error | `-1` |
| `recip 2.0` | type error | `0.5` |
| `signum (-3)` / `abs (-3)` | type error | `-1` / `3` |
| `1/2 == 2/4` at `Rational` | n/a | `True` |
| `numerator (3 % 6)` | n/a | `1` |
| `(1 :: Int) + 1.0` | type error | type error (unchanged — `Fractional Int`) |
| `(1 :: Int) + (2 :: Integer)` | type error | type error (unchanged) |
| `Ratio Double` accepted | — | accepted (A2 divergence) |

And the deletion criterion: after this stage,
`grep -c 'is_int_literal\|int_literal_widens_to\|annotate_formal_arg_types\|scheme_pinned_params\|declared_binding_ty\|numeric_leaf_of'`
over `rill-lang/src` is **0**.

---

## 4. Non-goals

- `Integral`/`Real`/`Enum`/`Bounded`/`Show` classes — OD-24, and the `Show`
  stage. A2/A6/A8/A11/A13 stay open until then.
- `Float` single precision — P3.
- `Complex` — not in the Prelude proper.
- User `default (...)` declarations — noted, not implemented.
- `^`, `^^`, `**`, `fromIntegral`, `realToFrac`, `toInteger`, `toRational`,
  `properFraction`, `round`/`floor`/`ceiling`/`truncate`, `gcd`/`lcm`,
  `even`/`odd` — all require `Integral`/`Real`/`RealFrac`; each is an OD row
  unless already recorded.
- `seq`, `$!` — unrelated.
- Relaxing `Ratio`'s payload constraint — see A2.

---

## 5. Risks and their measurement plan

| Risk | Why it is real | How it is measured |
|---|---|---|
| The marker pass touches every literal site | 46 heuristics exist; deleting them changes inference everywhere | `1 == 1.0`, `f 2` at `Double`/`Integer`/`Int`, `Ix` offsets stay `Int`, array indices, `show_*`, patterns |
| Defaulting fires too early | §4.3.4 is at the generalisation point; doing it at the use site re-creates OD-27 | `f :: Num a => a -> a; f = 1 + 2` must NOT default inside the body |
| Defaulting fires too late | a `main` of `1 + 2` must run | `1 + 2` alone compiles and runs as `Integer` |
| `Ratio`'s explicit instances lose to derived ones | measured (1.3b) | `1/2 == 2/4` → `True`; `compare (1/2) (1/3)` → `1` |
| Structural `Ord` sneaks back | measured (1.3a) | `Ratio{1,2} > Ratio{1,3}` → `True` |
| Literal text lost before `fromRational` | the fractional node holds an `f64`, lossy for `0.1` | `0.1 :: Rational` → `1/10`; `toRational` deferred, so assert via `numerator`/`denominator` of `fromRational`-elaborated value |
| A class tower is faked | the user's explicit constraint | grep for `Integral`/`Real`/`Enum` in `src`: must appear only in comments citing OD-24 |
| Regressions in the 1157-test suite | 46 sites change | full gates per task |

---

## 6. Assumptions and departures, each with an owner

Everything below is a **recorded assumption**, not silent scaffolding. The first
block is genuine H2010 semantics (an "owner: none" row means the mechanism *is*
the Report's); the second is a departure someone must close later.

### 6.1 Genuine H2010 semantics implemented here

| # | What | Where |
|---|---|---|
| A1 | A literal is a strict exact value (`Integer` / `Rational` from its digits) widened by `fromInteger`/`fromRational`; its type is decided by unifying that application's variable (§6.4.1) | 2.1 |
| A5 | Defaulting is a pass at the binding's generalisation point, not a use-site default (§4.3.4) | 2.3 |
| A9 | `Expr::FractionalLit` carries the literal's exact decimal fraction (`Ratio Integer`) and its source text, not an `f64`, so `fromRational` rounds exactly once (§6.4.1) | 1.4, 2.1 |
| A10 | With no context, defaulting chooses `Integer` for an integer literal and `Double` for a float one | 2.1 |

### 6.2 Departures, to be closed later

| # | Departure | Owner | Observable? |
|---|---|---|---|
| A2 | `Ratio a` carries no `Integral a =>` context, so `Ratio Double` is accepted where GHC rejects it | OD-24 / P4 | yes: `Ratio Double` compiles |
| A3 | `Rational` is `Ratio Integer` (a synonym) — needs the alias parser fix. If that fix slips, `Rational` is unwritable and that is a **blocker**, not a departure | this stage | — |
| A4 | `%` is `Integer -> Integer -> Ratio Integer`, not `Integral a => a -> a -> Ratio a` | OD-24 / P4 | yes: `%` at `Int` rejected |
| A6 | `gcd` is `Integer -> Integer -> Integer`, not `Integral a =>`, so `Ratio` arithmetic is written for the `Integer` payload | OD-24 / P4 | only for a non-`Integer` payload |
| A13 | `Ord`'s `min`/`max` class defaults do not resolve (`instance Ord Q where { compare … }` then `q1 < q2` → "argument type is not concrete"; `min 1.0 2.0` → "no kernel for method `min`") | a default-body stage | yes: `min`/`max` unusable, `min`/`max` of any type is a type error |
| A8 | `Ord.compare` returns `Int`, not `Ordering` | `Show`/`Enum` stage | yes |
| A11 | A user-written `default (...)` is ignored; only the fixed `(Integer, Double)` is used | later item | only for programs that write `default (...)` |
| A14 | `i32` stays a parameter-boundary concern: no `Int32` language type; `ParamValue::Int(i32)` widens to `Value::Int(i64)` losslessly (fixed **in** this stage, listed here so the decision is explicit) | this stage | today a parameter above 2^53 loses low bits via `f64` |

### 6.3 What the user's constraint forbids, restated as a review check

This stage must not: synthesise `Integral`/`Real`/`Enum`/`Bounded`/`Show`; list
"classes we pretend to have" in a `matches!`; leave a derived structural
instance on a type whose instances are semantic; or represent one concept two
ways. A reviewer finding any of these in the diff should reject the task — the
row it belongs to is in 6.2, and its owner is named there.

---

## 7. Sizing and order of work

This is a project with a written design, not a small change.

**Files and what they owe:**

| File | Change |
|---|---|
| `types/infer.rs` | (i) the primitive-instance branch returns the applied RESULT, not the unapplied arrow (finding 1.3-i); (ii) a builtin's declared parameter type is threaded into its arguments (1.3-ii); (iii) a residual numeric-constraint store + the defaulting pass at a binding's close (2.3) |
| `types/ty.rs` | `Ratio`/`Rational` in PRELUDE, polymorphic instances; `fromInteger : Integer -> a`; `fromRational` on `Fractional`; `%`/`numerator`/`denominator`; `type_aliases` value becomes a resolved type |
| `ast.rs` | `Def::TypeAlias.target` becomes a `TypeExpr`; `Expr::Int` → `Expr::IntegerLit`; `Expr::Double` → `Expr::FractionalLit` (exact fraction + source text); patterns likewise |
| `parser.rs` | alias target is a type expression; `%` joins the fixity table (`Tok::Percent` is lexed at `lexer.rs:547` and unused) |
| `lexer.rs` | keep the literal's source text for exact decimal conversion |
| `lower.rs` | delete the heuristics; emit the `fromInteger`/`fromRational` application; `literal_to_i64` stays the ONLY `Int` truncation boundary |
| `reduce.rs` | delete `annotate_formal_arg_types` **only if** measurement shows the expected type now arrives; it exists to carry a type β-reduction loses |
| `ir.rs` / `backend/interp.rs` | six instructions, with `ValueFromInteger { dst, src, target }` |
| `render.rs` / `program_serde.rs` | the changed literal payloads round-trip |
| `tests/` | any test asserting `1 + 2` is `Int` now asserts `Integer`; do **not** loosen the comparison, or the divergence becomes unpinnable |

**Order of work (each step independently green):**

1. `type` alias target is a type expression (AST + parser + serde). Unblocks A3
   and is the only serialization-contract change.
2. The two inference defects 1.3-i and 1.3-ii. These are the real unblocking
   step: after them `fromInteger`/`fromRational` are ordinary methods.
3. `Ratio`/`Rational` in the PRELUDE with polymorphic instances, `get` and
   normalization via `gcd`. (Closes findings 1.3a/b and A6.)
4. `%`, `numerator`, `denominator` as ordinary Prelude definitions over the
   record fields (`numerator r = r.num`) — a builtin would be a second mechanism
   beside `.num`.
5. The six kernels (§2.5). Independent of the literal work and unblocks `Num`'s
   methods.
6. Exact fractional literals: `Expr::Double` → `Expr::FractionalLit` and
   `Pattern::LitDouble` → `Pattern::FractionalLit`, carrying the exact decimal
   fraction and the source text (A9, §1.4).
7. Literals become `fromInteger`/`fromRational` applications; delete the 46
   heuristics. **This cannot land before step 2**: `int_literal_widens_to` is
   today the only reason `show_double 2` compiles (`infer.rs:5844`), so deleting
   it before the parameter-type threading exists would turn six GHC-conformant
   tests in `tests/integer_type.rs` and one in `tests/dictionaries.rs` into
   failures. Those tests assert GHC's values and may not be weakened.
8. Defaulting `(Integer, Double)` at the binding's close (2.3).
9. `ParamValue::Int(i32)` reaches the language as `Value::Int(i64)` by widening,
   not through `f64` (`backend/interp.rs:21`, `register.rs:42`) — A14. Small and
   independent of the literal work; it can land any time after step 1.
10. Docs: OD-27 closed; A2/A4/A6/A8/A11/A13/A14 recorded.

**Not in this stage, and why:** A13 (`min`/`max` defaults), A2/A4/A6 (`Integral`
class and `gcd`'s signature), A8 (`Ordering`), A11 (user `default`), `Float`,
`Complex`, `fromIntegral`/`toInteger`/`toRational`/`round`/`floor` — each needs a
class or type this stage does not add.

**The user's constraint, restated as a review gate.** A reviewer must reject the
task if the diff: synthesises `Integral`/`Real`/`Enum`/`Bounded`/`Show`; adds a
`matches!` list naming classes we pretend to have; leaves a derived structural
instance on a type whose instances are semantic; or represents one concept two
ways (a `Ratio`-shaped type in the type system and a leaf at runtime, two places
truncating `Integer` to `Int`, two mechanisms for reading a field). Each
remaining departure is named in 6.2 with its owner — a departure without an
owner is a bug, not a plan item.
