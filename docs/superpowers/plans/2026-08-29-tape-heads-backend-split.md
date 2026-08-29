# Unified Backend Model — Subgraph IR, Heads as Resource-Backed Builtins (final plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix `moonlight_delay` in drift. A program is `input → output`; a tape echo is two subgraphs (recording, playback), each a clean program bounded by active rill-io backends or NullBackends. `write_head`/`read_head` are **internal resource-backed rill-lang builtins** (like oscillators) referencing a shared `TapeLoop`. Subgraph is a first-class IR concept; single `Runtime::launch` drives one or two callbacks. Topology unchanged.

**Architecture:** Restore the resource machinery (TapeLoop/ResourceRegistry/ParamType::Resource in rill-core; `compile_program_with_resources`/`new_with_resources` in rill-lang). rill-sampler registers `write_head`/`read_head` resource-backed builtins wrapping its `ReadHead`/`WriteHead` algorithms. `rill-lang::graph` compiles a `GraphSpec` (with `NodeBackendKind` Active/Passive) into `CompiledStream { subprograms: Vec<SubProgram>, resources }` via a generalized partition at the topo-sort stage. `Runtime::launch` dispatches on the stream (1 or 2 callbacks). rill-graph stays a thin frontend with direct names.

**Tech Stack:** Rust; rill-core (resource machinery, BackendMeta), rill-sampler (heads/builtins/tape), rill-lang (graph module, Runtime), rill-graph (thin), rill-adrift, drift.

---

## Context / environment

- Spec: `rill/docs/superpowers/specs/2026-08-29-tape-heads-backend-split-design.md` (final).
- Branch `feature/remove-tape-bridge` at `/home/mikek/Projects/digitalrats/rill`. HEAD: `532fe0b`.
- **CRITICAL:** never `git add -A`/`git add .`; stage only each task's files; verify with `git status` before committing.
- Current history: `e4af3d9` (BackendMeta), `b6ca787`+`b6a834c`+`4e272fe` (tape→sampler move + resource removal), `532fe0b` (draft graph module).
- Known mid-rework breakage to fix: `rill-lang/src/lower.rs:154` and `types/infer.rs:403` have missing `&Resource` match arms (leftover from the partial Task-3 removal); the draft `graph` module needs the rework in Task 4.

---

## Task 1 (DONE): `BackendMeta { active }`

Committed (`e4af3d9`). Do not redo.

## Task 2: Restore the resource machinery

**Files:**
- Modify: `rill/rill-core/src/buffer/{mod.rs,tape.rs,registry.rs}` (restore)
- Modify: `rill/rill-core/src/builtin.rs` (`ParamType::Resource`, `register_resource_block`/`register_resource_multichannel_block`, resource factories) — preserve the user's `min_args` Record arm
- Modify: `rill/rill-lang/src/lib.rs` (`compile_program_with_resources`, `compile_program_inner`, `extract_resources`)
- Modify: `rill/rill-lang/src/program.rs` (`new_with_resources`)
- Modify: `rill/rill-lang/src/lower.rs`, `types/infer.rs`, `ir.rs` (restore `Resource` handling — fixes the missing match arms)
- Test: `rill/rill-lang/tests/shared_resources.rs` (restore)

- [ ] **Step 1:** Restore `TapeLoop`/`TapeWriter`/`TapeReader`/`tape_handles` in `rill-core/src/buffer/tape.rs` (git history `b6ca787~1` has the pre-move version — use `git show b6ca787~1:rill-core/src/buffer/tape.rs`), `ResourceRegistry` in `registry.rs`, and their `mod`/`pub use` in `buffer/mod.rs`.
- [ ] **Step 2:** Restore `ParamType::Resource` and the resource factory machinery in `rill-core/src/builtin.rs` (from `git show b6a834c~1:rill-core/src/builtin.rs`), keeping the user's `min_args` Record arm.
- [ ] **Step 3:** Restore `compile_program_with_resources`/`compile_program_inner`/`extract_resources`/`ResourceDecl` in `rill-lang/src/lib.rs` (from `git show b6a834c~1:rill-lang/src/lib.rs`).
- [ ] **Step 4:** Restore `new_with_resources` + the resource handling in `RillProgram::build` in `rill-lang/src/program.rs`.
- [ ] **Step 5:** Restore the `Resource` arms in `lower.rs`, `types/infer.rs`, `ir.rs`.
- [ ] **Step 6:** Restore `rill/rill-lang/tests/shared_resources.rs` (the shared-registry test). Run `cargo test -p rill-lang 2>&1 | tail -10` (all pass) and `cargo check --workspace 2>&1 | tail -15`.
- [ ] **Step 7:** Commit.
```bash
git add rill/rill-core/src/buffer/ rill/rill-core/src/builtin.rs rill/rill-lang/src/lib.rs rill/rill-lang/src/program.rs rill/rill-lang/src/lower.rs rill/rill-lang/src/types/infer.rs rill/rill-lang/src/ir.rs rill/rill-lang/tests/shared_resources.rs
git commit -m 'revert(rill-lang): restore tape resource machinery (heads are resource-backed builtins)'
```

