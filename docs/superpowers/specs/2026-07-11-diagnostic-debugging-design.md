# Diagnostic and Debugging Infrastructure for Rill

**Date:** 2026-07-11
**Status:** Draft
**Author:** brainstormed with opencode

## Motivation

The legacy execution infrastructure (rill-graph Port-based DAG and `ProcessingState`) has been removed. The new execution system is entirely based on rill-lang. However, the `chiptune_stc` example does not work, and there is **zero diagnostic tooling** — the execution pipeline is a complete black box. There is no way to:

- See which node/step is executing
- Detect signal anomalies (NaN, Inf, clipping, DC offset)
- Trace parameter changes and command routing
- Inspect intermediate signal values
- Pause and examine state at a specific point

This design introduces two levels of diagnostic infrastructure:

1. **Runtime telemetry/logging** — language-level debug constructs (`watch`, `assert`, `trace`, `signal_break`) that compile into probe points; data flows from the RT thread through lock-free queues to a collector thread.
2. **Interactive debugger** (`rill-analyzer`) — a gdb-style REPL with breakpoints, stepping, state inspection, parameter modification, and Lua scripting.

## Architecture Overview

```
┌─────────────────────────────────────────────────┐
│ rill-lang (core, feature = "debug")             │
│  ProbePoint IR  +  ProbeSlot  +  global_pause   │
│  1 new Instr variant, zero-cost when disabled    │
└──────────────┬──────────────────────────────────┘
               │ depends on
┌──────────────▼──────────────────────────────────┐
│ rill-telemetry (diagnostic infrastructure)       │
│  DebugRegistry    — watch/assert/trace lowering  │
│  ProbeState       — DashMap<ProbeId, ...>        │
│  CollectorThread  — SpscQueue → text/JSON        │
│  AnalyzerProtocol — commands, responses, config  │
│  Formatters       — human-readable + JSON        │
└──────────────┬──────────────────────────────────┘
               │ depends on
┌──────────────▼──────────────────────────────────┐
│ rill-analyzer (user-facing UX)                  │
│  REPL        — stdin/stdout, command parser      │
│  Lua runtime — mlua, .rill-analyzer.lua          │
│  CLI binary  — clap, GraphDef loading            │
└─────────────────────────────────────────────────┘
```

**Separation of concerns:**

- **rill-lang** provides exactly one minimal primitive: `ProbePoint` IR instruction. This is the *only* debug concept the language core knows about.
- **rill-telemetry** owns all diagnostic logic: lowering language constructs to ProbePoint IR, managing probe state, collecting frames, formatting output. Can be used directly by any host application without `rill-analyzer`.
- **rill-analyzer** is a thin UX layer: REPL, Lua scripting, CLI. Consumes `rill-telemetry` API.

**Zero-cost for production:** rill-lang's `debug` Cargo feature gates the ProbePoint instruction and all related runtime structures. When disabled, the IR has no ProbePoint variant, the engine allocates no probe buffers, and there is zero overhead. Headless IoT deployments never pay for debug infrastructure.

**No IPC for MVP:** Since rill is primarily an embeddable core, all communication is inter-thread within a single process. Debugger REPL runs in its own thread alongside the RT thread and collector thread. A separate debugger process (IPC via shared memory + signals) is deferred to a future iteration.

## Component Design

### 1. rill-lang: ProbePoint IR Instruction

#### IR

One new variant added to `Instr` enum:

```rust
// rill-lang/src/ir.rs
pub enum Instr {
    // ... existing 15 variants (Const, LoadInput, ReadState, ReadDelay, ...) ...
    ProbePoint {
        id: ProbeId,  // u32 — unique identifier for the probe
        src: Reg,     // source register (signal value)
        dst: Reg,     // destination register (pass-through)
    },
}

pub type ProbeId = u32;
```

`ProbePoint` is a **pass-through**: copies the value from `src` to `dst` and simultaneously sends it to the probe buffer. It does not alter control flow.

#### ProbeSlot

Each ProbePoint ID maps to one `ProbeSlot` stored in `RillGraphEngine`:

