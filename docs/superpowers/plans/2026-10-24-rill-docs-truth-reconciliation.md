# Documentation-to-Code Truth Reconciliation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every live document in the rill repo state only what the code actually does, archiving the deleted signal/graph model and recording missing-but-documented capabilities as gaps.

**Architecture:** Work document-by-document, in dependency order (facts and scripts first, then the reference guide, then architecture chapters, then READMEs/AGENTS). Each task verifies claims by reading code or running a minimal program, corrects or archives, and commits. A deny-list script plus a gap register make the result checkable and prevent re-drift.

**Tech Stack:** Markdown (mdBook `docs/src/`), Rust workspace (`rill-core`, `rill-lang`, `rill-adrift`), bash (`scripts/check-docs.sh`), GHC not required.

**Spec:** `docs/superpowers/specs/2026-10-24-rill-docs-truth-reconciliation-design.md`

## Global Constraints

- **Code is the only authority.** Never correct a document by copying another document; verify each claim against `Cargo.toml`, public items, or a running program.
- **Do not edit historical material:** `docs/superpowers/**` (except the two sanctioned writes below), `.superpowers/**`, `docs/book/**`, and `CHANGELOG.md` below the first `## [` released heading.
- **Sanctioned writes into history:** (a) new files under `docs/superpowers/archive/2026-10-24-deleted-signal-graph-model/`, (b) `docs/superpowers/DOCS-GAPS.md`.
- **Do not modify source code** (`*.rs`, `Cargo.toml`, benches). This plan is documentation + scripts only.
- **English only**, except the existing Russian `docs/src/guides/world-of-automatons.md` (explicit exception; it has no stale references and is not edited).
- **Deleted language facts (verified 2026-10-24):** `_`, `!`, `A : B`, `A , B`, `<:`, `:>`, `~`, `@`, `3i` are **parse errors**; `param(...)` is a **compile error in every position**; `?name` is the working control surface; `main : IO ()` takes no launch arguments; `from`/`take` are not in scope in `main : IO ()`.
- **Deleted API facts:** `RillProgram`, `run_value()`, `value_outputs()`, `compile_with_ffi()`, `compile_runner()`, `CompiledGraphEngine`, `graph_engine`, `process_tick`, `GraphBuilder`, `ProcessingState`, `NodeClosure`, `load_graph_json`, `Node`/`Source`/`Processor`/`Sink` traits do not exist.
- **Real API:** `compile::<T, BUF>(src, ForeignRegistry) -> ProgramRunner<T, BUF>`, `ProgramRunner::run()`, `set_stdin`, `world_mut().set_param`; value path `compile_eval::<T, BUF>(defs, expr, mode)` + `test_eval`; traits `Algorithm`, `MultichannelAlgorithm`, `IoDriver`/`IoBackend`, `Rack`.
- Commit messages single-quoted (`git commit -m '...'`).

## Review Focus

1. **A "current" chapter still teaching a deleted construct** — e.g. a surviving example using `_`/`<:`/`param(...)`. Verify every code block in a touched file by attempting to compile it (or marking it as an archived/historical block).
2. **An archived chapter still linked from the live tree** — `SUMMARY.md` or any README/guide link pointing at a moved chapter. Grep inbound links after every move.
3. **A workspace count/list that is plausible but wrong** — crate count, feature flags, `unsafe` crate list, publish count. Re-derive from `Cargo.toml`/`scripts/publish.sh`; never carry the old number.
4. **A gap silently deleted** — a documented-but-missing capability removed instead of recorded. Every removal of a capability sentence must have a `DOCS-GAPS.md` row.
5. **`check-docs.sh` that is born red or trivially green** — it must scan the live tree only, stop CHANGELOG at the first released heading, and fail on a real injected hit (task adds a self-test).

---

## File Structure

