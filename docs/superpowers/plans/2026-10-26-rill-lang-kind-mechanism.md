# rill-lang: a real kind mechanism (K1–K2)

> **Status:** **K1–K2 DONE** — commit `5fdced5` (1047 tests).
> **Predecessors:** `2026-09-26-rill-lang-hkt-collections-design.md` (HKT
> representation), `2026-10-25-rill-lang-runtime-dictionaries-design.md`
> (dictionary model), `2026-10-25-rill-lang-dictionaries-stage1.md` (the
> single type-name registry this plan extends).

## Why this comes before the remaining type-name work

Measured on `5a5b404`:

| Check | rill today | GHC 9.10.3 |
|---|---|---|
| `f :: Pair Int -> Int` (a `* -> *` in `*` position) | **accepted** | `Expecting one more argument to '(,) Int'; Expected a type, but '(,) Int' has kind '* -> *'` |
| `f :: List -> Int` (bare constructor) | **accepted** | `Expecting one more argument to '[]'; Expected a type, but '[]' has kind '* -> *'` |
| `f :: Maybe -> Int` | **accepted** | `Expecting one more argument to 'Maybe'; Expected a type, but 'Maybe' has kind '* -> *'` |
| `f :: FixedBuffer -> Int` | **accepted** | n/a (rill-specific), but the same rule must apply |
| `f :: Pair Int Double -> Int` | accepted | accepted |
| `instance Functor Pair` (arity 2 in an arity-1 class) | rejected: ``Pair` has arity 2, but `Functor` expects arity 1` | kind error |
| `instance Functor Int` | rejected: `Int is not a type constructor` | kind error |
| `instance Monoid [a]` | not expressible (no `[a]` type syntax) | accepted |

So the mechanism is **half there**: instance heads are arity-checked, but nothing
checks a type's kind **in a type position**. A `* -> *` constructor used where a
type is required is silently accepted, which means a function can be declared
over a partially applied constructor and nothing notices.

Consequence for the pending naming work: `instance Monoid [a]` (task 5.5c) is a
head whose argument is a **variable**, and `FixedBuffer a` is a constructor whose
kind must be known. Neither is checkable without K1–K2, which is why 5.5b/5.5c
are deferred behind this plan as well as behind dictionaries.

## Scope

### K1: kinds are real, and checked in type position

- A `Kind` in `TypeEnv`: `*`, `* -> *`, `* -> * -> *`. Enough for the Report's
  types; no kind polymorphism, no `Nat`.
- Every registry entry carries a kind. It is **derived**, never a second list: a
  scalar or a zero-parameter `data` type is `*`; a constructor with arity 1
  (`List`, `Maybe`, `Set`, `FixedBuffer`) is `* -> *`; arity 2 (`Map`, `Pair`,
  `Either`, `Array`) is `* -> * -> *`. The arity already lives in
  `ctor_kinds`/`data_arities`, so `kind_of(name)` is a projection, not new data.
- **A type expression must have kind `*` wherever a type is expected**: a
  signature, a field type, a constructor payload, a class method's argument.
- A **type variable** (lowercase) has the kind its binder gives it; an unbound
  lowercase name in a class-method signature is kind `*` unless the class's
  `arity` says otherwise (the existing rule).
- Error wording follows GHC: *"expecting one more argument to `List`; expected a
  type, but `List` has kind `* -> *`"*. The count of missing arguments comes from
  the kind, so `Pair Int` reports one missing and bare `Pair` reports two.

### K2: instance heads are kind-checked for every instance

- The existing arity check in `validate_instances` becomes the **kind** check
  (same rule, expressed as kinds), and its `from_prelude` skip is narrowed: the
  head is checked for prelude instances too, while the **body** keeps the trust
  flag (the field-name-shadowing reason for that skip is about bodies, not heads).
  If a prelude head fails, that is a real defect to fix, not a reason to skip.
- A head with a **variable** argument (`instance Monoid [a]`) binds that variable
  at the class variable's kind and must kind-check. The syntax for `[a]` is
  5.5c's job; K2 must accept the form once the parser produces it, so the check
  is written against the type representation, not against source spelling.

### Out of scope

- **Capacity (`Cap`) — removed from the language** (user, 2026-10-26). Nothing to
  implement; the HKT design has been corrected.
- Kind inference, kind polymorphism, kind signatures, constraints of non-`*`
  kinds, `TypeOperators`, promotion.
- Type-level list/tuple **syntax** (`[a]`, `(a, b)`) — that is 5.5c. K2 only makes
  the checker able to accept a variable-headed instance once such syntax exists.
- The method-resolution rewrite — the dictionary work (design §3.7 steps 2–4).

## Relationship to HKT

