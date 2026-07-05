# CHANGELOG

## [0.5.0-beta.7] — In Progress

### ⏱️ Sample-accurate parameter automation (`rill-core`, `rill-graph`, `rill-io`, `rill-patchbay`)

Fixes tick-driven control (sequencers, servos) collapsing under backends that
batch many `block_size` chunks into a single I/O callback (e.g. PipeWire's
12288-frame buffer = 48 × 256 chunks). Previously all parameter writes for a
callback were applied at the first chunk, so the AY chip in `chiptune_stc`
rendered ~4 register states/s instead of ~48.8 in release builds — the melody
dragged. Correct playback on ALSA / debug PipeWire was incidental timing.

- **`SetParameter.sample_pos: Option<u64>`** — optional absolute sample position
  at which a parameter change should take effect. `None` = apply on drain
  (legacy behaviour, unchanged for UI/MIDI-driven writes). Builder:
  `SetParameter::new(...).with_sample_pos(pos)`.
- **`ClockTick.io_quantum: u32`** — frames the backend processes per I/O
  callback (its quantum). Defaults to `samples_since_last`; chunking backends
  set the whole callback size. Builder: `ClockTick::with_io_quantum(n)`.
- **Graph applies parameters per block** — the graph actor now queues writes
  that carry a `sample_pos` and applies each during the 256-sample block whose
  range contains it (`ProcessingState::process_block` / `Graph::process_block`),
  instead of flushing everything at drain time. Writes without `sample_pos`
  still apply immediately (preserves duplex/legacy paths).
- **Producers look ahead by one quantum** — because an asynchronous control
  module reacting to a tick in callback *N* can only be rendered in callback
  *N+1*, producers stamp `sample_pos = tick.sample_pos + tick.io_quantum`. The
  `chiptune_stc` example and the ClockTick-driven `Servo` writes do this;
  MIDI/UI-driven `Servo` writes stay immediate (no latency on live input).
- Cost: ~one I/O quantum of control latency (PipeWire ≈ 278 ms, ALSA ≈ 5.8 ms) —
  acceptable for playback, reducible with a smaller backend buffer.

### 🎛️ Graph adopts the backend's hardware sample rate

The graph has no clock of its own — it runs inside the backend process callback
and now adopts the rate carried by each `ClockTick`.

- **`ProcessingState` re-initialises nodes on rate change** — when the driving
  `ClockTick.sample_rate` differs from the rate the nodes were built with (e.g.
  JACK locked to 48 kHz while the graph was configured for 44.1 kHz), every node
  is re-`init`ed so chip clocks and filter coefficients match the real rate.
- **JACK backend** — the `ClockTick` now carries the *actual* JACK hardware rate
  (was `config_rate`), fixing playback running `hw_rate / config_rate` too fast
  (e.g. +8.8 % at 48 kHz vs 44.1 kHz) with no resampling.
- **PortAudio backend** — request a large DMA buffer
  (`PA_BUFFER_BLOCKS × block_size`, default 16 × 256 = 4096 frames) instead of a
  single 256-frame period, then chunk it back into `block_size` pieces in the
  callback, sending one `ClockTick` per rill block. A single 256-frame period
  was unstable through the PipeWire ALSA plugin (crackling); a large buffer
  fixes stability but, when driven as one tick, starved the sequencer (~6×
  slow). The chunk loop gives the sequencer the correct ~172 ticks/s *and* a
  stable buffer. `PA_BUFFER_BLOCKS` is a documented constant — ~8 blocks is the
  empirical stability floor on common hardware (hence PipeWire's own large
  default); it also sets the control look-ahead latency
  (`block_size × PA_BUFFER_BLOCKS / sample_rate` ≈ 93 ms at 16). The old
  forced-duplex workaround is removed.

### 📦 Version bump and cleanup

- All 18 crates bumped to `0.5.0-beta.7`.
- Documentation updated: `SensorDef::Osc` described in architecture docs,
  `rill-osc` README cross-references `OscSensor`, patchbay README covers
  `midi`/`osc` feature flags, stale `0.5.0-beta.2` references fixed
  throughout docs.

### 🔌 I/O Backend Extraction (`rill-core`, `rill-io`, `rill-graph`)

Major architecture change: backends extracted from graph nodes to the
orchestrator layer. Signal graph is now pure DSP (no I/O knowledge), all
hardware interaction lives in `ProcessingState` + backend traits.

**`rill-core` (`io.rs`):**
- **`IoBackend` → `IoDriver` + `IoCapture` + `IoPlayback`** — single
  monolithic trait split into three orthogonal capabilities. One struct
  can implement any combination: `IoDriver` runs the clock loop,
  `IoCapture` reads input samples, `IoPlayback` writes output samples.
  Mirrors the `MidiInput`/`MidiOutput` split on the audio side.
- **`BufferView`** trait — zero-copy DMA access during I/O callback:
  `read_input(channel, dst)` and `write_output(channel, src)`. Nodes
  hold `Arc<dyn BufferView>` and read/write directly without
  intermediate ring buffers.
- **`ProcessingState`** — new: owns graph runtime parts (actor mailbox,
  node storage, parent rack ref). Created via
  `graph.into_processing_state()`. Wired with backends via
  `wire_backends(capture, playback)`. Drives processing loop:
  `process_block(&ClockTick)` → DSP → `send_clock_tick()`.
- **`ParameterWrite`** trait — polymorphic parameter injection into
  the graph mid-cycle (used by PipeWire per-chunk params).
- **Removed:** `IoNode`, `ActiveNode` traits — backends no longer
  injected into graph nodes.

**`rill-io`:**
- **`DirectView`** — interleaved/planar DMA access via raw pointers,
  implements `BufferView`. Created per-callback by each backend.
  `read_input()`/`write_output()` operate directly on hardware DMA
  buffers — no copies between graph and backend.
- **`OutputWindow`** — adapter for backends that need partial buffer
  writes. Wraps `IoPlayback` + `DirectView`, handles multi-chunk DMA.
- **`ClockTick.is_final`** — flag for chunking backends, gating
  `send_clock_tick()`. Note: current chunking backends (PipeWire, JACK) leave it
  `true` on every chunk, so control modules receive **one `ClockTick` per
  `block_size` block**; sample-accurate placement is handled by
  `SetParameter.sample_pos` + `ClockTick.io_quantum` (see the entry at the top
  of this file), not by coalescing ticks per buffer.
- **PipeWire backend** — major rewrite for chunk processing. DMA buffer
  split into chunks of `block_size`, per-chunk parameter updates via
  `ParameterWrite`. Zero-fill DMA remainder after chunk loop. (Buffer size is
  whatever PipeWire allocates — the backend does not yet negotiate
  `SPA_PARAM_Buffers`.)
- **JACK backend** — chunk processing by `block_size`, uses orchestrator
  `running` flag for shutdown. `run()` returns immediately
  (callback-driven), `stop()` coordinates with JACK thread.
- **PortAudio backend** — unchanged structurally, gains `DirectView`
  + `OutputWindow` for output path.
- **ALSA backend** — unchanged structurally, poll-driven (`snd_pcm_wait`),
  gains same view/window pattern.

**`rill-graph` (`backend_factory.rs`):**
- **`BackendFactory` refactored.** Constructor signature changed:
  `fn(params) -> Box<dyn IoBackend>` → `fn(params) -> (Arc<dyn IoDriver>,
  Option<Arc<dyn IoCapture>>, Option<Arc<dyn IoPlayback>>)`.
- **Bundle types:** `DuplexBundle` (driver + capture + playback),
  `OutputBundle` (driver + playback), `InputBundle` (driver + capture).
- **`create_any()`** — returns whatever capabilities the backend provides.
  Replaces `create() -> Box<dyn IoBackend>`.
- **Caching** — backends cached by name in factory, reused across racks.

**Backend lifecycle (complete):**
```
orchestrator:
  1. factory.create_any(name, params) → (driver, capture, playback)
  2. graph.into_processing_state() → ProcessingState
  3. state.wire_backends(capture, playback)
  4. driver.set_process_callback(|tick| { state.process_block(&tick); })
  5. driver.run(running)

callback (RT thread):
  state.process_block(&tick) → Source::generate → DSP → Sink::consume
  state.send_clock_tick(&tick) [gated on tick.is_final]
```

