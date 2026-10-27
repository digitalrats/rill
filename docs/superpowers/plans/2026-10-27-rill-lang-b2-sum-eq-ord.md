# rill-lang: B2 — `Eq`/`Ord` for sum types, and the `Data` vs `App` split

> **Status:** SUPERSEDED — see `2026-10-27-rill-lang-b2-one-con.md`.
> The user decision (2026-10-27) replaced the "one unify rule" draft with a full
> merge of `App` and `Data` into `ValueTy::Con`, matching GHC's `TyConApp`:
> four of the remaining large tasks (5.5c, OD-18, `deriving`, `kind_of`) all ask
> the same question the split answers twice.
> **Predecessor:** B1 (`d5bb035`) erased the `Bool` special case, which exposed
> this. Tracked as **OD-26 / B2**.

## What B1 broke, and what was already broken

B1 made `Bool` an ordinary sum type. That means it now goes through the same
machinery as `Maybe`/`Either`/user sums — and that machinery has a defect:

| Expression | Before B1 | After B1 | GHC |
|---|---|---|---|
| `False == False` | `True` | **type error** | `True` |
| `False < True` | `True` | **type error** | `True` |
| `compare True False` | worked | **type error** | `GT` |
| `data MyB = Off \| On; Off < On` | type error | type error | `True`-equivalent |

The error is always the same:

```
cannot unify value type App("Bool", []) with Data("Bool", [])
```

**Root cause, measured.** Two type constructors are produced for the same name:

| Name | `data_types` | `ctor_kinds` | `data_arities` | Result |
|---|---|---|---|---|
| `Bool` | yes | none | **none** | `App("Bool", [])` in some paths, `Data("Bool", [])` in others |
| `Tape` | yes | none | **1** | `Data` |
| `Maybe` | yes | 1 | none | `App` |
| `List` | no | 1 | none | `App` |

`ty.rs:1222/1253/1259` and `infer.rs:2563/2573` all guard the registration with
`if !tyvars.is_empty()` — so a **nullary** declaration never enters
`data_arities`, and `infer.rs:5266` (`if data_arities.contains_key(head) {
Data } else { App }`) then builds `App` for it while other paths build `Data`.

## The exact seam (measured — this is the whole task)

There is already **one** correct arity answer, `TypeEnv::type_ctor_arity`
(`ty.rs:2380`):

```rust
if let Some(a) = self.ctor_kinds.get(name)  { return Some(*a); }
if let Some(a) = self.data_arities.get(name) { return Some(*a); }
self.data_types.get(name).map(|_| 0)   // a monomorphic declaration is arity 0
```

Its last line already treats a nullary declaration as arity **0**. The defect is
that the `Data`-vs-`App` DECISION does not call it — it calls
`data_arities.contains_key` inline in three places, and a nullary type is absent
there:

| Site | Current test | `Bool` today |
|---|---|---|
| `ty.rs:2688` `ctor_application` | `data_arities.contains_key(head)` | **false** → `App` |
| `infer.rs:5266` builtin-sig instantiation | `data_arities.contains_key(head)` | **false** → `App` |
| `infer.rs:1684` ctor application | `data_arities.contains_key(ctor)` | **false** → `App` |
| `ty.rs:2420` `kind_of` | `ctor_kinds \|\| data_arities` | **false** → `None`, i.e. "no kind" |

So `Bool` gets `App("Bool", [])` where a declared type is expected, while
`ValueTy::bool_ty()` and the sum-construction path build `Data("Bool", [])` — and
unification rejects the pair. **The fix is to route every one of those decisions
through `type_ctor_arity`** (or a `is_declared_type(name)` derived from it), not
to add a table.

**This is why the earlier attempt drifted:** with `data_arities` looking like the
only arity source, "give it a kind" appears to need a new structure. It does not —
`type_ctor_arity` already exists and `kind_of` only needs to stop second-guessing
it. **Adding a parallel kind/arity table is a FAILURE of this task**; the
required change deletes inline questions in favour of the one function that
already answers them.

## Why this is not a one-line fix



`infer.rs:2602` documents the current state as **deliberate**:

> *parameterized user SUMS are intentionally NOT registered in `data_arities`
> (only parameterized RECORDS are). A sum's match-pin and ctor-construction paths
> stay monomorphic `Data(name, [])`, so a parameterized sum as a typeclass
> instance would fail instance-body validation with the confusing
> `Data("Opt", [Var(_)])` vs `Data("Opt", [])` unify error.*

So `data_arities` was being used as a **proxy for "can be an instance"** rather
than as arity data, because the sum constructor paths were monomorphic. That
workaround is exactly the "several sources of truth" defect this effort removes:
one table answering two different questions.

## Scope

### Step 1: one answer to "is this a type constructor, and with what arity"

- `data_arities` (or its replacement) records **every** declared data type with
  its real arity, including `0`. A nullary type is arity 0, not absent.
