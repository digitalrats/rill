# CHANGELOG

## [Unreleased]

### 🔄 Actor Model Rewrite — `LocalActor`, `Actor::spawn()`, Patchbay Removal

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

- **WDF SIMD (rill-core-wdf)**:
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

- **WDF tape module** in `rill-core-wdf`:
  - `RecordHead<T>`, `PlaybackHead<T>` — analog tape physics, `Algorithm<T>`
  - `OpAmp<T>` — operational amplifier as `WdfElement<T>`
  - `CassetteDeck` in `rill-analog-effects` refactored to use heads from `rill-core-wdf`

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
- `rill-analog-effects::OperationalAmplifier` — replaced by `rill_core_wdf::OpAmp`

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

- **`rill-core-wdf`** — WDF-ядро: элементы (R, C, L, диод), адаптеры (последовательный,
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
| `rill-core-wdf` | WDF-ядро (элементы, адаптеры, анализ) |
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