```rust
// rill-lang/src/graph_engine.rs
struct ProbeSlot<T: Transcendental> {
    enabled: AtomicBool,          // is this probe active?
    break_flag: AtomicBool,       // set = RT thread will spin here
    paused_flag: AtomicBool,      // set by RT thread when actually paused
    last_value: AtomicU64,        // f64::to_bits(), lock-free non-RT read
    queue: Arc<SpscQueue<ProbeFrame, 64>>,
}

struct ProbeFrame {
    value_bits: u64,              // f64::to_bits() — lock-free copy
    timestamp_us: u64,            // monotonic clock, microseconds since engine start
    block_index: u64,
}
```

**Execution logic** for `ProbePoint { id, src, dst }`:

1. `regs[dst] = regs[src]` — pass-through copy
2. If `!slot.enabled.load(Acquire)` → return immediately (zero overhead when probe off)
3. `slot.last_value.store(value)` — atomic write for non-RT inspection
4. `slot.queue.try_push(ProbeFrame { value_bits: value.to_bits(), timestamp_us: now_us, block_index })` — non-blocking
5. If `slot.break_flag.load(Acquire)`:
   - `slot.paused_flag.store(true, Release)`
   - Spin on `slot.break_flag.load(Acquire) && !global_resume.load(Acquire)` with `core::hint::spin_loop()`
   - `slot.paused_flag.store(false, Release)`

#### Global Pause

A single `AtomicBool` in `RillGraphEngine`, checked **between Steps** (not inside ProbePoint). Controlled by the debugger via mpsc channel:

```rust
struct DebugControl {
    global_pause: Arc<AtomicBool>,
    global_resume: Arc<AtomicBool>,
}
```

- `global_pause = true` → engine blocks before executing the next Step
- `global_resume = true` → engine resumes all ProbePoint spin-loops + inter-step blocking

#### Command Probe (Actor Mailbox Tracing)

In addition to signal-level `ProbePoint`, the engine needs to trace actor commands arriving at `RillGraphEngine`. This is critical for debugging parameter routing: which `CommandEnum` variants arrive, from whom, when, and whether they are actually applied.

**Hook point:** `RillGraphEngine::process()` calls `actor.drain()` at the start of each frame, which applies queued `CommandEnum::SetParameter` (and other variants). Under the `debug` feature, each drained command is pushed to a dedicated `CommandLog` SpscQueue.

```rust
// rill-lang/src/graph_engine.rs
#[cfg(feature = "debug")]
struct CommandLog {
    enabled: AtomicBool,
    queue: Arc<SpscQueue<CommandFrame, 256>>,  // larger queue — commands can burst
}

struct CommandFrame {
    block_index: u64,
    timestamp_us: u64,
    command_kind: CommandKind,  // SetParameter, ClockTick, etc.
    node_name: String,           // target node (from ScheduledGraph.program_names)
    param_name: Option<String>,  // parameter name
    value_repr: String,          // human-readable value representation
    raw_bytes: Option<[u8; 32]>, // raw bytes for binary params
}

enum CommandKind {
    SetParameter,
    ClockTick,
    Automaton,
    Unknown(u8),
}
```

**Execution logic** (`actor.drain()` with debug):

```
for cmd in actor.drain() {
    match &cmd {
        CommandEnum::SetParameter { index, value, .. } => {
            // apply parameter normally
            programs[index].apply_param(value.clone());
            // debug: push to command log
            if command_log.enabled.load(Acquire) {
                command_log.queue.try_push(CommandFrame {
                    block_index,
                    timestamp_us: now_us,
                    command_kind: CommandKind::SetParameter,
                    node_name: program_names[index].clone(),
                    param_name: Some(param_defs[index].name.clone()),
                    value_repr: format_param_value(&value),
                    raw_bytes: value.as_bytes(),
                });
            }
        }
        // ... other variants similarly wrapped ...
    }
}
```

This is **not** an IR instruction — it's a debug hook embedded in the engine's `drain()` loop. The command log is a single SpscQueue per engine, not per probe point. It captures timing (block_index, timestamp_us), target (node_name, param_name), and payload (value_repr, raw_bytes).

