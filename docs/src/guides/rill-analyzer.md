# rill-analyzer — Interactive Debugger

`rill-analyzer` is a gdb-style interactive debugger for Rill signal processing applications. It connects to running processes via shared memory, inspects signal values at probe points, traces parameter changes, and controls execution (pause, step, continue).

## Installation

`rill-analyzer` is built as part of the workspace:

```bash
cargo build --release -p rill-analyzer
```

The binary supports three operating modes:

| Mode | Command | Use case |
|------|---------|----------|
| **Local** | `rill-analyzer run <graph.json>` | Run a graph locally with embedded debugger |
| **Attach** | `rill-analyzer attach <pid>` | Connect to a running rill process |
| **Launch** | `rill-analyzer launch <target>` | Start a process and connect immediately |

## Local Mode

```bash
rill-analyzer run graph.json
```

Loads a serialized graph, creates a `RillGraphEngine`, and opens an interactive REPL. The debugger runs in the same process — signals, commands, and probe data all flow through inter-thread channels.

Options:
- `--no-repl` — only log telemetry, no interactive prompt
- `--json` — machine-parseable JSON output
- `--log <file>` — write telemetry to a log file
- `--script <file>` — execute a Lua script in batch mode

## Attach Mode

```bash
rill-analyzer attach 12345
```

Connects to PID 12345 through the shared memory region at `/dev/shm/rill-debug-12345`. The target process must be compiled with `--features debug` and have created the shmem region (automatic with `ModularSystem::launch()` or manual via `rill_adrift::debug_init::init_shmem()`).

Attach flow:
1. Opens the shmem region
2. Verifies the magic number (`RILL`) and version
3. Registers as debugger (writes its PID to the control header)
4. Opens a REPL — all commands and responses go through lock-free ring buffers

## Launch Mode

```bash
# Serialized graph
rill-analyzer launch graph.json

# Rill-lang DSL source
rill-analyzer launch chip.rll

# Arbitrary binary with arguments
rill-analyzer launch ./my-app -- --verbose --port 8080

# Cargo command
rill-analyzer launch -- cargo run --example chiptune_stc -- --file music.stc pipewire
```

When the target ends with `.json`, it's launched via `drift --graph <file>`. When it ends with `.rll`, the source is compiled and launched via drift. Otherwise, the target is executed directly with `RILL_DEBUG_SHMEM` in the environment.

## REPL Commands

The REPL uses prefix-matching — `b` for `break`, `c` for `continue`, `p` for `print`, etc.

### Execution Control

| Command | Shortcut | Description |
|---------|----------|-------------|
| `break <probe>` | `b` | Set a breakpoint at the given probe |
| `clear [<probe>]` | — | Clear breakpoint(s) |
| `continue` | `c` | Resume execution |
| `step [<n>]` | `s` | Execute N blocks, then pause |
| `pause` | — | Pause the engine |
| `quit` | `q` | Exit the debugger |

### Inspection

| Command | Shortcut | Description |
|---------|----------|-------------|
| `info nodes` | `i nodes` | List all graph nodes with arity |
| `info probes` | `i probes` | List all probes with status (ON/OFF/BREAK) and last value |
| `print <probe>` | `p` | Show the last value of a specific probe |
| `watch <probe>` | `w` | Enable continuous probe output |
| `unwatch <probe>` | — | Disable continuous probe output |

### Command Tracing

| Command | Description |
|---------|-------------|
| `trace commands` | Enable command logging (shows all SetParameter, ClockTick) |
| `untrace commands` | Disable command logging |

### Control-Path Inspection

| Command | Description |
|---------|-------------|
| `info automata` | List all registered automatons (servos) |
| `info sensors` | List all registered sensors (MIDI, OSC) |
| `info queues` | Show queue statistics (capacity, fill level) |

## Example Session

```
$ rill-analyzer launch chiptune_stc -- --file music.stc --no-wait pipewire
[rill-analyzer 0.1] launched PID 118258 (shmem: /dev/shm/rill-debug-118258)

(rla) info nodes
  #0    node_0            in:0 out:1

(rla) b node_0
  Breakpoint set on probe 'node_0'

(rla) c
  [rill-analyzer] running...

(rla) p node_0
  node_0 = 0.4000

(rla) w node_0
  [block 20419] probe[0] node_0 = 0.2888
  [block 20420] probe[0] node_0 = 0.1777
  [block 20421] probe[0] node_0 = 0
  ...

(rla) q
```

## Lua Scripting

`rill-analyzer` embeds Lua 5.4 via `mlua`. All REPL commands are exposed as Lua functions:

```lua
-- .rill-analyzer.lua — auto-loaded on startup
set_breakpoint("node_0")
continue()

while true do
    local val = get_value("node_0")
    if val > 0.9 then
        print(string.format("CLIPPING: %.4f", val))
    end
    step(1)
end
```

**Available Lua functions:**

| Function | REPL equivalent |
|----------|-----------------|
| `set_breakpoint(probe)` | `break <probe>` |
| `clear_breakpoint(probe)` | `clear <probe>` |
| `continue_()` | `continue` |
| `step(n)` | `step [<n>]` |
| `pause()` | `pause` |
| `get_value(probe)` | `print <probe>` |
| `list_probes()` | `info probes` |
| `list_nodes()` | `info nodes` |

The `.rill-analyzer.lua` file in the current directory is auto-loaded on startup. Use `--script <file>` for explicit script execution.

## JSON Output Mode

When `--json` is specified, all output is formatted as JSON lines (one object per line):

```json
{"type":"probe","probe":"node_0","value":0.4000,"frame":20418}
{"type":"command","frame":20417,"kind":"SetParameter","node":"","param":"register_write","value":"Bytes([112,4,0,0,124,...])"}
{"type":"break","probe":"node_0","value":0.4000,"frame":20418}
```

This mode is designed for agent-based automation and scripting — each line is a complete, self-contained JSON object.

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│ RILL PROCESS (debug feature enabled)                     │
│                                                          │
│  RT THREAD           SpscQueue        COLLECTOR THREAD   │
│  Engine.process() ─────────────────→  drains probes      │
│  probe capture                        drains commands    │
│  command logging                       ↕ shmem ring buf  │
│  debug_control ←──── atomics ──────→  resp → debugger    │
│                                     cmd ← debugger       │
└─────────────────────────────────────┬────────────────────┘
                                      │ /dev/shm/rill-debug-<pid>
┌─────────────────────────────────────│────────────────────┐
│ rill-analyzer                       │                    │
│  REPL → stdin → cmd_tx ─────────────┘                    │
│  stdout ← formatter ← resp_rx                            │
└──────────────────────────────────────────────────────────┘
```