## Task 3: Heads as resource-backed builtins in rill-sampler

**Files:**
- Create: `rill/rill-sampler/src/tape/lang.rs` (register `write_head`/`read_head`)
- Modify: `rill/rill-sampler/src/tape/mod.rs`, `rill/rill-sampler/src/lib.rs`, `rill/rill-sampler/src/tape/{read_head,write_head}.rs` (restore `BlockBuiltin`/resource accessors)
- Test: `rill/rill-sampler/tests/tape_builtins.rs`

- [ ] **Step 1:** Add `set_writer`/`set_reader`-style resource accessors back to `ReadHead`/`WriteHead` (they were dropped during the move) so the builtins can receive the `TapeWriter`/`TapeReader` from the registry.
- [ ] **Step 2:** Create `rill/rill-sampler/src/tape/lang.rs` registering resource-backed builtins (model on the pre-move `rill-digital-effects/src/lang/tape.rs`, from `git show b6ca787~1:rill-digital-effects/src/lang/tape.rs`):

```rust
pub fn register_tape_builtins<T: Transcendental + 'static>(reg: &mut rill_lang::builtin::Registry<T>) {
    reg.register_resource_multichannel_block(
        rill_core::builtin::BuiltinSig {
            name: "write_head",
            params: vec![
                rill_core::builtin::ParamType::Signal,
                rill_core::builtin::ParamType::Signal,
                rill_core::builtin::ParamType::Resource,
                rill_core::builtin::ParamType::Float,
                rill_core::builtin::ParamType::Float,
            ],
            signal_outs: 1,
            kind: rill_core::builtin::BuiltinKind::Block,
            param_names: vec!["delay_time", "feedback"],
        },
        |_signal_ins, p, sr, registry, resource| {
            let mut wh = crate::tape::write_head::WriteHead::<T, 64>::new(sr);
            wh.set_delay_time(p[0] as f32);
            wh.set_feedback(p[1] as f32);
            if let Some(writer) = registry.writer(resource) { wh.set_writer(writer); }
            Box::new(wh)
        },
    );
    // read_head: resource block (0-in, 1-out), param "delay".
}
```