**DSL integration:** The `trace("node_name")` construct enables command tracing for a specific node. When enabled, all commands targeting that node are logged alongside signal probes.

#### Builtin: `__probe`

```rust
// rill-lang/src/builtin.rs — internal, not exposed to user DSL
__probe(id: u32, signal: T) -> T
```

Compiles to `Ir::ProbePoint`. Users never call `__probe` directly — it is emitted by `rill-telemetry`'s `DebugRegistry` when lowering `watch()`, `assert()`, etc.

#### Feature Gate

```toml
# rill-lang/Cargo.toml
[features]
debug = []
```

When `debug` is **off**:
- `Instr::ProbePoint` variant does not exist in the enum
- `RillGraphEngine` has no `probe_buffers` field
- `__probe` builtin is not registered
- Zero runtime overhead, zero dead code

### 2. rill-telemetry: Diagnostic Infrastructure

#### Crate Structure

```
rill-telemetry/src/
├── lib.rs               # re-exports, feature gates
├── probe.rs             # TelemetryProbe (existing — unchanged)
├── collector.rs         # TelemetryCollector (existing — unchanged)
├── debug/
│   ├── mod.rs
│   ├── registry.rs      # DebugRegistry: lowers watch/assert/trace → ProbePoint
│   ├── state.rs         # ProbeState, ProbeStateManager
│   ├── collector_thread.rs  # CollectorThread: drains SpscQueue, handles breakpoints
│   ├── protocol.rs      # AnalyzerCommand, AnalyzerResponse, AnalyzerConfig
│   └── formatter/
│       ├── mod.rs
│       ├── text.rs      # human-readable (colored tables, ASCII)
│       └── json.rs      # structured JSON lines
```

`debug/` module is gated behind `rill-lang`'s `debug` feature.

#### DebugRegistry

```rust
// rill-telemetry/src/debug/registry.rs
pub struct DebugRegistry {
    inner: Registry,                     // standard rill_lang::Registry
    next_probe_id: AtomicU32,
    probes: Arc<DashMap<ProbeId, ProbeState>>,
}

impl DebugRegistry {
    pub fn new(base: Registry) -> Self;

    /// Register high-level debug functions in the DSL namespace:
    ///   watch(signal)
    ///   assert(signal, condition)
    ///   trace("name")
    ///   signal_break(signal, condition?)
    /// Each compiles to one or more __probe() calls with appropriate flags
    pub fn register_debug_builtins(&self);
}
```

Each language construct lowers to ProbePoint IR with specific ProbeState configuration:

| DSL Construct | ProbeState Flags |
|---|---|
| `watch(signal)` | `enabled=true`, `breakpoint=false` |
| `assert(signal, "!is_nan()")` | `enabled=true`, `breakpoint=true`, `condition=Some("!is_nan()")` |
| `trace("node_name")` | `enabled=true`, `breakpoint=false`, emits frame-begin/end events |
| `signal_break(signal)` | `enabled=true`, `breakpoint=true` |
| `signal_break(signal, "peak > 0.99")` | `enabled=true`, `breakpoint=true`, `condition=Some("peak > 0.99")` |

#### ProbeState

```rust
// rill-telemetry/src/debug/state.rs
pub struct ProbeState {
    pub name: String,                        // e.g. "osc1_output", "mixer/master"
    pub enabled: bool,
    pub breakpoint: bool,
    pub condition: Option<String>,           // conditional break expression
    pub watch_log: VecDeque<ProbeFrame>,     // last N frames in session memory
}

pub struct ProbeStateManager {
    probes: Arc<DashMap<ProbeId, ProbeState>>,
    probe_buffers: Arc<Vec<ProbeSlot<f64>>>,  // reference to engine's buffers
    global_pause: Arc<AtomicBool>,
    global_resume: Arc<AtomicBool>,
}

impl ProbeStateManager {
    pub fn set_breakpoint(&self, probe_id: ProbeId, condition: Option<&str>);
    pub fn clear_breakpoint(&self, probe_id: ProbeId);
    pub fn enable_probe(&self, probe_id: ProbeId);
    pub fn disable_probe(&self, probe_id: ProbeId);
    pub fn get_last_value(&self, probe_id: ProbeId) -> Option<f64>;
    pub fn pause(&self);
    pub fn resume(&self);
    pub fn step(&self, count: u32);          // resume for N frames, then pause
    pub fn list_all(&self) -> Vec<(ProbeId, String, bool, bool)>;
}
```

