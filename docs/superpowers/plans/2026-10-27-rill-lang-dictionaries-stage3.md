# rill-lang: dictionary stage 3 — operators must select a dictionary first

> **Status:** **3a DONE** (`d9d6bfb`, 1096 tests); **3b pending**.
> **Design:** `2026-10-25-rill-lang-runtime-dictionaries-design.md` §3.7 step 3
> (pass a dictionary parameter), §3.3/§3.4 (what it replaces).
> **Predecessor:** stage 2 (`105bf22`) built and proved the table.

## Why stage 3 cannot start where the design says it starts

Stage 2 measured five gaps. One of them blocks stage 3 outright:

> **Infix operators record no selection.** `1.0 == 2.0` and `1.0 < 2.0` produce
> `dicts=0`, because operators still go through the legacy `Expr::Arith` /
> `Expr::Cmp` inference paths, which ask only "is this numeric?" and never touch
> a class. Method-call syntax (`(<) a b`) does record.

Consequence: a dictionary has no producer on the operator path, and operators are
**how real programs use classes**. Passing a dictionary parameter (stage 3) before
that is fixed would mean passing evidence nothing ever asks for.

There is also a second, structural blocker: stage 3 changes the *calling
convention* of `ConstrainedDef` (today `lower_constrained_apply_site` specialises
the body per call site — `lower.rs:4418`). Making the dictionary an **argument**
means the body must be lowered **once**, and that is the same deletion stage 4
performs (`lower_constrained_apply_site` / `instantiate_method` /
`lower_primitive_method` are one mechanism). Measured scope: `Expr::Arith` 42
sites, `Expr::Cmp` 23, `Expr::Logic` also legacy — and the five specialisation
paths total 24 sites.