**Removed:** `LofiInput` node (`rill-lofi`). Replaced by
`LofiChipSource` — a `Source` node wrapping any `Algorithm<f32>` +
`ChipEmulator` + `ParameterWrite`. `IoControl` trait provides
`write_data()` channel for chip register writes via the backend's
control interface.

### 🎹 MIDI Output (`rill-io`, `rill-patchbay`, `rill-adrift`)

MIDI output infrastructure — rill as MIDI master, sending Clock, Transport,
and (future) Note messages to external devices.

**`rill-io` — backend architecture:**
- **`MidiBackend` → `MidiInput`** (breaking rename) — trait now accurately
  reflects its input-only role (`poll() -> Vec<MidiMessage>`).
- **`MidiOutput` trait** (new) — `send(&mut self, &MidiMessage) -> IoResult<()>`,
  symmetric to `MidiInput`. Together they mirror the audio-side
  `IoCapture`/`IoPlayback` separation — input and output are distinct
  traits, each backend implements the direction(s) it supports.
- **`MidirBackend`** — struct refactored: `_conn` field changed from
  `MidiInputConnection<()>` to `MidirConnection` enum (`Input`/`Output`
  variants). New constructors: `new_output()`, `new_output_by_name()`
  using `midir::MidiOutput::connect()`. Backend can now be opened in
  either direction — reused across both `MidiInput` and `MidiOutput`
  trait impls.
- **`AlsaSeqBackend`** — struct unchanged (`seq::Seq` is inherently
  bidirectional). New `new_output()` constructor opens with
  `Direction::Playback` + `PortCap::WRITE` (vs `Capture` + `READ` for
  input). New `midi_to_alsa_event()` helper — reverse of existing
  `alsa_event_to_midi()` — converts `MidiMessage` to ALSA `Event` for
  `event_output()` + `drain_output()`.
- **`JackMidiBackend`** — most significant struct change: `rx` split to
  `Option<Receiver<MidiMessage>>`, new `tx: Option<SyncSender<MidiMessage>>`.
  `JackMidiHandler` (process callback) becomes **bidirectional**:
  `MidiIn` port → channel → `MidiInput::poll()`, and channel →
  `MidiOut` port → `MidiOutput::send()`. Both directions coexist in
  one JACK client — `connect()` opens input, `connect_output()` opens
  output. Same pattern for internal comms (input drains `tx → rx`,
  output feeds `tx → rx` in reverse).

**`rill-patchbay`:**
- **`MidiClockGenerator`** — output-side counterpart of `MidiClockTracker`.
  Pure math: converts `ClockTick` → `Vec<ControlEvent::MidiClock>` using 24ppqn
  (24 pulses per quarter note). Derives tick spacing from absolute sample
  position — no cumulative drift. Transport state machine: Start resets phase,
  Stop/Continue follow standard MIDI transport semantics. 6 unit tests.
- **`spawn_midi_clock_output()`** — actor owning `MidiClockGenerator` +
  `Box<dyn MidiOutput>`. Receives `ClockTick` via Rack broadcast and
  `MidiTransport` commands, serializes via `serialize_to_midi()`, sends
  through backend.
- **`serialize_to_midi()`** — reverse of `parse_midi()`. Converts
  `ControlEvent::MidiClock` → `0xF8`, `MidiTransport` → `0xFA/0xFB/0xFC`,
  `MidiNote` → `0x90/0x80`. Round-trip tests: `parse_midi(serialize_to_midi(e)) == e`.
- **`ClockDef { backend, port_name, auto_start }`** — serializable MIDI clock
  output descriptor. Added to `ModuleDef::Clock(ClockDef)` variant.
- Re-exports: `MidiClockGenerator`, `spawn_midi_clock_output`, `serialize_to_midi`,
  `ClockDef`.

**`rill-adrift`:**
- **`ModuleDef::Clock(ClockDef)`** variant in adrift serialization layer,
  for `ModularSystemDef` JSON documents.
- **`ClockConstructor`** — registered in `ModuleFactory` as `"clock"`.
  Creates `MidiOutput` backend, calls `spawn_midi_clock_output()`,
  supports `auto_start`.
- **`to_pb_module()` + rack dispatch** — `ClockDef` conversion and
  module ID extraction for rack actor fan-out.

**Design doc + plan:** `docs/superpowers/specs/2026-06-30-midi-output-design.md`,
`docs/superpowers/plans/2026-06-30-midi-output-plan.md`.

### ⚡ Servo conflict resolution (`rill-patchbay`)

- **`Servo::with_control()`** / **`Servo::with_conflict()`** — builder methods
  to configure `ControlStrategy` and `ConflictStrategy` on a Servo.
- **`with_control(Modulation { depth })`** — automaton output modulates around
  `state.base`, combinable with HID input via `BasePlusModulation`.
- **`with_conflict(TouchOverride)`** — HID input freezes automaton via
  `state.frozen`, resumes on `UiRelease`.
- **`with_conflict(BasePlusModulation)`** — HID input updates `state.base`;
  automaton modulates around it on next `ClockTick`.
- **`ServoConstructor`** now passes `ServoDef.control_strategy` and
  `ServoDef.conflict_strategy` through to Servo construction.
- **`Control` handler fallback mapping arm** now checks `ConflictStrategy`:
  was ignoring `state.frozen` and `state.base` — now respects all three
  strategies.
- **Dead code removed:** `UiCommand` enum (`strategy.rs`) — never used.
- **Docs:** all PortCombiner references replaced with Servo+strategy
  architecture diagrams across `README.md`, `patchbay-rack.md`,
  `actor.md`, `two_thread_architecture.md`.

---

## [0.5.0-beta.5]

### 🕐 Unified RenderContext (Breaking)

**`rill-core` (`time/render.rs`):**
- `RenderContext` — single stack-allocated context per processing block:
  `sample_pos`, `samples_since_last`, `sample_rate`, `transport: TransportState`,
  `speed_ratio` (hardware clock correction, default 1.0).
- `TransportState` — `is_playing`, `bpm`, `frame_pos`, `time_sig_num/den`,
  `bar_start_frame`. Replaces `ClockTick::tempo: Option<f32>`.
- Musical methods moved from `ClockTick` to `RenderContext`:
  `beat_position()`, `musical_position()`, `is_new_bar()`, `is_new_beat()` —
  now use configurable `time_sig_num/den` (no longer hardcoded 4/4).
- `ProcessContext` and `ActionContext` removed — replaced by `&RenderContext`
  throughout the trait system.

**Trait signatures (breaking):**
- `Algorithm::process(input, output)` — `ctx` parameter removed (97.4% of impls
  ignored it; 2 tape heads now use `init()` for sample rate).
- `Source::generate(&RenderContext, …)`, `Processor::process(&RenderContext, …)`,
  `Sink::consume(&RenderContext, …)`, `Router::route(&RenderContext, …)` —
  all use `&RenderContext` instead of `&ClockTick`.
- `Port::propagate()` — context parameter removed; single `&RenderContext` flows
  through the DAG without re-wrapping.
- `Port::run_action()` — context parameter removed.
- `Port::pre_process()` — `_tick` parameter removed.

**Graph:**
- `Graph::run()` I/O callback creates one `RenderContext` per block and passes it
  to both `process_block()` and `propagate()` — no more `ProcessContext` +
  `ActionContext` duplication.
- `Graph.system_clock: Option<Arc<SystemClock>>` — when set, creates
  `RenderContext::with_tempo()` with BPM from the shared clock.

### 🎛️ MIDI Clock Sync

**`rill-patchbay` (`midi_clock.rs`):**
- `MidiClockTracker` — counts 24ppqn clock pulses (0xF8), derives BPM via
  running average, writes atomically into `Arc<SystemClock>`.
- `MidiClockStrategy` trait with three built-in strategies:
  `FreeRunning` (BPM only), `ResetOnStart` (position reset on Start),
  `SongPosition` (position reset + `is_playing()` flag).
