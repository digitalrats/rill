# rill-lang: Program JSON contract + graph reconstruction on the unified model — Plan 1

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unblock `cargo test --workspace` (hangs on `rill-lang/src/graph/reconstruct.rs`, 11 errors on removed AST variants) by rewriting graph reconstruction to build rill-lang AST in the new unified model — signals are values (`FixedBuffer`), a graph node is just a function, there are no nodes/ports/signal-path in the AST — and add a `Program` ↔ JSON (AST) serialization contract with round-trip tests.

**Architecture:** The AST (`Program`/`Def`/`Expr`) is the single representation; it already derives `serde`. Plan 1 makes `Program` ↔ JSON a tested public contract and keeps the existing node-graph layer (`GraphSpec`/`GraphDef`/`GraphBuilder`, the drift preset format) working: only `reconstruct.rs` — the GraphSpec→AST bridge that used the removed Faust combinators (`Wire`/`Par`/`Merge`/`Split`/`Cut`, `Def::Anchor`) — is rewritten to emit the new IO-`main` form over plain value composition. No signal-path, no node wiring in the AST.

**Tech Stack:** Rust, `rill-lang` crate (`serde`/`serde_json` feature-gated), `rill-lang/src/graph/` module, integration tests in `rill-lang/tests/`.

**Spec:** This plan implements the design approved in the planning conversation: no separate graph IR; the JSON contract is a serialized rill-lang `Program` (SADT page = `Program`, later a `Module`); a graph node is a plain function over values — no signal path, no nodes, no ports in the AST. Drift keeps consuming the old node-graph format for now; migrating drift/rill-adrift and the tape-duplex path is a later plan.

## Global Constraints

- All docs and inline comments in **English**; the conversation language (Russian) does not apply to code.
- **Zero warnings**: `cargo check --all-features --workspace` and `cargo clippy --all-features --workspace` must be clean. No blanket `#[allow(..)]`, no `#![allow(missing_docs)]` — missing docs are added, not suppressed.
- Commits use Conventional Commits syntax and **single quotes** in `git commit -m '...'`.
- No new third-party dependencies without explicit confirmation; prefer the workspace's existing `serde`/`serde_json`.
- `rill-lang` denies `unsafe_code`.
- Working branch: `feature/rill-lang-arrays` (git-flow). Do not edit `develop`/`master`.

## Review Focus

These are the inputs/graph shapes most likely to be mishandled by a reader, that the graph feature's tests must pin down in Task 2:

