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

## Missing capabilities (Haskell 2010 / GHC Prelude present, rill absent)

| ID | Capability | Reproduction | Status |
|---|---|---|---|
| MC-1 | `param(name, default[, min, max])` | compile error "param first argument must be a string literal" in every position; `?name` is the working surface | open (docs plan records it) |
| MC-2 | Launch arguments on `main` | runner takes none | open |
| MC-3 | Lazy list consumers/producers beyond `length` (`map`/`filter`/`fold`/`concat_map`/`++` over an infinite `Cons` chain fault loudly) | `take 5 (map f (from 1))` faults | scheduled — conformance Task 5 |
| MC-4 | `from`/`take` in the Prelude | absent; user-definable `from` works, `take` cannot reduce | scheduled — conformance Task 5 |
| MC-5 | Recursive Prelude helpers in Strict mode | dropped from Strict (compiler stack overflow otherwise); `unknown identifier` | open — needs a dedicated diagnostic |
