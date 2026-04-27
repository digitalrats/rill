# rill-core

Unified core of the Rill ecosystem — base traits, math, buffers, queues, time, macros, and executor.

## Key components

- **traits** — `AudioNode`, `ParameterId`, `PortId`, `Clock`, `Source`/`Processor`/`Sink`
- **math** — `AudioNum` trait for audio-specific numeric operations (dB, phase, MIDI)
- **buffer** — `PipeBuffer`, `FanOutBuffer`, `FanInBuffer`, `RingBuffer`, `DelayLine`
- **queues** — non-blocking `CommandQueue<T>`, `TelemetryQueue`, `MicroControlObserver`
- **time** — `ClockTick`, `SystemClock`, tempo and beat tracking
- **macros** — `processor_node!`, `source_node!`, `sink_node!`, `with_parameters!`
- **error** — `AudioError`, typed error system for audio applications

## Links

- Repository: <https://github.com/DigitalRats/rill>
- Documentation: <https://docs.rs/rill-core>