#### CollectorThread

```rust
// rill-telemetry/src/debug/collector_thread.rs
pub struct CollectorThread {
    handle: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl CollectorThread {
    pub fn spawn(
        config: AnalyzerConfig,
        probes: Arc<DashMap<ProbeId, ProbeState>>,
        signal_queues: Vec<Arc<SpscQueue<ProbeFrame, 64>>>,
        command_queue: Option<Arc<SpscQueue<CommandFrame, 256>>>,
        global_pause: Arc<AtomicBool>,
        global_resume: Arc<AtomicBool>,
        cmd_rx: mpsc::Receiver<AnalyzerCommand>,
        resp_tx: mpsc::Sender<AnalyzerResponse>,
    ) -> Self;
}
```

The collector thread has a main loop:

1. **Poll** all signal probe SpscQueues for new frames
2. **Poll** the command log queue (if present) for new `CommandFrame`s
3. **For each signal frame:** check if the probe has a conditional breakpoint; if condition is met → set `global_pause`
4. **Format** frame data according to `AnalyzerConfig` (text or JSON)
5. **Output** to stdout and/or log file
6. **Process** commands from `cmd_rx` (set/clear breakpoints, step, continue, etc.)
7. **Send** responses via `resp_tx`

#### Analyzer Protocol

```rust
// rill-telemetry/src/debug/protocol.rs
pub enum AnalyzerCommand {
    SetBreakpoint { probe_id: ProbeId, condition: Option<String> },
    ClearBreakpoint { probe_id: ProbeId },
    Continue,
    Step { count: u32 },
    GetProbeValue { probe_id: ProbeId },
    GetProbeValues { probe_id: ProbeId, count: usize },
    ListNodes,
    ListProbes,
    ListBuffers,
    ListParams,
    GetSchedule,
    SetParam { node: String, param: String, value: f64 },
    ExamineBuffer { buffer: usize },
    EnableProbe { probe_id: ProbeId },
    DisableProbe { probe_id: ProbeId },
    CheckNan,
    CheckClip,
    CheckDc,
    SaveState { path: PathBuf },
    RestoreState { path: PathBuf },
    Quit,
}

pub enum AnalyzerResponse {
    Ok { message: String },
    ProbeValue { probe_id: ProbeId, name: String, value: f64, frame: u64 },
    ProbeValues { probe_id: ProbeId, values: Vec<ProbeFrame> },
    Nodes { nodes: Vec<NodeInfo> },
    Probes { probes: Vec<ProbeInfo> },
    Buffers { buffers: Vec<BufferInfo> },
    Params { params: Vec<ParamInfo> },
    Disassembly { instructions: Vec<String> },
    BufferDump { data: Vec<f64> },
    State { data: HashMap<String, serde_json::Value> },
    CheckResult { probe: String, status: String, details: String },
    Error { message: String },
    Paused { probe: Option<String>, reason: String },
}

pub struct AnalyzerConfig {
    pub output: OutputMode,
    pub log_file: Option<PathBuf>,
    pub interactive: bool,
}

pub enum OutputMode {
    Text,
    Json,
}
```

#### Formatters

**Text formatter** (`colored` crate):
- Probe values: `[frame 42] osc1_output = 0.8472`
- Breakpoint hit: red bold `BREAK: lofi_chip peak=1.23 (clipping!)`
- Tables: `info nodes` prints aligned table with borders
- Check results: green `OK` / red `FAIL`

**JSON formatter:**
- Each event is one JSON line (JSONL)
- Fields: `{"type":"probe","probe":"osc1","value":0.847,"frame":42}`
- Same information, machine-parseable

