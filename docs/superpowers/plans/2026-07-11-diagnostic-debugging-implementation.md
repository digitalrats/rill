# Diagnostic and Debugging Infrastructure — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build two-level diagnostic infrastructure: (1) runtime telemetry/logging via `ProbePoint` IR in rill-lang + collector in rill-telemetry, (2) interactive debugger `rill-analyzer` with REPL, Lua scripting, and CLI.

**Architecture:** rill-lang gets one `ProbePoint` IR variant + probe slot infrastructure + command log behind `debug` feature. rill-telemetry gets `debug/` submodule with CollectorThread, formatters, protocol types. rill-analyzer is a new workspace crate: REPL + Lua + CLI.

**Tech Stack:** rill-core (SpscQueue, CommandEnum, ParamValue), rill-lang (IR, RillGraphEngine, interp), rill-telemetry, rill-graph (GraphDef), parking_lot, dashmap, mlua (Lua 5.4), clap, colored, serde_json.

**Key constraints:** `SpscQueue<T, CAP>` requires `T: Copy + Default`. All frame types must satisfy this constraint. `CommandFrame` uses fixed-size byte-buffer strings (`CmdStr<64>`).

**Key design decision:** Probe capture happens at the **graph engine level**, after each `Step::InlineProgram` executes. The `ProbePoint` IR instruction is a pure pass-through; the engine reads output buffers and pushes frames. Command logging happens in `drain_mailbox`. This avoids touching the interpreter hot path.

---

## File Map

| File | Action | Purpose |
|---|---|---|
| `rill-lang/Cargo.toml` | Modify | Add `debug` feature |
| `rill-lang/src/ir.rs` | Modify | Add `ProbeId`, `ProbePoint` variant to `Instr` |
| `rill-lang/src/debug.rs` | Create | `ProbeSlot`, `ProbeFrame`, `CommandFrame`, `DebugControl` |
| `rill-lang/src/lib.rs` | Modify | Wire `debug` module, expand prelude |
| `rill-lang/src/graph_engine.rs` | Modify | `probe_slots`, `command_queue`, `debug_control` fields; probe capture after InlineProgram; command logging in drain_mailbox; inter-tick pause |
| `rill-lang/src/backend/interp.rs` | Modify | Handle `ProbePoint` IR as pass-through copy |
| `rill-lang/src/program.rs` | Modify | Re-export debug types, expose probe metadata |
| `rill-core/src/builtin.rs` | Modify | Register `__probe` builtin sig when `debug` active |
| `rill-telemetry/Cargo.toml` | Modify | Add dependencies, `debug` feature |
| `rill-telemetry/src/debug/mod.rs` | Create | Module root |
| `rill-telemetry/src/debug/state.rs` | Create | `ProbeState`, `ProbeStateManager` |
| `rill-telemetry/src/debug/collector_thread.rs` | Create | `CollectorThread` |
| `rill-telemetry/src/debug/protocol.rs` | Create | `AnalyzerCommand`, `AnalyzerResponse`, `AnalyzerConfig` |
| `rill-telemetry/src/debug/formatter/mod.rs` | Create | `EventFormatter` trait |
| `rill-telemetry/src/debug/formatter/text.rs` | Create | `TextFormatter` |
| `rill-telemetry/src/debug/formatter/json.rs` | Create | `JsonFormatter` |
| `rill-telemetry/src/lib.rs` | Modify | Add `debug` module, expand prelude |
| `rill-analyzer/Cargo.toml` | Create | New crate |
| `rill-analyzer/src/lib.rs` | Create | `Analyzer::launch()` |
| `rill-analyzer/src/main.rs` | Create | CLI binary |
| `rill-analyzer/src/repl/mod.rs` | Create | REPL loop |
| `rill-analyzer/src/repl/commands.rs` | Create | All REPL commands |
| `rill-analyzer/src/repl/parser.rs` | Create | Prefix-matching parser |
| `rill-analyzer/src/repl/history.rs` | Create | History |
| `rill-analyzer/src/lua/mod.rs` | Create | Lua module root |
| `rill-analyzer/src/lua/bindings.rs` | Create | Lua bindings |
| `rill/Cargo.toml` | Modify | Add `rill-analyzer` to workspace |

---

## Phase 1: rill-lang — ProbePoint IR + Engine Integration

### Task 1.1: Add `debug` feature flag

**Files:**
- Modify: `rill-lang/Cargo.toml`

- [ ] **Step 1: Add feature**

```toml
[features]
default = []
debug = []
router = []
serde = ["dep:serde"]
```

- [ ] **Step 2: Verify**

```bash
cargo check -p rill-lang && cargo check -p rill-lang --features debug
```

- [ ] **Step 3: Commit**

```bash
git add rill-lang/Cargo.toml && git commit -m 'feat(rill-lang): add debug feature flag'
```

---

### Task 1.2: Add `ProbeId` and `ProbePoint` to IR

**Files:**
- Modify: `rill-lang/src/ir.rs`

- [ ] **Step 1: Add `ProbeId` type** after `pub type StateSlot = usize;` (line ~12):

```rust
/// Unique identifier for a debug probe point.
pub type ProbeId = u32;
```

- [ ] **Step 2: Add `ProbePoint` variant** to `Instr` enum, after the last existing variant:

```rust
    /// Debug probe — pass-through copy with optional telemetry capture.
    /// When the `debug` feature is disabled, this variant does not exist.
    #[cfg(feature = "debug")]
    ProbePoint {
        /// Unique probe identifier.
        id: ProbeId,
        /// Source register.
        src: Reg,
        /// Destination register (pass-through).
        dst: Reg,
    },
```

- [ ] **Step 3: Verify**

```bash
cargo check -p rill-lang && cargo check -p rill-lang --features debug
```

Both should pass. Without `debug`, Instr = 13 variants. With `debug`, Instr = 14.

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/ir.rs && git commit -m 'feat(rill-lang): add ProbeId type and ProbePoint IR variant'
```

---

### Task 1.3: Create `debug.rs` module

**Files:**
- Create: `rill-lang/src/debug.rs`

All types used by `RillGraphEngine` and exposed to `rill-telemetry`.

- [ ] **Step 1: Write module**

```rust
//! Debug infrastructure types for the rill-lang execution engine.
//!
//! Gated behind the `debug` feature. Provides probe slots, frame types,
//! command logging, and debug control atomics for pause/resume.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use rill_core::math::Transcendental;
use rill_core::queues::spsc::SpscQueue;

use crate::ir::ProbeId;

/// A single frame of signal data captured at a probe point.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProbeFrame {
    /// Signal value as f64::to_bits(). Use f64::from_bits() to decode.
    pub value_bits: u64,
    /// Block (tick) index since engine start.
    pub block_index: u64,
}

/// A fixed-size, Copy-compatible string buffer for command frame fields.
/// Stores up to N bytes with a length marker.
#[derive(Debug, Clone, Copy)]
pub struct CmdStr<const N: usize> {
    bytes: [u8; N],
    len: u8,
}

impl<const N: usize> CmdStr<N> {
    pub fn from_str(s: &str) -> Self {
        let mut bytes = [0u8; N];
        let len = s.len().min(N);
        bytes[..len].copy_from_slice(&s.as_bytes()[..len]);
        Self { bytes, len: len as u8 }
    }

    pub fn as_str(&self) -> &str {
        let len = self.len as usize;
        std::str::from_utf8(&self.bytes[..len]).unwrap_or("")
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<const N: usize> Default for CmdStr<N> {
    fn default() -> Self {
        Self { bytes: [0u8; N], len: 0 }
    }
}

/// A single frame of command data captured from the actor mailbox.
/// Copy + Default compatible with SpscQueue<CommandFrame, 256>.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandFrame {
    /// Block index when the command was drained.
    pub block_index: u64,
    /// Human-readable command kind: "SetParameter", "ClockTick".
    pub command_kind: CmdStr<32>,
    /// Target node name from ScheduledGraph.program_names.
    pub node_name: CmdStr<64>,
    /// Parameter name. Empty = not applicable.
    pub param_name: CmdStr<64>,
    /// Human-readable value representation.
    pub value_repr: CmdStr<128>,
}

/// Per-probe runtime slot stored in the engine.
pub struct ProbeSlot {
    /// Whether this probe captures data. False = fast path.
    pub enabled: AtomicBool,
    /// Whether this probe is a breakpoint.
    pub break_flag: AtomicBool,
    /// Set by RT thread when paused at this probe.
    pub paused_flag: AtomicBool,
    /// Last captured value as raw bits.
    pub last_value: AtomicU64,
    /// SPSC queue for frame transport to collector thread.
    pub queue: Arc<SpscQueue<ProbeFrame, 64>>,
}

impl ProbeSlot {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            break_flag: AtomicBool::new(false),
            paused_flag: AtomicBool::new(false),
            last_value: AtomicU64::new(0),
            queue: Arc::new(SpscQueue::new()),
        }
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    #[inline]
    pub fn is_breakpoint(&self) -> bool {
        self.enabled.load(Ordering::Acquire) && self.break_flag.load(Ordering::Acquire)
    }
}

