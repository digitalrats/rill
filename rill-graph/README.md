# rill-graph

Static DAG signal graph — topology and port connections only.
Processing is driven by `Port::propagate` (not an external engine).

## Key components

- **`Graph`** — immutable DAG container, topology is fixed at build time
- **`GraphBuilder`** — the only way to build a graph (`Source` → `Processor` → `Sink`),
  fills `downstream_input_ptrs`, `parent`, `upstream_buffer` for zero-copy routing
- **Kahn's algorithm** — topological sort with cycle detection
- **`Port::propagate`** — recursive signal propagation:
  1. Copy data to downstream input ports (skipped for zero-copy `upstream_buffer` ports)
  2. Run port algorithm (`run_action`)
  3. Call `pre_process` (feedback mix)
  4. Call the downstream node's `process_block` (`generate`/`process`/`consume`)
  5. `snapshot_feedback` on output ports
  6. Recurse through output ports' `downstream_input_ptrs`
- **Zero-copy routing** — 1:1 and fan-out connections read directly from upstream
  output buffer via `upstream_buffer`. Copy only for fan-in and feedback.
- **Hard-RT safe** — no heap allocations, no locks, no syscalls in the
  signal path. All `Port::propagate` data structures are pre-allocated at
  graph construction time (`downstream_nodes`, `downstream_input_ptrs`).
  Communication with the control thread is exclusively through the graph
  actor mailbox (`ActorRef<CommandEnum>`, lock-free `MpscQueue`).
- **SIMD-friendly** — fixed buffer position in memory for the graph's lifetime
- **Port routing** — connections and feedback buffers live on ports
- **Feedback support** — `port.pre_process` / `port.snapshot_feedback`
- **Port types** — `Signal`, `Control`, `Clock`, `Feedback`, `Param`

## Top-level processing entry point

`ProcessingState::process_block(&tick)` drains the graph actor mailbox
(applying `SetParameter` writes — sample-accurate ones, carrying `sample_pos`,
are applied during the 256-sample block that contains their target position),
calls `Source::generate`, then `Port::propagate` to cascade through the DAG.
The graph runs entirely inside the backend process callback and adopts the
sample rate carried by each `ClockTick`.

## Dependencies

- `rill-core` — `Node`, `Source`/`Processor`/`Sink` traits, `ClockTick`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-graph>
