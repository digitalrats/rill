# rill-graph

Real-time audio graph with block processing, topological sort, and
the [`AudioEngine`] — a real-time safe graph engine.

## Key components

- **`AudioGraph`** — immutable DAG container, topology is fixed at build time
- **`GraphBuilder`** — the only way to build a graph (`Source` → `Processor` → `Sink`)
- **Kahn's algorithm** — topological sort with cycle detection
- **`AudioEngine<T, BUF_SIZE>`** — drives the graph:
  - `process_tick(tick)` — clock boundary: drains commands (anti-ack on
    overwrite), runs `pre_process` (feedback mix), applies parameter changes
  - `process_block(tick)` — convenience: `process_tick` + topo-order node
    processing + snapshot + propagate
  - `spawn()` — consumes the engine and runs it in a dedicated audio thread
  - `running_flag()` — `Arc<AtomicBool>` for cooperative shutdown
- **Two-thread architecture** — audio thread (hard RT) runs the engine,
  control thread (soft RT) runs `PatchbayManager` via queues
- **Push and pull models** — Source-active (push) and Sink-active (pull)
  are both supported; `process_block` processes topo-order regardless
- **Port routing** — connections and feedback buffers stored on ports
- **Feedback support** — deferred feedback via `port.pre_process` /
  `port.snapshot_feedback`
- **Port types** — `Audio`, `Control`, `Clock`, `Feedback`, `Param`

## Dependencies

- `rill-core` — `AudioNode`, `Source`/`Processor`/`Sink` traits, `ClockTick`,
  `CommandQueue`, `TelemetryQueue`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-graph>
