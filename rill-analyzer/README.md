# rill-analyzer

Interactive gdb-style debugger for Rill signal processing applications.

Part of the [Rill](https://github.com/DigitalRats/rill) signal processing ecosystem.

## Overview

`rill-analyzer` connects to running Rill processes via shared memory, inspects
signal values at probe points, traces parameter changes, and controls execution
(pause, step, continue). It supports three operating modes:

- **Local:** `rill-analyzer run graph.json` — embedded debugger in the same process
- **Attach:** `rill-analyzer attach <pid>` — connect to a running process
- **Launch:** `rill-analyzer launch <target>` — start a process and connect

## Quick Start

```bash
cargo build --release -p rill-analyzer

# Launch a debug session
rill-analyzer launch -- cargo run --example chiptune_stc --features "lofi,pipewire,io,debug" -- --file music.stc --no-wait pipewire
```

## REPL Commands

| Command | Shortcut | Description |
|---------|----------|-------------|
| `break <probe>` | `b` | Set breakpoint |
| `continue` | `c` | Resume execution |
| `step [<n>]` | `s` | Step N blocks |
| `print <probe>` | `p` | Show probe value |
| `info nodes` | `i nodes` | List graph nodes |
| `info probes` | `i probes` | List probes with status |
| `quit` | `q` | Exit |

Full command reference: [rill-analyzer Guide](https://rill-adrift.io/guides/rill-analyzer.html)

## License

Apache 2.0 — see [LICENSE.md](../LICENSE.md). Example code is additionally available under MIT.
