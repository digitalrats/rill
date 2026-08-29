# Unified Backend Model — Subgraph IR, Heads as Resource-Backed Builtins (final)

> **Status:** Design — final 2026-08-29.
> **Date:** 2026-08-29
> **Scope:** Fix `moonlight_delay` in drift. A program is `input → output`. A tape echo is two independent subgraphs, each a clean program whose boundaries are active rill-io backends or NullBackends. `write_head`/`read_head` are **internal resource-backed rill-lang builtins** (like oscillators) referencing a shared `TapeLoop` via the resource machinery (restored). Subgraph is a first-class IR concept; `Runtime::launch` drives one or two callbacks. Topology unchanged. **No bridge node, no `BridgeAlgorithm`, no `DuplexSchedule`, no `TapeSystem`.**

## The unified backend model

A backend is defined by metadata including whether it is **active** (creates a callback that drives the signal):

| Backend | Crate | Active |
|---|---|---|
| `input` / `output` (PortAudio, ALSA, PipeWire, JACK, Null) | `rill-io` | yes — always a callback |
| software generators (`sine`, `saw`, `sampler`, `read_head`) | DSP/sampler | no (passive) |
| `write_head` (consumes into a buffer) | sampler | no (passive) |

A program is **`input → output`**. A graph partitions into subgraphs; each subgraph is a program whose ends are either an **active** backend (rill-io — real program I/O) or a **NullBackend** (a passive boundary):

| Subgraph | Input end | DSP (internal) | Output end |
|---|---|---|---|
| sine (single) | **NullBackend** (no active input) | `sine → lowpass` | **rill-io** |
| tape recording | **rill-io** `[capture, fb]` | `stereo_sum → write_head(tape)` | `[dryL, dryR]` |
| tape playback | `[dryL, dryR]` | `read_head(tape) → tap_mixer → mix` | **rill-io** `[fb, out]` |

## Heads are internal resource-backed builtins

`write_head` (2-in: dry + feedback, 1-out, writes the tape) and `read_head` (0-in, 1-out, reads a delayed tap) are **rill-lang builtins** compiled inside their subgraph, exactly like oscillators. They reference a named `TapeLoop` resource.

