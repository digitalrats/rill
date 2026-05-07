# CHANGELOG

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
