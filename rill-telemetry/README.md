# rill-telemetry

Real-time telemetry probes and collectors for monitoring audio processing.

## Key components

- **`TelemetryProbe`** — audio stream monitoring probe (RMS, peak, envelope)
- **`TelemetryCollector`** — data collector for telemetry events
- **Real-time safety** — violation tracking and micro-control observation
- **Queue integration** — non-blocking telemetry queue for feedback to external systems

## Dependencies

- `rill-core` — `AudioNode`, `Processor` trait, `CommandQueue`, `TelemetryQueue`

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-telemetry>
