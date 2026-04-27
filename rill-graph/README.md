# rill-graph

Real-time audio graph with block processing and topological sort.

## Key components

- **`AudioGraph`** — immutable DAG container, topology is fixed at build time
- **`GraphBuilder`** — the only way to build a graph (`Source` → `Processor` → `Sink`)
- **Kahn's algorithm** — topological sort with cycle detection
- **Port routing** — connections and feedback buffers stored on ports
- **Copy-based routing** — buffers copied between ports (zero-copy planned)
- **Feedback support** — deferred feedback via `feedback_buffer`
- **Port types** — `Audio`, `Control`, `Clock`, `Feedback`, `Param`

## Dependencies

- `rill-core` — `AudioNode`, `Source`/`Processor`/`Sink` traits, `ClockTick`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-graph>
