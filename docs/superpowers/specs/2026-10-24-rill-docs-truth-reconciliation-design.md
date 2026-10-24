# rill: Documentation-to-Code Truth Reconciliation — design

> **Status:** Design approved-in-conversation (2026-10-24); implementation planned
> separately via `superpowers:writing-plans`.
> **Date:** 2026-10-24
> **Scope:** every user-facing and contributor-facing document in the rill repo
> **except** historical material: `docs/superpowers/**`, `.superpowers/**`,
> generated `docs/book/`, and released `CHANGELOG` history.

---

## 0. Executive summary

The documentation describes a language and a runtime that no longer exist. This is
not a handful of stale API names:

- **The signal (arrow) language was deleted** in commit `752bc69` — "delete signal
  combinators, wire, cut, imaginary literals". `main = _ * 0.5`, `_ , _`, `sin : abs`,
  `<:`, `:>`, `~`, `@` are **parse errors** today (verified by running them).
- **The graph execution engine is gone**: `rill-graph` is not a workspace member and
  its directory does not exist; `CompiledGraphEngine`, `graph_engine.rs`,
  `process_tick` do not exist; `Node`/`Source`/`Processor`/`Sink`/
  `ProcessingState` are not traits in `rill-core` (the real traits are
  `Algorithm`, `MultichannelAlgorithm`, `IoDriver`/`IoBackend`, `Rack`, `Param`).
- **`param(...)` does not compile in any position** (verified). The working
  outside→program channel is `?name` (value track) and `?name` as a builtin argument.
- **`RillProgram`, `run_value()`, `value_outputs()`, `compile_with_ffi()`,
  `compile_runner()` do not exist.** The real API is
  `compile::<T, BUF>(src, ForeignRegistry)` → `ProgramRunner::run()` for
  `main : IO ()`, and `compile_eval` + `test_eval` (test-only) for value expressions.
- **Lazy `from n`/`take` are not reachable from `main : IO ()`** (verified:
  "unknown identifier `from`"). `complete`-mode recursion *does* work.
- Code doc-tests pass (4/4) — the compiled crate is the only authority; the drift is
  entirely in markdown prose.

The task: every document outside the historical set states only what the code
actually does. Criterion (user decision): **no statement the code refutes may
remain.**

---

## 1. User decisions (binding)

| Decision | Value |
|---|---|
| Criterion | **No false statement**; structure/tone of surviving docs preserved |
| Deleted arrow-model & graph chapters | **Remove from current docs and archive** under `docs/superpowers/archive/` (history preserved, not maintained as current) |
| `param()` in docs | Docs teach **`?name`** (the only working mechanism); `param()` becomes a **gap-register row** with exact reproduction |
| Orphan docs outside mdBook (`docs/architecture.md`, `docs/plans/`) | **Archive**, do not maintain two truths |
| Benchmark tables in the guide | **Delete** (numbers came from `lang_bench`, which does not compile; the GHC-parity harness will produce honest numbers later) |
| Truth source | **Code only** — `Cargo.toml`, public items, and runnable behaviour; never another document |

---

## 2. Goals, non-goals, anti-goals

### Goal

**G1.** No document outside the historical set contains a claim the code refutes:
no removed API, crate, trait, language construct, execution model, benchmark
number, or capability the runtime lacks.

### Non-goals

- Not a stylistic rewrite; no new chapters; no re-organisation beyond removing
  archived material from the live tree.
- Not modifying source code. If the code is behind a desirable behaviour, that is a
  **gap-register row**, not a doc sentence pretending it works.
- Not translating; English stays the rule (one Russian article is an explicit
  exception and carries no stale references).

**Historical material is read-only except for two sanctioned write paths:** (a)
appending archived documents under `docs/superpowers/archive/<date>-<slug>/`, and (b)
writing the gap register `docs/superpowers/DOCS-GAPS.md`. No existing file under
`docs/superpowers/**`, `.superpowers/**`, or `docs/book/**` may be edited; released
`CHANGELOG` history is likewise untouched.

### Anti-goals

- Replacing one false claim with another by copying a neighbouring doc.
- Deleting a section that documents a *real* capability just because a nearby
  sentence was false.
- "Fixing" a doc to match another stale doc.
- Editing code to match a doc.
- Silent removal of a documented-but-missing capability (it must become a gap row).

---

## 3. Verified drift inventory (2026-10-24)

### 3.1 Deleted language surface (the largest block)

