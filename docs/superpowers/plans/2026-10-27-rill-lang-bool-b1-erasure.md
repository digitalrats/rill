# rill-lang: `Bool` as a real data type — B1 (erase the special case)

> **Status:** **DONE** — commit `d5bb035` (1109 tests). Tracked as **OD-26 / B1**.
> **Decision (user, 2026-10-27):** take **B** — full erasure — split into two
> independent tasks. This plan is **B1**. `Eq`/`Ord` for sum types and the
> `App` vs `Data` unification defect are **B2**, a separate task.

## Why

`Bool` is 7 special points in the core, where Haskell 2010 (§4.1.1) simply
declares `data Bool = False | True`:

| Special point | Today | After B1 |
|---|---|---|
| `ValueTy::Bool` | leaf variant | `Data("Bool", [])` |
| `Value::Bool` | inline arm (1 byte) | `Value::Sum(0\|1, [])` |
| `ValueKind::Bool` | own rank | `Sum` rank |
| `Tok::KwTrue` / `Tok::KwFalse` | 2 keywords | gone; `True`/`False` are constructors |
| `Expr::Bool` | own AST node | `Ref("True")` / `Ref("False")` |
| `Pattern::LitBool` | own match path | ordinary constructor pattern |
| `Expr::Logic` + `ValueInstr::ValueLogic` | IR primitive that forces **both** operands | gone — `&&`/`||` become Prelude functions |

The last row is **OD-25**: `false && loop 1.0` currently fails with
`value-track resource exhausted`, where GHC prints `False`.

**Why erasure fixes OD-25 without new machinery:** the Report's own definition is

```haskell
True  && y = y
False && _ = False
```

and rill **already evaluates arguments lazily** (measured: `(\y -> 1.0) (loop
1.0)` → `1.0`, `const 1.0 (loop 1.0)` → `1.0`). So a constructor-pattern function
short-circuits for free.

## Measured facts that shape the work

- `Value` is 48 bytes; the `Bool` arm is 1 byte, the `Sum` arm 32. So B1 makes
  every boolean an arena allocation in the **interpreter**. This is accepted: a
  JIT can represent nullary constructors as a tag, and uniformity is worth more
  than the interpreter's byte.
- **`BranchCtor` already exists** (`ir.rs:311`): `{ slot, ctor, then, els }`,
  and `value_reg_ctor` (`interp.rs:405`) already tests `Value::Sum(c, _)` against
  a ctor tag. `if` currently uses `ValueTerm::Branch` + `value_reg_is_true`
  (`interp.rs:238`, `:388`). B1 converts `if` to `BranchCtor` and deletes
  `value_reg_is_true` — no new branching mechanism.
- User sums with nullary constructors **parse and run** today:
  `data MyB = Off | On; g x = case x of { Off -> 0.0; On -> 1.0 }; g On` → `1.0`.
- **`Eq`/`Ord` on a sum type does NOT work today**: `Off < On` →
  `cannot unify value type App("MyB", []) with Data("MyB", [])`. So after B1,
  `false < true` (which works today: `True`) will stop working until **B2**.
  That regression is accepted and is B2's job — it is recorded, not hidden.

## Scope (B1 only)

- [x] **Step 1: Declare the type.** `data Bool = False | True` in the PRELUDE,
      registered as a sum type with two nullary constructors.
- [x] **Step 2: Constructors in expressions.** `True` / `False` parse and resolve
      to the constructors (`Expr::Ref`). `true` / `false` stay accepted as
      **aliases** so existing programs and tests do not have to change their
      spelling; the Report's spelling (`True`/`False`) becomes the canonical one.
- [x] **Step 3: Constructors in patterns.** `True` / `False` work as constructor
      patterns, matching the same way `case` on a sum already does.
- [x] **Step 4: Delete the special case.** Remove `ValueTy::Bool`,
      `Value::Bool`, `ValueKind::Bool`, `Expr::Bool`, `Pattern::LitBool`,
      `Tok::KwTrue`/`KwFalse` (after step 2 keeps the aliases), and
      `ValueInstr::ValueBool`. Every match arm the compiler reports is either
      replaced by the `Sum`/`Data` path or deleted. **The `Bool` value type is
      now `Data("Bool", [])` everywhere.**
- [x] **Step 5: `if` uses `BranchCtor`.** The condition is a `Data("Bool", [])`
      value; `True` is ctor tag 1, `False` tag 0 (declaration order: `False |
      True`, so `False` = 0). Delete `value_reg_is_true` and any
      `ValueTerm::Branch` use whose only purpose was a boolean condition.
