# rill-lang: dictionary stage 3b — recursion for constrained definitions

> **Status:** ready to implement.
> **Design:** `2026-10-25-rill-lang-runtime-dictionaries-design.md` §3.7 step 3,
> goal **G3** ("Recursive polymorphic functions work" — removes the documented
> `recursive class-constrained definition` limitation).
> **Predecessors:** stage 2 (`105bf22`, dictionaries built at seat), stage 3a
> (`d9d6bfb`, operators record selections), B1/B2 (`d5bb035`, `3dbfbaf`).

## What is already true (measured on `3dbfbaf`)

Stage 3's *other* half is further along than the plan assumed. Verified:

| Case | Today |
|---|---|
| `f :: Num a => a -> a; f x = x + x` compiles | yes |
| … and **runs** at `Double` | yes → `4.0` |
| … called twice acts consistently | yes → `10.0` |
| recursive constrained definition **compiles** | yes |
| recursive constrained definition **called** | **`recursive class-constrained definition \`sumTo\``** |

So the one remaining defect is **recursion**. Everything else in stage 3 works
through the existing per-call-site path.

## The defect

`lower_constrained_body` (`lower.rs:4462`) guards with a `constrained_lifting`
set:

```rust
if self.constrained_lifting.contains(name) {
    return Err(CompileError::Type {
        msg: format!("recursive class-constrained definition `{name}`"), … });
}
self.constrained_lifting.insert(name.to_string());
let result = self.lower_with_where_defs(where_defs, body, expected);
self.constrained_lifting.remove(name);
result
```

The guard exists because the current strategy is **expansion**: a call site
solves the class variable and lowers the definition's *body inline*. A recursive
call inside that body would expand forever, so it is rejected. That is exactly
the cost the design set out to remove:

> **G3** Recursive polymorphic functions work (removes the documented
> `recursive class-constrained definition` limitation).

## The fix, and why it is precisely this

The guard is only needed because the body is expanded per call site. Give the
body **one** entity that a recursive reference can point at — then recursion is an
ordinary self-call, not an infinite expansion.

Two shapes are possible; the design requires the second:

1. **A shared cell per constrained definition** (what `shared_cell_result_tys`
   already does for named λ-definitions) — cheap, keeps the specialisation.
2. **A real function whose leading parameters are the dictionaries** (design
   §3.0/§3.7 step 3): `f :: Num a => a -> a` becomes
   `f = \dict -> \x -> …`, one body, dictionary passed in.

**Take (2).** Shape (1) would make recursion work while keeping the
monomorphisation that §3.4 says to delete, i.e. it would leave two models —
exactly the anti-goal. And with dictionaries already materialised at seat
(stage 2) and selections recorded (stage 3a), (2) is a wiring change, not a
redesign.

## Steps

- [ ] **Step 1: A constrained definition becomes one lowered function.**
  Lower the body **once**, with the class dictionaries as leading parameters.
  Its type is `f :: Num a => a -> a` → `Thunk([Dict("Num"), a], [a])`.
  `ValueTy::Dict(String)` and `Scheme.constraints` already exist.
- [ ] **Step 2: The call site passes the dictionary.** Inference already records
  the selection (`TypedProgram::dict_selections`); the call site appends the
  `Dict` argument from the `Ir::dicts` slot built in stage 2. No new solving.
- [ ] **Step 3: Delete the recursion guard and its state.**
  `constrained_lifting`, `lower_constrained_body`'s guard, and the
  `recursive class-constrained definition` diagnostic all go. A recursive
  reference is an ordinary self-call to the one function.
- [ ] **Step 4: Delete the expansion machinery that only existed for recursion.**
  With one body, `lower_constrained_apply_site` / `lower_constrained_apply` /
  `lower_constrained_apply_inner` lose their reason to specialise. Report which
  of them survives and why; anything that only re-expanded the body must go.
  **Do not leave a second path beside the new one** — `grep` must not show two
  ways to reach a constrained definition's body.
