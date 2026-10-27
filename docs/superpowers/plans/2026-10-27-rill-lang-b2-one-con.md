# rill-lang: B2 — one type-constructor application (`Con`), as GHC's `TyConApp`

> **Status:** ready to implement.
> **Supersedes** the earlier B2 draft (a unify rule between `App` and `Data`).
> **Predecessor:** B1 (`d5bb035`) erased the `Bool` special case, which exposed
> the split. Tracked as **OD-26 / B2**.

## The defect

rill has **two** representations for one concept — "a type constructor applied to
its arguments":

```rust
ValueTy::Data(String, Vec<ValueTy>)   // a declared data type
ValueTy::App(String, Vec<ValueTy>)    // a builtin constructor
```

GHC has **one**: `TyConApp TyCon [Type]`. Where GHC needs to know "is this an
algebraic data type", it asks the `TyCon` (`isAlgType`), it does **not** use a
different `Type` shape.

The split is not merely redundant — it is **arbitrary and already
inconsistent**. Measured on `cae7059`:

| Type | `data_types` | `ctor_kinds` | Behaviour today |
|---|---|---|---|
| `Bool` | yes | none | **`Data` in some paths, `App` in others** — the bug |
| `Maybe`, `Either`, `Pair` | yes | yes | mostly `App` |
| `STArray` | yes | yes | mostly `App` |
| `Tape`, `ST`, `IO`, `Kleisli` | yes | none | `Data` |
| `List`, `Set`, `Map`, `Array`, `FixedBuffer` | no | yes | `App` |

There is no criterion. `Bool` and `Tape` sit in the same row of that table, and
only `Bool` breaks — because it is nullary and other paths build `Data` for it.

Consequences, all measured:

- `False == False`, `False < True` → `cannot unify value type App("Bool", []) with Data("Bool", [])`.
- `unify` has **two identical rules** for the two variants (same-name,
  same-arity, elementwise) and **no** rule between them.
- `TypeEnv::kind_of` carries a second guess
  (`ctor_kinds || data_arities`) because `type_ctor_arity` alone "does not
  distinguish an exact zero from an unrecorded arity".
- `TypeEnv::derive_eq_ord` (`ty.rs:2269`) unions `data_types ∪ ctor_kinds ∪
  newtypes` and derives structural `Eq`/`Ord` for **every** declared name —
  including opaque handles (`World`, `ST`, `STRef`, `Tape`, `Kleisli`), which must
  not be ordered.

## What this unblocks (the reason to do it now, not later)

| Pending work | Why it needs one shape |
|---|---|
| 5.5c `instance Monoid [a]` | a **variable-headed** instance asks "which constructor, how many args" — the same question the split answers twice |
| OD-18 (`Array Int Int` ≡ `Array Int Bool`) | `Array` is `App` while `Data` arguments are checked; one shape means one rule |
| `deriving` correctness | today it cannot tell a comparable data type from an opaque handle |
| `kind_of` | a third place guessing arity |
| B3 (`Compare` kernel) | orders values; needs one notion of "constructor tag" |

## Scope

### Step 1: one variant

`ValueTy::Data(String, Vec<ValueTy>)` and `ValueTy::App(String, Vec<ValueTy>)`
merge into **`ValueTy::Con(String, Vec<ValueTy>)`** — GHC's `TyConApp`.

- **`Newtype` stays separate.** It is not the same concept: a newtype
  *unwraps* at runtime (`Value::Newtype(inner)`), and unification treats it
  distinctly. Merging it would be a different change.
- **`TyConApp(TypeVarId, args)` stays** — it is GHC's `AppTy` (variable head).
- **`Buffer(Box<ValueTy>)` stays** — it is a dedicated, already-checked shape
  (task 5.5pre-1).

### Step 2: one rule in `unify`

Delete both existing arms and write one `(Con(x, ax), Con(y, ay))` arm. There must
be **no** arm mentioning `Data` or `App` afterwards.

### Step 3: "is it declared?" becomes a query, not a shape

- Add `TypeEnv::is_declared_data(&self, name: &str) -> bool` — reads
  `data_types`. It answers questions *about* a constructor; it never picks a
  representation.
- Every place that currently branches `data_arities.contains_key(...)` to choose
  `Data` vs `App` (`ty.rs:2688`, `infer.rs:5266`, `infer.rs:1684`) simply
  constructs `Con` — the branch disappears.

### Step 4: `kind_of` stops second-guessing

`kind_of` must project `type_ctor_arity` only. The `ctor_kinds || data_arities`
guard exists solely because arity could not distinguish "exact 0" from
"unrecorded"; with one shape that ambiguity is gone. Also make
`type_ctor_arity` record **every** declared type including arity 0 (that is what
`data_types.get(name).map(|_| 0)` already does — keep it, and delete the callers'
workarounds).

### Step 5: `derive_eq_ord` stops deriving for opaque handles

Derive structural `Eq`/`Ord` only for names whose declaration is a **record or a
sum** — not for `DataInfo::Opaque` (`World`, `STState`, `STRef`, `ST`,
`STArray`'s state parts, `Tape`, `IO`, `InputStream`, `OutputStream`) and not for
scalars that already have explicit instances. Report exactly which names stop
receiving a derived instance; each one must be a type that cannot meaningfully be
ordered.

### Step 6: prove

| Expression | Required |
|---|---|
| `False == False` | `True` |
| `False == True` | `False` |
| `False < True` | `True` |
| `data MyB = Off \| On; Off < On` | ordering by tag |
| `data Opt a = Some a \| None; Some 1.0 == Some 1.0` | `True` |
| `data Opt a = Some a \| None; Some 1.0 < Some 2.0` | `True` |
| `1.0 < 2.0` (regression guard) | `True` |
| `List Int` vs `List Bool` | still both constructible (OD-18 remains open) |

**`compare` is NOT in this task.** Measured: `compare 1.0 2.0` fails with
*"kernel `Compare`, which has no IR instruction yet (design §3.9)"* — a general
kernel gap, on `Double`, not a sum-type problem. That is **B3**.

The structural ordering for sums already exists and is correct
(`interp.rs:808`): tags first, then payloads. So `False < True` needs only the
representation fix.

## Constraints

- **No `Bool`-specific branch anywhere** (grep over type construction,
  unification, ordering). A `Bool`-named case means this task failed.
- **No parallel kind/arity table.** `type_ctor_arity` is the one answer;
  `is_declared_data` is a query. If you feel the need for a third structure, stop
  and report.
- **No debug code in the commit**: no `std::env::var` tracing, no `eprintln!`
  probes, no leftover `zz_*.rs`.
- `#![deny(unsafe_code)]`; English comments.
- Tests asserting the old `App`/`Data` spelling are wrong — fix them and say so.
- Restore the test B1 rewrote
  (`haskell2010_syntax_gaps::fixity_4_4_2::comparison_operators_are_non_associative`)
  if it now type-checks.

## Gates

```
cargo test -p rill-lang --features dsp
cargo clippy -p rill-lang --features dsp
cargo fmt --check
cargo test -p rill-lang --features "serialization dsp" --test program_json
```
No `--all-targets`.

## Acceptance

- `ValueTy` has **no** `Data` and no `App` variant; there is one `Con`.
- `unify` has **one** constructor-application arm.
- `is_declared_data` is a query; no code chooses a representation from it.
- `kind_of` is a pure projection of `type_ctor_arity`.
- `derive_eq_ord` gives no instance to an opaque handle (list them).
- All proofs in step 6 pass; `compare` untouched.
- Gates clean; no debug artefacts.