- `is_playing: Arc<AtomicBool>` — shared flag, set on MIDI Start/Continue,
  cleared on Stop. Sequencers and automations check this before producing output.
- Integrated into `MidiHub` — optional via `MidiHub::with_clock_tracker()`.
  The tracker's `SystemClock` feeds BPM to `Graph.system_clock`.

### 🌐 OSC Sensor (`rill-patchbay`)

- **`OscSensor`** (`osc.rs`) — OSC input sensor modelled after `MidiHub`/`spawn_midi_sensor`.
  Binds a UDP socket in a dedicated OS thread, decodes incoming OSC packets
  via `rill-osc`, produces `ControlEvent::Osc { address, args }` events.
  Bundles unwound recursively. Implements `Module` + `Sensor` traits.
- **`spawn_osc_sensor()`** — actor-model variant: spawns a control actor for
  `SetEnabled` commands + UDP recv loop in OS thread. Sends
  `CommandEnum::Control(event)` to the servo for mapping.
- **`parse_osc()`** — converts `OscMessage` → `ControlEvent::Osc`.
  Numeric args (`Int`, `Float`) collected; strings and blobs silently dropped.
- **`SensorDef::Osc { port, mappings }`** — serializable descriptor variant
  in `module_def.rs`. `into_sensor()` gated on `any(feature = "midi", feature = "osc")`.
- **`OscConstructor`** — registered in `ModuleFactory` via `rill-adrift`:
  creates mapping-only servo + `spawn_osc_sensor()` pair. Activated by
  `ModuleDef::Sensor(SensorDef::Osc { ... })`.
- **Feature gate:** `osc = ["dep:rill-osc"]` in `rill-patchbay`;
  `rill-adrift/osc` enables `rill-patchbay/osc` passthrough.
- Existing `EventPattern::OscAddress` / `OscPattern` matching in servo works
  out-of-the-box — sensor produces `ControlEvent::Osc`, servo matches via
  `EventPattern::matches()`.

### 🔌 JACK MIDI + Transport

**`rill-io`:**
- `JackMidiBackend` — JACK MIDI input backend. Registers a `MidiIn` port,
  bridges JACK process callback to `MidiBackend::poll()` via mpsc channel
  (same pattern as `MidirBackend`).
- `JackBackend::set_system_clock()` — JACK transport sync: reads BPM from
  `TransportBBT` in process callback, writes atomically to `SystemClock`.

### 🔈 Lofi: DC Offset + Output Ceiling

**`rill-lofi` (`config.rs`, `lofi_processor.rs`):**
- `LofiConfig.dc_offset` — subtracted from signal after dry/wet (before gain).
  Default 0.0. Use 0.5 for AY-3-8910 to centre [0, 1] around zero.
- `LofiConfig.output_ceiling` — hard clamp `[-ceiling, +ceiling]` (default 1.0).
- Formula order: `(dry_wet_mix - offset) * gain, clamp to ±ceiling`.
- New parameters exposed as `"dc_offset"` and `"output_ceiling"` in
  `LofiProcessor` metadata → available through `SourceDef.parameters`.
- 3 new tests: offset removal, ceiling clamp, combined behaviour.

**Registration (`rill-adrift/src/registration.rs`):**
- `rill/lofi_input` constructor now reads `dc_offset`, `output_gain`,
  `output_ceiling` from `Params`.

### 🧱 Physical Modeling in `rill-core-model`

**Four new resonant model modules** (`rill-core-model`):

- **`string`** — 1D digital waveguide with fractional-delay allpass interpolation,
  stiffness dispersion, and frequency-dependent damping. Implements
  `Algorithm<T>` + `ParameterizedAlgorithm<T, Params = StringParams<T>>`.
- **`plate`** — 2D FDTD waveguide mesh on rectangular grid with clamped/free
  boundary conditions. Impulse excitation at configurable position.
- **`modal`** — parallel bank of 2-pole resonant filters for modal synthesis.
  Pre-built presets: `bell_modes()` (5 modes, inharmonic bell ratios) and
  `marimba_modes()` (3 modes, harmonic bar ratios).
- **`cavity`** — `HelmholtzCavity` (single Helmholtz resonator with optional
  reed excitation for wind instrument modeling) and `CavityArray` (1D chain
  of coupled cavities for wave propagation experiments / acoustic metamaterials).

All four types implement `Algorithm<T>` + `ParameterizedAlgorithm<T>`.
24 new tests.

### ♻️ `ParameterizedAlgorithm` → `rill-core`

**`rill-core` (`traits/algorithm.rs`):**
- `ParameterizedAlgorithm<T>` trait added — typed parameter access for any
  `Algorithm` (`params()`, `set_params()`, `set_parameter()`). Generic over
  `type Params: Clone + Send + Sync`. Previously lived in `rill-core-dsp`.

**`rill-core-dsp`:**
- `rll-core-dsp/src/algorithm.rs` — now re-exports `ParameterizedAlgorithm`
  from `rill-core`; definition removed.
- `Algorithm`, `AlgorithmCategory`, `AlgorithmMetadata`, `ActionContext`,
  `ProcessResult` no longer re-exported from `rill-core-dsp` — all consumers
  import directly from `rill_core::traits`.
- 7 filter `ParameterizedAlgorithm` impls unchanged.

### 📦 `rill-core-wdf` → `rill-core-model`

- Crate renamed: `rill-core-wdf` → `rill-core-model`
- Internal module `filters` → `wdf` (path: `rill_core_model::wdf::*`)
- All imports across workspace updated (4 crates, 14 docs, 3 scripts)
- Current module listing: `macros`, `analysis`, `constants`, `wdf`, `tape`,
  `string`, `plate`, `modal`, `cavity`

### 📝 Terminology: «audio» → «signal» / «I/O»

**Public API (breaking):**
- `rill_oscillators::audio` → `rill_oscillators::signal` — module rename
- `PortType::is_audio_rate()` → `is_signal_rate()` in `rill-core`
- `AudioTimer` → `SignalTimer` in `rill-core`
- `AudioConfig` → `IoConfig` in `rill-core`
- `RackCase::audio_thread` → `signal_thread` in `rill-adrift`

**Cargo.toml descriptions** — «audio» → «signal» / «I/O» in 7 crates: `rill-graph`, `rill-sampler`, `rill-telemetry`, `rill-router`, `rill-osc`, `rill-digital-effects`, `rill-adrift`.

**Documentation** — «audio thread» → «signal thread», «audio data» → «signal data», «audio backends» → «I/O backends», «audio path» → «signal path», etc. (~120 occurrences across .rs doc comments, architecture docs, AGENTS.md, README.md).

`IoBackend` in `rill-core` formally positioned as a **generic I/O archetype** — applicable to any discrete data stream, not just audio.

**Preserved:** `rill-io` and `rill-lofi` keep «audio» terminology (genuinely audio-specific — hardware I/O, emulators).

### 🎛️ AY-3-8910 Emulator Fixes

**`rill-lofi`:**
- **Mixer register R7 bit layout** — bits 0–2 = tone A/B/C, 3–5 = noise A/B/C (fixed; was grouping bits 0-1,2-3,4-5 per channel)
- **Envelope period divider** — `f / (16 × EP)` → `f / (256 × EP)` per AY-3-8910 datasheet
- **Noise LFSR output bit** — save bit 0 before shift (was reading bit 16 after shift)
- Test `test_mixer_register_bit_mapping` updated for correct layout

### 🏭 Module Factory

**`rill-patchbay/src/module_factory.rs` (new):**
- `ModuleConstructor` trait — `construct(id, params, system, graph_ref) → BoxedModule`
- `ModuleFactory` — `register_fn(type_name, drain, closure)`, `register_fn_send()`
- `Drain` enum — `OsThread { interval_ms }`, `TokioTask { interval_ms }` (for many actors without OS thread overhead)
- `GenericModule` — factory-provided `Module` impl, no manual struct needed