| Documented | Reality |
|---|---|
| Wire `_`, cut `!`, `A : B` compose, `A , B`, `<:`, `:>`, `~`, `@`, imaginary `3i` | **parse errors** (deleted in `752bc69`); `Tok::Wire` still lexes but the parser rejects it |
| "A program is an arrow", "Combinators", "Operator precedence", primitives-as-arrows tables | describe a deleted language |
| `param("name", default[, min, max])` | **compile error in every position**; `?name` is the real control surface |
| `main : (String…) -> IO ()` launch arguments | the runner takes no arguments |
| `from n` infinite lists / `take` in value position | not in scope in `main : IO ()` (verified) |

### 3.2 Deleted runtime/API

| Documented | Reality |
|---|---|
| `rill-graph`, `GraphBuilder`, `GraphIr` | crate absent, not a member |
| `CompiledGraphEngine`, `graph_engine.rs`, `process_tick` | absent |
| `Node`, `Source`, `Processor`, `Sink`, `ProcessingState`, `NodeClosure` | absent from `rill-core`; real traits: `Algorithm`, `MultichannelAlgorithm`, `IoDriver`/`IoBackend`, `Rack` |
| `RillProgram`, `run_value()`, `value_outputs()`, `compile_with_ffi()`, `compile_runner()` | absent |
| `compile::<f32>(src)` | now `compile::<T, BUF>(src, ForeignRegistry)` → `ProgramRunner::run()` |
| `load_graph_json` (serialization guide) | absent |

### 3.3 Stale workspace facts

Crate count/list (`rill-graph` row; "17 active"), `#![deny(unsafe_code)]` crate list,
"all 19 crates" publish count, feature-flag tables, two-thread architecture as
written.

### 3.4 Stale numbers

Benchmark tables in `docs/src/guides/rill-lang.md` produced by a bench that no
longer compiles.

### 3.5 Files affected (stale-hit counts, audit)

`docs/src/guides/rill-lang.md` (1455 lines, 39), `CHANGELOG.md` (13, *Unreleased*
only), `docs/src/guides/getting-started.md` (9), `AGENTS.md` (8),
`docs/architecture.md` (7, orphan), `docs/src/architecture/graph.md` (6),
`docs/src/architecture/overview.md` (5), `rill-lang/README.md` (5),
`docs/src/index.md` (4), `docs/src/reference/crates.md` (3),
`docs/src/architecture/core.md` (3), plus `patchbay-rack.md`, `rill-analyzer.md`,
`serialization.md`, `two-thread-arch.md`, `rill-io/README.md`, `SUMMARY.md`.

---

## 4. The real system (what docs must say)

Verified from code — the replacement narrative all architecture docs converge on:

- **No execution tracks.** A program is either `main : IO ()` (run to completion by
  `ProgramRunner::run()`) or a value expression (`compile_eval` + `test_eval`,
  test-only).
- **DSP algorithms are `Algorithm<T>` / `MultichannelAlgorithm<T>`** traits; built-ins
  are whole-buffer. No `Node`/`Source`/`Sink` graph.
- **I/O is `IoDriver`/`IoBackend`** (`rill-core/src/io.rs`), implemented in
  `rill-io` backends (PortAudio/ALSA/PipeWire/JACK/null).
- **`rill-adrift::modular::ModularSystem`** hosts racks; its module docs state the
  control plane (automatons/servos/sensors) is **constructed but not driven** (no
  `ClockTick` source) — docs must say this, not pretend a live control plane.
- **Control values**: `?name` in the value track and as a builtin argument;
  `param(...)` does not compile.
- **`rill-graph`, `rill-router`, `rill-analyzer`**: not usable at this revision
  (`rill-analyzer` excluded; its `rill_lang::graph` dependency does not build).

---

## 5. Deliverables

1. **Live docs corrected** under the rules of §3–§4: `docs/src/**` (mdBook),
   top-level `*.md` (README, AGENTS, CONTRIBUTING, MANIFESTO, SECURITY, etc.),
   crate `README.md`s with stale claims, `CHANGELOG` *Unreleased* only.
   **Who decides:** the task implementer makes all correctness calls against the
   code (the only authority); anything genuinely ambiguous is recorded in
   `DOCS-GAPS.md` as an open question rather than guessed. There is no separate
   approver for a per-sentence correction — the reviewer checks them against the
   code at review time.