**The resource machinery is restored** (it was removed mid-flight and is needed again):
- `rill-core`: `TapeLoop`/`TapeWriter`/`TapeReader`/`tape_handles`, `ResourceRegistry`, `ParamType::Resource`, `register_resource_block`/`register_resource_multichannel_block`.
- `rill-lang`: `compile_program_with_resources` (compile against a shared registry), `RillProgram::new_with_resources`, `extract_resources`.
- The **shared** `TapeLoop` is created once (the tape backend's buffer) and injected into both subgraphs' head builtins at compilation — the write head holds the unique `TapeWriter`, each read head clones a `TapeReader` over the same instance.

Placement: tape types + resource machinery in `rill-core`; `ReadHead`/`WriteHead` algorithms and the `write_head`/`read_head` builtins in `rill-sampler` (which owns the tape backend buffer).

## Generalized backend attribute

`GraphSpecNode.backend: Option<NodeBackendKind>`:

```rust
pub enum NodeBackendKind { Active, Passive }
```

- `Active` — attached to a rill-io callback backend (capture/playback attachment points).
- `Passive` — software generators (`sine`, `saw`, `sampler`) and heads (`write_head`, `read_head`); no callback.
- `None` — pure transforms (`mixer`, `biquad`, `dry_wet`).

## Subgraph at IR level

`CompiledStream` becomes a first-class list of sub-programs:

```rust
pub struct SubProgram {
    pub engine: ProgramEngine<f32>,
    /// input/output ends: which are active backends vs NullBackends
    pub input_backend: Option<ActiveBackend>,   // None = NullBackend input
    pub output_backend: Option<ActiveBackend>,  // None = NullBackend output
    /// free I/O arities (capture/fb in, dryL/R out, etc.)
}

pub struct CompiledStream {
    pub subprograms: Vec<SubProgram>,  // 1 for plain, 2 for tape echo
    pub resources: ResourceRegistry<f32>,  // shared tape
}
```

- Plain graph (one active backend): one `SubProgram`; its DSP includes the generators as compiled builtins; the non-active end is a NullBackend (no program I/O there).
- Tape echo (two active backends): two `SubProgram`s (recording `[capture, fb] → [dryL, dryR]`, playback `[dryL, dryR] → [fb, out]`); the tape is the shared resource; `write_head`/`read_head` builtins live inside.

## Generalized partition (topological-sort stage)

In `rill-lang`'s reconstruction:
1. From each **`Active`** backend attachment, walk forward (input) / backward (output) over signal edges, collecting nodes, stopping at a `Passive` boundary or a node claimed by another region.
2. A graph with one active backend → one subgraph. A graph with two active backends separated by passive heads → two subgraphs.
3. Cross-region edges → program I/O: `dry` (recording → playback), `fb` (playback → recording).

## Duplex stream (`Runtime::launch`, single method)

`Runtime::launch(driver, capture, playback, stream, running)` dispatches on `CompiledStream`:
- one `SubProgram` → one callback: `capture → program → playback`.
- two `SubProgram`s → two-pass callback:

```
recording pass:  [capture, fb_buf] → recording_subprogram → [dryL, dryR]
                 write_head builtin writes the shared tape; dry → dry_buf
playback pass:   [dryL, dryR] → playback_subprogram → [fb, out]
                 read_head builtins read the shared tape; out → playback; fb → fb_buf (1-tick shadow)
```

No `launch_duplex`, no `TapeSystem` — the single `Runtime::launch` covers both cases.

## Crate layout

| Item | Crate |
|---|---|
| `TapeLoop`, `TapeWriter/Reader`, `tape_handles` | `rill-core` (`buffer`) |
| `ResourceRegistry`, `ParamType::Resource`, resource factories | `rill-core` (`builtin`, `buffer`) |
| `compile_program_with_resources`, `new_with_resources`, `extract_resources` | `rill-lang` |
| `ReadHead`, `WriteHead` algorithms | `rill-sampler` (`tape`) |
| `write_head`/`read_head` resource-backed builtins | `rill-sampler` (`tape` lang registration) |
| `GraphSpec`, `NodeBackendKind`, reconstruction, partition, `SubProgram`/`CompiledStream` | `rill-lang` (`graph`) |
| `BackendMeta { active }`, `BackendFactory` metadata | `rill-core` (`io`), `rill-graph` (`backend_factory`) |
| thin frontend, `GraphDef` ↔ populate → `GraphSpec`, direct names | `rill-graph` |

## rill changes

| Area | Change |
|---|---|
| `rill-core` | Restore `TapeLoop`/`ResourceRegistry`/resource machinery (undo Task-3 removal); keep `BackendMeta` |
| `rill-sampler` | Keep `ReadHead`/`WriteHead` algorithms; register `write_head`/`read_head` resource-backed builtins |
| `rill-lang` | Restore resource compile path; `graph` module final: `NodeBackendKind`, subgraph partition, `SubProgram`/`CompiledStream`; `Runtime::launch` single method |
| `rill-graph` | Thin frontend; delegate; direct names (no name table) |
| `presets`/`tests` | Direct builtin names |
| `drift` | `moonlight_signal.rs` |

## What is NOT done

- No `BridgeAlgorithm`, `tape_bridge`, `DuplexSchedule`, `TapeSystem`.
- No `rill/input`/`rill/output` builtins.
- No name-translation table / `rill/` prefixes.
- No change to the `moonlight_delay` topology.
- Multichannel tape = one `TapeLoop` per channel (follow-up).

## Testing

1. Resource machinery: shared registry wires write/read heads to one tape.
2. rill-sampler: `write_head`/`read_head` builtins; `TapeBackend`/tape buffer write→read.
3. `GraphSpec` reconstruction: stereo→mono channel selection; heads compile as builtins.
4. Partition: sine graph → 1 subprogram (NullBackend in, rill-io out); tape graph → 2 subprograms with correct I/O + cross-ports.
5. `Runtime::launch`: one and two-callback paths deliver signal; finite.
6. drift `moonlight_signal.rs` — both tests pass.

## Implementation phases

| # | Phase | Scope |
|---|-------|-------|
| 1 | **Restore resource machinery** | rill-core (`TapeLoop`, `ResourceRegistry`, `ParamType::Resource`, resource factories) + rill-lang (`compile_program_with_resources`, `new_with_resources`) |
| 2 | **Heads as resource-backed builtins** | rill-sampler registers `write_head`/`read_head` wrapping the `ReadHead`/`WriteHead` algorithms |
| 3 | **`GraphSpec` + partition + subgraph IR** | rill-lang `graph`: `NodeBackendKind`, reconstruction, generalized partition, `SubProgram`/`CompiledStream` |
| 4 | **rill-graph thin + direct names** | delegate; remove name table; presets/tests direct names |
| 5 | **`Runtime::launch`** | single method, one/two callbacks, shared resource |
| 6 | **rill-adrift + drift** | backend attachments, `moonlight_signal.rs` |
| 7 | **Verify** | workspace tests, clippy, fmt |
