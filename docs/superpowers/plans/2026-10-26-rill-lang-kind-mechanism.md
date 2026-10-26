# rill-lang: a real kind mechanism (K1–K2)

> **Status:** planned, not started.
> **Predecessors:** `2026-09-26-rill-lang-hkt-collections-design.md` (HKT
> representation), `2026-10-25-rill-lang-runtime-dictionaries-design.md`
> (dictionary model).

## Why this comes before the remaining type-name work

Measured on `2b00a05`:

| Check | rill today | GHC |
|---|---|---|
| `f :: Pair Int -> Int` (a `* -> *` in `*` position) | **accepted** | `Expecting one more argument to 'Pair Int'; Expected a type, but 'Pair Int' has kind '* -> *'` |
| `instance Functor Pair` (arity 2 in an arity-1 class) | rejected: ``Pair` has arity 2, but `Functor` expects arity 1` | kind error |
| `instance Functor Int` | rejected: `Int is not a type constructor` | kind error |
| `f :: [Int] -> [Int]` | **parse error** | accepted |
| `fmap (\x -> x) xs` where `xs : List a` | `cannot resolve method 'fmap' of 'Functor': the argument type does not apply 'f'` | accepted |
| `fmap (\x -> x + 1.0) (Just 1.0)` | `2.0` | `2.0` |

So the mechanism is **half there**: instance heads are arity-checked, but nothing
checks a type's kind in a type position, and a method over an abstract `f` only
resolves when the concrete constructor is `Maybe`-like (one argument, real
instance body). `List` fails.

Consequence for the pending naming work: `instance Monoid [a]` (task 5.5c) is a
head whose argument is a **variable**, and `FixedBuffer a` (task 5.5pre-1) is a
constructor whose kind must be known. Neither is checkable without K1–K2, which
is why 5.5b/5.5c are deferred here rather than blocked on dictionaries alone.

## Scope

### K1: kinds are real, and checked in type position

- A `Kind` notion in `TypeEnv`: `*`, `* -> *`, `* -> * -> *` (enough for the
  Report's types; no kind polymorphism, no `Nat`).
- Every name in the registry (5.5a) carries a kind: a scalar or a `data` type
  with no parameters is `*`; `List`/`Maybe`/`Set`/`FixedBuffer` are `* -> *`;
  `Map`/`Pair`/`Either`/`Array` are `* -> * -> *`; a class variable's kind comes
  from its declaration (already inferred as `arity`).
- **A type expression must have kind `*` wherever a type is expected** — a
  signature, a field, an argument. `Pair Int` alone is `* -> *`, so it is an
  error, exactly as GHC says.
- Errors name the situation the way GHC does: *"expecting one more argument to
  `Pair Int`; expected a type, but `Pair Int` has kind `* -> *`"*.

### K2: instance heads are kind-checked for all instances

- The existing arity check in `validate_instances` becomes a **kind** check, and
  its `from_prelude` bypass is removed *after* proving the prelude's own heads
  pass. Keep the trust flag for body checking; drop it for the head kind.
- A head with a **variable** argument (`instance Monoid [a]`) binds that
  variable at the class's kind; the check must accept it. This is the form 5.5c
  needs.

### Out of scope

- **Capacity (`Cap`) — removed from the language** (user, 2026-10-26: lists with a
  capacity were an early iteration, obsolete). Nothing to implement.
- Kind inference, kind polymorphism, kind signatures, constraints of non-`*`
  kinds, `TypeOperators`.
- The method-resolution rewrite — that is the dictionary work (design §3.7
  steps 2–4), which K1–K2 unblock but do not replace.

## Relationship to HKT

The HKT design's **representation** survives and is reused: `ValueTy::TyConVar`,
`ValueTy::TyConApp`, `TypeclassInfo.arity`, and the kind-var matching rule. Its
**resolution strategy** does not: §2.6 says resolution "instantiates the instance
body, β-substitutes the arguments, and inlines it — no runtime dispatch, no
dictionaries", and the dictionary design supersedes exactly that (§6: "**Pauses
Task P-A**"; anti-goal: "keeping monomorphisation as a fallback"). So this plan
keeps the types and replaces the resolution with dictionaries, later.

## Steps

- [ ] **Step 1: Introduce `Kind`** in the type environment, derived from the
      registry entry that 5.5a created. No behaviour change; a debug accessor
      proves every registered name has a kind.
- [ ] **Step 2: Implement the position check.** Reject a kind-`* -> *` (or
      higher) type where `*` is required. Prove: `Pair Int` in a signature is an
      error; `Pair Int Double` is fine; `List Double` is fine; `List` alone is an
      error.
- [ ] **Step 3: Convert the instance-head arity check into a kind check** and
      remove the `from_prelude` bypass for the head once the prelude passes
      (report every prelude head you had to look at).
- [ ] **Step 4: Accept a variable-headed instance.** `instance Monoid [a]` must
      kind-check. (The head is accepted; its *body* semantics belong to the
      dictionary work.)
- [ ] **Step 5: Prove the divergences are gone** — the four rows in the table
      above, plus a test that an arity-2 constructor in an arity-1 class is a
      *kind* error with a kind-worded message.
- [ ] **Step 6: Commit** — `git commit -m 'feat(lang): a real kind mechanism'`

## Acceptance

- `f :: Pair Int -> Int` is rejected with a kind error naming `* -> *`.
- `instance Functor Pair` is rejected as a kind error.
- `instance Monoid [a]` kind-checks.
- The prelude still loads; all baseline tests stay green.
- No `Cap` anywhere.