**`rill-patchbay/src/serialization/mod.rs`:**
- `ModuleDef::Custom { type_name, params }` — dispatch through `ModuleFactory` in `build_servos()`

**`rill-adrift/src/modular/mod.rs`:**
- `ModularSystem.module_factory: ModuleFactory` — `module_factory_mut()` for pre-launch registration
- Rack actor drain loop: `tokio::spawn` → `std::thread::spawn` (avoids `Send` requirement on handler)

### 🎭 Actor Model Unification

**`rill-core-actor`:**
- **Removed:** `Actor<M>` (old `Send` variant), `LocalActor<M>`, `ActorCell` trait, `MessageDispatcher`, `build_actor()`
- **Added:** `spawn_detached(name, make_handler, ms)` — handler created inside spawned thread, `ActorRef` returned immediately
- **Added:** `spawn_detached_tokio(name, make_handler, ms)` — same but on tokio task (handler: `Send`)
- `spawn(name, handler)` — remains for inline drain (Graph, Rack)
- **Actor design rule:** handler is always created on the thread where it is drained; never crosses thread boundary; `Send` bound removed from handler closure

### 🔧 Sequencer & Servo Fixes

**`rill-patchbay/src/automaton/sequencer.rs`:**
- Removed dead `Step.value` and `Step.curve` fields (`Step` now only has `duration`)
- Fixed `step_duration()` formula: removed `× 4.0` factor (now `1.0` = quarter note, not whole note)

**`rill-patchbay/src/engine.rs`:**
- Added `Servo::with_table()` builder — propagates `table` from `ServoDef` to `Servo`
- `Servo::spawn()` uses `spawn_detached_tokio` — handler created inside tokio task, no actor crossing thread boundary

**`rill-patchbay/src/serialization/mod.rs`:**
- `build_servos()` now propagates `ServoDef.table` → `Servo::with_table()`

### 🎵 Chiptune Examples

**`rill-adrift/examples/chiptune.rs`:**
- 3-channel AY melody: Ch A (melody), Ch B (bass), Ch C (snare), 16 steps × 120ms, bass changes every 4 steps
- Fixed Output `channels=1` (was defaulting to stereo, causing PipeWire panic)
- Duration: `120ms → 0.24` quarter-note beats (matching fixed `step_duration` formula)
- Removed unused `HashMap` import

**`rill-adrift/examples/chiptune_stc.rs`:**
- Rewritten to use `ModularSystemDef` + `ModuleFactory` (`register_fn` with `Drain::OsThread`)
- STC player registered as `ModuleDef::Custom { type_name: "stc_player" }`
- Removed: manual `GraphBuilder`, `graph.run()`, `StcModule` struct, `sys.spawn()`, `actor.drain()`, `thread::spawn`

### 🔩 RackCase Fix

**`rill-adrift/src/modular/case.rs`:**
- `RackCase::stop()` — added `handle.thread().unpark()` before `handle.join()` (was hanging on exit)
- `tasks` type: `Vec<tokio::task::JoinHandle>` → `Vec<std::thread::JoinHandle>`

### 📝 Documentation

**`docs/src/guides/chip-emulators.md`:**
- Rewritten: accurate register map, architecture diagram, `io_write` control chain, lofi processing chain
- **Known Limitations** section — output sampling, anti-aliasing, register change timing, I/O ports, phase delay
- **Timing accuracy** section — tone/envelope/noise frequency formulas, accuracy bounds

**`docs/src/architecture/actor.md`:**
- Updated for current API: `Actor<M>`, three `spawn` variants, handler-creation design rule

### 🧹 Cleanup

- **Removed:** dead `Actor<M>` (Send variant), `ActorCell`, `build_actor()`, `Step.value`/`Step.curve`
- `rill-io/Cargo.toml` — removed unused `base64` dependency
- `rill-core-actor/Cargo.toml` — added optional `tokio` dependency (feature-gated `spawn_detached_tokio`)
- PortAudio callback — removed debug `base64` output

### 🏗️ Architecture: RackDef unification + CaseDef removal

**`rill-adrift/src/modular/serialization.rs`:**
- New `RackDef` with `graph: GraphDef` field — graph lives inside the rack, not in a separate `CaseDef`
- New `ModuleDef::Graph { graph: GraphDef }` variant — multiple graphs per rack
- `build_servos()` moved from `rill-patchbay` to `rill-adrift`
- `ModularSystemDef.racks: Vec<RackDef>` replaces `cases: Vec<CaseDef>`
- `CaseDef` removed entirely — `patchbay: Option<RackDef>` no longer needed

**`rill-adrift/src/modular/mod.rs`:**
- `launch()` simplified: single loop over `def.racks`, no `has_rack` check
- Rack actor drain: `tokio::spawn` → `std::thread::spawn` (avoids `Send` requirement)
- Graph construction stays in `launch()` (not via factory)

**`rill-patchbay/src/serialization/mod.rs`:**
- `RackDef` → `PatchbayDef` (backward-compatible rename, without `graph` field)
- `ModuleDef` (without `Graph` variant) + `build_servos()` remain in rill-patchbay

**`rill-adrift/src/modular/config.rs`:**
- `LaunchConfig.rack_def` type: `RackDef` → `PatchbayDef`

### 🔌 CommandEnum::Stop + Drain::IoCallback

**`rill-core/src/queues/signal.rs`:**
- `CommandEnum::Stop` + `CommandType::Stop` — shutdown command for I/O loops

**`rill-patchbay/src/module_factory.rs`:**
- `Drain::IoCallback` variant — for graph modules with inline drain (not yet used via factory)

### 📝 Documentation

**`docs/src/architecture/actor.md`:**
- Updated for current API: `Actor<M>`, three `spawn` variants, handler-creation design rule

**`docs/src/guides/chip-emulators.md`:**
- Rewritten: accurate register map, known limitations, timing accuracy section

### Previous (0.5.0-beta.4)

**`rill-core-actor`:**
- `Actor<M>` — handler: `Send`, для многопоточных акторов (Patchbay через tokio)
- `LocalActor<M>` — handler: `!Send`, для однопоточных (Graph, RackCase)
- `ActorSystem::spawn()` / `spawn_local()` — создание акторов с handler-замыканием
- `ActorRef<M>` — lock-free handle для отправки сообщений, единственный внешний интерфейс
- Удалены: `ActorCell` trait, `Mbox`, `MessageDispatcher`, `ActorRef::new_pair()`, generic `ActorSystem<M>`

**`rill-graph`:**
- `GraphBuilder::build(&ActorSystem)` — создаёт актор с handler'ом, захватывающим nodes
- `Graph::run()` — tick-замыкание владеет actor'ом напрямую (без `*mut Graph`)
- Nodes хранятся в `Rc<UnsafeCell<Vec<NodeVariant>>>` — interior mutability на одном потоке
- Удалены: `*mut NodeVariant`, `*mut Graph`, `ActorCell` impl, `mailbox` поле
- Сигнальные тесты: `test_graph_source_to_sink`, `test_graph_source_proc_sink`

**`rill-patchbay`:**
- **`Patchbay` struct удалён.** Вместо него — `Servo::spawn(self) → ActorRef<CommandEnum>`
  - Создаёт актор с полным handler'ом (ClockTick → automaton.step → SetParameter)
  - Запускает `std::thread` drain loop (1ms interval)
  - Внешний код получает только `ActorRef` — никакого прямого доступа к состоянию
- **`Servo` больше не `Module`** — автономный актор, не type-erased box
- `PatchbayDef` → `RackDef` — `build_servos(&ActorSystem, &graph_ref) → HashMap<String, ActorRef>`
- `add_lfo`, `add_envelope`, `add_boxed_servo` удалены — сборка в `launch()` напрямую
- `Module` trait — только для Sensor; убраны `drain()`, `update()`
- Channel-forwarding (mpsc) между actor'ами удалён — каждый актор самодрейнится

**`rill-adrift`:**
- **`RackCase`** — минимальный хост: `modules: HashMap<String, ActorRef>`, `tasks: Vec<JoinHandle>`
  - Удалены: `patchbay`, `incoming`, `outgoing`, `ActorCell` impl, межкейсовый routing
  - `handle() → ActorRef` — для `parent_ref` в Graph
  - `stop()` — abort всех tasks, join audio thread