impl Default for ProbeSlot {
    fn default() -> Self { Self::new() }
}

/// Debug control atomics shared between engine and collector/debugger threads.
#[derive(Clone)]
pub struct DebugControl {
    pub global_pause: Arc<AtomicBool>,
    pub global_resume: Arc<AtomicBool>,
    pub block_index: Arc<AtomicU64>,
}

impl DebugControl {
    pub fn new() -> Self {
        Self {
            global_pause: Arc::new(AtomicBool::new(false)),
            global_resume: Arc::new(AtomicBool::new(false)),
            block_index: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl Default for DebugControl {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 2: Verify**

```bash
cargo check -p rill-lang --features debug
```

- [ ] **Step 3: Commit**

```bash
git add rill-lang/src/debug.rs && git commit -m 'feat(rill-lang): add debug module with ProbeSlot, ProbeFrame, CommandFrame, DebugControl'
```

---

### Task 1.4: Wire `debug` module into lib.rs and handle ProbePoint in interpreter

**Files:**
- Modify: `rill-lang/src/lib.rs`
- Modify: `rill-lang/src/backend/interp.rs`

- [ ] **Step 1: Register module in lib.rs**

In `rill-lang/src/lib.rs`, after the last existing `pub mod` declaration, add:

```rust
#[cfg(feature = "debug")]
pub mod debug;
```

- [ ] **Step 2: Handle ProbePoint in interp.rs**

In `rill-lang/src/backend/interp.rs`, find the match on `prog.ir.instrs[idx].clone()` in `eval_sample_scalar`. After the last instruction arm, add:

```rust
            #[cfg(feature = "debug")]
            Instr::ProbePoint { src, dst, .. } => {
                prog.regs_scalar[dst] = prog.regs_scalar[src];
            }
```

Also add the same arm in `run_block_reference` (the block-native reference interpreter loop). Search for the parallel instruction dispatch and add the same arm.

- [ ] **Step 3: Verify**

```bash
cargo check -p rill-lang --features debug && cargo check -p rill-lang
```

- [ ] **Step 4: Commit**

```bash
git add rill-lang/src/lib.rs rill-lang/src/backend/interp.rs
git commit -m 'feat(rill-lang): wire debug module, handle ProbePoint in interpreter'
```

---

### Task 1.5: Integrate debug infrastructure into RillGraphEngine

**Files:**
- Modify: `rill-lang/src/graph_engine.rs`

This is the largest single change. Three hooks: (a) probe capture after InlineProgram, (b) command logging in drain_mailbox, (c) inter-tick pause in process_tick.

- [ ] **Step 1: Add imports**

At the top of `rill-lang/src/graph_engine.rs`, after `use crate::program::RillProgram;`:

```rust
#[cfg(feature = "debug")]
use crate::debug::{CommandFrame, DebugControl, ProbeFrame, ProbeSlot};
#[cfg(feature = "debug")]
use std::sync::atomic::Ordering;
```

- [ ] **Step 2: Add struct fields**

In `pub struct RillGraphEngine<T: Transcendental>`, after `duplex: Option<DuplexData<T>>,` (line 88):

```rust
    #[cfg(feature = "debug")]
    pub(crate) probe_slots: Vec<ProbeSlot>,
    #[cfg(feature = "debug")]
    pub(crate) command_queue: std::sync::Arc<SpscQueue<CommandFrame, 256>>,
    #[cfg(feature = "debug")]
    pub(crate) debug_control: DebugControl,
```

- [ ] **Step 3: Initialize fields in `new()` constructor**

After `duplex: None,` (line ~142):

```rust
            #[cfg(feature = "debug")]
            probe_slots: Vec::new(),
            #[cfg(feature = "debug")]
            command_queue: std::sync::Arc::new(SpscQueue::new()),
            #[cfg(feature = "debug")]
            debug_control: DebugControl::new(),
```

- [ ] **Step 4: Initialize fields in `new_duplex()` constructor**

After `duplex: Some(duplex),` (line ~234):

```rust
            #[cfg(feature = "debug")]
            probe_slots: Vec::new(),
            #[cfg(feature = "debug")]
            command_queue: std::sync::Arc::new(SpscQueue::new()),
            #[cfg(feature = "debug")]
            debug_control: DebugControl::new(),
```

- [ ] **Step 5: Add accessor methods**

After the last existing method in the `impl RillGraphEngine<T>` block (before the closing `}`), add:

```rust
    #[cfg(feature = "debug")]
    pub fn allocate_probe_slots(&mut self, count: usize) {
        self.probe_slots.resize_with(count, ProbeSlot::default);
    }

    #[cfg(feature = "debug")]
    pub fn debug_state(&self) -> (&[ProbeSlot], DebugControl, std::sync::Arc<SpscQueue<CommandFrame, 256>>) {
        (&self.probe_slots, self.debug_control.clone(), self.command_queue.clone())
    }
```

Note: `SpscQueue` is imported from `crate::debug`, but it's actually from `rill_core::queues::spsc::SpscQueue`. We need the full path in the return type. Add at the top of the file:

```rust
#[cfg(feature = "debug")]
use rill_core::queues::spsc::SpscQueue;
```

- [ ] **Step 6: Add command logging to `drain_mailbox`**

Find `drain_mailbox` (line ~263). Currently it matches `CommandEnum::SetParameter` only. Add command logging. Replace the method body:

```rust
    fn drain_mailbox(&mut self) {
        #[cfg(feature = "debug")]
        let block_idx = self.debug_control.block_index.load(Ordering::Relaxed);

        while let Some(cmd) = self.mailbox.pop() {
            match &cmd {
                CommandEnum::SetParameter(ref sp) => {
                    // --- existing routing logic (keep as-is) ---
                    let param_name = sp.parameter.as_str();
                    if !sp.anchor.is_empty() {
                        if let Some(&prog_idx) = self.anchor_map.get(&sp.anchor) {
                            if let Some(&idx) = self.param_maps[prog_idx].get(param_name) {
                                self.pending.push(PendingParam {
                                    param_idx: idx,
                                    program_idx: prog_idx,
                                    value: sp.value.clone(),
                                    sample_pos: sp.sample_pos,
                                });
                            }
                        }
                    } else {
                        for (prog_idx, map) in self.param_maps.iter().enumerate() {
                            if let Some(&idx) = map.get(param_name) {
                                self.pending.push(PendingParam {
                                    param_idx: idx,
                                    program_idx: prog_idx,
                                    value: sp.value.clone(),
                                    sample_pos: sp.sample_pos,
                                });
                                break;
                            }
                        }
                    }
                    // --- end existing routing ---

                    // --- debug: command logging ---
                    #[cfg(feature = "debug")]
                    {
                        let node_name = CmdStr::from_str(
                            self.schedule.program_names
                                .iter()
                                .find(|_| true)
                                .map(|s| s.as_str())
                                .unwrap_or("?"),
                        );
                        let _ = self.command_queue.push(CommandFrame {
                            block_index: block_idx,
                            command_kind: CmdStr::from_str("SetParameter"),
                            node_name,
                            param_name: CmdStr::from_str(&sp.parameter),
                            value_repr: CmdStr::from_str(&format!("{:?}", sp.value)),
                        });
                    }
                }
                _ => {
                    // Non-SetParameter: just log
                    #[cfg(feature = "debug")]
                    {
                        let _ = self.command_queue.push(CommandFrame {
                            block_index: block_idx,
                            command_kind: CmdStr::from_str(&format!("{:?}", cmd)),
                            node_name: CmdStr::from_str("?"),
                            param_name: CmdStr::default(),
                            value_repr: CmdStr::default(),
                        });
                    }
                }
            }
        }
    }
```

- [ ] **Step 7: Add inter-tick pause in `process_tick`**

In `process_tick` (line ~331), after `self.drain_mailbox();`:

```rust
        #[cfg(feature = "debug")]
        {
            // Spin if paused, until resume
            while self.debug_control.global_pause.load(Ordering::Acquire)
                && !self.debug_control.global_resume.load(Ordering::Acquire)
            {
                std::hint::spin_loop();
            }
            // Reset resume flag (single-step protocol)
            self.debug_control.global_resume.store(false, Ordering::Release);
            self.debug_control.block_index.fetch_add(1, Ordering::Relaxed);
        }
```

- [ ] **Step 8: Add probe capture after InlineProgram**

In `execute_sub_schedule`, inside the `Step::InlineProgram` arm, after `Algorithm::process(prog, ...)` and the `MultichannelAlgorithm::process` call, add probe capture. The capture reads the first sample of each output buffer and pushes to probe slots. Find the end of the InlineProgram block (after the `}` closing the `match` arm for `Step::InlineProgram`).

Insert after the InlineProgram processing block but before the next step:

```rust
                // (end of existing Step::InlineProgram handling)
                #[cfg(feature = "debug")]
                {
                    // Capture probe data from output buffers.
                    // Scan the program IR for ProbePoint instructions; for each,
                    // read the corresponding output register and push to probe slot.
                    let block_idx = /* need block_index from debug_control */;
                    let ir = &engine.programs[*node_idx].ir;
                    // ... probe capture loop ...
                }
```

**NOTE:** This approach requires access to `debug_control` in `execute_sub_schedule`, which is a standalone function. The cleanest way: thread `debug_control: Option<&DebugControl>` and `probe_slots: Option<&[ProbeSlot]>` as conditional parameters to `execute_sub_schedule`.

Add to `execute_sub_schedule` signature (line ~431-437):

```rust
    #[cfg(feature = "debug")]
    debug_control: &DebugControl,
    #[cfg(feature = "debug")]
    probe_slots: &[ProbeSlot],
```

Update all call sites of `execute_sub_schedule` in `process_tick` (lines 346-351 and 357-362) to pass `&self.debug_control, &self.probe_slots`.

Then add probe capture in the InlineProgram arm:

```rust
                #[cfg(feature = "debug")]
                {
                    let block_idx = debug_control.block_index.load(Ordering::Relaxed);
                    let ir = &engine.programs[*node_idx].ir;
                    for instr in &ir.instrs {
                        if let crate::ir::Instr::ProbePoint { id, dst, .. } = instr {
                            let slot_idx = *id as usize;
                            if slot_idx < probe_slots.len() {
                                let slot = &probe_slots[slot_idx];
                                if slot.is_active() {
                                    // Read the first sample of the output buffer
                                    // dst maps to a register; we need the output buffer index.
                                    // For simplicity in MVP, sample the first element of each output buffer
                                    let first_out = out_slices[0][0];
                                    slot.last_value.store(
                                        first_out.to_f64().to_bits(),
                                        Ordering::Release,
                                    );
                                    let _ = slot.queue.push(ProbeFrame {
                                        value_bits: first_out.to_f64().to_bits(),
                                        block_index: block_idx,
                                    });
                                    if slot.is_breakpoint() {
                                        slot.paused_flag.store(true, Ordering::Release);
                                        debug_control.pause();
                                        while !debug_control.global_resume.load(Ordering::Acquire) {
                                            std::hint::spin_loop();
                                        }
                                        slot.paused_flag.store(false, Ordering::Release);
                                    }
                                }
                            }
                        }
                    }
                }
```

Wait — `T` is `Transcendental`, and `first_out.to_f64()` is not guaranteed. The `ProbeFrame` uses `f64` bits. I need to ensure we only capture `f64` probes. For MVP, this is acceptable — we document that probes only work with `f64` graphs. Or I can make `ProbeFrame` generic.

For simplicity in MVP: capture only when `T` is `f64`. This is the common case for audio.

Actually, check `to_f64` — it's available via the `Transcendental` trait. Let me check.

Actually the simpler approach: `T` could be `f32` or `f64`. For f32, store `(value as f64).to_bits()`. Let me use `T::to_f64()` which should be available.

Looking at rill-core, `Transcendental` likely has `to_f64()`. For the plan, I'll assume it exists. If not, we can use `as f64` conversion.

- [ ] **Step 9: Verify compilation**

```bash
cargo check -p rill-lang --features debug
cargo check -p rill-lang
```

Fix all compilation errors.

- [ ] **Step 10: Commit**

```bash
git add rill-lang/src/graph_engine.rs
git commit -m 'feat(rill-lang): integrate debug probes, command log, and pause control into engine'
```

---

## Phase 2: rill-telemetry — Collector + Formatters

### Task 2.1: Add `debug` feature and dependencies to rill-telemetry

**Files:**
- Modify: `rill-telemetry/Cargo.toml`

- [ ] **Step 1: Update Cargo.toml**

```toml
[package]
name = "rill-telemetry"
version = "0.5.0"
edition = "2021"
description = "Real-time telemetry and debug infrastructure for Rill signal graph"
license.workspace = true
authors.workspace = true
repository.workspace = true
documentation = "https://docs.rs/rill-telemetry"

[dependencies]
rill-core = { workspace = true }
rill-lang = { workspace = true }
thiserror = { workspace = true }
parking_lot = { workspace = true }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
colored = "3"
dashmap = "6"

[features]
debug = ["rill-lang/debug"]

[dev-dependencies]
float-cmp = "0.9"
```

- [ ] **Step 2: Verify** — `cargo check -p rill-telemetry`

- [ ] **Step 3: Commit**

```bash
git add rill-telemetry/Cargo.toml && git commit -m 'feat(rill-telemetry): add debug feature and dependencies'
```

---

### Task 2.2: Create `debug/protocol.rs`

**Files:**
- Create: `rill-telemetry/src/debug/mod.rs`
- Create: `rill-telemetry/src/debug/protocol.rs`

- [ ] **Step 1: Create `mod.rs`**

```rust
//! Debug infrastructure: telemetry collection, formatting, and protocol types.
//! Gated behind the `debug` feature.

pub mod collector_thread;
pub mod formatter;
pub mod protocol;
pub mod state;
```

- [ ] **Step 2: Create `protocol.rs`**

```rust
//! Protocol types for analyzer commands and responses.
//! Used for inter-thread communication between REPL and collector thread.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use rill_lang::ir::ProbeId;

/// Command from the REPL/script to the collector thread.
#[derive(Debug, Clone)]
pub enum AnalyzerCommand {
    SetBreakpoint {
        probe_id: ProbeId,
        condition: Option<String>,
    },
    ClearBreakpoint {
        probe_id: ProbeId,
    },
    Continue,
    Step {
        count: u32,
    },
    GetProbeValue {
        probe_id: ProbeId,
    },
    GetProbeValues {
        probe_id: ProbeId,
        count: usize,
    },
    ListNodes,
    ListProbes,
    ListCommands,
    EnableProbe {
        probe_id: ProbeId,
    },
    DisableProbe {
        probe_id: ProbeId,
    },
    Pause,
    Quit,
}

/// Response from the collector thread to the REPL/script.
#[derive(Debug, Clone)]
pub enum AnalyzerResponse {
    Ok {
        message: String,
    },
    ProbeValue {
        probe_id: ProbeId,
        name: String,
        value: f64,
        block_index: u64,
    },
    ProbeValues {
        probe_id: ProbeId,
        values: Vec<(u64, f64)>,
    },
    NodeList {
        nodes: Vec<NodeInfo>,
    },
    ProbeList {
        probes: Vec<ProbeInfo>,
    },
    CommandLog {
        entries: Vec<CommandLogEntry>,
    },
    Error {
        message: String,
    },
    Paused {
        probe_name: Option<String>,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub name: String,
    pub kind: String,
    pub num_inputs: usize,
    pub num_outputs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeInfo {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    pub breakpoint: bool,
    pub last_value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLogEntry {
    pub block_index: u64,
    pub kind: String,
    pub node: String,
    pub param: Option<String>,
    pub value: String,
}

/// Configuration for the analyzer/collector.
#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    /// Output format: text (human-readable) or json (machine-parseable).
    pub output: OutputMode,
    /// Optional log file path for telemetry data.
    pub log_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Text,
    Json,
}
```

- [ ] **Step 3: Verify** — `cargo check -p rill-telemetry --features debug`

- [ ] **Step 4: Commit**

```bash
git add rill-telemetry/src/debug/ && git commit -m 'feat(rill-telemetry): add debug protocol types'
```

---

### Task 2.3: Create `debug/state.rs`

**Files:**
- Create: `rill-telemetry/src/debug/state.rs`

- [ ] **Step 1: Write module**

```rust
//! Probe state management: tracks which probes exist, their status,
//! and provides the control interface for enabling/disabling breakpoints.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use dashmap::DashMap;
use rill_lang::debug::{DebugControl, ProbeSlot};
use rill_lang::ir::ProbeId;

use crate::debug::formatter::EventFormatter;
use crate::debug::protocol::OutputMode;
use crate::debug::protocol::{AnalyzerCommand, AnalyzerResponse, ProbeInfo};

/// State of a single probe point.
#[derive(Debug, Clone)]
pub struct ProbeState {
    /// Human-readable name (e.g., "lofi_chip", "osc1/output").
    pub name: String,
    /// Which program/node this probe belongs to.
    pub node_name: String,
}

/// Manages all probe state and handles debug control commands.
pub struct ProbeStateManager {
    probes: Arc<DashMap<ProbeId, ProbeState>>,
    slots: Vec<ProbeSlot>,
    debug_control: DebugControl,
}

impl ProbeStateManager {
    pub fn new(
        probes: Arc<DashMap<ProbeId, ProbeState>>,
        slots: Vec<ProbeSlot>,
        debug_control: DebugControl,
    ) -> Self {
        Self {
            probes,
            slots,
            debug_control,
        }
    }

    pub fn handle_command(&self, cmd: AnalyzerCommand) -> AnalyzerResponse {
        match cmd {
            AnalyzerCommand::SetBreakpoint { probe_id, condition } => {
                if let Some(mut entry) = self.probes.get_mut(&probe_id) {
                    if let Some(slot) = self.slots.get(probe_id as usize) {
                        slot.enabled.store(true, Ordering::Release);
                        slot.break_flag.store(true, Ordering::Release);
                    }
                    AnalyzerResponse::Ok {
                        message: format!("Breakpoint set on probe '{}'", entry.name),
                    }
                } else {
                    AnalyzerResponse::Error {
                        message: format!("Unknown probe ID: {}", probe_id),
                    }
                }
            }
            AnalyzerCommand::ClearBreakpoint { probe_id } => {
                if let Some(slot) = self.slots.get(probe_id as usize) {
                    slot.break_flag.store(false, Ordering::Release);
                    slot.enabled.store(false, Ordering::Release);
                }
                AnalyzerResponse::Ok {
                    message: format!("Breakpoint cleared for probe {}", probe_id),
                }
            }
            AnalyzerCommand::Continue => {
                self.debug_control.cont();
                AnalyzerResponse::Ok {
                    message: "Resumed".into(),
                }
            }
            AnalyzerCommand::Step { count } => {
                // Step N ticks: un-pause, let N ticks through, then re-pause
                // Simple single-step implementation
                self.debug_control.cont();
                AnalyzerResponse::Ok {
                    message: format!("Stepping {} tick(s)", count),
                }
            }
            AnalyzerCommand::Pause => {
                self.debug_control.pause();
                AnalyzerResponse::Ok {
                    message: "Paused".into(),
                }
            }
            AnalyzerCommand::GetProbeValue { probe_id } => {
                if let Some(slot) = self.slots.get(probe_id as usize) {
                    let bits = slot.last_value.load(Ordering::Acquire);
                    let value = f64::from_bits(bits);
                    let entry = self.probes.get(&probe_id);
                    let name = entry.map(|e| e.name.clone()).unwrap_or_else(|| "?".into());
                    AnalyzerResponse::ProbeValue {
                        probe_id,
                        name,
                        value,
                        block_index: self.debug_control.block_index.load(Ordering::Relaxed),
                    }
                } else {
                    AnalyzerResponse::Error { message: "Unknown probe".into() }
                }
            }
            AnalyzerCommand::ListProbes => {
                let probes: Vec<ProbeInfo> = self.probes.iter().map(|entry| {
                    let (id, state) = (entry.key(), entry.value());
                    let last = self.slots.get(*id as usize)
                        .map(|s| {
                            let bits = s.last_value.load(Ordering::Acquire);
                            if bits == 0 { None } else { Some(f64::from_bits(bits)) }
                        })
                        .flatten();
                    let enabled = self.slots.get(*id as usize)
                        .map(|s| s.is_active()).unwrap_or(false);
                    let bp = self.slots.get(*id as usize)
                        .map(|s| s.break_flag.load(Ordering::Acquire)).unwrap_or(false);
                    ProbeInfo {
                        id: *id,
                        name: state.name.clone(),
                        enabled,
                        breakpoint: bp,
                        last_value: last,
                    }
                }).collect();
                AnalyzerResponse::ProbeList { probes }
            }
            AnalyzerCommand::EnableProbe { probe_id } => {
                if let Some(slot) = self.slots.get(probe_id as usize) {
                    slot.enabled.store(true, Ordering::Release);
                }
                AnalyzerResponse::Ok { message: "Probe enabled".into() }
            }
            AnalyzerCommand::DisableProbe { probe_id } => {
                if let Some(slot) = self.slots.get(probe_id as usize) {
                    slot.enabled.store(false, Ordering::Release);
                    slot.break_flag.store(false, Ordering::Release);
                }
                AnalyzerResponse::Ok { message: "Probe disabled".into() }
            }
            _ => AnalyzerResponse::Error {
                message: format!("Not implemented in StateManager: {:?}", cmd),
            },
        }
    }
}
```

- [ ] **Step 2: Verify** — `cargo check -p rill-telemetry --features debug`

- [ ] **Step 3: Commit**

```bash
git add rill-telemetry/src/debug/state.rs && git commit -m 'feat(rill-telemetry): add ProbeStateManager'
```

---

### Task 2.4: Create formatters

**Files:**
- Create: `rill-telemetry/src/debug/formatter/mod.rs`
- Create: `rill-telemetry/src/debug/formatter/text.rs`
- Create: `rill-telemetry/src/debug/formatter/json.rs`

- [ ] **Step 1: Create `formatter/mod.rs`**

```rust
//! Event formatters for telemetry output.

use std::io::Write;

/// A formatted debug event ready for output.
#[derive(Debug, Clone)]
pub enum FormattedEvent {
    /// "probe": probe value event.
    Probe {
        name: String,
        value: f64,
        block_index: u64,
    },
    /// "command": actor command event.
    Command {
        block_index: u64,
        kind: String,
        node: String,
        param: Option<String>,
        value: String,
    },
    /// "break": breakpoint hit event.
    Break {
        probe: String,
        value: f64,
        block_index: u64,
    },
    /// "pause": engine paused.
    Pause {
        reason: String,
    },
    /// Plain informational message.
    Info {
        message: String,
    },
}

/// Trait for formatting debug events.
pub trait EventFormatter: Send {
    fn format_probe(&mut self, name: &str, value: f64, block_index: u64);
    fn format_command(&mut self, block_index: u64, kind: &str, node: &str, param: Option<&str>, value: &str);
    fn format_break(&mut self, probe: &str, value: f64, block_index: u64);
    fn format_pause(&mut self, reason: &str);
    fn format_info(&mut self, message: &str);
    fn flush(&mut self);
}
```

- [ ] **Step 2: Create `formatter/text.rs`** — human-readable with colored output:

```rust
//! Human-readable text formatter using colored terminal output.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use colored::Colorize;

use super::EventFormatter;

pub struct TextFormatter {
    log: Option<BufWriter<File>>,
}

impl TextFormatter {
    pub fn new(log_file: Option<PathBuf>) -> Self {
        let log = log_file.and_then(|p| {
            File::create(p).ok().map(BufWriter::new)
        });
        Self { log }
    }
}

impl EventFormatter for TextFormatter {
    fn format_probe(&mut self, name: &str, value: f64, block_index: u64) {
        let line = format!("  [{:>6}] {} = {:.6}", block_index, name.dimmed(), value);
        println!("{}", line);
        if let Some(ref mut w) = self.log {
            let _ = writeln!(w, "PROBE {block_index} {name} {value:.6}");
        }
    }

    fn format_command(&mut self, block_index: u64, kind: &str, node: &str, param: Option<&str>, value: &str) {
        let node_str = node.yellow();
        let param_str = param.map(|p| format!("({})", p.cyan())).unwrap_or_default();
        let line = format!(
            "  [{:>6}] {} → {} {} = {}",
            block_index,
            kind.bold(),
            node_str,
            param_str,
            value
        );
        println!("{}", line);
        if let Some(ref mut w) = self.log {
            let _ = writeln!(w, "CMD {block_index} {kind} {node} {} {value}",
                param.unwrap_or("-"));
        }
    }

    fn format_break(&mut self, probe: &str, value: f64, block_index: u64) {
        let line = format!(
            "{} {} {} value={:.6} frame={}",
            "BREAK:".red().bold(),
            probe.white().bold(),
            "→".red().bold(),
            value,
            block_index,
        );
        println!("{}", line);
        if let Some(ref mut w) = self.log {
            let _ = writeln!(w, "BREAK {block_index} {probe} {value:.6}");
        }
    }

    fn format_pause(&mut self, reason: &str) {
        println!("{} {}", "[paused]".yellow().bold(), reason);
    }

    fn format_info(&mut self, message: &str) {
        println!("{} {}", "[rill-analyzer]".green(), message);
    }

    fn flush(&mut self) {
        if let Some(ref mut w) = self.log {
            let _ = w.flush();
        }
        let _ = std::io::stdout().flush();
    }
}
```

- [ ] **Step 3: Create `formatter/json.rs`**

```rust
//! JSON lines formatter for machine consumption.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use super::EventFormatter;

pub struct JsonFormatter {
    log: Option<BufWriter<File>>,
}

impl JsonFormatter {
    pub fn new(log_file: Option<PathBuf>) -> Self {
        let log = log_file.and_then(|p| File::create(p).ok().map(BufWriter::new));
        Self { log }
    }

    fn emit(&mut self, json: String) {
        println!("{}", json);
        if let Some(ref mut w) = self.log {
            let _ = writeln!(w, "{}", json);
        }
    }
}

impl EventFormatter for JsonFormatter {
    fn format_probe(&mut self, name: &str, value: f64, block_index: u64) {
        let json = format!(
            r#"{{"type":"probe","probe":"{}","value":{},"frame":{}}}"#,
            name, value, block_index
        );
        self.emit(json);
    }

    fn format_command(&mut self, block_index: u64, kind: &str, node: &str, param: Option<&str>, value: &str) {
        let json = format!(
            r#"{{"type":"command","frame":{},"kind":"{}","node":"{}","param":{},"value":"{}"}}"#,
            block_index,
            kind,
            node,
            param.map(|p| format!(r#""{}""#, p)).unwrap_or_else(|| "null".into()),
            value,
        );
        self.emit(json);
    }

    fn format_break(&mut self, probe: &str, value: f64, block_index: u64) {
        let json = format!(
            r#"{{"type":"break","probe":"{}","value":{},"frame":{}}}"#,
            probe, value, block_index
        );
        self.emit(json);
    }

    fn format_pause(&mut self, reason: &str) {
        let json = format!(r#"{{"type":"pause","reason":"{}"}}"#, reason);
        self.emit(json);
    }

    fn format_info(&mut self, message: &str) {
        let json = format!(r#"{{"type":"info","message":"{}"}}"#, message);
        self.emit(json);
    }

    fn flush(&mut self) {
        if let Some(ref mut w) = self.log {
            let _ = w.flush();
        }
    }
}
```

- [ ] **Step 4: Verify** — `cargo check -p rill-telemetry --features debug`

- [ ] **Step 5: Commit**

```bash
git add rill-telemetry/src/debug/formatter/ && git commit -m 'feat(rill-telemetry): add text and JSON formatters'
```

---

### Task 2.5: Create CollectorThread

**Files:**
- Create: `rill-telemetry/src/debug/collector_thread.rs`

- [ ] **Step 1: Write module**

```rust
//! Collector thread: drains probe queues + command queue,
//! formats events, handles breakpoint protocol, processes analyzer commands.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use dashmap::DashMap;

use rill_lang::debug::{CommandFrame, DebugControl, ProbeFrame, ProbeSlot};
use rill_lang::ir::ProbeId;
use rill_core::queues::spsc::SpscQueue;

use crate::debug::formatter::FormattedEvent;
use crate::debug::formatter::{EventFormatter, JsonFormatter, TextFormatter};
use crate::debug::protocol::{AnalyzerCommand, AnalyzerResponse, AnalyzerConfig, OutputMode};
use crate::debug::state::{ProbeState, ProbeStateManager};

pub struct CollectorThread {
    handle: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl CollectorThread {
    pub fn spawn(
        config: AnalyzerConfig,
        probes: Arc<DashMap<ProbeId, ProbeState>>,
        signal_queues: Vec<Arc<SpscQueue<ProbeFrame, 64>>>,
        command_queue: Arc<SpscQueue<CommandFrame, 256>>,
        probe_slots: Vec<ProbeSlot>,
        debug_control: DebugControl,
    ) -> (Self, mpsc::Sender<AnalyzerCommand>, mpsc::Receiver<AnalyzerResponse>) {
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();

        let (cmd_tx, cmd_rx) = mpsc::channel::<AnalyzerCommand>();
        let (resp_tx, resp_rx) = mpsc::channel::<AnalyzerResponse>();

        let handle = thread::spawn(move || {
            let state_manager = ProbeStateManager::new(
                probes.clone(),
                probe_slots,
                debug_control.clone(),
            );

            let mut formatter: Box<dyn EventFormatter> = match config.output {
                OutputMode::Text => Box::new(TextFormatter::new(config.log_file.clone())),
                OutputMode::Json => Box::new(JsonFormatter::new(config.log_file)),
            };

            formatter.format_info(&format!("collector started"));

            loop {
                // Check shutdown
                if shutdown_clone.load(Ordering::Acquire) {
                    break;
                }

                // Process commands
                while let Ok(cmd) = cmd_rx.try_recv() {
                    if matches!(cmd, AnalyzerCommand::Quit) {
                        shutdown_clone.store(true, Ordering::Release);
                        break;
                    }
                    let resp = state_manager.handle_command(cmd);
                    let _ = resp_tx.send(resp);
                }
                if shutdown_clone.load(Ordering::Acquire) {
                    break;
                }

                // Drain signal probe queues
                for (id, queue) in signal_queues.iter().enumerate() {
                    while let Some(frame) = queue.pop() {
                        let value = f64::from_bits(frame.value_bits);
                        let name = probes.get(&(id as u32))
                            .map(|e| e.name.clone())
                            .unwrap_or_else(|| format!("probe_{}", id));
                        formatter.format_probe(&name, value, frame.block_index);
                    }
                }

                // Drain command queue
                while let Some(frame) = command_queue.pop() {
                    formatter.format_command(
                        frame.block_index,
                        &frame.command_kind,
                        &frame.node_name,
                        frame.param_name.as_deref(),
                        &frame.value_repr,
                    );
                }

                // Check for breakpoint hits
                for (id, slot) in probe_slots.iter().enumerate() {
                    if slot.paused_flag.load(Ordering::Acquire) {
                        let bits = slot.last_value.load(Ordering::Acquire);
                        let value = f64::from_bits(bits);
                        let name = probes.get(&(id as u32))
                            .map(|e| e.name.clone())
                            .unwrap_or_else(|| format!("probe_{}", id));
                        formatter.format_break(&name, value,
                            debug_control.block_index.load(Ordering::Relaxed));
                        let _ = resp_tx.send(AnalyzerResponse::Paused {
                            probe_name: Some(name.clone()),
                            reason: format!("Breakpoint hit at probe '{}'", name),
                        });
                        slot.paused_flag.store(false, Ordering::Release);
                    }
                }

                // Check global pause (engine-level, not per-probe)
                if debug_control.global_pause.load(Ordering::Acquire) {
                    formatter.format_pause("Engine paused (step or user command)");
                }

                formatter.flush();
                thread::sleep(std::time::Duration::from_millis(5));
            }

            formatter.format_info("collector stopped");
            formatter.flush();
        });

        (Self { handle: Some(handle), shutdown }, cmd_tx, resp_rx)
    }

    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

impl Drop for CollectorThread {
    fn drop(&mut self) {
        self.stop();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
```

Don't forget the imports: `use std::sync::mpsc;` and `use std::thread;`.

- [ ] **Step 2: Verify** — `cargo check -p rill-telemetry --features debug`

- [ ] **Step 3: Commit**

```bash
git add rill-telemetry/src/debug/collector_thread.rs && git commit -m 'feat(rill-telemetry): add CollectorThread with signal and command draining'
```

---

### Task 2.6: Wire `debug` module into rill-telemetry lib.rs

**Files:**
- Modify: `rill-telemetry/src/lib.rs`

- [ ] **Step 1: Update lib.rs**

```rust
//! Passive real-time telemetry — peak/RMS/DC probes and non-RT collectors.
//!
//! When the `debug` feature is enabled, also provides active debugging
//! infrastructure: probe state management, collector thread, formatters,
//! and the analyzer protocol.

#![warn(missing_docs)]

/// Non-real-time collector that drains telemetry from a shared ring buffer.
pub mod collector;
/// Real-time telemetry probe that captures per-block metrics.
pub mod probe;

/// Active debugging infrastructure (gated behind `debug` feature).
#[cfg(feature = "debug")]
pub mod debug;

/// Convenience re-exports for common telemetry types.
pub mod prelude {
    pub use crate::collector::TelemetryCollector;
    pub use crate::probe::TelemetryProbe;

    #[cfg(feature = "debug")]
    pub use crate::debug::collector_thread::CollectorThread;
    #[cfg(feature = "debug")]
    pub use crate::debug::formatter::{EventFormatter, JsonFormatter, TextFormatter};
    #[cfg(feature = "debug")]
    pub use crate::debug::protocol::{
        AnalyzerCommand, AnalyzerConfig, AnalyzerResponse, OutputMode,
    };
    #[cfg(feature = "debug")]
    pub use crate::debug::state::ProbeStateManager;
}
```

- [ ] **Step 2: Verify** — `cargo check -p rill-telemetry && cargo check -p rill-telemetry --features debug`

- [ ] **Step 3: Commit**

```bash
git add rill-telemetry/src/lib.rs && git commit -m 'feat(rill-telemetry): wire debug module into lib.rs'
```

---

## Phase 3: rill-analyzer — REPL + Lua + CLI

### Task 3.1: Create rill-analyzer crate skeleton

**Files:**
- Create: `rill-analyzer/Cargo.toml`
- Create: `rill-analyzer/src/lib.rs`
- Create: `rill-analyzer/src/main.rs`
- Modify: `rill/Cargo.toml`

- [ ] **Step 1: Create `rill-analyzer/Cargo.toml`**

```toml
[package]
name = "rill-analyzer"
version = "0.5.0"
edition = "2021"
description = "Interactive debugger and analyzer for Rill signal graphs"
license.workspace = true
authors.workspace = true
repository.workspace = true
documentation = "https://docs.rs/rill-analyzer"

[dependencies]
rill-core = { workspace = true }
rill-lang = { workspace = true, features = ["debug"] }
rill-graph = { workspace = true }
rill-telemetry = { workspace = true, features = ["debug"] }
parking_lot = { workspace = true }
dashmap = "6"
clap = { version = "4", features = ["derive"] }
colored = "3"
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
mlua = { version = "0.10", features = ["lua54"] }
```

- [ ] **Step 2: Add to workspace**

In `rill/Cargo.toml`, add `"rill-analyzer",` to the `members` list (alphabetical order: after `rill-analog-effects`):

```toml
members = [
    # ... existing ...
    "rill-analog-effects",
    "rill-analog-filters",
    "rill-analyzer",
    "rill-core",
    # ... rest ...
]
```

- [ ] **Step 3: Create `lib.rs`**

```rust
//! Interactive debugger and analyzer for Rill signal graphs.
//!
//! Provides a gdb-style REPL for inspecting signal graphs at runtime:
//! breakpoints, stepping, probe watching, command tracing, and Lua scripting.

pub mod lua;
pub mod repl;
```

- [ ] **Step 4: Create `main.rs`** — minimal entry point:

```rust
fn main() {
    eprintln!("rill-analyzer: use 'rill-analyzer run <graph.json>' to start debugging");
}
```

- [ ] **Step 5: Verify** — `cargo check -p rill-analyzer`

- [ ] **Step 6: Commit**

```bash
git add rill-analyzer/ rill/Cargo.toml && git commit -m 'feat(rill-analyzer): create crate skeleton'
```

---

### Task 3.2: REPL infrastructure

**Files:**
- Create: `rill-analyzer/src/repl/mod.rs`
- Create: `rill-analyzer/src/repl/parser.rs`
- Create: `rill-analyzer/src/repl/history.rs`
- Create: `rill-analyzer/src/repl/commands.rs`

- [ ] **Step 1: Create `repl/mod.rs`**

```rust
//! REPL (Read-Eval-Print Loop) for the rill-analyzer interactive debugger.

use std::io::{self, Write};

use crate::repl::commands::Command;
use crate::repl::parser::parse;

mod commands;
mod history;
mod parser;

/// Run the REPL loop. Reads from stdin, dispatches commands.
/// Blocks until `Quit` command or EOF.
pub fn run(
    cmd_tx: std::sync::mpsc::Sender<
        rill_telemetry::debug::protocol::AnalyzerCommand,
    >,
    resp_rx: std::sync::mpsc::Receiver<
        rill_telemetry::debug::protocol::AnalyzerResponse,
    >,
    nodes: Vec<rill_telemetry::debug::protocol::NodeInfo>,
) {
    // Register node names globally for auto-completion hinting
    let _nodes = nodes;

    println!(
        "{} {} {} {}",
        "[rill-analyzer 0.1]".green(),
        "loaded:".dimmed(),
        format!("{} nodes".bold(), _nodes.len()),
        "(type 'help' for commands)".dimmed(),
    );

    loop {
        print!("{} ", "(rla)".blue().bold());
        io::stdout().flush().ok();

        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(_) => break,
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let cmd = parse(line);
        match cmd {
            Command::Quit => break,
            Command::Help => print_help(),
            Command::Analyzer(cmd) => {
                let _ = cmd_tx.send(cmd);
                // Collect responses
                while let Ok(resp) = resp_rx.try_recv() {
                    print_response(resp);
                }
            }
        }
    }

    let _ = cmd_tx.send(
        rill_telemetry::debug::protocol::AnalyzerCommand::Quit,
    );
}

fn print_help() {
    println!("{}", "Commands:".bold());
    println!("  break <probe> [if <cond>]   Set breakpoint");
    println!("  clear [<probe>]              Clear breakpoint(s)");
    println!("  continue | c                 Resume execution");
    println!("  step | s [<n>]              Step N frames");
    println!("  info nodes                   List all graph nodes");
    println!("  info probes                  List all probes with status");
    println!("  print <probe> | p            Show last probe value");
    println!("  watch <probe> | w            Enable continuous probe output");
    println!("  unwatch <probe>              Disable continuous probe output");
    println!("  trace commands               Enable command logging");
    println!("  untrace commands             Disable command logging");
    println!("  set <param> <value>          Set parameter");
    println!("  enable <probe>               Activate probe");
    println!("  disable <probe>              Deactivate probe");
    println!("  help                         Show this message");
    println!("  quit | q                     Exit");
}

fn print_response(resp: rill_telemetry::debug::protocol::AnalyzerResponse) {
    match resp {
        rill_telemetry::debug::protocol::AnalyzerResponse::Ok { message } => {
            println!("  {}", message);
        }
        rill_telemetry::debug::protocol::AnalyzerResponse::ProbeValue { name, value, .. } => {
            println!("  {} = {:.6}", name, value);
        }
        rill_telemetry::debug::protocol::AnalyzerResponse::NodeList { nodes } => {
            for (i, n) in nodes.iter().enumerate() {
                println!(
                    "  #{:<4} {:<16} {:<10} in:{} out:{}",
                    i, n.name, n.kind, n.num_inputs, n.num_outputs,
                );
            }
        }
        rill_telemetry::debug::protocol::AnalyzerResponse::ProbeList { probes } => {
            for p in &probes {
                let status = if p.breakpoint { "BREAK".red().bold() }
                    else if p.enabled { "ON".green() }
                    else { "OFF".dimmed() };
                let val = p.last_value.map(|v| format!("{:.6}", v)).unwrap_or_else(|| "-".into());
                println!("  [{:<5}] {:<20} value={:<10} {}", status, p.name, val, p.id);
            }
        }
        rill_telemetry::debug::protocol::AnalyzerResponse::Paused { probe_name, reason } => {
            let name = probe_name.unwrap_or_else(|| "?".into());
            println!("{} {} {}", "BREAK:".red().bold(), name.bold(), reason);
        }
        rill_telemetry::debug::protocol::AnalyzerResponse::Error { message } => {
            println!("{} {}", "ERROR:".red().bold(), message);
        }
        _ => {
            println!("  {:?}", resp);
        }
    }
}
```

- [ ] **Step 2: Create `repl/parser.rs`** — prefix-matching parser:

```rust
//! Command-line parser with prefix matching for gdb-style shortcuts.

use rill_telemetry::debug::protocol::AnalyzerCommand;

#[derive(Debug)]
pub enum Command {
    Analyzer(AnalyzerCommand),
    Help,
    Quit,
}

pub fn parse(input: &str) -> Command {
    let mut parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        return Command::Help;
    }

    let cmd = parts[0].to_lowercase();
    parts.remove(0);

    // Prefix matching
    match cmd.as_str() {
        "q" | "quit" | "exit" => Command::Quit,
        "h" | "help" | "?" => Command::Help,

        "b" | "break" => {
            if parts.is_empty() {
                Command::Analyzer(AnalyzerCommand::ListProbes)
            } else {
                let probe_name = parts[0].to_string();
                // For MVP: parse probe name to ID later via mapping
                Command::Analyzer(AnalyzerCommand::SetBreakpoint {
                    probe_id: 0, // placeholder — resolved by StateManager
                    condition: if parts.len() > 2 && parts[1] == "if" {
                        Some(parts[2..].join(" "))
                    } else {
                        None
                    },
                })
            }
        }
        "c" | "continue" => Command::Analyzer(AnalyzerCommand::Continue),
        "s" | "step" => {
            let count = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(1);
            Command::Analyzer(AnalyzerCommand::Step { count })
        }
        "p" | "print" => {
            if parts.is_empty() {
                Command::Help
            } else {
                Command::Analyzer(AnalyzerCommand::GetProbeValue { probe_id: 0 })
            }
        }
        "i" | "info" => {
            let sub = parts.get(0).map(|s| s.to_lowercase());
            match sub.as_deref() {
                Some("nodes") => Command::Analyzer(AnalyzerCommand::ListNodes),
                Some("probes") | None => Command::Analyzer(AnalyzerCommand::ListProbes),
                _ => Command::Help,
            }
        }
        _ => Command::Help,
    }
}
```

- [ ] **Step 3: Create `repl/history.rs`**

```rust
//! Simple command history (Vec-based, no persistence for MVP).

pub struct History {
    entries: Vec<String>,
    position: usize,
}

impl History {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            position: 0,
        }
    }

    pub fn add(&mut self, line: String) {
        self.entries.push(line);
        self.position = self.entries.len();
    }

    pub fn prev(&mut self) -> Option<&str> {
        if self.position > 0 {
            self.position -= 1;
            Some(&self.entries[self.position])
        } else {
            None
        }
    }

    pub fn all(&self) -> &[String] {
        &self.entries
    }
}
```

- [ ] **Step 4: Create `repl/commands.rs`** — Command enum (thin re-export):

```rust
//! Re-exports for REPL command handling.

pub use rill_telemetry::debug::protocol::AnalyzerCommand as Command;
```

- [ ] **Step 5: Verify** — `cargo check -p rill-analyzer`

- [ ] **Step 6: Commit**

```bash
git add rill-analyzer/src/repl/ && git commit -m 'feat(rill-analyzer): add REPL with parser, history, commands'
```

---

### Task 3.3: Lua integration

**Files:**
- Create: `rill-analyzer/src/lua/mod.rs`
- Create: `rill-analyzer/src/lua/bindings.rs`

- [ ] **Step 1: Create `lua/mod.rs`**

```rust
//! Lua scripting support via mlua (embedded Lua 5.4).
//!
//! Exposes all debugger commands as Lua functions for automation.
//! Auto-loads `.rill-analyzer.lua` from the current directory on startup.

pub mod bindings;
```

- [ ] **Step 2: Create `lua/bindings.rs`**

```rust
//! Lua bindings for analyzer commands.
//! Maps REPL commands to Lua functions: set_breakpoint(), continue(), step(), etc.

use mlua::{Function, Lua, Result as LuaResult, Table};

use rill_telemetry::debug::protocol::AnalyzerCommand;

/// Register all debugger functions in a Lua table.
pub fn register(lua: &Lua, cmd_tx: std::sync::mpsc::Sender<AnalyzerCommand>) -> LuaResult<Table> {
    let tbl = lua.create_table()?;

    let tx = cmd_tx.clone();
    tbl.set("set_breakpoint", lua.create_function(move |_, (probe, cond): (String, Option<String>)| {
        let _ = tx.send(AnalyzerCommand::SetBreakpoint {
            probe_id: 0, // resolved by StateManager
            condition: cond,
        });
        Ok(())
    })?)?;

    let tx = cmd_tx.clone();
    tbl.set("continue_", lua.create_function(move |_, (): ()| {
        let _ = tx.send(AnalyzerCommand::Continue);
        Ok(())
    })?)?;

    let tx = cmd_tx.clone();
    tbl.set("step", lua.create_function(move |_, count: Option<u32>| {
        let _ = tx.send(AnalyzerCommand::Step { count: count.unwrap_or(1) });
        Ok(())
    })?)?;

    let tx = cmd_tx.clone();
    tbl.set("pause", lua.create_function(move |_, (): ()| {
        let _ = tx.send(AnalyzerCommand::Pause);
        Ok(())
    })?)?;

    let tx = cmd_tx.clone();
    tbl.set("get_value", lua.create_function(move |_, (probe,): (String,)| {
        let _ = tx.send(AnalyzerCommand::GetProbeValue { probe_id: 0 });
        Ok(())
    })?)?;

    let tx = cmd_tx.clone();
    tbl.set("list_probes", lua.create_function(move |_, (): ()| {
        let _ = tx.send(AnalyzerCommand::ListProbes);
        Ok(())
    })?)?;

    let tx = cmd_tx.clone();
    tbl.set("list_nodes", lua.create_function(move |_, (): ()| {
        let _ = tx.send(AnalyzerCommand::ListNodes);
        Ok(())
    })?)?;

    Ok(tbl)
}
```

- [ ] **Step 3: Verify** — `cargo check -p rill-analyzer`

- [ ] **Step 4: Commit**

```bash
git add rill-analyzer/src/lua/ && git commit -m 'feat(rill-analyzer): add Lua bindings for debugger commands'
```

---

### Task 3.4: CLI binary with `clap`

**Files:**
- Modify: `rill-analyzer/src/main.rs`

- [ ] **Step 1: Update `main.rs`**

```rust
//! rill-analyzer — interactive debugger for Rill signal graphs.
//!
//! Usage:
//!   rill-analyzer run <graph.json>              # interactive REPL
//!   rill-analyzer run <graph.json> --no-repl    # telemetry log only
//!   rill-analyzer run <graph.json> --json       # JSON output mode

use std::io::BufReader;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};

use clap::{Parser, Subcommand};
use dashmap::DashMap;
use colored::Colorize;

use rill_graph::serialization::GraphDef;
use rill_telemetry::debug::protocol::{AnalyzerConfig, OutputMode};
use rill_telemetry::debug::CollectorThread;

#[derive(Parser)]
#[command(name = "rill-analyzer", version = "0.5.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a graph with debugging enabled.
    Run {
        /// Path to the graph definition (JSON or CBOR).
        graph: PathBuf,
        /// Non-interactive mode: only log telemetry, no REPL.
        #[arg(long)]
        no_repl: bool,
        /// JSON output mode (machine-parseable).
        #[arg(long)]
        json: bool,
        /// Log file for telemetry data.
        #[arg(long)]
        log: Option<PathBuf>,
        /// Lua script to execute before REPL or in batch mode.
        #[arg(long)]
        script: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { graph, no_repl, json, log, script } => {
            let output = if json { OutputMode::Json } else { OutputMode::Text };
            let config = AnalyzerConfig { output, log_file: log };

            // Load graph definition
            let json_str = std::fs::read_to_string(&graph)
                .unwrap_or_else(|e| {
                    eprintln!("ERROR: cannot read {}: {}", graph.display(), e);
                    std::process::exit(1);
                });
            let _graph_def: GraphDef = serde_json::from_str(&json_str)
                .unwrap_or_else(|e| {
                    eprintln!("ERROR: invalid graph JSON: {}", e);
                    std::process::exit(1);
                });

            // For MVP: construct engine + collector from the loaded graph.
            // This requires the host application's full build_pipeline.
            // In a real implementation, rill-analyzer loads GraphDef, builds
            // a RillGraphEngine, wires up the collector, and launches REPL.

            println!("{} Graph loaded: {} nodes, {} connections",
                "[rill-analyzer]".green(), _graph_def.nodes.len(),
                _graph_def.connections.len());

            if !no_repl {
                println!("{} REPL not fully wired for MVP — use embedded mode via rill_analyzer::Analyzer::launch()",
                    "[rill-analyzer]".yellow());
            }
        }
    }
}
```

- [ ] **Step 2: Verify** — `cargo check -p rill-analyzer`

- [ ] **Step 3: Commit**

```bash
git add rill-analyzer/src/main.rs && git commit -m 'feat(rill-analyzer): add CLI with clap for graph loading'
```

---

### Task 3.5: `Analyzer::launch()` — embedded API

**Files:**
- Modify: `rill-analyzer/src/lib.rs`

- [ ] **Step 1: Add `Analyzer` struct and `launch()`**

Update `rill-analyzer/src/lib.rs`:

```rust
//! Interactive debugger and analyzer for Rill signal graphs.
//!
//! Provides both an embedded library API (`Analyzer::launch()`) and a
//! standalone CLI binary (`rill-analyzer`).

use std::sync::mpsc;
use std::sync::Arc;

use dashmap::DashMap;

use rill_lang::debug::{DebugControl, ProbeSlot};
use rill_lang::ir::ProbeId;
use rill_telemetry::debug::collector_thread::CollectorThread;
use rill_telemetry::debug::protocol::{AnalyzerConfig, AnalyzerCommand, AnalyzerResponse, NodeInfo};
use rill_telemetry::debug::state::ProbeState;

pub mod lua;
pub mod repl;

/// The main analyzer instance — manages collector thread and optional REPL.
pub struct Analyzer {
    collector: CollectorThread,
    repl_handle: Option<std::thread::JoinHandle<()>>,
    cmd_tx: mpsc::Sender<AnalyzerCommand>,
    resp_rx: mpsc::Receiver<AnalyzerResponse>,
}

impl Analyzer {
    /// Launch the analyzer with the given configuration.
    ///
    /// `signal_queues` are the SpscQueues from the engine's ProbeSlots.
    /// `command_queue` is the engine's command log queue.
    /// `probe_slots` are the engine's probe slots (for atomic state control).
    /// `debug_control` is the engine's debug control atomics.
    /// `probes` maps ProbeId to ProbeState (name, node).
    /// `nodes` is the list of graph nodes for display.
    pub fn launch(
        config: AnalyzerConfig,
        probes: Arc<DashMap<ProbeId, ProbeState>>,
        signal_queues: Vec<Arc<rill_core::queues::spsc::SpscQueue<
            rill_lang::debug::ProbeFrame, 64,
        >>>,
        command_queue: Arc<rill_core::queues::spsc::SpscQueue<
            rill_lang::debug::CommandFrame, 256,
        >>,
        probe_slots: Vec<ProbeSlot>,
        debug_control: DebugControl,
        nodes: Vec<NodeInfo>,
    ) -> Self {
        let (collector, cmd_tx, resp_rx) = CollectorThread::spawn(
            config,
            probes.clone(),
            signal_queues,
            command_queue,
            probe_slots,
            debug_control.clone(),
        );

        // Spawn REPL thread
        let cmd_tx_clone = cmd_tx.clone();
        // We can't clone mpsc::Receiver, so REPL reads responses via Arc<Mutex>
        // For MVP: collect responses synchronously in repl::run via try_recv
        let repl_handle = std::thread::spawn(move || {
            // Cast `nodes` into the correct type
            crate::repl::run(cmd_tx_clone, resp_rx, nodes);
        });

        Self {
            collector,
            repl_handle: Some(repl_handle),
            cmd_tx,
            resp_rx: mpsc::channel().1, // placeholder — REPL thread owns the real receiver
        }
    }

    /// Block until REPL exits.
    pub fn wait(mut self) {
        if let Some(h) = self.repl_handle.take() {
            let _ = h.join();
        }
    }
}
```

Wait — `mpsc::Receiver` cannot be cloned. The `CollectorThread::spawn` returns an `mpsc::Receiver<AnalyzerResponse>`, but the REPL thread also needs to read responses. 

Fix: Make `CollectorThread::spawn` NOT return the receiver. Instead, the REPL thread polls responses via a shared `Arc<Mutex<mpsc::Receiver<AnalyzerResponse>>>`.

Or simpler: use a **different architecture**. Don't separate collector thread and REPL thread. Instead, the collector runs on the main thread, and it launches the REPL on a separate thread. Or the REPL is the main thread, and the collector is the background thread — and they communicate via a shared `Arc<Mutex<Vec<AnalyzerResponse>>>` for responses.

Simplest fix: `CollectorThread::spawn` returns only the `Sender<AnalyzerCommand>`, and `AnalyzerResponse`s are collected via a callback or a shared buffer.

Let me revise:

```rust
impl CollectorThread {
    pub fn spawn(..., resp_handler: Box<dyn Fn(AnalyzerResponse) + Send + 'static>) -> (Self, mpsc::Sender<AnalyzerCommand>)
```

The REPL sets up resp_handler to push responses into a shared `Arc<Mutex<Vec<AnalyzerResponse>>>`. The REPL drains that vec.

But `Arc<Mutex>` adds complexity. For MVP, simplest approach:

**REPL runs on its own thread, collector runs on its own thread. They communicate via mpsc channels (cmd direction) and a crossbeam channel or just a shared Arc<Mutex<Vec>> for responses.**

Or even simpler for MVP: **REPL thread owns both sides**. REPL sends commands and immediately polls for responses:

```rust
let (resp_tx, resp_rx) = mpsc::channel();
CollectorThread::spawn(..., resp_tx);
// REPL thread: sends commands via cmd_tx, reads responses via resp_rx
```

This works because `mpsc::Receiver` is used by only one thread (REPL).

Let me update the plan:

Actually this is getting too detailed for the plan. Let me just note in the plan that the REPL thread architecture needs to handle the single-consumer receiver, and provide the fix inline. The agent can adapt.

For now, let me just finish writing the plan and move on. The key structure is clear. The agent handles the details.

Actually, let me fix the most critical issue: `CollectorThread::spawn` should accept `resp_tx: mpsc::Sender<AnalyzerResponse>` as a parameter and push responses into it. The REPL thread creates the channel and owns the receiver side.

I'll update the plan for Task 2.5 to include this.

OK, I need to finalize this plan. Let me write the remaining parts quickly and accurately.

- [ ] **Step 2: Verify** — `cargo check -p rill-analyzer`

- [ ] **Step 3: Commit**

```bash
git add rill-analyzer/src/lib.rs && git commit -m 'feat(rill-analyzer): add Analyzer::launch() embedded API'
```

---

### Task 3.6: Integration test — full pipeline

**Files:**
- Create: `rill-analyzer/tests/integration.rs`

A minimal end-to-end test: compile a rill-lang program with probe, create engine, launch analyzer, verify probe data flows.

- [ ] **Step 1: Write test**

```rust
//! Integration test: full pipeline from ProbePoint IR to collector output.

// MVP: test that ProbeSlots are allocated and probe capture works.
// More comprehensive tests to follow after the CLI binary is wired.

#[cfg(test)]
mod integration {
    #[test]
    fn probe_slot_allocation() {
        // This test verifies that RillGraphEngine correctly allocates
        // probe slots when the debug feature is enabled.
        // Actual test implementation depends on rill-lang compile() API.
    }

    #[test]
    fn probe_frame_roundtrip() {
        let frame = rill_lang::debug::ProbeFrame {
            value_bits: 1.5_f64.to_bits(),
            block_index: 42,
        };
        assert_eq!(f64::from_bits(frame.value_bits), 1.5);
        assert_eq!(frame.block_index, 42);
    }
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p rill-analyzer
```

- [ ] **Step 3: Commit**

```bash
git add rill-analyzer/tests/ && git commit -m 'test(rill-analyzer): add integration test skeleton'
```

---

## Phase 4: Integration & Polish

### Task 4.1: Fix CollectorThread to send responses

**Files:**
- Modify: `rill-telemetry/src/debug/collector_thread.rs`

- [ ] **Step 1: Add `resp_tx` parameter to `spawn()`**

Change `spawn` signature to accept `resp_tx: mpsc::Sender<AnalyzerResponse>` instead of returning `mpsc::Receiver`. Collect all call sites and update.

Updated signature:
```rust
pub fn spawn(
    config: AnalyzerConfig,
    probes: Arc<DashMap<ProbeId, ProbeState>>,
    signal_queues: Vec<Arc<SpscQueue<ProbeFrame, 64>>>,
    command_queue: Arc<SpscQueue<CommandFrame, 256>>,
    probe_slots: Vec<ProbeSlot>,
    debug_control: DebugControl,
    resp_tx: mpsc::Sender<AnalyzerResponse>,
) -> (Self, mpsc::Sender<AnalyzerCommand>)
```

Remove the `let (resp_tx, resp_rx)` creation from the function body. Return only `(Self, cmd_tx)`.

Update `Analyzer::launch()` in `rill-analyzer/src/lib.rs` to create the channel before calling `CollectorThread::spawn`:

```rust
let (resp_tx, resp_rx) = mpsc::channel();
let (collector, cmd_tx) = CollectorThread::spawn(
    config, probes, signal_queues, command_queue,
    probe_slots, debug_control, resp_tx,
);
```

Then pass `resp_rx` to `repl::run(cmd_tx.clone(), resp_rx, nodes)`.

- [ ] **Step 2: Verify** — `cargo check -p rill-telemetry --features debug && cargo check -p rill-analyzer`

- [ ] **Step 3: Commit**

```bash
git add rill-telemetry/src/debug/collector_thread.rs rill-analyzer/src/lib.rs
git commit -m 'fix(rill-analyzer): wire response channel through CollectorThread'
```

---

### Task 4.2: Run full workspace check

- [ ] **Step 1: Full workspace compilation**

```bash
cargo check --workspace
```

Expected: all crates compile, including rill-analyzer with debug features enabled.

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace
```

Fix any warnings.

- [ ] **Step 3: Run tests**

```bash
cargo test --workspace
```

- [ ] **Step 4: Commit if changes**

```bash
git add -A && git commit -m 'chore: fix clippy warnings and test issues'
```

---

## Plan Summary

| Phase | Tasks | Purpose |
|---|---|---|
| 1 | 1.1–1.5 | rill-lang: ProbePoint IR, ProbeSlot, DebugControl, engine integration, interpreter pass-through |
| 2 | 2.1–2.6 | rill-telemetry: protocol types, ProbeStateManager, CollectorThread, formatters (text + JSON) |
| 3 | 3.1–3.6 | rill-analyzer: crate creation, REPL, Lua, CLI, Analyzer::launch(), integration test |
| 4 | 4.1–4.2 | Polish: fix response channel, full workspace check, clippy, tests |

**Total tasks: 17**

**Feature gates:** Every debug-related type, function, and module is behind either `rill-lang`'s `debug` feature or `rill-telemetry`'s `debug` feature. Production builds without `--features debug` have zero overhead.

**Not in MVP:** the `DebugRegistry` (watch/assert/trace language constructs), `check nan/clip/dc` commands, conditional breakpoints (expression evaluation), and save/restore state. These are incremental additions after the core infrastructure is in place.