2. **Archive move**: deleted-model chapters and orphan documents
   (`docs/architecture.md`, `docs/plans/two_thread_architecture.md`,
   `docs/src/architecture/graph.md` content) moved to
   `docs/superpowers/archive/2026-10-24-deleted-signal-graph-model/` (git mv,
   history preserved), and removed from `SUMMARY.md` / inbound links.
3. **Gap register** `docs/superpowers/DOCS-GAPS.md`: every
   documented-but-missing capability ( `param()` with min/max, launch arguments,
   `from`/`take` in the value track, graph engine, `rill-graph`, value-path
   `run_value`, etc. ), each with a one-line reproduction (error text or run
   result) and status (`open` / `by design` / `superseded`).
4. **`scripts/check-docs.sh`**: deny-list scan over live markdown, excluding
   `docs/superpowers/**`, `.superpowers/**`, `docs/book/**`, and `CHANGELOG.md`
   below the first released `## [` heading. Exits non-zero on a hit. Documented in
   `AGENTS.md` commands.
5. **Truth rule** added to `CONTRIBUTING.md`: code is the authority; a doc claim
   about code must name the identifier and where it was verified.

---

## 6. Method

For each live document, every concrete claim (identifier, signature, crate, trait,
construct, count, number) is checked:

- **Identifier/signature** → grep the workspace for its definition/export.
- **Construct/capability** → compile and run a minimal program using it (the
  technique that produced §3's findings).
- **Count** → re-derive from `Cargo.toml` / `scripts/publish.sh`.
- **Number** → only keep if reproducible now; otherwise delete (user decision).

A claim the code refutes is **corrected**; a claim about a deleted subsystem is
**moved to the archive**, not silently deleted; a claim about a missing-but-desired
capability becomes a **gap row**.

---

## 7. Acceptance criteria

1. `scripts/check-docs.sh` exits 0 on the fixed tree (deny-list: `RillProgram`,
   `run_value`, `value_outputs`, `compile_with_ffi`, `compile_runner`,
   `CompiledGraphEngine`, `graph_engine`, `process_tick`, `GraphBuilder`,
   `ProcessingState`, `NodeClosure`, `load_graph_json`, `param(` outside a quoted
   error reproduction, plus `main = _`, `<:`, `:>` as live examples).
2. No live document states a removed construct as usable; each §3 row is resolved
   (corrected or archived).
3. Workspace facts (member list/count, `unsafe` list, publish count, feature flags,
   threading/IO model) match `Cargo.toml`, `scripts/publish.sh`, and the real
   traits.
4. `SUMMARY.md` and all inbound links contain no reference to an archived chapter.
5. `DOCS-GAPS.md` exists, has one row per gap with a reproduction, and is linked
   from the truth rule in `CONTRIBUTING.md`.
6. `cargo test --doc -p rill-lang` still passes; no source file changed by this task.
7. `mdbook build docs/` succeeds.

---

## 8. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Rewriting one falsehood into another | Code-only authority; verify claims by running them |
| Archiving something that is actually current | Archive only what the code refutes; a real capability stays (corrected) |
| The task becomes a full rewrite | Chapters are corrected or archived; no new narrative invented |
| Counts drift again | `check-docs.sh` + truth rule in CONTRIBUTING |
| Overlapping sibling plan (memory layers) changes arena docs | Docs state the *current* arena model, not the planned one; the sibling plan updates docs when it lands |
| CHANGELOG history trip the scan | Scan stops at the first released heading (user-sanctioned mechanism) |
| `.superpowers/**` drafts trip the scan | Explicit exclusion in the script |

---

## 9. Branch, base, and ordering

- **Base:** this task branches from `develop` as `feature/docs-truth-reconciliation`
  (git-flow). It does **not** stack on `feature/rill-lang-memory-layers`.
- **Why not the memory-layers branch:** the memory plan will rewrite the
  arena/memory prose again when it lands. This task documents the arena **as it is
  on `develop` today**; the memory-layers work is responsible for updating those
  passages when it merges (its own plan already lists docs as a deliverable).
- **Ordering:** this task may land before or after the memory plan; neither depends
  on the other's code. If both are in flight, land this one first — it removes the
  false model that would otherwise confuse the memory plan's doc updates.
- **Conflict surface:** `docs/src/guides/rill-lang.md` (memory-model section) and
  `CHANGELOG.md` *Unreleased*. Both are handled by the rule "state today's code";
  the memory plan owns the post-merge wording for its own change.