| File | Responsibility |
|---|---|
| `scripts/check-docs.sh` | Deny-list scan over live markdown; exits non-zero on a hit |
| `docs/superpowers/DOCS-GAPS.md` | Register of documented-but-missing capabilities |
| `docs/superpowers/archive/2026-10-24-deleted-signal-graph-model/` | Archived deleted-model chapters and orphan documents |
| `docs/src/SUMMARY.md` | Live chapter index; must not link archived material |
| `docs/src/guides/rill-lang.md` | Canonical language reference (largest correction surface) |
| `docs/src/guides/getting-started.md` | Entry guide |
| `docs/src/architecture/{overview,core,graph,actor,patchbay-rack,midi}.md` | Architecture chapters |
| `docs/src/guides/{serialization,two-thread-arch,rill-analyzer}.md` | Guide chapters with stale claims |
| `docs/src/index.md`, `docs/src/reference/crates.md` | Landing/reference pages |
| `AGENTS.md`, `CONTRIBUTING.md`, `README.md`, `rill-lang/README.md`, `rill-io/README.md`, `CHANGELOG.md` | Top-level and crate docs |
| `docs/architecture.md`, `docs/plans/two_thread_architecture.md` | Orphan documents (to archive) |

---

### Task 1: Truth infrastructure — deny-list script and gap register

**Files:**
- Create: `scripts/check-docs.sh`
- Create: `docs/superpowers/DOCS-GAPS.md`
- Test: `scripts/check-docs.sh --self-test` (in-script)

**Interfaces:**
- Consumes: nothing.
- Produces: `scripts/check-docs.sh` (exit 0 = clean, 1 = hit, prints `file:line: term`); `docs/superpowers/DOCS-GAPS.md` (markdown table with columns `Capability | Current behaviour | Reproduction | Status`).

- [ ] **Step 1: Write the script and prove it fails on a real hit**

`scripts/check-docs.sh` must:
1. Build the scan list from git: `git ls-files '*.md'`.
2. Exclude: `docs/superpowers/**`, `.superpowers/**`, `docs/book/**`, `docs/superpowers/archive/**`.
3. For `CHANGELOG.md`, scan only lines before the first line matching `^## \[[0-9]`.
4. Search for the deny-list (case-sensitive, fixed strings): `RillProgram`, `run_value(`, `value_outputs(`, `compile_with_ffi`, `compile_runner`, `CompiledGraphEngine`, `graph_engine`, `process_tick`, `GraphBuilder`, `ProcessingState`, `NodeClosure`, `load_graph_json`.
5. Print `path:line: term` for each hit and exit 1 if any; exit 0 with `docs clean` otherwise.
6. `--self-test`: create a temp file with one deny-listed term, run the scanner body against it, expect exit 1, then delete it; print `self-test ok`.

- [ ] **Step 2: Run the self-test and the real scan**

Run: `bash scripts/check-docs.sh --self-test`
Expected: `self-test ok`.
Run: `bash scripts/check-docs.sh`
Expected: exits 1 today, listing the known stale hits (this is the baseline the later tasks must empty). Record the count in the gap register's header.

- [ ] **Step 3: Create the gap register with the known rows**

Populate `docs/superpowers/DOCS-GAPS.md` with one row per capability the design lists:
`param(name, default[, min, max])` (compile error: "param first argument must be a string literal"), launch arguments on `main` (runner takes none), `from`/`take` in the value track (error: "unknown identifier `from`"), graph engine (`CompiledGraphEngine`/`GraphBuilder` absent), `rill-graph` crate (absent from workspace), `run_value()`/`value_outputs()`/`RillProgram` (absent), `load_graph_json` (absent), and **publish script drift** (`scripts/publish.sh` lists 16 crates including the absent `rill-graph`/`rill-router`; reproduction: `grep -n 'rill-graph\|rill-router' scripts/publish.sh`). Status `open` unless the design says `superseded`.

- [ ] **Step 4: Verify**

Run: `bash scripts/check-docs.sh` — expect the same hit list as Step 2, and confirm the register lists every deny-list term as a row.

- [ ] **Step 5: Commit**

```bash
git add scripts/check-docs.sh docs/superpowers/DOCS-GAPS.md
git commit -m 'docs: add docs truth deny-list check and gap register'
```

---

### Task 2: Archive the deleted signal/graph model

**Files:**
- Create (via `git mv`): `docs/superpowers/archive/2026-10-24-deleted-signal-graph-model/graph.md`, `.../architecture.md`, `.../two_thread_architecture.md`
- Modify: `docs/src/SUMMARY.md`
- Modify: `docs/src/architecture/overview.md:54`, `docs/src/guides/getting-started.md:170` (inbound graph links)
- Modify: `CONTRIBUTING.md:101` (links at `docs/architecture.md`, which this task archives)
- Modify: every live file with an inbound link to the moved content