1. **A node with zero signal inputs (generator**: `sine`/`saw`/`noise`/`sampler`) — produces a `FixedBuffer` with no buffer argument; must appear as a producer with no graph input edge, not as a program input.
2. **A node with multiple signal inputs** (e.g. `mixer`, `delay`'s dry/wet) — the reconstruction must thread every consumed buffer value, not assume single-input chains.
3. **Fan-out** — one producer's output feeding several consumers must be computed once (a `let`-bound value), not duplicated in a way that changes observable behavior or fails.
4. **Unconnected consumer inputs = program inputs** — a free input port must surface as a `main` buffer parameter; an unconnected sink output must be emitted via `writeOutput`.
5. **Scalar-param actor capture** — preserve the existing behavior that a node's scalar params are reachable as `?name` actor params (drift automation), so presets do not silently change.

## Task 1: `Program` ↔ JSON serialization contract

**Files:**
- Create: `rill-lang/src/program_serde.rs`
- Modify: `rill-lang/src/lib.rs` (declare/re-export the module under `serialization`)
- Test: `rill-lang/tests/program_json.rs`

**Interfaces:**
- Consumes: `crate::ast::Program` (already `derive(Serialize, Deserialize)` under the `serde` feature); `rill-lang/Cargo.toml` feature `serialization` (enables `serde`/`serde_json`).
- Produces (later tasks and downstream rely on these exact names):
  - `pub fn program_to_json(program: &Program) -> Result<String, serde_json::Error>` — pretty-printed JSON.
  - `pub fn program_from_json(json: &str) -> Result<Program, serde_json::Error>`

- [ ] **Step 1: Write the failing round-trip test**

In `rill-lang/tests/program_json.rs` (test target `program_json`, `required-features = ["serialization", "dsp"]`):

```rust
use rill_lang::ast::Program;
use rill_lang::program_serde::{program_from_json, program_to_json};

fn roundtrip(src: &str) {
    let program: Program = rill_lang::parser::parse(
        &rill_lang::lexer::tokenize(src).unwrap(),
        src.as_bytes(),
    )
    .unwrap();
    let json = program_to_json(&program).unwrap();
    let back: Program = program_from_json(&json).unwrap();
    assert_eq!(back, program, "Program↔JSON round-trip must be exact for:\n{src}");
}
```

Cases to cover (`roundtrip(...)` each):
```rust
roundtrip("main = 1.0");
roundtrip("foreign onepole : Float -> Float -> FixedBuffer f32 -> FixedBuffer f32; main = ()");
roundtrip("main = if (1.0 > 0.5) then 2.0 else 3.0");
roundtrip("data Shape = Circle Float | Square; main = Circle 1.0");
roundtrip("main = let g = 4.0 * ?gain in g + 1.0");
// io-main form with a mode pragma
roundtrip("{-# MODE IO #-} main = do { writeOutput [1.0]; return () }");
```

- [ ] **Step 2: Run test to verify it fails / doesn't yet build**

Run: `cargo test -p rill-lang --features serialization,dsp --test program_json`
Expected: FAIL because `rill_lang::program_serde` does not exist (module not found).

- [ ] **Step 3: Implement `program_serde.rs`**

A `#[cfg(feature = "serialization")]` module exposing the two functions in the **Interfaces** block. `program_to_json` serializes `serde_json::to_string_pretty(program)`; `program_from_json` deserializes `serde_json::from_str(json)`. Register it in `rill-lang/src/lib.rs` (`#[cfg(feature = "serialization")] pub mod program_serde;`).

- [ ] **Step 4: Run the test; also compile a program from the deserialized AST**

Add to the test a case that compiles the round-tripped program:
```rust
let json = program_to_json(&program).unwrap();
let back = program_from_json(&json).unwrap();
// compile original and round-tripped; both must compile
```
Run: `cargo test -p rill-lang --features serialization,dsp --test program_json`
Expected: PASS.

- [ ] **Step 5: Clippy + fmt gate**

Run: `cargo clippy -p rill-lang --features serialization,dsp --all-targets`
and `cargo fmt -- --check`. Expected: no warnings, clean.

- [ ] **Step 6: Commit**

```bash
git add rill-lang/src/program_serde.rs rill-lang/src/lib.rs rill-lang/tests/program_json.rs rill-lang/Cargo.toml
git commit -m 'feat(rill-lang): Program JSON serialization contract with round-trip tests'
```

## Task 2: Rebuild graph reconstruction on the unified (values-only) model

**Files:**
- Modify (rewrite): `rill-lang/src/graph/reconstruct.rs`
- Test: `rill-lang/tests/graph_reconstruct.rs`

**Interfaces:**
- Consumes: `crate::graph::spec::GraphSpec` (unchanged — kept as the drift-node-graph format), `crate::graph::spec::{GraphSpecNode, GraphSpecEdge, GraphEdgeKind}`, `crate::types::ffi::{ffi_sig_from_typeexpr, FfiParam, FfiSig}`, `crate::types::ty::TypeEnv`, `crate::builtin::Registry<T>`.
- Produces (signatures unchanged so `compile.rs`/`partition.rs` keep compiling — do **not** rename):
  - `pub fn compile_spec<T: Transcendental + 'static, const BUF: usize>(spec: &GraphSpec, registry: &crate::builtin::Registry<T>, sample_rate: f32) -> Result<crate::program_engine::ProgramEngine<T, BUF>, CompileError>`
  - `pub fn reconstruct(spec: &GraphSpec) -> Result<Program, CompileError>`

**Semantic contract of the new `reconstruct`:**
- Emits **no** `Expr::Wire/Par/Merge/Split/Cut` and **no** `Def::Anchor`. `Expr` values are composed as ordinary values.
- Produces a `Def::Eval` named `main` whose form matches the current program-I/O convention: free/unconnected signal input channels become `main`'s buffer λ-parameters, and sink outputs / boundary outputs are emitted with `writeOutput` inside the IO-`do` form.
- A node is lowered to a value-level composition of its builtin applied to its params and its incoming buffer values (from its signal `FfiParam`s), threaded in `FfiSig::param_names` order. The last per-node scalar param keeps the existing actor-param behavior (`?name`, late-bound), preserving drift automation. Fan-out of a shared value is bound once (a `let`) rather than recomputed.
- The algorithm (topo-order over `Signal` edges, per-node value expression, threading, `writeOutput` for sinks/boundary outputs) is the implementer's choice within this contract; do what the signature and tests leave open.

> Note: `FfiParam::Signal` maps to a consumed buffer value; `FfiParam::Scalar` to a literal/actor-param; `FfiParam::Resource` to a shared-tape reference; `VariadicSignal`/`Record` appear per the existing signature and are kept out of scope unless a test needs them (none below do).

- [ ] **Step 1: Update the failing test to assert the new target behavior**

Rewrite `rill-lang/tests/graph_reconstruct.rs` so each case builds a `GraphSpec` (same helpers as today — `GraphSpecNode`/`GraphSpecEdge`/`edge`) and asserts **behavioral** output of the compiled engine, not removed AST shape. Example first case (adjust to real output order):

```rust
use rill_lang::graph::compile_spec;
// sine → onepole: one generator feeding one consumer.
// Inputs: none (sine is a generator). Output: the onepole's buffer.
```

Keep the existing behavioral asserts (engine compiles, `process` yields expected `[f32]`) as far as they still hold; where the new IO form changes input binding, update the input values the test feeds. Every Review-Focus case above that applies to a plain (non-tape) graph gets a test here.

- [ ] **Step 2: Run the test to verify it fails (does not yet compile)**

Run: `cargo test -p rill-lang --features dsp,graph --test graph_reconstruct`
Expected: FAIL — `reconstruct.rs` still references `Expr::Wire` (compile error) or, after a stub, the assertions fail.

- [ ] **Step 3: Rewrite `reconstruct`/`compile_spec` per the semantic contract**

Implement the new value-composition reconstruction. One concrete, allowed shape (implementer may choose seam):

```text
main p0 p1 ... = do
    let v0 = <producer0 expression>   -- let-bound once for fan-out
        ...
    writeOutput [ <sink/boundary output expressions> ]
    return ()
```

where each producer is an `Expr::Apply { name, args }` (or `ApplyExpr`) with args ordered per the builtin's `FfiSig` (scalar literals/`?actor` params and consumed buffer values). Keep the FFI-catalog lookup for the signature (as the old code did via `TypeEnv::with_builtins()` + `ffi_sig_from_typeexpr`).

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p rill-lang --features dsp,graph --test graph_reconstruct`
Expected: PASS.

- [ ] **Step 5: Green default workspace + all-features check**

Run: `cargo test --workspace` and `cargo check --all-features --workspace`.
Expected: `cargo test --workspace` builds (reconstruct no longer breaks it); `--all-features` check clean.

- [ ] **Step 6: Clippy + fmt gate**

Run: `cargo clippy --all-features --workspace` and `cargo fmt -- --check`.
Expected: no warnings, clean.

- [ ] **Step 7: Commit**

```bash
git add rill-lang/src/graph/reconstruct.rs rill-lang/tests/graph_reconstruct.rs
git commit -m 'fix(rill-lang): rebuild graph reconstruction on the unified values-only model'
```

## Task 3: Workspace-wide verification (build / clippy / tests)

**Files:**
- Test: none new — verification only.

**Interfaces:**
- Consumes: all of Tasks 1–2.

- [ ] **Step 1: Full default + all-features test and lint sweep**

Run, in order:
```bash
cargo test --workspace
cargo check --all-features --workspace
cargo clippy --all-features --workspace
cargo fmt -- --check
```
Expected: all green; the arrow suite (previously 17/17) and `rill-lang` suites still pass; the previously blocked `cargo test --workspace` now completes.

- [ ] **Step 2: Confirm drift-facing surface is intact (no format break)**

Run: `cargo check -p rill-adrift --all-features` (rill-adrift keeps using the retained `graph` node-IR — `GraphDef`/`GraphBuilder`/`graph::compile` are unchanged by Tasks 1–2).
Expected: builds clean; no change needed in `rill-adrift` in this plan.

- [ ] **Step 3: Commit any stragglers**

```bash
git add -A
git commit -m 'chore(rill-lang): workspace builds clean after program-json and graph reconstruction'
```
(Skip if Step 1 left nothing to commit.)

## Deferred to later plans (out of scope here)

- Migrating `rill-adrift` modular (`RackDef.graph`) and the drift preset format to the serialized `Program` (JSON = AST); SADT/R-schema rendering in drift.
- Tape-duplex rework: `graph/compile.rs` `CompiledStream::Duplex`, `partition.rs`, and the recording template (`render_recording` still emits the removed `:>`/`<:`/`:` surface syntax — it currently compiles as a Rust string; its runtime correctness is deferred, so its graph tests are **not** enabled by default features).
- `Program` feature: modules/import (SADT page → `Module`).