- **`launch()`**:
  1. Создаёт актор RackCase (с `Arc<Mutex<HashMap>>` для модулей)
  2. Запускает drain thread актора (пересылает ВСЕ сообщения всем модулям)
  3. Строит граф на audio thread
  4. Получает `graph_ref` через oneshot канал
  5. `rack_def.build_servos()` — создаёт Servo'ы с drain threads
  6. Регистрирует servo ActorRef'ы в RackCase модулях
- Удалены: `create_case()`, `load_patchbay()`, `load_graph()`, `create_patchbay()`, `tick()`,
  `start_osc()`, OSC, `control`, `control_shared`, `control_arc`, `AutomatonFactory`

**Архитектура ClockTick → Sequencer → Graph:**
```
Graph.run() → tick: parent_ref.send(ClockTick)
  → RackCase actor (drain thread): for ref in modules: ref.send(msg)
  → Servo actor (drain thread): ClockTick → automaton.step() → graph_ref.send(SetParameter)
```

### 🔧 Сопутствующие исправления
- PortAudio: off-by-one в `write()` — `cap / nch` теперь используется как bound цикла (был краш `index out of bounds: 256`)
- `advanced_player`: комментарий `--features "cpal,…"` → `"portaudio,…"`
- `play_wav`: пример ручной сборки графа переписан (был заглушкой `let _ = system`)

### SIMD acceleration (feature/simd)

- **Vector infrastructure**:
  - `SimdDetector` — real CPU feature detection via `std::arch` (SSE2/AVX/NEON/SIMD128)
  - `VectorMask<T, N>` completed for `F32x4`, `F32x8`, `F64x2`, `ScalarVector4`
  - `VectorReduce`, `VectorScalarOps` traits with blanket impls
  - `Scalar::from_usize()` added to core math trait
  - Dead `expr` module + `vec_expr!`/`vec_eval!` stubs removed

- **Algorithm SIMD (rill-core-dsp)**:
  - `BasicOscillator` — 6 waveforms via `ScalarVector4` block processing (4 samples/iter)
  - Saw BLEP — `VectorMask::select` replaces per-lane scalar conditional (2.5× speedup)
  - `InterpolatedReader` — 4-wide lerp math for linear/cubic interpolation
  - `CombFilter` — batched 4-sample read/write when `delay_samples >= 4`
  - `NoiseGenerator` — White (batched xorshift), Brown (unrolled integrator), Blue/Violet (4-wide diff)
  - `Biquad` — block state-space 4×4 feedforward matrix via `BiquadBlock` precomputation
  - `Resampler<T>` — sample-rate converter on `InterpolatedReader` (44.1k→48k etc.)

- **Node-level SIMD**:
  - `Distortion` — HardClip/Tube 4-wide SIMD; zero-copy port output
  - `DryWetMix` — 4-wide multiply-add, stereo in one pass
  - `WriteHead` — batched 4-sample math per tape write
  - `pre_process()` — feedback mix via 4-wide add (all feedback nodes accelerated)
  - 8 nodes: direct port buffer write eliminates 2 `[T; BUF_SIZE]` copies per block per node

- **WDF SIMD (rill-core-model)**:
  - `process_incident_vector` on `Resistor`, `Capacitor`, `Inductor`, `Diode` via `ScalarVector4`
  - Diode Newton-Raphson vectorized with `VectorMask::all()` early exit
  - `process_batch_simd` free function for batch processing
  - `simd.rs` deleted (378 LOC) — no more parallel SIMD type hierarchy

- **I/O SIMD**:
  - Generic `f32_to_i16_chunk` / `i16_to_f32_chunk` in `rill-core::math::functions` (reusable for ALSA, rill-lofi)
  - ALSA backend uses SIMD f32↔i16 conversion
  - PipeWire byte→f32 batched 4-sample conversion
  - Deinterleave/interleave SIMD in PipeWire backend

- **Infrastructure**:
  - `FixedBuffer` now `#[repr(align(16))]` (hardware SIMD-ready)
  - `const { assert!(BUF_SIZE % 4 == 0) }` in `processable.rs` (monomorphization-time check)
  - Criterion benchmarks: vector ops, 6 oscillators, 3 filters, 4 noise types, reader/resampler
  - Benchmark results at `docs/superpowers/specs/2026-05-10-simd-benchmark-results.md`
  - **Key finding:** `ScalarVector4` + LLVM auto-vectorization matches/exceeds explicit `wide` crate on x86_64. Rill outperforms JUCE (C++) by 10-160× on key DSP primitives.

### ✨ Patchbay architecture refactor (feature/refactor/midi-hub, feature/refactor/sensor-midi)

- **Automaton trait redesigned**:
  - `(config, &mut internal, &current, time, action) → ParamValue`
  - `type Internal: Clone` — mutable automaton-specific state (phase, RNG, step counter)
  - `initial_internal()`, `reset()` with default impls
  - All state moved inside structs; old `State`/`Output` associated types removed
  - All 6 automata (LFO, envelope, sequencer, function, random, cellular) updated
  - LFO: now uses `self.waveform` — all 8 waveform types functional (was hardcoded to Sine)
  - Random: `update_rate` field drives throttling via `last_update_time` in Internal

- **Servo as actor**:
  - `Servo<A: Automaton>` implements `ActorCell<Msg = AutomatonMsg>`
  - `AutomatonMsg { Tick(ClockTick), SetEnabled(bool), Reset }` — unified queue for clock + commands
  - `Servo::update()` drains mailbox before stepping (same pattern as `Graph::run`)
  - `Servo::handle()` returns `ActorRef<AutomatonMsg>` for external control
  - `Servo::with_table(Vec<ParamValue>)` — table-based step-to-value mapping for sequencers
  - `SequencerAutomaton` returns `ParamValue::Int(step_index)` → Servo looks up in table

- **Sensor trait** — unified external input bridge:
  - `trait Sensor { attach(), start(), stop() }` — MIDI, OSC, knobs, acoustic analysis
  - `MidiHub` implements `Sensor` — no more `Arc<Mutex<Patchbay>>`
  - `Patchbay::event_mailbox` — single `MpscQueue<ControlEvent>` for ALL sensors
  - `event_handle() → ActorRef<ControlEvent>`, `drain_events()` called from `drain_clock()`
  - Multiple sensors can run independently, all events via one lock-free mailbox

- **Hearing module** for future acoustic sensors:
  - `PitchDetector`, `EnvelopeFollower`, `ZeroCrossing` — audio analysis algorithms
  - Ready for wiring into graph telemetry (audio feedback → control signals)

### 🗑️ Removed

- **crossbeam-channel** — removed from all crates (rill-core, rill-patchbay, rill-adrift)
  - `CommandQueue` (crossbeam-based) deleted; `Command` trait kept in `rill-core::queues`
  - `TelemetryTx` (crossbeam wrapper) deleted; `Telemetry` types kept for future use
  - `Observer` moved to `rill-patchbay`, now uses `ActorRef<Telemetry>`
  - `SequencerHandle` (crossbeam command channel) deleted
  - `attach_sequencer()` (crossbeam `Receiver<Telemetry>` parameter) deleted