- [ ] **Step 5: Prove.**

  > **Correction (user, 2026-10-27): an earlier version of this step was
  > impossible.** It asked for `sumTo n = if n == 0.0 then 0.0 else n + sumTo (n - 1.0)`
  > with the signature `Num a => a -> a`. That body is **not** well-typed Haskell:
  > a floating literal in a `Num a` body needs `Fractional a`, and GHC rejects it
  > (`Could not deduce 'Fractional a' arising from the literal '1.0' from the
  > context: Num a`). Worse, rill has **no literal polymorphism yet** — a `1.0`
  > literal is monomorphic `Double` — so demanding that program would have
  > required stage 7 inside stage 3b. The proofs below are the correct ones.

  | Proof | Required |
  |---|---|
  | `sumTo :: Num a => a -> a -> a; sumTo zero n = if n == zero then zero else n + sumTo zero (n - zero);` called as `sumTo 0.0 4.0` | compiles **and runs** → `10.0` |
  | a mutually recursive pair of constrained definitions | compiles and runs |
  | one constrained definition used at two different types | **one** body — prove structurally (one fragment / a body count), not "it compiles" |
  | non-recursive unchanged | `f 2.0` → `4.0`; `f 2.0 + f 3.0` → `10.0` |
  | `no instance of …` | still a **compile-time** error |

  **Do not** use a monomorphic floating literal inside a `Num a` body as a proof.
  `fromInteger` is the only lawful way to write "the literal one" there, and
  making it resolve against the GIVEN dictionary is **stage 7**; if it does not
  resolve yet, that is expected and is not a 3b failure — report it, do not fix
  it.
- [ ] **Step 6: Full suite + gates.**

## Acceptance

- `recursive class-constrained definition` no longer exists as a diagnostic.
- `sumTo 4.0` → `10.0`.
- Mutually recursive constrained definitions work.
- A constrained definition has **one** lowered body, provable structurally.
- Exactly one path reaches a constrained definition's body.
- `no instance` stays compile-time.
- All baseline tests green; gates clean.

## Out of scope

- **B3** (`KernelId::Compare` — the three-valued `compare`).
- Stage 4's deletion of `lower_primitive_method` / `instantiate_method` **for
  methods** (this task touches only *constrained definitions*).
- Literal `fromInteger` and defaulting (stage 7).
- OD-18 (`Array`/`STArray` parameters).

## Risk, stated up front

This changes a **calling convention**. If a constrained definition is reached
from a context where no dictionary is available (a prelude body, a value with no
selection recorded — two of stage 2's reported gaps), the call has nothing to
pass. In that case: report it as BLOCKED with the exact case rather than
inventing a fallback or silently reverting to expansion. A silent fallback is the
one outcome that would be worse than the current limitation.

## Correction log

**2026-10-27 (user).** The penetration criterion for this task originally
included

```haskell
sumTo :: Num a => a -> a
sumTo n = if n == 0.0 then 0.0 else n + sumTo (n - 1.0)
```

That program is **wrong in two independent ways**, and asking for it is what made
the implementation look stuck:

1. **Not well-typed Haskell.** A floating literal in a `Num a` body needs
   `Fractional a`. GHC: *"Could not deduce 'Fractional a' arising from the
   literal '1.0' from the context: Num a"*. The lawful spelling is
   `fromInteger 1`.
2. **Outside rill's current type system.** rill has no literal polymorphism yet:
   a `1.0` literal is monomorphic `Double` (measured: `1.0 + 1` →
   `cannot unify value type Double with Int`). Requiring that program would have
   pulled stage 7 (`fromInteger` + defaulting) into stage 3b.

The correct 3b proof passes the zero/one in as a parameter, which is
dictionary-polymorphic without needing literals. Verified: that form **compiles**
today; only the recursive *call* still fails with
`recursive class-constrained definition` — which is exactly what 3b removes.