**Interfaces:**
- Consumes: Task 1's script (for the final scan).
- Produces: the archive directory; `SUMMARY.md` without the graph chapter; no live inbound links.

- [ ] **Step 1: Move the orphan documents**

```bash
mkdir -p docs/superpowers/archive/2026-10-24-deleted-signal-graph-model
git mv docs/architecture.md docs/superpowers/archive/2026-10-24-deleted-signal-graph-model/architecture.md
git mv docs/plans/two_thread_architecture.md docs/superpowers/archive/2026-10-24-deleted-signal-graph-model/two_thread_architecture.md
git mv docs/src/architecture/graph.md docs/superpowers/archive/2026-10-24-deleted-signal-graph-model/graph.md
```

- [ ] **Step 2: Add a header banner to each archived file**

First line of each moved file: `> **ARCHIVED 2026-10-24 — describes a removed subsystem.** Kept as history; do not follow it as current. See docs/superpowers/DOCS-GAPS.md.`

- [ ] **Step 3: Remove the graph chapter from the live index**

In `docs/src/SUMMARY.md`, delete the line `- [Signal graph (rill-graph)](architecture/graph.md)`.

- [ ] **Step 4: Find and fix inbound links**

Run: `grep -rn 'architecture/graph\.md\|docs/architecture\.md\|two_thread_architecture' --include='*.md' docs/src *.md rill-*/README.md | grep -v superpowers`
Expected hits to resolve: `docs/src/SUMMARY.md:11`, `docs/src/architecture/overview.md:54`, `docs/src/guides/getting-started.md:170`, `CONTRIBUTING.md:101`. Replace each live link with a sentence pointing at the current execution model (Task 5's wording) — never a link to the archive. (`CHANGELOG.md` hits are released history: leave them.)

- [ ] **Step 5: Verify the index still builds**

Run: `mdbook build docs/`
Expected: success, no "file not found" for `architecture/graph.md`.

- [ ] **Step 6: Commit**

```bash
git add -A docs/superpowers/archive docs/src/SUMMARY.md docs/architecture.md docs/plans docs/src/architecture/graph.md
git commit -m 'docs: archive deleted signal/graph model chapters'
```

---

### Task 3: Rewrite the language reference's deleted-language chapters (`rill-lang.md`, part 1)

**Files:**
- Modify: `docs/src/guides/rill-lang.md` — sections `## A first program`/`### A program is an arrow` (lines ~64–123), `## Primitives` (443–461), `## Combinators` (462–508), `## Parameters` → `param()` subsections (703–830), `## Benchmarks` (974–1025), `## Multi-IO and graph compilation` → `### Graph compilation` (831–863)
- Test: each retained code block compiles via a scratch Rust test (template below)

**Interfaces:**
- Consumes: Task 1's gap-register rows; the verified real API facts (Global Constraints).
- Produces: a reference whose examples either compile or are explicitly labelled historical.

- [ ] **Step 1: Establish the compile-check harness for doc examples**

Create a temporary test file `rill-lang/tests/zz_doc_examples.rs` (delete before the final commit) with one `#[test]` per code block you intend to keep, each running `compile::<f32, 256>(src, ForeignRegistry::new())` (or `compile_eval` for value expressions) and asserting the documented result. This is the evidence that a kept example is real.

- [ ] **Step 2: Replace the arrow chapter with the real program model**

Replace `### A program is an arrow` with a section stating: a program is a list of definitions ending in `;`; `main : IO ()` runs to completion via `ProgramRunner::run()`; a value expression is evaluated via `compile_eval` + `test_eval`; DSP algorithms are built-ins called with juxtaposed arguments (`sine 440.0 0.5 0.0`). Add the sentence: "Signal-block combinators (`:`, `,`, `<:`, `:>`, `~`, `@`) and the wire/cut construct were **removed**; see the gap register." No example may use them.

- [ ] **Step 3: Delete the primitives/combinator tables and the precedence section**

Remove `## Primitives`, `## Combinators`, `### Operator precedence`, `### Idioms`. Replace with one short "Expressions and application" subsection that documents only what parses: literals, arithmetic, comparison/logic, `if/then/else`, `case`, lambda, juxtaposed application, records, list/map literals, `!` indexing. Verify each by compile-check.

- [ ] **Step 4: Replace `param()` documentation with `?name`**

Rewrite `## Parameters` and `## Actor parameters` to teach only `?name` (value track read and builtin-argument form, and `?name=default`), including `world_mut().set_param(...)` and `params_meta()`. Delete the `param(name, default[, min, max])` subsection and the `?name` vs `param()` comparison table. Move min/max clamping out (recorded as a gap row in Task 1). Verify the retained examples compile and that `?g` reads.

- [ ] **Step 5: Delete the benchmark tables**

Remove the `## Benchmarks` chapter's three tables (compilation, runtime, built-ins) and the surrounding claims. Replace with one sentence: measurements from the previous API revision are not reproducible and are superseded by the GHC-parity harness plan; see `docs/superpowers/specs/2026-10-24-rill-lang-ghc-parity-benchmark-design.md`.

- [ ] **Step 6: Remove graph-compilation claims**

In `## Multi-IO and graph compilation`, replace `### Graph compilation` with a note that `compile_graph()`/`CompiledGraphEngine` do not exist, `rill-analyzer` is excluded from the workspace, and graph compilation is deferred.

- [ ] **Step 7: Verify**

Run the scratch test file: `cargo test -p rill-lang --features dsp --test zz_doc_examples`
Expected: all retained examples pass. Delete `rill-lang/tests/zz_doc_examples.rs`.
Run: `bash scripts/check-docs.sh`
Expected: hits reduced by every row resolved in this file.

- [ ] **Step 8: Commit**

```bash
git add docs/src/guides/rill-lang.md
git commit -m 'docs(lang): remove deleted signal/graph/param model from the reference'
```

---

### Task 4: Correct the rest of `rill-lang.md` (runtime, serialization, collections, memory)

**Files:**
- Modify: `docs/src/guides/rill-lang.md` — `## Execution model and performance` (912–973), `## Serialization` (875–891), `## Using it in an application` → `### The pure-value tick` (888–911), `## Value channels and first-class data` (1026–1414), `## Status` (1415–1455)
- Test: scratch compile-check harness from Task 3 (re-create, delete after)

**Interfaces:**
- Consumes: Task 3's compiled-verified reference style.
- Produces: a guide whose runtime/collection/memory claims match `program_runner.rs`, `program.rs`, and the arena as it exists on `develop`.

- [ ] **Step 1: Fix the execution model chapter**

`### Scheduling` and `### The IO executor` must describe `ProgramRunner::run()` (seat + `run_main_io`) and drop `CompiledGraphEngine`, `ProcessingState`, `NodeClosure`, graph-node scheduling. Keep `### β-reduction` only if it matches the current interpreter; verify by reading `rill-lang/src/backend/interp.rs`.

- [ ] **Step 2: Fix the pure-value tick**

`### The pure-value tick` must state the real surface: `compile_eval` + `test_eval` (test-only) and `world.result()`; delete `run_value()`/`value_outputs()` references.

- [ ] **Step 3: Fix serialization claims**

In `## Serialization`, keep only what exists: `rill-lang` cargo feature `serialization`, `program_serde`, `serde_def::compile_def`/`RillLangDef`. Delete `GraphDef`/`NodeDef`/`ConnectionDef`/`GraphBuilder`/`load_graph_json` claims (verify by grepping `rill-lang/src/program_serde.rs` and `rill-adrift/src/registration.rs`).

- [ ] **Step 4: Fix collection and memory claims**

In `## Value channels and first-class data`, verify each documented collection operation by compile-check (`arrayFromList`, `arrayIndexFlat`, `newArrayFlat`/`writeArrayFlat`/`readArrayFlat`, `runSTArray`, `map`, `fold`, `length`, `fromList`/`toList`). Remove any operation the compiler rejects. In `### Memory model`, state the arena as implemented today (page-based `ChunkedVec`, RC, COW) without promising the layered model.

- [ ] **Step 5: Fix the status section**

`## Status` must match `Cargo.toml` members and the real feature set. Re-derive, do not copy.

- [ ] **Step 6: Verify and commit**

Run the scratch compile checks and `bash scripts/check-docs.sh`; delete the scratch file.
```bash
git add docs/src/guides/rill-lang.md
git commit -m 'docs(lang): correct runtime, serialization, collections and memory claims'
```

---

### Task 5: Architecture chapters (`overview`, `core`, `two-thread-arch`, `patchbay-rack`, `actor`)

**Files:**
- Modify: `docs/src/architecture/overview.md`, `core.md`, `patchbay-rack.md`, `actor.md`
- Modify: `docs/src/guides/two-thread-arch.md`
- Test: `mdbook build docs/`; `scripts/check-docs.sh`

**Interfaces:**
- Consumes: the real-system narrative (design §4).
- Produces: architecture chapters describing `Algorithm`/`MultichannelAlgorithm`/`IoDriver`/`IoBackend`/`Rack`, `ModularSystem` with an inert control plane, and no graph engine.

- [ ] **Step 1: Replace `core.md`'s deleted trait set**

Delete `### Node`, `### Source, Processor, Sink`, `### ProcessingState` (verified absent from `rill-core/src/traits`). Document the real modules in `rill-core/src/traits/`: `algorithm`, `multichannel_algorithm`, `param`, `parameter_write`, `rack`, `buffer_view`, `error`, plus `io.rs`'s `IoDriver`/`IoBackend`/`IoCapture`/`IoPlayback`.

- [ ] **Step 2: Fix `overview.md`'s layer diagram and concepts**

Remove "Signal graph (DAG)", `CompiledGraphEngine`, `rill-graph` from the diagram and prose; describe the real path: `compile` → `ProgramRunner` → `main : IO ()`; `ModularSystem` racks; `IoDriver` backends; control plane constructed-but-inert.

- [ ] **Step 3: Fix the two-thread guide**

Rewrite `docs/src/guides/two-thread-arch.md` around `IoDriver::set_callback` + `run()`, the backends in `rill-io`, and the inert `rill-adrift::modular` control plane (quote its own module doc). No `CompiledGraphEngine`/`process_tick`.

- [ ] **Step 4: Fix patchbay/actor stale references**

Check each claim in `patchbay-rack.md` and `actor.md` against `rill-patchbay/src` and `rill-core-actor/src`; correct or remove graph-engine references (the `MidiInputDef` legacy note may stay if the type exists).

- [ ] **Step 5: Verify and commit**

Run `mdbook build docs/` and `bash scripts/check-docs.sh`.
```bash
git add docs/src/architecture docs/src/guides/two-thread-arch.md
git commit -m 'docs: rewrite architecture chapters for the current runtime'
```

---

### Task 6: Entry guides and reference pages

**Files:**
- Modify: `docs/src/guides/getting-started.md`, `docs/src/index.md`, `docs/src/reference/crates.md`, `docs/src/guides/serialization.md`, `docs/src/guides/rill-analyzer.md`
- Test: each Rust code block in `getting-started.md` compiles (scratch crate or `cargo test` doc-harness); `mdbook build docs/`

**Interfaces:**
- Consumes: Task 5's architecture narrative.
- Produces: an onboarding path that works.

- [ ] **Step 1: Rewrite `getting-started.md`**

Replace the graph-builder example with the real quick start (`compile` + `ProgramRunner::run`, or a builtin-calling program). Fix the "pure-value program ... `run_value()`" block to `compile_eval` + `test_eval`. Remove the "Two-Thread Architecture" section's graph-engine claims and link Task 5's guide.

- [ ] **Step 2: Fix `index.md` and `reference/crates.md`**

Replace `run_value()`/`compile_runner()`/`compile_graph()` with the real API; re-derive the crate table from `Cargo.toml` (14 members at this revision) and mark deferred crates (`rill-graph` absent, `rill-analyzer` excluded) explicitly.

- [ ] **Step 3: Fix `serialization.md` and `rill-analyzer.md`**

`serialization.md`: keep only `program_serde`/`compile_def`; archive-or-remove the `GraphDef`/`GraphBuilder` walkthrough (moved content is already archived in Task 2 for architecture; here, delete and point at the gap register). `rill-analyzer.md`: state it is excluded from the workspace and does not build at this revision.

- [ ] **Step 4: Verify and commit**

```bash
git add docs/src/guides/getting-started.md docs/src/index.md docs/src/reference/crates.md docs/src/guides/serialization.md docs/src/guides/rill-analyzer.md
git commit -m 'docs: fix entry guides and reference pages'
```

---

### Task 7: Top-level docs and crate READMEs

**Files:**
- Modify: `AGENTS.md`, `CONTRIBUTING.md`, `README.md`, `rill-lang/README.md`, `rill-io/README.md`, `CHANGELOG.md` (Unreleased only)
- Test: `scripts/check-docs.sh`; manual crate-list comparison against `Cargo.toml`

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces: contributor-facing docs that match the workspace; a truth rule in `CONTRIBUTING.md`.

- [ ] **Step 1: Fix `AGENTS.md`**

Re-derive the crate table and count from `Cargo.toml` — **14 members** at this revision (`rill-core`, `rill-core-actor`, `rill-core-dsp`, `rill-patchbay`, `rill-digital-effects`, `rill-telemetry`, `rill-lofi`, `rill-io`, `rill-core-model`, `rill-osc`, `rill-sampler`, `rill-adrift`, `rill-lang`, `rill-fft`); drop the `rill-graph`/`rill-router` rows. Re-derive the `#![deny(unsafe_code)]` list — **6 crates** (`rill-core`, `rill-core-dsp`, `rill-core-model`, `rill-fft`, `rill-lang`, `rill-patchbay`).
**Note — do not "fix" `scripts/publish.sh` here:** it currently lists 16 crates including the absent `rill-graph` and `rill-router`, so it is *itself* stale. The doc must state the publish count the script actually lists **and** record the discrepancy in `DOCS-GAPS.md` (row: "publish script lists two crates that are not workspace members", reproduction: `grep -n 'rill-graph\|rill-router' scripts/publish.sh`). Fixing the script is out of scope (it is code, not documentation).
Rewrite "Threading model", "I/O callback", "Real-time safety" references to `CompiledGraphEngine`/`GraphBuilder`/`port.propagate()`/`ProcessingState` into the real `IoDriver`/`ProgramRunner`/`ModularSystem` picture. Fix the feature-flag table per each crate's `Cargo.toml`.

- [ ] **Step 2: Fix `README.md` and `rill-lang/README.md`**

Top-level README: replace the `GraphBuilder` example and the `rill-graph` table row. `rill-lang/README.md`: keep the migration table (it is accurate) but delete the "Combinators (block-diagram algebra)" section, which claims a deleted surface, and remove the stale `param(...)`/`main = _ * 0.5` spellings from the migration table's left column **only if** they would read as current — keep them as historical labels if the column header says "Removed".

- [ ] **Step 3: Add the truth rule to CONTRIBUTING.md**

Add a short section: code is the authority; any doc claim about code names the identifier and where it was verified; run `scripts/check-docs.sh` before a docs PR; a documented-but-missing capability goes to `DOCS-GAPS.md`, not into prose as if it worked.

- [ ] **Step 4: Fix `CHANGELOG.md` Unreleased entries only**

Correct any Unreleased claim that asserts a removed API as present. Do not touch released sections.

- [ ] **Step 5: Verify and commit**

Run `bash scripts/check-docs.sh` → expect exit 0. Run `cargo test --doc -p rill-lang` → expect 4/4.
```bash
git add AGENTS.md CONTRIBUTING.md README.md rill-lang/README.md rill-io/README.md CHANGELOG.md
git commit -m 'docs: align top-level and crate docs with the workspace'
```

---

### Task 8: Final verification

**Files:**
- Test: whole tree

**Interfaces:**
- Consumes: all tasks.

- [ ] **Step 1: Run the deny-list scan**

Run: `bash scripts/check-docs.sh`
Expected: `docs clean`, exit 0.

- [ ] **Step 2: Re-run the script's self-test**

Run: `bash scripts/check-docs.sh --self-test`
Expected: `self-test ok`.

- [ ] **Step 3: Build the book**

Run: `mdbook build docs/`
Expected: success; no dead links to archived files.

- [ ] **Step 4: Verify doc-tests and no source changes**

Run: `cargo test --doc -p rill-lang` → 4/4.
Run: `git diff --stat develop...HEAD -- '*.rs' 'Cargo.toml'` → empty.

- [ ] **Step 5: Verify the gap register and archive**

Run: `git ls-files docs/superpowers/archive/2026-10-24-deleted-signal-graph-model/` → three files.
Run: `test -f docs/superpowers/DOCS-GAPS.md` → exists.

- [ ] **Step 6: Commit (if verification produced fixes)**

```bash
git add -A
git commit -m 'docs: final truth-reconciliation verification fixes'
```
