# DOCS-GAPS / open conformance defects

> Living register. Every entry is an **open defect** or a **missing capability**,
> never "acceptable behaviour". Per `AGENTS.md`: a defect that predates a change is
> still a defect; "pre-existing" describes age, not status.
>
> Status values: `open` (no owner yet), `scheduled` (a plan task will close it),
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
| OD-7 | `scripts/publish.sh` lists 16 crates including the absent `rill-graph`/`rill-router` | `grep -n 'rill-graph\|rill-router' scripts/publish.sh` | open |
| OD-8 | Docs describe the deleted signal/graph model (`_`, `<:`, `param()`, `CompiledGraphEngine`) | see `docs/superpowers/specs/2026-10-24-rill-docs-truth-reconciliation-design.md` | scheduled — docs plan |

## Missing capabilities (Haskell 2010 / GHC Prelude present, rill absent)

| ID | Capability | Reproduction | Status |
|---|---|---|---|
| MC-1 | `param(name, default[, min, max])` | compile error "param first argument must be a string literal" in every position; `?name` is the working surface | open (docs plan records it) |
| MC-2 | Launch arguments on `main` | runner takes none | open |
| MC-3 | Lazy list consumers/producers beyond `length` (`map`/`filter`/`fold`/`concat_map`/`++` over an infinite `Cons` chain fault loudly) | `take 5 (map f (from 1))` faults | scheduled — conformance Task 5 |
| MC-4 | `from`/`take` in the Prelude | absent; user-definable `from` works, `take` cannot reduce | scheduled — conformance Task 5 |
| MC-5 | Recursive Prelude helpers in Strict mode | dropped from Strict (compiler stack overflow otherwise); `unknown identifier` | open — needs a dedicated diagnostic |