So stage 3 is split: **3a** makes operators select (P-A's operator half), **3b**
turns the dictionary into a parameter and lowers a constrained body once.

---

## Task 3a: operators go through the class, not through `Expr::Arith`/`Expr::Cmp`

**What this is:** the *unfinished half of P-A*, scoped to exactly what stage 3
needs — an operator must resolve through its class method so inference records the
instance the dictionary will be built from.

**Measured preconditions** (all verified on `b157cad`):

- `parser.rs:215` (`desugar_infix_op`) is the **single choke point** that decides
  `Expr::Arith` / `Expr::Logic` / `Expr::Cmp` for infix operators and sections.
- `TypeEnv::class_of_method` already answers the mapping: `==`/`/=` → `Eq`,
  `<`/`<=`/`>`/`>=`/`compare` → `Ord`, `+`/`-`/`*`/`-` → `Num`, `/`/`recip` →
  `Fractional`.
- The method-call path already records selections correctly
  (`record_selection` at `infer.rs:557`, called from four dispatch sites), and
  `1.0 == 2.0` in **method** spelling does produce a dictionary.

**Approach:** desugar an infix operator to the **same AST the method spelling
produces** — a `Ref(op)` application — instead of a bespoke node. Then inference's
existing dispatch records the selection with no new code, and lowering's existing
method path applies. Sections must go through the same desugaring (the current
`desugar_infix_op` is shared by the Pratt loop and sections, which keeps them
byte-identical — preserve that property).

**Do not keep two paths.** `Expr::Arith` / `Expr::Cmp` are deleted once nothing
constructs them. `Expr::Logic` (`&&`, `||`) stays: it is short-circuit control
flow, not a class method, and Haskell 2010 has no `Logic` class.

- [x] **Step 1: Desugar operators to method calls.** `desugar_infix_op` produces
      `Ref(op) applied to lhs, rhs` for every class-method operator. Keep
      `Expr::Logic` as-is.
- [x] **Step 2: Prove selection is recorded.** `1.0 == 2.0` and `1.0 < 2.0` must
      produce a non-empty dictionary table (`Eq Double` / `Ord Double`, with
      `Ord`'s `Eq` superclass). This is the whole point of 3a.
- [x] **Step 3: Delete the legacy paths.** Remove `Expr::Arith` / `Expr::Cmp`
      construction and their inference/lowering arms; the compiler finds every
      site. `infer_arith` / `lower_arith` go with them.
- [x] **Step 4: Close the semantic holes the legacy path hid.** Because
      operators now unify one `Num a` across both operands, these become what
      Haskell 2010 requires, and they are the acceptance evidence:

      | Expression | Before | After (GHC) |
      |---|---|---|
      | `1 == 1.0` | `False` (silent wrong answer) | **type error** or `True` once unified — must not stay silently wrong |
      | `5 < 2.0` | `True` (silent wrong answer) | **type error** |
      | `(1::Int) + 1.0` | `2.0` (silent) | **type error** (`No instance for Fractional Int`) |
      | `1 + 2` | `Int` | `Int` (unchanged) |
      | `1.0 + 2.0` | `2.0`-style `Double` | unchanged |

      This is OD-16 / OD-17, and closing them is **conformance work**, not a side
      effect: `AGENTS.md` forbids a documented gap standing in for behaviour.
- [x] **Step 5: Full suite + all three gates.** Every test that asserted a
      legacy numeric coercion is wrong by the table above — fix it and cite the
      GHC result.

---

## Task 3b: a polymorphic function takes its dictionary as a parameter

**Precondition:** 3a landed, so an operator inside a constrained body selects a
dictionary.

- [ ] **Step 1: A constrained definition's type carries a leading `Dict` per
      constraint** (design §3.0): `f :: Num a => a -> a` elaborates to
      `Thunk([Dict(Num)], Thunk([a],[a]))`. `Scheme.constraints` already exists
      and `ValueTy::Dict(String)` already exists.
- [ ] **Step 2: Lower the body ONCE.** `ConstrainedDef` stops being specialised
      per call site: the body becomes one fragment whose leading parameters are
      the dictionaries. `lower_constrained_apply_site` and `instantiate_method`
      are deleted, not kept beside the new path.
- [ ] **Step 3: The call site passes the dictionary** the selection table names
      (`TypedProgram::dict_selections` → the `Ir::dicts` index from stage 2).
- [ ] **Step 4: Recursion works** (design G3). A constrained definition may call
      itself; today that is the documented `recursive class-constrained
      definition` limitation (`lower.rs:4571` guard). This must pass.
- [ ] **Step 5: Prove it.** `f :: Num a => a -> a; f x = x + x` applied at
      `Double` and at `Int` uses the **same body** and different dictionaries;
      a recursive constrained definition compiles; `no instance of …` stays a
      compile error.

## Acceptance (both sub-tasks)

- Operators resolve through classes; `1.0 == 2.0`, `1.0 < 2.0`, `1.0 + 2.0`
  produce dictionaries.
- `1 == 1.0` and `5 < 2.0` are **not** silently wrong — they are type errors.
- `(1::Int) + 1.0` is a type error.
- A constrained definition has **one** body and takes dictionaries as parameters.
- Recursive constrained definitions compile.
- `instantiate_method` and `lower_primitive_method` are gone; `grep` proves no
  second dispatch path remains.
- All three gates green.

## Explicitly out of scope

Stage 4's slot projection on **non-operator** method values (`lower_method_value`,
`lower_operator_value`), stage 5's method-as-value selector, stage 6's superclass
projection (the table already orders supers; projection comes later), stage 7's
literal `fromInteger` and defaulting, stage 8's deletion of `ConstrainedDef`.
Stage 3 makes the dictionary reach the body; it does not yet project slots for
every method form.

## Outcome of 3a (`d9d6bfb`)

Operators are now class-method sugar, desugared to the same `Expr::Apply { name }`
shape the `++`/`<>` operators already used. Controller-verified:

| Expression | Dictionary table |
|---|---|
| `1.0 == 2.0` | `[Eq Double]` |
| `1.0 < 2.0` | `[Eq Double, Ord Double]` |
| `1.0 + 2.0` | `[Eq, Ord, Num Double]` |
| `1.0 / 2.0` | `[Eq, Ord, Num, Fractional Double]` |
| `(+) 1.0 2.0` (prefix) | identical to the infix table |
| `(+ 1.0) 2.0` (section) | identical to the infix table |

Before stage 3a all of these were `dicts=0`.

**OD-13/16/17 closed.** These are now type errors (they were silent wrong
answers):

| Expression | Before | Now |
|---|---|---|
| `1 == 1.0` | `False` | `cannot unify value type Int with Double` |
| `5 < 2.0` | `True` | type error |
| `2 < 1.5` | `True` | type error |
| `(1::Int) + 1.0` | `2.0` | type error |

Same-type arithmetic is unchanged: `show_int (1 + 2)` → `3`,
`show_bool (1 == 1)` → `True`, `1.0 + 2.0` → `3.0`.
`&&`/`||` still short-circuit (`false && ((1.0/0.0) == 0.0)` → `False`) and
`Expr::Logic` was left untouched, because it is control flow, not a class method.

**Deleted:** `Expr::Arith`, `Expr::Cmp`, `ArithOp`, `infer_arith`, `lower_arith` —
`grep` finds zero occurrences of `Expr::Arith|Expr::Cmp`. `CmpOp` survives as an
**IR instruction** operand (`ValueCompare`), which is a different layer and is
correct.

**Still present, owned by stage 4:** `instantiate_method`,
`lower_primitive_method`, `lower_method_value`, `lower_operator_value`,
`lower_constrained_apply_site`. 3a removed the *legacy operator* path; the
*specialisation* paths are stage 4's deletion.

**Open concern for stage 7:** `1 == 1.0` is a type error at this stage. GHC prints
`True` for it because a literal is `fromInteger`-polymorphic and both operands
unify at one `Num a`. That becomes true when stage 7 makes literals
`fromInteger`; until then a type error is correct and strictly better than the
silent `False` it used to return.