### 3. rill-analyzer: REPL and CLI

#### Crate Structure

```
rill-analyzer/
├── Cargo.toml
├── src/
│   ├── main.rs            # CLI entry point (clap)
│   ├── lib.rs             # Library API
│   ├── repl/
│   │   ├── mod.rs
│   │   ├── commands.rs    # Command implementations using rill-telemetry API
│   │   ├── parser.rs      # Command line parser (simple prefix matching)
│   │   └── history.rs     # Command history, search
│   ├── lua/
│   │   ├── mod.rs
│   │   ├── bindings.rs    # Lua bindings: set_breakpoint(), continue(), etc.
│   │   └── init.lua       # Built-in Lua init script
│   └── cli.rs             # clap argument definitions
```

#### Dependencies

```toml
[dependencies]
rill-core = { path = "../rill-core" }
rill-lang = { path = "../rill-lang", features = ["debug"] }
rill-graph = { path = "../rill-graph" }
rill-telemetry = { path = "../rill-telemetry" }
parking_lot = "0.12"
dashmap = "6"
thiserror = "2"
mlua = { version = "0.10", features = ["lua54"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
colored = "3"
clap = { version = "4", features = ["derive"] }
```

#### CLI

```bash
# Launch graph with interactive REPL
rill-analyzer run graph.json

# Launch with JSON output (agent-friendly)
rill-analyzer run graph.json --json

# Non-interactive: only log telemetry
rill-analyzer run graph.json --no-repl --log telemetry.log

# Batch mode: run until breakpoint, dump state, exit
rill-analyzer run graph.json --script init.lua

# Post-mortem: analyze a telemetry log file
rill-analyzer analyze telemetry.log
```

#### REPL Commands

**Execution control:**
```
break <probe> [if <cond>]    set breakpoint (optionally conditional)
tbreak <probe> [if <cond>]   temporary breakpoint (fires once)
clear [<probe>]              clear breakpoint(s)
continue [c]                 resume execution
step [s] [<n>]              execute N frames, then pause
next [n]                     execute to next ProbePoint in current Step
finish                       execute to end of current Step
```

**State inspection:**
```
info nodes                   list all graph nodes
info probes                  list all probes with status
info buffers                 buffer pool usage
info params                  all parameters with current values
print <probe> [p]           last value at probe
print <probe> -n <count>     last N values at probe
examine <buffer> [x]         dump buffer contents (first N samples)
watch <probe>                enable continuous log output
unwatch <probe>              disable continuous log output
```

**Code inspection:**
```
disassemble [<node>] [dis]   show IR/schedule for node or entire graph
list <node>                  show DSL source (if available)
schedule                     show complete ScheduledGraph step order
```

**Modification:**
```
set <param> <value>          set parameter (routed via CommandEnum::SetParameter)
enable <probe>               activate probe
disable <probe>              deactivate probe
```

**State and scripts:**
```
save <path>                  save program state for post-mortem
restore <path>               restore program state
source <file>                execute Lua script
history                      show command history
```

**Command tracing (actor mailbox):**
```
trace commands [<node>]      log all incoming actor commands (SetParameter, ClockTick, ...)
untrace commands             stop command tracing
info commands [<node>]       show recent command history: time, node, param, value
break on command <param>     pause when a specific parameter changes
```

**Automatic checks:**
```
check nan [<node>]           check for NaN on all/specific outputs
check clip [<node>]          check for clipping (|value| > 1.0)
check dc [<node>]            check for DC offset
```

Command parser uses prefix matching: `b` = `break`, `c` = `continue`, `p` = `print`, `s` = `step`, `x` = `examine`, `dis` = `disassemble`, `q` = `quit`.

#### Lua Scripting

Lua is the scripting language. The REPL itself runs on `mlua` (embedded Lua 5.4). All commands are exposed as Lua functions:

```lua
-- .rill-analyzer.lua — auto-loaded on startup
set_breakpoint("lofi_chip", "peak > 0.99")
continue()

while halted() do
    local val = get_value("lofi_chip")
    if val > 0.5 then
        print(string.format("WARNING: chip_out = %.4f", val))
    end
end
```

