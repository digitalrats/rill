# rill-lang: `Bool` as a real data type (true/false constructors)

> **Status:** planned — one decision needed before implementation (see
> "The decision" below).
> **Blocks:** OD-25 (`&&`/`||` must be lazily-evaluating functions, not an IR
> primitive), and the general rule that a Haskell 2010 data type is spelled the
> way the Report spells it.

## The defect

`Bool` is a **primitive** in rill, not a data type:

| Layer | Today |
|---|---|
| `ValueTy` | `Bool` — a leaf variant alongside `Int`/`Double`/`String` |
| `Value` | `Value::Bool(bool)` — an inline payload |
| syntax | `true` / `false` are **keywords** (`Tok::KwTrue`/`KwFalse`), not constructor names |
| patterns | `Pattern::LitBool`, lowered to a literal test (`emit_literal_test`) |
| constructor names | `True` / `False` **do not exist** — `unknown constructor 'True'` (verified) |

Haskell 2010 §4.1.1 declares:

```haskell
data Bool = False | True
```

`False` and `True` are **constructors of a data type**, defined in the Prelude.
Consequences of rill's divergence, all measured:

| Consequence | Evidence |
|---|---|
| `True` / `False` unusable in patterns | `myAnd True y = y` → `unknown constructor 'True'` |
| a user cannot define `&&` from equations | `True && y = y; False && _ = False` is not expressible |
| `&&`/`||` had to become an IR primitive | `ValueInstr::ValueLogic` forces both operands (`interp.rs:1788`) → **OD-25**: no short-circuit where GHC prints `False` for `False && error "x"` |
| `if` over a user-defined boolean-like type is impossible | `if` requires `ValueTy::Bool` exactly |

## Why this blocks OD-25

The correct definition of `&&` is the Report's own:

```haskell
True  && y = y
False && _ = False
```

Its short-circuit is not a special mechanism: `False && _` ignores the second
argument, and rill **already** evaluates arguments lazily (measured:
`(\y -> 1.0) (loop 1.0)` → `1.0`, `const 1.0 (loop 1.0)` → `1.0`). So once
`True`/`False` are real constructors, `&&`/`||` become ordinary Prelude
functions, `Expr::Logic` and `ValueInstr::ValueLogic` are deleted, and the
short-circuit falls out of laziness — no branch lowering needed.

## The decision: how far to take it

`ValueTy::Bool` + `Value::Bool` are used in 37 + 14 `src` sites and 4 + 15 test
sites; `Expr::Bool` in 31, `Pattern::LitBool` in 7. There are two credible
shapes, and they differ in what "Bool is a data type" actually means.

**Option A — nominal only (smaller).** `Bool` stays a primitive *representation*
but becomes a *declared* data type with two constructors: `data Bool = False |
True` in the Prelude, `True`/`False` accepted as constructors, constructor
patterns usable, and `true`/`false` kept as **aliases of the constructors**
(the Report's own reading — it defines `Bool`, not the keywords rill invented).
`ValueTy::Bool` and `Value::Bool` stay as the representation. `&&`/`||` become
Prelude functions over constructor patterns.

**Option B — fully erased (larger).** `ValueTy::Bool` and `Value::Bool` are
removed entirely; `Bool` becomes `ValueTy::Data("Bool", [])` and the values
become `Value::Sum(0|1, [])`, exactly like `Maybe`. Nothing about `Bool` is
special in the type system, the arena, the RC walkers, or the renderer.

| | A | B |
|---|---|---|
| `True`/`False` in patterns | yes | yes |
| `&&`/`||` as Prelude functions (OD-25 fixed) | yes | yes |
| `ValueTy::Bool`/`Value::Bool` | kept | **deleted** |
| `if` condition check | stays `ValueTy::Bool` | needs `Data("Bool")` |
| RC walkers / arena / renderer | unchanged | `Bool` arms deleted |
| risk | low | medium (touches the value core) |
| conformance | `Bool` is a data type *nominally* | `Bool` is a data type *structurally* |

**Recommendation: A now, B later if it ever earns its keep.** A closes OD-25 and
makes `True`/`False` real, which is what the Report's *surface* requires, without
rewriting the value representation. B is more uniform but buys nothing extra for
conformance: no Haskell 2010 program can tell whether `Bool` is stored as a tag
or as a nullary `Sum`.

## Steps (Option A)

- [ ] **Step 1: Declare the type.** `data Bool = False | True` in the Prelude,
      registered as a sum type so the instance/kind machinery sees a real data
      type with two nullary constructors.
- [ ] **Step 2: Constructors in expressions.** `True` / `False` resolve to the
      constructors; `true` / `false` become aliases of them (both spellings work;
      the Report spells the constructors).
- [ ] **Step 3: Constructors in patterns.** `Pattern` accepts `True`/`False`
      constructor patterns and matches them; `Pattern::LitBool` becomes an alias
      of that path or is removed.
- [ ] **Step 4: Rewrite `&&` / `||` as Prelude functions** with the Report's
      equations, and delete `Expr::Logic` + `ValueInstr::ValueLogic`.
- [ ] **Step 5: Prove OD-25 is fixed**: `false && loop 1.0` → `False`,
      `true || loop 1.0` → `True` (currently both are resource exhaustion),
      while `true && loop 1.0` still diverges.
- [ ] **Step 6: Prove `True`/`False` work as constructors** in both expression
      and pattern position (`myAnd True y = y` compiles and runs).
- [ ] **Step 7: Full suite + gates.** Any test using `true`/`false` keeps working
      through the alias.

## Acceptance

- `data Bool = False | True` is declared and registered.
- `True` / `False` work in expressions **and** patterns; `true` / `false` still work.
- `&&` / `||` are Prelude **functions**; `Expr::Logic` / `ValueInstr::ValueLogic`
  are gone.
- `false && loop 1.0` → `False`; `true || loop 1.0` → `True`;
  `true && loop 1.0` diverges.
- All baseline tests green; gates clean.

## Out of scope

Option B's full erasure (`Value::Bool` → `Value::Sum`). Deriving `Eq`/`Ord` for
`Bool` beyond what already works. `if` re-typing against a user-defined
`Bool`-like type.