- [ ] **Step 3:** `tape/mod.rs` + lib.rs export `register_tape_builtins` (or a `register` fn mirroring the crate's pattern). Add rill-lang as a dependency of rill-sampler if not already (check — the sampler may already use `rill_lang`; if adding creates a cycle, register via a trait — but rill-lang does not depend on rill-sampler, so rill-sampler → rill-lang is acyclic).
- [ ] **Step 4:** Test `rill/rill-sampler/tests/tape_builtins.rs`: build a registry with `register_tape_builtins` + the DSP/core builtins; `compile_graph("tape_0 = TapeLoop 1024\nmain = (_, _) :> write_head tape_0 0.5 0.3", &reg, sr)` and `main = read_head tape_0 0.1` compile and run finite; write then read through a shared `ResourceRegistry` (mirror the old `shared_resources.rs` flow).
- [ ] **Step 5:** Run + commit.
```bash
git add rill/rill-sampler/src/tape/ rill/rill-sampler/src/lib.rs rill/rill-sampler/tests/tape_builtins.rs rill/rill-sampler/Cargo.toml
git commit -m 'feat(rill-sampler): write_head/read_head resource-backed builtins'
```

## Task 4: `rill-lang::graph` final — `NodeBackendKind`, subgraph IR, `SubProgram`/`CompiledStream`

**Files:**
- Modify: `rill/rill-lang/src/graph/{spec.rs,reconstruct.rs,partition.rs,compile.rs,mod.rs}`
- Test: `rill/rill-lang/tests/graph_reconstruct.rs`, `graph_partition.rs` (update)

Rework the draft `graph` module (from `532fe0b`) per the final design.

- [ ] **Step 1:** Add `NodeBackendKind` + `GraphSpecNode.backend: Option<NodeBackendKind>`:

```rust
pub enum NodeBackendKind { Active, Passive }
// GraphSpecNode gains: pub backend: Option<NodeBackendKind>,
```

- [ ] **Step 2:** `reconstruct.rs` — heads compile as **builtins** (not structural markers). The channel-aware reconstruction already compiles any registered builtin; ensure `write_head`/`read_head` are registered in the test registries and compile normally. Keep direct name resolution, channel selection (Split/Cut), free-input wires, feedback-as-free-input, topo sort.
- [ ] **Step 3:** `partition.rs` — generalized partition: `Active` backends seed subgraphs; `Passive` nodes (heads + generators) are boundaries. A graph with one active backend → one region; two active backends separated by passive heads → two regions. Cross-region edges → cross-ports. Keep `SubGraph { nodes, out_edges, in_edges }`.
- [ ] **Step 4:** `compile.rs` — `SubProgram`/`CompiledStream`:

```rust
pub struct SubProgram {
    pub engine: ProgramEngine<f32>,
    /// None = NullBackend end (no program I/O there)
    pub input_backend: Option<ActiveBackend>,
    pub output_backend: Option<ActiveBackend>,
}
pub struct ActiveBackend { pub name: String, pub node: usize, pub port: usize }

pub struct CompiledStream<T: Transcendental> {
    pub subprograms: Vec<SubProgram<T>>,
    pub resources: rill_core::buffer::ResourceRegistry<T>,
}

pub fn compile<T: Transcendental + 'static>(spec: &GraphSpec, registry: &Registry<T>, sample_rate: f32)
    -> Result<CompiledStream<T>, CompileError>
```

  - 0/1 active backends → one `SubProgram` (heads/generators compiled inside).
  - 2 active backends with passive heads between → two `SubProgram`s; build sub-`GraphSpec`s per region (recording `[capture, fb] → [dryL, dryR]`, playback `[dryL, dryR] → [fb, out]`); compile each via `compile_spec`; register the shared tape in `CompiledStream.resources` and compile both against it (via `compile_program_with_resources`).
- [ ] **Step 5:** Update tests:
  - `graph_reconstruct.rs`: stereo→mono compiles; a `write_head`/`read_head` graph compiles as builtins.
  - `graph_partition.rs`: sine graph → 1 subprogram (NullBackend input, rill-io output); tape graph → 2 subprograms with correct I/O + cross-ports; shared tape registered.
- [ ] **Step 6:** Run `cargo test -p rill-lang 2>&1 | tail -20` + `cargo check --workspace 2>&1 | tail -15`. Commit.
```bash
git add rill/rill-lang/src/graph/ rill/rill-lang/tests/graph_reconstruct.rs rill/rill-lang/tests/graph_partition.rs
git commit -m 'feat(rill-lang): NodeBackendKind, subgraph IR, SubProgram/CompiledStream'
```

## Task 5: rill-graph thin frontend + direct names

**Files:** `rill/rill-graph/src/{graph.rs,serialization.rs,lib.rs}`, `rill/rill-adrift/tests/*`, presets.

- [ ] **Step 1:** `to_graph_spec` — include `backend` classification (Active for capture/playback attachment points, Passive for heads/generators, None for transforms).
- [ ] **Step 2:** `compile_def` delegates to `rill_lang::graph::compile`.
- [ ] **Step 3:** Remove `ast_from_def`/`resolve_builtin_name` and the name table.
- [ ] **Step 4:** Direct names in tests + `drift/presets/moonlight_delay_system.json`/`tape_delay_system.json` (`rill/mixer`→`mixer`, `rill/write_head`→`write_head`, `rill/read_head`→`read_head`, `rill/dry_wet_mix`→`dry_wet`, `rill/lofi`→`lofi`, `rill/biquad`→`biquad`).
- [ ] **Step 5:** Run `cargo test -p rill-graph` + `cargo test -p rill-adrift` + `cargo check --workspace`. Commit.

## Task 6: `Runtime::launch` — one or two callbacks from `CompiledStream`

**Files:** `rill/rill-lang/src/runtime.rs`, test `duplex_runtime.rs`.

- [ ] **Step 1:** `Runtime::launch(driver, capture, playback, stream: CompiledStream<f32>, running)`:
  - 1 subprogram → one callback: `capture → subprogram → playback`.
  - 2 subprograms → two-pass callback (recording `[capture, fb_buf]→[dryL,dryR]`, tape written by the internal write_head builtin; playback `[dryL,dryR]→[fb,out]`, read_heads read the shared tape; fb shadow buffer).
- [ ] **Step 2:** Test with mock driver/capture/playback (pattern from `drift/tests/moonlight_signal.rs`): duplex delivers signal (peak > 0.1), finite.
- [ ] **Step 3:** Run + commit.

## Task 7: rill-adrift + drift wiring

**Files:** `rill/rill-adrift/src/modular/mod.rs`, `drift/tests/moonlight_signal.rs`, `drift/presets/*`.

- [ ] **Step 1:** `ModularSystem` builds the `GraphSpec` (with backend attachments) and launches via `Runtime::launch`.
- [ ] **Step 2:** Rewrite `moonlight_signal.rs` for the duplex stream.
- [ ] **Step 3:** `cargo test` in drift → `2 passed`. Commit (drift repo).

## Task 8: Workspace verification + clippy

- [ ] `cargo test --workspace`, `cargo clippy --workspace`, `cargo fmt`; drift `cargo test` + `cargo clippy`. Fix all warnings in touched crates. Commit stragglers; update spec status.

---

## Known limitations (out of scope)

- Partition supports ≤2 active subgraphs (N-way follow-up).
- Multichannel tape = one `TapeLoop` per channel (follow-up).