**Lua API surface:**

```lua
-- Execution
set_breakpoint(probe, condition?)   -- break <probe> [if <cond>]
tbreak(probe, condition?)           -- tbreak
clear_breakpoint(probe?)            -- clear [<probe>]
continue()                          -- continue
step(n?)                            -- step [<n>]
next_()                             -- next (reserved word in Lua)
finish()                            -- finish

-- Inspection
get_value(probe) → number           -- print <probe>
get_values(probe, count) → table    -- print <probe> -n <count>
list_nodes() → table                -- info nodes
list_probes() → table               -- info probes
list_params() → table               -- info params

-- Modification
set_param(node, param, value)       -- set <param> <value>
enable_probe(probe)                 -- enable <probe>
disable_probe(probe)                -- disable <probe>

-- State
halted() → bool                     -- true if engine is paused
save_state(path)                    -- save <path>
restore_state(path)                 -- restore <path>

-- Checks
check_nan(node?) → table            -- check nan [<node>]
check_clip(node?) → table           -- check clip [<node>]
check_dc(node?) → table             -- check dc [<node>]
```

**Script autoloading:** On startup, `rill-analyzer` looks for `.rill-analyzer.lua` in the current directory and executes it. Command-line flag `--script <file>` runs a specific script in batch mode (no REPL).

#### Example Session

```
$ rill-analyzer run chiptune_stc.json
[rill-analyzer 0.1] loaded: 3 nodes, 2 connections
(rla) info nodes
  #0  "lofi_chip"     Processor    in:1  out:1
  #1  "stc_player"    Custom       in:0  out:0
  #2  "output"        Sink         in:1  out:0
(rla) trace commands
Tracing all actor commands (Ctrl+C to stop)
  frame=1    SetParameter  lofi_chip.register_write  Bytes([0xFF, 0x07, ...])
  frame=1    SetParameter  lofi_chip.register_write  Bytes([0x8E, 0x00, ...])
  frame=1    ClockTick     stc_player                (no params)
  frame=2    SetParameter  lofi_chip.register_write  Bytes([0xFF, 0x07, ...])
  ...
(rla) untrace commands
Command tracing stopped.
(rla) b lofi_chip
Breakpoint set on probe "lofi_chip"
(rla) c
[rill-analyzer] running... (Ctrl+C to interrupt)
BREAK: lofi_chip  value=0.042  frame=1
(rla) p lofi_chip
lofi_chip = 0.042
(rla) w lofi_chip
Watching lofi_chip (Ctrl+C to stop)
  frame=2    value=0.038
  frame=3    value=-0.051
  frame=4    value=0.067
  ...
(rla) check clip
  lofi_chip:  OK   (max=0.89)
  output:     CLIP (max=1.23, 12 frames)
(rla) q
```

### 4. Integration with Existing Infrastructure

#### rill-telemetry (existing)

The existing `TelemetryProbe` / `TelemetryCollector` remain unchanged. They provide passive, block-level monitoring (peak, RMS, DC offset). The new debug infrastructure in `rill-telemetry` is complementary:

- **Passive monitoring** (existing): insert `TelemetryProbe` in a signal chain, collect periodic metrics
- **Active debugging** (new): set breakpoints on ProbePoints, inspect values, step through frames

Both use `SpscQueue<TelemetryBlock>` as the data transport, making them composable. A `TelemetryProbe`'s output can be fed into the collector thread's formatting pipeline.

#### rill-graph

No changes to `rill-graph`. `GraphBuilder::build_ir()` produces `GraphIr` as before. Debug instrumentation is added during compilation (via `DebugRegistry`), not during graph building.

#### rill-adrift

`rill-adrift` re-exports `rill-telemetry` behind `cfg(feature = "telemetry")`. The new debug infrastructure should be re-exported similarly:

```rust
#[cfg(feature = "telemetry")]
pub mod telemetry {
    pub use rill_telemetry::*;
}
#[cfg(feature = "telemetry")]
pub mod debug {
    pub use rill_telemetry::debug::*;
}
```