The HKT design's **representation** survives and is reused: `ValueTy::TyConVar`,
`ValueTy::TyConApp`, `TypeclassInfo.arity`, the kind-var matching rule. Its
**resolution strategy** does not: §2.6 says resolution "instantiates the instance
body, β-substitutes the arguments, and inlines it — no runtime dispatch, no
dictionaries", and the dictionary design supersedes exactly that (§6: "**Pauses
Task P-A**"; anti-goal: "keeping monomorphisation as a fallback"). This plan keeps
the types and leaves resolution to the dictionary work.

## Steps

- [x] **Step 1: Introduce `Kind`** as a projection of the existing arity data.
      No behaviour change. A debug accessor proves every registry name has a kind
      and that the kind agrees with `ctor_kinds`/`data_arities`.
- [x] **Step 2: Check kind `*` in type position.** Implement it in the one
      converter the signature path uses (`sig_ty_to_vty`) **and** the other
      (`type_expr_to_vty`), so no path is left unguarded — a second, unguarded
      converter is exactly the "several sources of truth" defect this plan exists
      to remove. A `* -> *` (or higher) type where `*` is required is an error.
- [x] **Step 3: Prove the K1 rows.** `f :: Pair Int -> Int`, `f :: List -> Int`,
      `f :: Maybe -> Int`, `f :: FixedBuffer -> Int` are rejected with a
      kind-worded message; `Pair Int Double`, `List Double`, `Maybe Double`,
      `FixedBuffer Double` are accepted. Also prove a **field** type and a
      **constructor payload** are checked, not just signatures.
- [x] **Step 4: Convert the instance-head arity check into the kind check** and
      narrow the `from_prelude` skip to the body only. Report every prelude head
      you had to look at; fix any that fails.
- [x] **Step 5: Accept a variable-headed instance.** `instance Monoid [a]` must
      kind-check once 5.5c gives the parser the syntax; until then, prove the same
      property with a form the parser already produces (e.g. a variable-headed
      head that is expressible today, or a unit test on the checker directly).
- [x] **Step 6: Prove no behaviour changed for well-kinded programs.** The full
      suite stays green; the prelude still loads.

## Acceptance

- `f :: Pair Int -> Int` is rejected, naming `* -> *` and the missing argument.
- `f :: List -> Int` and `f :: Maybe -> Int` are rejected the same way.
- `instance Functor Pair` is rejected as a kind error.
- `instance Monoid [a]` kind-checks (checker-level until 5.5c's syntax lands).
- The prelude loads; all baseline tests stay green; no `Cap` anywhere.
- There is exactly **one** place that answers "what is this name's kind".

## Outcome (implemented, `5fdced5`)

`Kind` is a projection of `TypeEnv::type_entry` → `ctor_kinds`/`data_arities`/
`scalars`; there is **one** place answering "what is this name's kind"
(`TypeEnv::kind_of`), and no new name table.

Controller-verified after the fact:

| Check | Result |
|---|---|
| `f :: Pair Int -> Int` | rejected: ``expecting 1 more argument to `Pair Int`; expected a type, but `Pair Int` has kind `* -> *` `` |
| `f :: List -> Int` / `f :: Maybe -> Int` / `f :: FixedBuffer -> Int` | rejected, same shape |
| `f :: Pair -> Int` (bare) | rejected: `2 more arguments`, kind `* -> * -> *` |
| `Pair Int Double`, `List Double`, `Maybe Double`, `FixedBuffer Double` | accepted |
| `data P`, `data Box a`, `Num a => a -> a` | accepted (type variables and user data unaffected) |
| `instance Functor Pair` | rejected as a kind error (`Pair` has kind `* -> * -> *`) |
| `instance Monoid (Pair Int)` | rejected — `Monoid` wants `*` |
| `instance Functor (Pair Int)` | **accepted and compiled** — `Pair Int` is `* -> *`, correct |
| `instance Monoid List` | accepted (arity 1) |

**Four enforcement paths** were guarded, not two: `sig_ty_to_vty`,
`type_expr_to_vty`, `instantiate_builtin_sig_vty`, and the `conv` entry points,
plus compiler-owned signatures at load. The prelude's heads are now checked too
(the `from_prelude` skip is narrowed to bodies); all passed, none needed fixing.

**New defect recorded — OD-22 (open):** `instance Functor (Pair Int)` kind-checks
but a method call on `Pair Int a` cannot dispatch (the registry keys the instance
by its full spelling while dispatch resolves the head constructor). Verified in
GHC that a partial head is dispatched correctly
(`instance Functor (MyPair Int)` → `fmap (+1) (MyPair 1 2)` = `(1,3)`), so this is
a real divergence, owned by the dictionary/dispatch work.