- The instance machinery asks its own question — *can this head be an instance?* —
  through the **kind** check added in K1–K2, not through the absence of an entry.
  A `* -> *` constructor used where `*` is required is already a kind error with
  a clean message, so the "clean message" reason for the workaround is now the
  kind checker's job.
- Consequence: `Data(name, args)` is produced consistently for every declared
  type; `App` remains for builtin constructors that are **not** declared data
  types (`List`, `FixedBuffer`, …) or — better — those become uniform too, if
  that is a contained change. Decide by measurement, and say which you chose.

### Step 2: sum constructor paths stop being monomorphic

`ValueTy::Data(name, [])` hard-coded in the constructor/match paths must carry
the declaration's type arguments, so `data Opt a = Some a | None` types as
`Data("Opt", [a])` rather than `Data("Opt", [])`. This is the real blocker the
comment describes.

### Step 3: `Eq`/`Ord` for sum types

- Structural `Eq` (`==`/`/=`) and `Ord` (`compare`) over a sum value: compare
  constructor tags first (`Eq` needs tag equality plus payload equality), then
  payloads left to right.
- Nullary sums (`Bool`) therefore order by tag — `False < True` because `False`
  is tag 0 and `True` is tag 1 by declaration order (Report §4.1.1 order).
- Derived instances: a sum with no user instance gets the structural one, the
  same way records do today.

### Step 4: prove the regression is reversed

**`compare` is NOT in this task's proofs** (see "Two defects, kept apart").

| Expression | Required | Served by |
|---|---|---|
| `False == False` | `True` | `ValueCompare` / `CmpOp::Eq` (exists) |
| `False == True` | `False` | same |
| `False < True` | `True` | `ValueCompare` / `CmpOp::Lt` (exists) |
| `data MyB = Off \| On; Off < On` | ordering by tag | `value_cmp_ref`'s `Sum` arm (`interp.rs:808` — **already correct**) |
| `data Opt a = Some a \| None; Some 1.0 == Some 1.0` | `True` | same |
| `data Opt a = Some a \| None; Some 1.0 < Some 2.0` | `True` | same |

## Two defects, kept apart (measured)

Do not fix both here. They are independent, and conflating them is how this task
drifts.

**Defect 1 — this task.** The `Data` vs `App` split. `Bool` is
`App("Bool", [])` in some paths and `Data("Bool", [])` in others, so unification
rejects the pair before any comparison runs.

**Defect 2 — NOT this task (design §3.9, tracked as B3).** `KernelId::Compare`
has **no IR instruction**, so `compare` fails for **every** type:

```
compare 1.0 2.0   →  primitive method `Ord.compare` for `Double` selects kernel
                     `Compare`, which has no IR instruction yet (design §3.9)
```

Note that failure is on `Double`, not on `Bool` — it is general, not sum-specific.
The fix is small and belongs to the kernel work: `value_cmp` (`interp.rs:796`)
already returns the `-1`/`0`/`1` ordering as an `i8`, and `ValueCompare` already
dispatches on `CmpOp`. What is missing is a three-valued result exposed as an
`Int` — no new comparison logic.

**And the crucial fact that keeps them apart:** the ordering logic for sums
**already exists and is correct**:

```rust
// interp.rs:808
(Sum(i, px), Sum(j, py)) => {
    let c = i.cmp(j) as i8;          // tag first
    if c != 0 { return c; }
    cmp_ref_slices(arena, px, py)    // then payloads
}
```

So `False < True` needs **only** Defect 1 fixed (`False` is tag 0, `True` tag 1 by
declaration order). Do **not** add a `Bool` ordering kernel, do not add a
`Compare` instruction, and do not touch `kernels.rs` in this task.

## Constraints

- **No `Bool`-specific branch anywhere.** `Bool` must be fixed by the general sum
  path, never by a `matches!` on its name — that would recreate exactly what B1
  removed. A `Bool`-named special case is a failure of this task.
- No second arity table. If `data_arities` cannot serve both questions, split it
  into two *clearly named* things and delete the overloaded use — do not add a
  parallel table beside it.
- `#![deny(unsafe_code)]`, English comments, no stray `zz_*.rs` files.

## Acceptance

- Every declared data type has exactly one consistent type representation
  (`Data(name, args)`), whatever its arity, and `App` is not produced for a
  declared type.
- `False == False`, `False < True`, `compare True False` work again.
- A nullary user sum and a parameterized user sum both order and compare.
- No occurrence of `"Bool"` in the type-construction, unification, or ordering
  paths (grep proof).
- All baseline tests green; the `fixity_4_4_2` test that B1 had to rewrite is
  restored to its direct form if it now type-checks.
- Gates clean.

## Out of scope

- **B3: `KernelId::Compare` / the three-valued `compare`.** Needs an IR
  instruction for every type, not just sums. `value_cmp` already computes the
  answer; only the exposure is missing. Tracked separately so this task stays
  about the `Data`/`App` split.
- `deriving` clauses as *syntax* (only the implicit structural instance).
- `Show` for sums (that is P2's `show`).
- Any JIT representation choice.
