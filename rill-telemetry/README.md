# rill-telemetry

Real-time telemetry probes and collectors for monitoring signal processing.
Provides both passive monitoring (peak, RMS, DC offset) and active debugging
infrastructure (probes, command logging, breakpoints, IPC).

## Key components

- **`TelemetryProbe`** — signal stream monitoring probe (RMS, peak, envelope)
- **`TelemetryCollector`** — data collector for telemetry events
- **Real-time safety** — violation tracking and micro-control observation
- **Queue integration** — non-blocking telemetry queue for feedback to external systems

### Debug infrastructure (`debug` feature)

- **`CollectorThread`** — background thread draining probe queues, command logs,
  and inspecting breakpoint hits. Formats output via `TextFormatter` (colored
  terminal) or `JsonFormatter` (JSON lines).
- **`ProbeStateManager`** — handles debugger commands: set/clear breakpoints,
  continue, step, pause, enable/disable probes.
- **`ShmemRegion`** — shared memory IPC at `/dev/shm/rill-debug-<pid>` with
  two lock-free ring buffers for `AnalyzerCommand`/`AnalyzerResponse`
  serialization via `serde_cbor`. Supports `rill-analyzer attach` and
  `rill-analyzer launch`.
- **`PatchbayInspector`** — control-path state inspection: automaton snapshots,
  sensor status, queue statistics. Wire into ModularSystem via `debug_init`.

## Dependencies

- `rill-core` — `Transcendental`, `SpscQueue`, `TelemetryBlock`
- `rill-lang` — `ProbeSlot`, `DebugControl`, `ProbeFrame`, `CommandFrame`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-telemetry>
