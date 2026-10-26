# DOCS-GAPS / open conformance defects

> Living register. Every entry is an **open defect** or a **missing capability**,
> never "acceptable behaviour". Per `AGENTS.md`: a defect that predates a change is
> still a defect; "pre-existing" describes age, not status.
>
> **A documented deviation is still an error.** Recording a divergence here does
> not make it acceptable, settled, or a design choice — it only makes it visible
> and trackable. Do not cite a row in this file as justification for behaviour, as
> a reason not to fix something, or as "by design" unless the row's status is
> explicitly `by design` (which requires a recorded rationale approved by the
> user, not a self-assigned label). When reasoning about what the system should
> do, every non-`closed` row counts as a bug.
>
> Status values: `open` (no owner yet), `scheduled` (a plan task will close it),
> `by design` (deliberate, user-approved rationale recorded in the row),
> `closed` (delete the row).

## Open defects (wrong behaviour, no licence to remain)

| ID | Defect | Reproduction | Status |
|---|---|---|---|
| OD-1 | `{-# MODE strict #-}` + `do` prints each `write` **twice** | `{-# MODE strict #-} main = do { write (Just "x") stdout; return () };` → `["x","x"]`, expected `["x"]`. Complete mode prints once. | open |
| OD-2 | `param_to_f64` maps a non-numeric `ParamValue` to `0.0` | `rill-lang/src/backend/interp.rs:22` — `_ => 0.0` | open |
| OD-3 | `value_cmp` returns `0` (Equal) for a dangling ref | `rill-lang/src/backend/interp.rs:791` — `_ => 0` | open |
| OD-4 | `arrayFromAssocs`/`arrayUpdate`/`arrayAccum` association lists lack positive cons-chain tests | shared helper `pooled_elems_of` now supports `Cons`; only `arrayFromList` is covered by a test | open |
| OD-5 | `lang_bench` does not compile (`RillProgram`, one-arg `compile`) | `cargo bench -p rill-lang --bench lang_bench` → 5 errors | scheduled — superseded by the GHC-parity harness plan |
| OD-6 | `%` as a first-class value unsupported | `(% 1.0 2.0)` → loud error; `(+)`,`(-)`,`(*)`,`(/)` work | open |
| OD-7 | `scripts/publish.sh` lists 16 crates including two that no longer exist as crates. **`rill-router`** was absorbed into `rill-lang` (its `eq`/`mixer`/`dry_wet` builtins now live in `rill-lang/src/builtins/eq.rs` and the `foreign` catalog in `types/ty.rs`). **`rill-graph`** was first folded into `rill-lang` as the `graph` module (`a1bf6cf`, "fold rill-graph into rill-lang behind a graph feature" — its execution system had already been unified with `rill-lang`, leaving it as just another language front-end), then the module was **deleted** in `6409361` ("delete legacy Registry and graph path"), so graph compilation exists nowhere today and is deferred per `rill-lang/README.md`. A real `publish.sh` run would fail on both. | `grep -n 'rill-graph\|rill-router' scripts/publish.sh`; `ls -d rill-graph rill-router` → both absent; `git log --oneline --all -- 'rill-lang/src/graph/*'` shows the fold (`a1bf6cf`) and the deletion (`6409361`); workspace has 14 members | open |
| OD-8 | Docs describe the deleted signal/graph model (`_`, `<:`, `param()`, `CompiledGraphEngine`) | see `docs/superpowers/specs/2026-10-24-rill-docs-truth-reconciliation-design.md` | scheduled — docs plan |
| OD-10 | Complete (lazy) mode: a `where`/`let`-bound FUNCTION that calls a *parameterised* sibling `where`/`let` binding loses the enclosing parameter. The nested closure's body names the enclosing definition's parameter, but the intermediate closure never captures it, so lowering reports `unknown \`x\` in value lowering`. Fixed for a local function calling a sibling VALUE binding (`h = x + 1.0`) and for a `where` function capturing the enclosing parameter directly; the transitive case below remains. | `dbl x = g 10.0 where { g y = y * h; h = j 1.0; j z = z + x; };` + `main = do { write (Just (show_float (dbl 1.5))) stdout; return () };` → `type error: unknown \`x\` in value lowering`. The same source works through `compile_eval` in Strict mode. | open |
| OD-11 | A guard inside a `let`-bound local function silently matches nothing: the guarded equation is not used, so the comprehension/`concat_map` over it yields an empty result instead of the guarded branch. **Silent wrong answer** — no error is reported. | `{-# MODE strict #-} main = do { write (Just (show_int (length (let ok x \| x > 1.0 = cons x (list) \| otherwise = list in concat_map ok [1,2,3])))) stdout; return () };` → `0` (expected `2`). The same guard at top level works; the two-equation form `let ok 1.0 = …; ok x = …` also works (`3`). | open |
| OD-12 | Complete mode: `length` over a `Cons` chain returns `0`, so any list built with `cons` in the default mode reports an empty length. | `{-# MODE complete #-} main = do { write (Just (show_int (length (cons 1 (cons 2 (list)))))) stdout; return () };` → `0` (expected `2`). Strict mode returns `2`. | scheduled — conformance Task 5 |
| OD-13 | **Infix operators are not class-method sugar.** `x == y`, `x + y`, `x < y` parse to `Expr::Cmp`/`Expr::Arith` and lower to comparison/arithmetic kernels directly, so they never reach class-method dispatch and record no dictionary selection. Haskell 2010 §3.4 defines every infix operator as sugar over a class method, so a body constrained `Ord a =>` must resolve `==` (`Eq`'s method) against the `Ord` witness — the prefix spelling `(==) x y` does, the infix spelling does not, and the two must be the same judgement. The operator-as-method desugaring is Task P-A of the typeclass-layer design (paused). | `f x y :: Ord a => a -> a -> Bool = x == y;` type-checks but records NO dictionary selection; `f x y :: Ord a => a -> a -> Bool = (==) x y;` records `Dict("Ord")` for class `Eq`. | scheduled — Task P-A (operator-as-method sugar) |
| OD-14 | **No ambiguity check on a class context.** A type signature whose context variable does not occur in the type is accepted, so an ambiguous constraint can be written and silently ignored: the definition type-checks and needs no instance. Haskell 2010 §4.1.1 / GHC require a class-context variable to appear in the type and reject the rest with "ambiguous type variable". Discovered during the dictionary groundwork (stage 1), where `infer_constrained_def_body` must skip a constraint whose variable its signature never mentions — the skip is correct *given* the signature is legal, and it is legal only because the check is missing. Verified on GHC 9.10.3: both reproductions below are `Could not deduce ‘Ix i0’ … The type variable ‘i0’ is ambiguous`. | `f :: Ix i => Int = 1; main = 0.0;` compiles (GHC: rejected). `f x :: Ix i => Int -> Int = x; main = 0.0;` compiles (GHC: rejected). The MENTIONED control `f p :: Ix i => Pair i i -> Int = rangeSize p;` is valid in both. | open — Haskell 2010 conformance audit area A4 |
| OD-15 | **An abstract constructor head `m a` does not unify with `Maybe a`.** A definition whose parameter type is a type-constructor variable applied to arguments cannot accept a concrete application of the same arity, so the higher-kinded polymorphic argument shape `h y :: m a -> Int = g y; h (Just 3)` is rejected. GHC accepts the equivalent program (`h :: m a -> Int` applied to `Just 3`). `unify_value`'s `TyConApp ~ App` arm already exists for the bare head; the failure is the *argument* unify leaving the head var bound to the bare constructor while the arity-specific application still mismatches. Discovered as the next real blocker while typing constrained bodies (stage 1, Task 3); it reproduces identically on `91db138`, i.e. it predates this branch. This is the shape `Functor f =>` / `Monad m =>` bodies need, so it gates the dictionary model's constraint-parameterised instances (design §3.10). | `g x :: Maybe Int -> Int = 1; h y :: m a -> Int = g y; main = h (Just 3);` → `cannot unify value type App("Maybe", [Int]) with App("m", [App("Maybe", [Int])])`. The same with `m Int` reports `App("m", [Int])`. The no-argument form `h y :: m a -> Int = 0;` is accepted, so it is the call/unify path, not the signature parse. | open — blocks design §3.10 (constraint-parameterised instance heads) |
| OD-16 | **Mixed `Int`/`Float` value arithmetic is accepted instead of unified.** `infer_arith`'s value-track branch checks that each operand is numeric but never unifies the two operands **with each other**, so an `Int` and a `Float` combine freely and the result silently widens to `Float`. Haskell 2010 gives one `Num a` to both operands, so a genuine `Int`-vs-`Float` pair is a type error (`No instance for (Fractional Int)` when the `Int` is pinned by a literal, `Couldn't match expected type ‘Int’ with actual type ‘Double’` when both are bindings). Found during the stage-2 close-out audit. **This makes design acceptance criterion 15 fail:** `(1::Int) + 1.0` must be a `Fractional Int` error, and it evaluates to `2.0` instead. On `91db138` the same source did not parse (`expr :: type` was absent), so the annotation work *exposed* this hole rather than causing it — the arithmetic rule is the defect. | `i :: Int = 1; f :: Double = 1.0; main = show_double (i + f);` → runs, `2.0` (GHC: type error). `main = (1 :: Int) + 1.0;` → runs, `2.0` (GHC: `No instance for ‘Fractional Int’`; design §3.6 and criterion 15 require a type error). `k :: Int = 1; main = show_double (k + 1.0);` → runs, `2.0`. Note the ASYMMETRY that proves it is an operand-typing hole: `main = show_double (1 + (2 :: Int));` **is** rejected (`cannot unify value type Int with Double`), because there the annotation flows through the expected type rather than meeting a second operand. **Renamed 2026-10-26:** the double-precision type was called `Float`; it is now `Double` (task 5.5pre-0). The behavioural defect is unchanged — only the spelling in these reproductions updated. | open — Haskell 2010 conformance audit area A4 |
| OD-17 | **A mixed `Int`/`Float` comparison is accepted AND silently wrong.** Two layers: (a) `Expr::Cmp` types by inferring both sides as value constants and returning `Bool` (no operand unification, same hole as OD-16); (b) the interpreter's `value_cmp_ref` has no `Int`/`Float` arm, so a cross-kind pair falls through to `_ => kind_rank(a).cmp(&kind_rank(b))`, where `kind_rank(Int) = 1` and `kind_rank(Float) = 2`. Every `Int` is therefore *less than* every `Float`, making the comparison both a type error missed at compile time and a wrong answer at run time. **A silent wrong answer is the worst class of defect** (`AGENTS.md`). Reproduces identically on `91db138`. The cross-kind arm is correct as a total order for heterogeneous collection keys (`Map`/`Set`/`sort`), so the fix belongs in typing (reject the mix) rather than in removing the arm. | `main = 5 < 2.0;` → `true` (expected `false`; GHC: type error). `main = 1 == 1.0;` → `false` (GHC: `True`). `main = 1 /= 1.0;` → `true` (GHC: `False`). `main = 2 < 1.5;` → `true` (expected `false`). `i :: Int = 1; f :: Double = 1.0; main = i == f;` → `false` (GHC: type error). The same-kind comparisons (`1 < 2`, `1.0 < 2.0`) are correct. | open — Haskell 2010 conformance audit area A5 (silent wrong answer) |

## Missing capabilities (Haskell 2010 / GHC Prelude present, rill absent)

| ID | Capability | Reproduction | Status |
|---|---|---|---|
| MC-1 | `param(name, default[, min, max])` | compile error "param first argument must be a string literal" in every position; `?name` is the working surface | open (docs plan records it) |
| MC-2 | Launch arguments on `main` | runner takes none | open |
| MC-3 | Lazy list consumers/producers beyond `length` (`map`/`filter`/`fold`/`concat_map`/`++` over an infinite `Cons` chain fault loudly) | `take 5 (map f (from 1))` faults | scheduled — conformance Task 5 |
| MC-4 | `from`/`take` in the Prelude | absent; user-definable `from` works, `take` cannot reduce | scheduled — conformance Task 5 |
| MC-5 | Recursive Prelude helpers in Strict mode | dropped from Strict (compiler stack overflow otherwise); `unknown identifier` | open — needs a dedicated diagnostic |

## Runtime typeclass dictionaries — planned, not implemented (stages 3–8)

> **Design:** `docs/superpowers/specs/2026-10-25-rill-lang-runtime-dictionaries-design.md`
> §3.7 "Migration order". **Plan:** `docs/superpowers/plans/2026-10-25-rill-lang-dictionaries-stage1.md`.
>
> Stages 0–2 of §3.7 are **done** (type-system groundwork: `Expr::Annot`,
> `ValueTy::Dict`, `Scheme.constraints`, `dict_selections`, constrained bodies
> typed under a given constraint; and the runtime value: `Value::Dictionary`,
> `DictSlot`, every RC walker). **Dispatch is still the OLD model** — a
> constrained definition is still specialised and β-reduced per call site, and
> **nothing constructs a dictionary at runtime**.
>
> The following are **planned but NOT implemented**. Each is open work, not an
> accepted behaviour: until it lands, the monomorphisation model below is what
> actually runs.

| Stage (§3.7) | Planned capability | State today |
|---|---|---|
| **3** | **Dictionary construction at seat** — build one dictionary per *used* `(class, type)` pair during `WorldState::seat`, into a `Vec<ArenaRef>` table on the world (design §3.2). | **Not implemented.** No dictionary is ever constructed by the compiler; `Value::Dictionary` exists only for hand-built test values. |
| **4** | **Dictionary parameters** — a polymorphic function takes one extra leading parameter per constraint (`f :: Num a => a -> a` elaborates to `\dict -> \x -> …`), driven by the `TypedProgram.dict_selections` table. | **Not implemented.** `dict_selections` is recorded by inference and **read by nobody**; lowering still re-derives dispatch from signatures and β-reduces a specialised body per call site. |
| **5** | **Slot projection** — a method call projects a slot from a dictionary instead of inlining a body; deletes `instantiate_method` and `lower_primitive_method` (design §3.4). | **Not implemented.** Both paths are live and are what runs today. |
| **6** | **Method-as-value selectors** — `fold (+)` is a closure over `(dict_ref, slot)` from one generated selector fragment per `(class, method)`; deletes `lower_method_value` and `lower_operator_value` (design §3.8). | **Not implemented.** Both paths synthesise AST per call site (`$op_a`/`$prim_a` names) and are what runs today. |
| **7** | **Superclass projection** — a superclass method resolves through `supers[i]` with no signature pattern-matching at lowering (design §3.3, §3.5). | **Not implemented at runtime.** `supers` is walked by the RC walkers but never populated by the compiler; inference records a `Dict(g)` evidence for superclass resolution, and lowering still uses `class_var_solved_ty`-style pattern matching. |
| **8** | **Literals as `fromInteger`** — numeric literals elaborate to `fromInteger dict n` / `fromRational dict …`, with defaulting, making `1 + 1.0 == 2.0` and `(1::Int) + 1.0` a type error follow from the model (design §3.6). | **Not implemented.** Literals are typed and lowered by the existing scalar rules; there is no `fromInteger` instruction and no defaulting pass. |
| **8** | **Delete the five lowering paths** — `ConstrainedDef` + `lower_constrained_apply_site`, `instantiate_method`, `lower_method_value`, `lower_operator_value`, `lower_primitive_method` gone, grep-proven (design §3.4, acceptance criterion 8). | **Not implemented.** All five are present and live. |

### Declared-but-unimplemented `KernelId` variants

`rill-lang/src/arena.rs` declares the whole primitive-operation surface so the
slot layout is stable, but **six variants have no IR instruction** and cannot be
executed (design §3.9 puts the instructions in scope for this model):

| Variant | Blocked on |
|---|---|
| `KernelId::Compare` | `ValueCompare3` (or a `-1/0/1` `ValueCompare`) — must keep the total order (`value_cmp_ref`: NaN greatest, NaN == NaN) |
| `KernelId::Negate` | a scalar `ValueUnOp` — buffers already have `UnOp::Neg` |
| `KernelId::Signum` | a `ValueUnOp` variant |
| `KernelId::FromInteger` | `ValueFromInteger` — **mandatory for literal elaboration (stage 8)** |
| `KernelId::Recip` | `ValueRecip` (`recip x = 1/x`) |
| `KernelId::Abs` (scalar) | a scalar `ValueUnOp` — the buffer form (`ValueBufferUn`) already exists |

`Eq`/`Ne`/`Lt`/`Gt`/`Le`/`Ge` and `Add`/`Sub`/`Mul`/`Div` map to existing
instructions and do execute.

### `supers` is heap-cloned in `pool_clone_value`

`interp::pool_clone_value`'s `Value::Dictionary` arm clones `supers` with
`Vec::clone` — a **heap allocation** — rather than serving it from the arena's
buffer pool the way `Record`/`Sum`/`List`/`Set`/`Array` do. There is no pool arm
for `supers`, and adding one needs a matching `detach_payload` arm so the pool's
take/put pair stays symmetric (the invariant
`arena::tests::array_clone_pooled_preserves_shape` pins for arrays). Not an RT
defect today because no dictionary is cloned on the execution path yet (stage 3
introduces that); it becomes one when dictionaries are constructed at seat.