### 5. Usage Modes

#### Mode 1: Embedded (library)

Host applications (e.g., drift) embed the infrastructure directly:

```rust
let engine = RillGraphEngine::new(scheduled_graph, ...);

// Option A: just telemetry, no REPL
let collector = rill_telemetry::debug::CollectorThread::spawn(
    AnalyzerConfig { interactive: false, log_file: Some("telemetry.log".into()), .. },
    probes, queues, pause, resume, cmd_rx, resp_tx,
);

// Option B: full interactive debugger
let analyzer = rill_analyzer::Analyzer::launch(
    AnalyzerConfig { interactive: true, .. },
    probes, probe_buffers, global_pause, global_resume,
);
analyzer.wait(); // blocks until REPL exits
```

#### Mode 2: Standalone CLI

```bash
rill-analyzer run graph.json              # interactive
rill-analyzer run graph.json --no-repl --log t.log  # log only
rill-analyzer run graph.json --script init.lua       # scripted
rill-analyzer analyze telemetry.log                   # post-mortem
```

#### Mode 3: Headless/Production

`rill-lang` compiled **without** `debug` feature. `rill-telemetry` is not linked. Zero overhead. No ProbePoint in IR, no probe buffers, no collector thread.

### 6. Performance and Safety

**RT-thread safety:**
- All probe operations are lock-free: `AtomicBool`, `AtomicCell`, `SpscQueue::try_push`
- Breakpoint spin-loop uses `core::hint::spin_loop()` — no blocking syscalls
- No allocations in the hot path
- Probe slot check is one `AtomicBool::load(Acquire)` — ~1ns when probe is disabled

**Collector thread:**
- Non-real-time; can allocate, format strings, write to files
- Drains SpscQueue without blocking RT thread
- Handles `mlua` execution (Lua scripts run on collector thread, not RT)

**When `debug` feature is off:**
- `Instr::ProbePoint` does not exist — enum is 15 variants instead of 16
- `RillGraphEngine` has zero debug-related fields
- No dead code, no runtime checks, no overhead
- Binary size unaffected

### 7. Implementation Plan

| Phase | What | Crate |
|---|---|---|
| 1 | `ProbePoint` IR, `ProbeSlot`, `global_pause`, `debug` feature | `rill-lang` |
| 2 | `DebugRegistry` (watch/assert/trace/signal_break → ProbePoint lowering) | `rill-telemetry` |
| 3 | `ProbeState`, `ProbeStateManager` | `rill-telemetry` |
| 4 | `CollectorThread` (SpscQueue → formatter → stdout/file) + `AnalyzerConfig` | `rill-telemetry` |
| 5 | `AnalyzerCommand`, `AnalyzerResponse`, protocol types | `rill-telemetry` |
| 6 | Formatters: text (colored tables) + JSON lines | `rill-telemetry` |
| 7 | REPL: command parser, history, command dispatch | `rill-analyzer` |
| 8 | Lua integration: `mlua` bindings, `.rill-analyzer.lua` autoload | `rill-analyzer` |
| 9 | CLI binary: `clap`, GraphDef loading, launch | `rill-analyzer` |
| 10 | `check nan/clip/dc` + post-mortem log analysis | `rill-analyzer` |

Phases 1-6 are mandatory for any diagnostic output. Phases 7-10 are incremental UX layers.

### 8. Open Questions

- **Conditional break expression language:** Currently described as string conditions (`"peak > 0.99"`, `"!is_nan()"`). Need a proper expression evaluator — consider embedding `meval` or a hand-written simple expression parser limited to arithmetic + comparisons + `is_nan()` / `is_infinite()`.
- **Save/restore semantics:** What state is saved? All program state, delays, and registers? How does this interact with the actor system (which has its own mailboxes)?
- **Node naming in probes:** How to map ProbePoint IDs to human-readable names when multiple programs run in the same engine? The `ScheduledGraph` has `program_names` — probe names should be derived from program names + signal names.
- **Integration with `MicroControlObserver`:** The patchbay observer already tracks timing violations. Should the debug infrastructure consume those events?