- [x] **Step 6: `&&` / `||` become Prelude functions** with the Report's
      equations. Delete `Expr::Logic` and `ValueInstr::ValueLogic`.
- [x] **Step 7: Prove OD-25 is fixed.** `false && loop 1.0` → `False`;
      `true || loop 1.0` → `True`; `true && loop 1.0` still diverges. Quote the
      three results.
- [x] **Step 8: Prove the constructors are real.** `myAnd True y = y; myAnd False
      _ = False` compiles and runs — this is what decides whether `&&` could be a
      function at all.
- [x] **Step 9: Full suite + gates.** `show_bool` keeps working (it is
      rill-specific; it must render the constructor, not a removed variant).

## Acceptance

- `Bool` is `Data("Bool", [])`; there is no `ValueTy::Bool`, `Value::Bool`,
  `ValueKind::Bool`, `Expr::Bool`, `Pattern::LitBool`, `Tok::KwTrue`,
  `Tok::KwFalse`, `ValueInstr::ValueBool`.
- `True` / `False` work in expression **and** pattern position; `true` / `false`
  still work as aliases.
- `&&` / `||` are Prelude **functions**; `Expr::Logic` and
  `ValueInstr::ValueLogic` are gone.
- `false && loop 1.0` → `False`; `true || loop 1.0` → `True`;
  `true && loop 1.0` diverges.
- `if` branches on the constructor tag via `BranchCtor`.
- All baseline tests green; gates clean.
- **`false < true` may regress to a type error — that is B2's job, and the plan
  says so instead of hiding it.**

## Out of scope — task B2

- `Eq`/`Ord` (and `deriving`) for **sum types** generally.
- The `App(name, [])` vs `Data(name, [])` unification defect
  (`Off < On` → `cannot unify value type App("MyB", []) with Data("MyB", [])`).
- Restoring `false < true` / `x == True` ordering once those land.

**Consequence to state in the report, not to paper over:** B1 removes a working
capability (`Bool` ordering) until B2 restores it for sum types generally. That
is the accepted cost of removing the special case; B1's report must list every
test that had to be changed or skipped because of it.

## Outcome (`d5bb035`)

Controller-verified after the fact.

**All seven special points are gone** (grep count zero for each):
`ValueTy::Bool`, `Value::Bool`, `ValueKind::Bool`, `Expr::Bool`,
`Pattern::LitBool`, `Tok::KwTrue`, `Tok::KwFalse`, `ValueInstr::ValueBool`,
`ValueInstr::ValueLogic`, `Expr::Logic`, `value_reg_is_true`.

| Check | Result |
|---|---|
| `True` / `False` in pattern position | `myAnd True y = y; myAnd False _ = False` compiles and runs → `False` |
| lowercase aliases | `true && false` → `False` |
| `show_bool True` | → `True` |
| `if` branching | via `BranchCtor` on the constructor tag; `ValueTerm::Branch` deleted |
| **OD-25** | pinned by `bool_data_type::od_25_short_circuit_comes_from_laziness`: `False && loop 1.0` → `False`, `True \|\| loop 1.0` → `True`, and the two controls (`True && loop`, `False \|\| loop`) still diverge — so the passing cases are real short-circuits, not a blanket skip |
| fixity | `infixr 3 &&` / `infixr 2 \|\|` preserved (`True \|\| True && False` = `True`) |
| operators as values | `(&&) True True`, `(\|\|) False True` work |

### The B2 regression, stated plainly

These worked before B1 and are **type errors** now, because user sum types have
no `Eq`/`Ord` and their unification is broken
(`App("Bool", [])` vs `Data("Bool", [])`):

| Expression | Before | After B1 |
|---|---|---|
| `False == False` | `True` | type error |
| `False < True` | `True` | type error |
| `compare True False` | worked | type error |

One existing test had to be rewritten
(`haskell2010_syntax_gaps::fixity_4_4_2::comparison_operators_are_non_associative`
used `(1.0==2.0)==(3.0>4.0)`, which now needs `Eq Bool`); it feeds both
comparisons to a constructor-pattern function instead, so non-associativity is
still asserted.

**No `Bool`-specific `Eq`/`Ord` was added** — that would recreate the special case
this task removes. Restoring boolean ordering is **B2**: `Eq`/`Ord`/`deriving`
for sum types plus the `App` vs `Data` unification defect.

### Deliberate deviation from the plan

`ValueTerm::Branch` was deleted rather than kept beside `BranchCtor`: once `if`,
guards and literal tests all move to `BranchCtor`, nothing but a boolean
condition ever used it. Keeping a dead variant would have been the "two paths"
defect this work exists to remove.
