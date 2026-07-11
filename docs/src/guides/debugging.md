# Debugging Rill Applications

Rill provides a two-level diagnostic infrastructure: **runtime telemetry** (signal probes, command logging) and an **interactive debugger** (`rill-analyzer`). Both are gated behind the `debug` Cargo feature — zero overhead in production builds.

## Architecture

```
┌─────────────────────────────────────────────────┐
│ rill-lang (core, feature = "debug")             │
│  ProbePoint IR  +  ProbeSlot  +  DebugControl   │
│  1 new Instr variant, zero-cost when disabled    │
└──────────────┬──────────────────────────────────┘
               │ depends on
┌──────────────▼──────────────────────────────────┐
│ rill-telemetry (diagnostic infrastructure)       │
│  ProbeStateManager  +  CollectorThread           │
│  CommandFormatter  +  ShmemRegion (IPC)          │
└──────────────┬──────────────────────────────────┘
               │ depends on
┌──────────────▼──────────────────────────────────┐
│ rill-analyzer (CLI + REPL)                      │
│  gdb-style interactive debugger                 │
│  Lua scripting, JSON output                     │
│  attach/launch via shared memory                │
└─────────────────────────────────────────────────┘
```

All diagnostic data flows from the signal thread (RT) through lock-free SPSC queues to a collector thread (non-RT), which formats and outputs events. No allocations, no locks, no syscalls in the signal path.

## Enabling Debugging

Add the `debug` feature to your Cargo features:

```bash
cargo build --features "debug"
```

For ModularSystem-based applications (using `rill-adrift`):

```bash
cargo run --example chiptune_stc --features "lofi,pipewire,io,debug" -- --file music.stc pipewire
```

The `debug` feature activates `rill-lang/debug`, `rill-graph/debug`, `rill-telemetry/debug`, and `rill-patchbay/debug`.

## Signal Probes

### How Probes Work

Each graph node gets an automatic probe at its output. The probe captures the first sample of every processed block and pushes it to a lock-free SPSC queue. A collector thread drains the queue and formats the output.

Probes are identified by the node name (`node_0`, `node_1`, etc.) and report both the block index and the signal value:

```
[block 1] probe[0] node_0 = 0
[block 2] probe[0] node_0 = 0
...
[block 20418] probe[0] node_0 = 0.4000
[block 20419] probe[0] node_0 = 0.2888
```

### Probe Lifecycle

1. `build_ir()` inserts a `ProbePoint` IR instruction after the node's `CallBlock`
2. The engine allocates `ProbeSlot`s — one per node — each with atomic flags (`enabled`, `break_flag`, `paused_flag`) and an SPSC queue
3. During processing, the engine captures the output buffer's first sample and pushes a `ProbeFrame { value_bits, block_index }` into the queue
4. `CollectorThread` drains the queue and formats the event via `TextFormatter` (colored terminal) or `JsonFormatter` (JSON lines)

### Enabling Specific Probes

Probes are auto-enabled in `ModularSystem::launch()` for each graph node. To enable/disable individual probes, use `rill-analyzer`:

```
(rla) enable <probe_id>
(rla) disable <probe_id>
```

## Command Logging

Every `SetParameter` command that successfully routes to a program parameter is logged. This lets you trace who changes what parameter and when:

```
[block 17] cmd SetParameter →  register_write: Bytes([112, 4, 0, 0, 124, ...])
[block 33] cmd SetParameter →  register_write: Bytes([112, 4, 0, 0, 124, ...])
```

After the `KeyFrame` API is enabled with `debug`, the log output includes:
- **block_index** — which processing block received the command
- **command_kind** — `SetParameter`, `ClockTick`, etc.
- **param_name** — the parameter being modified
- **value_repr** — human-readable value representation

Commands that fail to route (parameter name not found, node not found) are **silently ignored** and do not appear in the command log. This makes the log a reliable indicator of successful parameter application.

## Pause and Resume

The debugger can pause the engine between processing blocks. The engine spins on an `AtomicBool` — no syscalls, no locks:

```rust
// Spin if paused, until resume
while self.debug_control.global_pause.load(Acquire)
    && !self.debug_control.global_resume.load(Acquire)
{
    std::hint::spin_loop();
}
```

The collector thread monitors `FLAG_PAUSED` in the shared memory region and calls `debug_control.pause()` / `debug_control.cont()` accordingly.

## Inter-Process Debugging via Shared Memory

For debugging a running process, `rill-analyzer` uses a shared memory region at `/dev/shm/rill-debug-<pid>`. The region contains:

```
Offset  Size    Field
─────────────────────────────────────
0       4       magic (0x52494C4C = "RILL")
4       4       version
8       8       process_pid
16      8       debugger_pid
24      4       flags (PAUSED | ATTACHED | SHUTDOWN)
28-64   …       ring buffer positions
64      ~32KB   CmdRingBuffer  (debugger → process)
~32KB   ~32KB   RespRingBuffer (process → debugger)
```

Each ring buffer is a lock-free SPSC circular buffer. Frames are serialized with `serde_cbor`. The debugger sends `AnalyzerCommand` through `CmdRingBuffer`, the process responds with `AnalyzerResponse` through `RespRingBuffer`.

**Signal protocol:** Only the debugger sends `SIGUSR1` to the rill process. The process never sends signals to the debugger — responses are read via polling.

### Attach Mode

```bash
rill-analyzer attach 12345
```

1. Opens `/dev/shm/rill-debug-12345`
2. Verifies magic and version
3. Registers as debugger (writes its PID)
4. Enters REPL — commands go through the shmem ring buffer

### Launch Mode

```bash
rill-analyzer launch ./my-app -- --flag value
```

1. Creates shmem region
2. Forks and executes the target with `RILL_DEBUG_SHMEM` in the environment
3. Child process opens the shmem and sets `FLAG_ATTACHED`
4. Parent waits for the flag, then enters REPL

If the target ends with `.json`, it's treated as a serialized graph and launched via `drift --graph`. If it ends with `.rll`, it's a rill-lang DSL source — compiled and launched via drift.

## Lifecycle Logging

When `debug` feature is enabled, `ModularSystem::launch()` adds lifecycle logging via the `log` crate:

```
rill-adrift: launching rack 'chiptune_stc' — 1 nodes, 1 modules
rill-adrift: rack 'chiptune_stc' engine built — 1 programs
rill-adrift: rack 'chiptune_stc' backend 'pipewire' started
rill-adrift: system launched with 1 rack(s)
rill-adrift: stopping system
```

Use `RUST_LOG=info` to see these logs, or integrate with your preferred logger implementation.

## RT Safety

All diagnostic data transport uses lock-free atomics and SPSC queues. The signal thread (RT) never allocates, locks, or blocks. The collector thread (non-RT) handles formatting, I/O, and IPC.

**Forbidden in the RT path:** `log::info!`, `eprintln!`, `println!`, any file or socket I/O. The only permitted path for RT diagnostics is pushing data through SPSC queues and atomics.

## Patchbay Inspector

Beyond signal probes, the debug infrastructure can inspect control-path state:

```rust
// Automaton state (via rill-analyzer)
(rla) info automata

// Sensor status (MIDI, OSC)
(rla) info sensors
```

The `PatchbayInspector` collects snapshots of Servo automaton state (enabled, value, time) and Sensor status (connected, event count) through `DashMap`-backed registries.