- **Manager** (806 LOC) — deprecated sync rack, zero external callers
- **SnapshotSequencer** + sequencer types (728 LOC in `sequencer/`)
- **SequencerDef** serialization (170 LOC)
- **sensor/physical.rs** (dead code referencing non-existent types)
- **automaton/mapping/** (156+183+155 LOC dead code)
- `MidiActor` renamed to `MidiHub`; `midi_actor.rs` → `midi.rs`
- `Graph::receive()` now drains via `ActorCell` (was manual `set_parameter` loop)

### 🔧 Fixes

- **RT safety**: MixerNode `vec![]` → `[f32; BUF_SIZE]` stack allocation
- **RT safety**: PortAudio `vec![]` temp buffer → `[f32; 8192]` stack
- **RT safety**: ParallelAdapter `Vec<T>` → `[T; 8]` stack allocation
- `Graph::receive()` — `debug_assert!` for SetParameter misconfiguration
- LFO: all 8 waveform types functional in `step()` (was hardcoded to Sine)
- Random: `update_rate` field drives throttling
- Documentation synced with code (12 discrepancies fixed)
- Zero compiler warnings with `--all-features`

### ✨ STC chiptune player (feature/feat/stc-player)

- **`rill-adrift/examples/chiptune_stc.rs`** — full Sound Tracker Compiled (STC) player
  - Plays ZX Spectrum chiptune files through the `Ay38910Backend` AY-3-8910 emulator
  - Loads the STC file (`Bonysoft - Popcorn (1993).stc`) via `include_bytes!`
  - Implements the libayemu-compatible event-driven architecture:
    - Per-channel byte-stream event reading with delay/interrupt timing
    - Per-frame pitch computation: `ST_TABLE[note + ornament[pos] + transposition] ± sample_delta`
    - 32-step sample (instrument) rendering with volume, tone/noise mixer masks, and pitch deltas
    - Synchronized 32-step ornament (pitch modulation) and sample position advancement
    - Sample repeat/loop logic, envelope triggering, position advancement on channel A end marker
  - Timing at 48.828 Hz Pentagon INT rate via `step_ms()` time accumulation from audio callbacks
  - Uses the same graph/clock architecture as `chiptune.rs` — validates the engine timing

## [0.5.0-beta.4] — 2026-05-08

### ✨ New

- **`IoNode` / `ActiveNode` trait hierarchy** in `rill-core::traits::node`:
  - `Node` — base trait, no backend, no run method
  - `IoNode: Node` — `resolve_backend(backend)` for I/O-capable nodes
  - `ActiveNode: IoNode` — `run(tick, running)` for the single driver node
  - `as_io_node_mut()` / `as_active_node_mut()` downcasting helpers on `Node`
  - `Input`, `Output`, `LofiInput` implement `IoNode`
  - `Input`, `Output` implement `ActiveNode`
  - `GraphBuilder::build()` uses downcasting instead of name-based matching
  - `Graph::run()` calls `ActiveNode::run()` instead of `Node::run()`
  - `GraphRunner` trait removed — replaced by `Box<dyn FnMut(u64, f32)>`
  - Inherent `resolve_backend()` convenience methods on `Input`/`Output`

- **Chip emulator architecture** — unified model for vintage sound chips:
  - `Ay38910Chip` + `Ay38910Backend` — AY-3-8910 / YM2149 (3 tone, noise, envelope)
  - `NesChip` + `NesBackend` — NES 2A03 APU (2 pulse + sweep, triangle, noise, DPCM)
  - `IoControl` trait in `rill-core::io` — uniform register write interface
  - `LofiInput<T, BUF_SIZE>` — `Source` node wrapping any `IoBackend` with lofi processing

- **WDF tape module** in `rill-core-model`:
  - `RecordHead<T>`, `PlaybackHead<T>` — analog tape physics, `Algorithm<T>`
  - `OpAmp<T>` — operational amplifier as `WdfElement<T>`
  - `CassetteDeck` in `rill-analog-effects` refactored to use heads from `rill-core-model`

- **`Transcendental` trait extended**: `tanh()`, `signum()`, `random()` — enables
  stochastic modeling in generic WDF/dsp code

- **NES 2A03 sweep unit** — full hardware sweep emulation (divider, direction, shift,
  period underflow/overflow mute)

### 🔧 Fixes

- **`rill-io`**: `set_process_callback` signature changed from `Fn()` to `Fn(f32)` —
  each backend passes its actual negotiated sample rate to the process callback.
  `ClockTick.sample_rate` now always reflects the true device rate.
- **`rill-io/jack`**: reads `client.sample_rate()` after activation, passes to callback.
- **`rill-io/alsa`**: queries `hw.get_rate()` after `set_rate(Nearest)`, enforces
  exact period match (`hw.get_period_size() == BUF_SIZE`), rejects mismatches.
  Fixed `write()` — was hardcoded for stereo, now handles N channels with proper interleaving.
- **`rill-io/pipewire`**: output chunk no longer hardcoded to 512 samples — uses
  `buf_frames * out_channels` for correct mono timing. `write()` fixed for N channels.
- **`rill-lofi/emulators`**: removed `unsafe impl Send/Sync` — backends run exclusively
  in the hard-RT audio thread.
- **`rill-core/io`**: `IoBackend` and `IoControl` traits no longer require `Send + Sync`.
- **`rill-core-actor`**: `ActorCell` no longer requires `Send`.
- **`rill-adrift/chiptune`**: `step()` uses `f64` timing (no millisecond quantization),
  `Ay38910Backend` lazily created with actual sample rate, `lofi.init(sr)` called
  for correct processor configuration.
- **`rill-adrift/record_mic`**: graph built inside audio thread spawn (no `Send` needed).

### ✨ New

- **`rill-io/portaudio`** — cross-platform PortAudio backend (`portaudio` feature).
  Exact buffer size, no `BufferSize::Default` issues, simpler API.
  Default backend replacing CPAL.

### 🧹 Removed

- **`rill-io/cpal`** — replaced by `rill-io/portaudio` (cross-platform, cleaner API)
- `Ay38910Emulator`, `NesEmulator` — replaced by `Chip` + `Backend` + `LofiInput`
- `rill-analog-effects::OperationalAmplifier` — replaced by `rill_core_model::OpAmp`

### 📖 Documentation

- New guide: **Chip Emulators** (`docs/src/guides/chip-emulators.md`)
- Examples section added to root `README.md` — all 5 `rill-adrift` examples
  described with `cargo run` commands
- Spec + plan for IoBackend-based emulator architecture in `docs/superpowers/`

## [0.5.0-beta.3] — 2026-05-07

### ✨ New

- **`rill-core-actor` crate** — actor model infrastructure:
  - `ActorRef<M>` — thread-safe handle, strong `Arc` reference, `send()` is lock-free and RT-safe
  - `ActorCell` trait — for types that own a mailbox and process messages
  - `MessageDispatcher<M>` — dispatcher with dead letters support
  - `ActorSystem<M>` — named mailbox registry, `route()`, `broadcast()`, dead letters

- **`rill-adrift`**: `serialization` added to default features — `serde` + `toml` available out of the box
- **`rill-adrift`: `config.toml`** — new example config file with `backend_name`, `backend_params`, `sample_rate`, `block_size`
- **`rill-adrift`: `RuntimeConfig`** now derives `serde::Deserialize` (behind `serialization` feature)
- **Missing graph nodes registered**:
  - `rill/moog_ladder` — digital Moog ladder filter (`rill-digital-filters`)
  - `rill/lofi` — lo-fi processor (`rill-lofi`, gated behind `lofi`)
  - `rill/analog_moog_ladder` — WDF Moog ladder filter (`rill-analog-filters`, gated behind `analog`)
  - `rill/cassette_deck` — cassette deck emulation (`rill-analog-effects`, gated behind `analog`)
  - `rill/parametric_eq` — parametric equalizer (`rill-router`)
  - `rill/graphic_eq` — graphic equalizer (`rill-router`)
  - All router nodes (`dry_wet_mix`, `mixer`, EQ) consolidated into `register_router()`

### 🧹 Removed

- `rill-core-dsp`: removed `unstable` feature (no code behind it, required nightly)
- `rill-patchbay`: `PatchbayEngine` removed (folded into `Engine`)
- `rill-core`: `traits::actor` module removed (moved to `rill-core-actor`)

### 🔧 Fixes

- `rill-io/pipewire`: fixed `AudioBackend::write` stub returning `0` instead of `buffer.len()`
- `rill-graph`: removed redundant `B as usize` cast, pre-existing clippy warnings fixed
- `rill-patchbay`, `rill-adrift`: fixed redundant closures, unused imports, unused variables
- **`rill-adrift`: `--no-default-features` compilation fixed**:
  - `register_all_nodes` no longer gated behind `io` (oscillators, filters, effects available without I/O)
  - `register_backends` call in `Runtime::new()` gated behind `io`
  - `cfg_from_params()` gated behind `io`
  - `Patchbay` import decoupled from `osc` feature
  - `ActorRef` import gated behind `any(osc, serialization)`
  - Dead `register_io` stub removed
- **`rill-adrift` examples**:
  - `play_json` renamed to `player` — now reads `config.toml` instead of hardcoded paths
  - All examples have explicit `required-features` (clear error with `--no-default-features`)
  - `play_wav`: unused `registration` import removed

### 📝 Documentation

- Architecture article: actor model (`docs/src/architecture/actor.md`) with RT boundary section
- `AGENTS.md`: quoting rules for commit messages with backticks
- All docs updated to reflect `Engine`, `*Def`, `ActorRef` naming

## [0.5.0-beta.1] — 2026-05-04

### 🎉 First beta release

All 17 crates published on crates.io at `0.5.0-beta.1`.

### ✨ New

- **WAV playback example** (`rill-adrift/examples/play_wav.rs`) — full pipeline
  from file to speaker: load WAV → SamplePlayer → BiquadFilter → AudioOutput
- **CLI backend selection** — `cargo run --example play_wav -- [backend] [file]`
- **24-bit WAV support** — `rill-sampler` now handles 24-bit PCM in addition to 16-bit

### 🔧 Improvements

- **All 4 audio backends produce clean audio**: CPAL, ALSA, PipeWire, JACK
- **OutputWindow pattern** — `write_output()` writes directly into DMA buffer,
  eliminating intermediate ring buffers and associated sizing issues (CPAL, PW, JACK)
- **Lock-free `IoRingBuffer`** — rewritten with `UnsafeCell` interior mutability,
  all methods take `&self`, no `Mutex`/`RwLock` in the RT path
- **No `thread::sleep` in any backend** — all backends are event-driven or
  callback-driven
- **WDF macros accept bare expressions** — `$pr:expr` replaces `$pr:tt`,
  no more unnecessary braces

### 🧹 Dependencies removed

- `parking_lot` — removed from `rill-io` dependencies (all uses replaced with
  `std::sync::Mutex`/`AtomicU32` or lock-free patterns)
- `crossbeam-channel` — removed from `rill-io` dependencies (start/stop via
  `AtomicBool` + `thread::park`/`unpark`, MIDI events via `std::sync::mpsc`)

### 🏗️ Infrastructure

- **CI** — GitHub Actions with 4 jobs: lint, test, test-minimal, doc
- **Pre-commit hook** — rejects direct commits to `develop`/`main`/`master`
- **`clippy.toml`** — workspace-level lint configuration (later removed,
  `needless_range_loop` allowed at workspace level)
- **491 tests** — all passing, 0 clippy warnings (excluding intentional
  `needless_range_loop` in SIMD code)

### 📚 Documentation

- **Root README**: 1270 → 154 lines, English only, no duplication
- **6 new mdBook chapters**: `core`, `graph`, `real-time-safety`,
  `world-of-automatons`, `git-flow`, overhauled `getting-started`
- **Doc comments** on all public API items — 0 missing-docs warnings
- **Doc link warnings**: 48 → 0
- **All 17 crate READMEs** present and up to date
- **`rill-sampler/README.md`** written from scratch
- **`rill-patchbay/README.md`** rewritten with green thread architecture
- **`rill-adrift/README.md`** expanded with feature flags table
- **`CHANGELOG.md`**, **`MANIFESTO.md`** moved to repository root

### 🧪 Quality

- `cargo clippy --workspace`: **0 warnings** (down from 755)
- `cargo doc --workspace --no-deps`: **0 warnings** (down from 48)
- `cargo test --workspace`: **491 passed, 0 failed**

---

## [0.4.0] — 2026-05-02

### 💥 Breaking changes

- **`Audio` → `Signal` rename** across the entire API surface:
  - `AudioNode` → `SignalNode`
  - `AudioBuffer` → `SignalBuffer`
  - `AudioError` / `AudioResult` → `SignalError` / `SignalResult`
  - `AudioGraph` → `SignalGraph`
  - `AudioEngine` → `SignalEngine`
  
  All crates bumped to `0.4.0`. Only `rill-io::AudioBackend` keeps its name
  (genuinely audio-specific trait).

---

## [0.4.1] — 2026-05-04

### ✨ Audio I/O backends — AudioIo trait

Реализован `AudioIo` для всех бэкендов:

| Бэкенд | Статус | Механизм вызова callback |
|--------|--------|--------------------------|
| `NullBackend` | ✅ | Заглушка, callback не дёргается |
| `PipewireBackend` | ✅ | RT callback (PW thread) |
| `JackBackend` | ✅ | RT callback (JACK thread) |
| `AlsaBackend` | ✅ | `snd_pcm_wait()` — event-driven, без `thread::sleep` |
| `CpalBackend` | ✅ | Thread + `thread::sleep(interval)` — poll-driven |

- **`AudioInput::init_backend(name, config)`** — узел сам создаёт бэкенд по
  имени (`null`, `alsa`, `cpal`, `pipewire`, `jack`), каждый под feature gate
- **`AudioOutput::set_active(source_idx)` + `start()`** — pull model (active
  Sink). Sink хранит ссылку на Source и дёргает `generate()` + `propagate()`
  при каждом цикле обработки. Callback идентичен push-модели.
- **`AudioOutput::consume()`** — читает из собственных входных портов
  (`self.inputs`), а не из параметра `signal_inputs` (пуст при вызове через
  `process_block` → `propagate`)
- **`ParamValue::as_str()`** — доступ к строковому значению `String`/`Choice`

### 🧹 Удалён глобальный реестр бэкендов

Из `rill-adrift` удалён `BACKEND_PTR`, `set_audio_backend()`,
`clear_audio_backend()`, `get_audio_backend()`. I/O узлы регистрируются
в фабрике без бэкенда — бэкенд создаётся внутри узла через
`init_backend()` при десериализации графа (параметр `"backend"`).

### ⚡ ALSA: poll → event-driven

- Убран `thread::sleep(1000μs)` из `run_alsa_thread()`. Вместо этого
  используется `pcm_playback.wait(None)` (`snd_pcm_wait()`). Тред спит
  в ядре, просыпается только когда DMA готов. Никакого busy-wait.

### 🧪 Тесты

- **`test_pull_model_sync_inject_and_verify`** — интеграционный тест
  pull model: граф SineOsc → AudioOutput через `GraphDocument`,
  `SyncBackend` с ручным триггером, верификация данных в output ring.
- **`test_alsa_pull_model`** — ALSA loopback через snd-aloop, проверка
  xruns после работы pull model.

### 📝 Документация

- **AGENTS.md**: раздел Hard-RT safety переписан. Две модели бэкендов
  (callback-driven / poll-driven), `thread::sleep()` запрещён в RT path.
  Добавлен Known issues (ALSA/CPAL poll loop). Threading model исправлен
  — ALSA больше не указан как RT thread.
- **README.md**: таблица версий обновлена (все 0.4.0).
- **docs/architecture.md**: версии крейтов обновлены (0.3.0 → 0.4.0).
- **docs/src/index.md, docs/src/guides/getting-started.md**: версии
  зависимостей обновлены (`"0.3"` → `"0.4"`).

---

## [0.3.2 / 0.3.1 / 0.3.1] — 2026-05-02

### 🆕 Новые крейты

| Крейт | Версия | Описание |
|-------|--------|----------|
| `rill-sampler` | 0.3.1 | Сэмплер + time-series reader (Source-узлы графа) |

### ✨ rill-core (0.3.2)

- **`Interpolate` trait** — дробно-индексное чтение `&[T]` с тремя стратегиями:
  `interpolate_linear`, `interpolate_cubic` (Hermite), `interpolate_nearest`.
  Blanket impl на `[T]` где `T: Transcendental + Copy` — работает для `Vec<T>`,
  `Box<[T]>`, `[T; N]` через `Deref`.

### ✨ rill-core-dsp (0.3.1)

- **`InterpolatedReader<T>`** — heap-буфер с дробной позицией, rate-ом и
  wrap-интерполяцией (clamp для семплов, periodic wrap для вейвтейблов).
  Основа для SamplePlayer и WavetableOscillator.
- **`WavetableOscillator<T, N>`** — переписан на `InterpolatedReader`.
  Добавлены `set_cubic()` / `is_cubic()`. Методы `Generator<T>`:
  frequency → rate, phase → normalized position, amplitude → gain.
- **`SamplePlayer<T>`** — воспроизведение буфера с loop-режимами.
  `LoopMode` (OneShot / Forward / PingPong), gate-управление, per-sample
  boundary check. Методы `Generator<T>`: частота отображается в rate,
  фаза — в normalized позицию.
- **`LoopMode`** — публичный enum для выбора стратегии зацикливания.

### ✨ rill-oscillators

- **`WavetableOscNode<T, BUF_SIZE, WT_SIZE>`** — Source-узел графа,
  обёртка над `WavetableOscillator`. Параметры: `"frequency"`,
  `"amplitude"`, `"phase"`, `"interpolation"` (choice: linear / cubic).

### ✨ rill-sampler (0.3.1)

- **`SamplePlayerNode<T, BUF_SIZE>`** — Source-узел для воспроизведения
  аудиосэмплов. Стерео (два output port — left/right). Параметры,
  automatable через patchbay: `"gate"`, `"rate"`, `"loop_mode"`,
  `"start"`, `"end"`, `"amplitude"`, `"interpolation"`, `"position"` (read-only).
- **`SampleBuffer<T>`** — контейнер для загруженных сэмплов с метаданными
  (sample_rate, channels, name). Mono / stereo deinterleaved.
- **WAV loading** (feature `"wav"`) — 16-bit PCM, mono/stereo, через `hound`.
- **`TimeSeriesReader<T>`** — читатель неравномерных временных рядов.
  Бинарный поиск по `timestamps` → отображение времени на дробный индекс
  → `Interpolate` trait. Три стратегии: Nearest, Linear, Cubic.
- **`TimeSeriesNode<T, BUF_SIZE>`** — мультиканальный Source-узел
  (N output ports, по одному на канал). Параметры: `"sample_rate"`
  (виртуальная частота), `"interpolation"`, `"play"`, `"speed"`,
  `"position"`. Заполняет блоки planar: `[ch0_s0, ch0_s1, ..., chN_sBUF-1]`.
- **`from_csv()`** — загрузка `t,channel,value` → `TimeSeriesReader<f64>`.
  Группировка по каналам, сортировка по времени, пропуск
  непарсируемых строк.

### 🏗️ Инфраструктура

- `rill-sampler` добавлен в workspace и `rill-adrift` (feature `"sampler"`,
  включён в default). Обновлён `scripts/publish.sh`.

### 📦 Публикации на crates.io

| Крейт | Версия |
|-------|--------|
| `rill-core` | 0.3.2 |
| `rill-core-dsp` | 0.3.1 |
| `rill-sampler` | 0.3.1 |

### 📊 Статистика

| Метрика | Значение |
|---------|----------|
| Крейтов в workspace | 17 активных |
| Добавлено тестов | +46 |

---

## [0.3.0] — 2026-04-27

### 🏗️ Фундаментальные изменения

Фреймворк переписан почти с нуля. Единый `rill-core` вместо россыпи мелких крейтов, новая система очередей и сигналов, модульная архитектура DSP.

#### Ядро

- **`rill-core`** — единый крейт ядра: трейты (`AudioNode`, `ParameterId`, `PortId`, `Clock`),
  математика (`AudioNum`, вектора), буферы (кольцевые, FIFO), очереди (`CommandQueue<T>`,
  `TelemetryQueue`), время (`ClockTick`, `SystemClock`), макросы
- **Типобезопасные идентификаторы**: `ParameterId` (с валидацией), `PortId` (с типом порта:
  AudioIn, AudioOut, Control, CV)
- **Очереди как единый механизм коммуникации**: неблокирующие MPMC очереди с политиками
  переполнения, телеметрия, наблюдатель микро-контроля
- **Векторный eDSL** — обобщённые математические абстракции над `AudioNum` через трейт `Vector`,
  подготовка к SIMD

#### DSP

- **`rill-core-dsp`** — единое хранилище DSP-алгоритмов: трейт `Algorithm`, фильтры (Biquad, SVF,
  Butterworth, Chebyshev, Comb, OnePole, MoogLadder), генераторы (Sine, Saw, Square, Triangle,
  Pulse, Noise, LFO, Envelope, FM), маппинг, сглаживание
- Все алгоритмы работают через `process_block` с `ScalarVector`
- Векторные макросы (`simple_algorithm!`, `filter_algorithm!`, `effect_algorithm!`,
  `generator_algorithm!`)

#### Аналоговое моделирование

- **`rill-core-model`** — WDF-ядро: элементы (R, C, L, диод), адаптеры (последовательный,
  параллельный), анализ, MoogLadder
- **`rill-analog-filters`** — аналоговые фильтры на WDF (WdfMoogLadder, WdfRcPole)
- **`rill-analog-effects`** — аналоговые эффекты (операционный усилитель, кассетный
  декастер)

#### Граф и управление

- **`rill-graph`** — аудиограф с топологической сортировкой, Source/Processor/Sink
- **`rill-patchbay`** — мир автоматов: LFO, огибающие, случайные блуждания, сенсоры,
  серво, маппинг
- **`rill-router`** — EQ (графический, параметрический) + микшер (каналы, посылы,
  мастер)

#### Обработка

- **`rill-digital-filters`** — цифровые фильтры как Processor-узлы
- **`rill-digital-effects`** — Delay, Distortion, Limiter
- **`rill-oscillators`** — Sine, Noise, LFO, Envelope как Processor-узлы
- **`rill-lofi`** — lo-fi процессор (bitcrush, downsampling, noise, wow&flutter)

#### Ввод/вывод

- **`rill-io`** — аудио-бекенды: NullBackend, CpalBackend, ALSA, PipeWire, JACK
- **`rill-telemetry`** — пробники и коллекторы телеметрии
- **`rill-server`** — OSC-сервер для удалённого управления (UDP, encode/decode,
  диспетчеризация по паттернам)

### 🆕 Новые крейты

| Крейт | Описание |
|-------|----------|
| `rill-core` | Единое ядро (трейты, очереди, математика, макросы) |
| `rill-core-dsp` | DSP-алгоритмы (фильтры, генераторы, векторные операции) |
| `rill-core-model` | WDF-ядро (элементы, адаптеры, анализ) |
| `rill-patchbay` | Автоматы, сенсоры, серво |
| `rill-router` | EQ + микшер |
| `rill-telemetry` | Пробники и коллекторы |
| `rill-analog-filters` | Аналоговые фильтры на WDF |
| `rill-analog-effects` | Аналоговые эффекты |
| `rill-server` | OSC-сервер |

### 🗑️ Удалённые крейты

| Крейт | Замена |
|-------|--------|
| `rill-core-traits` | `rill-core` |
| `rill-signal` | `rill-core::queues` |
| `rill-buffers` | `rill-core::buffer` + `rill-core-dsp::buffer` |
| `rill-automation` | `rill-patchbay` |
| `rill-control` | `rill-patchbay` |
| `rill-eq` | `rill-router::eq` |
| `rill-mixer` | `rill-router::mixer` |
| `rill-hp` | `rill-core-dsp` (f64) |

### 📊 Статистика

| Метрика | Значение |
|---------|----------|
| Крейтов в workspace | 15 активных |
| Тестов | 300+ |
| Версия | 0.3.0 (единая для всех крейтов) |

---

## [0.2.0] — 2026-02-23

### Крупнейший рефакторинг: Единое ядро rill-core

- Создан `rill-core` (объединение `rill-core-traits` + `rill-signal`)
- Все крейты обновлены до версии 0.2.0
- `ParameterId` (экспериментальный), `PortId` выделен в отдельный модуль
- Удалены старые крейты: `rill-core-traits`, `rill-signal`
