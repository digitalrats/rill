# Two-Thread Architecture — Audio + Control (current implementation)

## Overview

Реализация двухпоточной модели. **Аудиопоток (hard/soft RT)** — `AudioInput`/`AudioOutput`
с `Port::propagate`. **Поток управления (soft RT, tokio)** — `rill-patchbay::engine::PatchbayEngine`.
Общаются через `MpscQueue<ParameterCommand>`.

```
[Control Thread (soft RT)]             [Audio Thread (hard RT)]
─────────────────────────────           ─────────────────────────
  PatchbayEngine                              AudioInput / AudioOutput
  ├── Automata (LFO, Env)                           │
  ├── PortCombiner                                  │
  ├── Sequencer                              Port::propagate()
  ├── Event mappings                          recursive DAG walk
  └── Sensors                                    │
       │                                 generate() / process()
       │ MpscQueue<ParameterCommand>.push()     / consume()
       ├─────────── lock-free ───────────────►  │
       │           queue                        │
       │                                    drain MpscQueue
```

## Current State

| Component | Where | Status |
|-----------|-------|--------|
| `MpscQueue<T>` (lock-free SPSC) | `rill-core::queues::mpsc` | ✅ Implemented, cross-thread bridge |
| `SpscQueue<T, CAP>` (SPSC ring) | `rill-core::queues::spsc` | ✅ Implemented, tested |
| `CommandQueue` | `rill-core::queues::command` | ✅ Implemented |
| `Telemetry` / `TelemetryQueue` | `rill-core::queues::telemetry` | ✅ Implemented |
| `MicroControlObserver` | `rill-core::queues::observer` | ✅ Implemented, tested |
| `Automaton` trait | `rill-patchbay::control` | ✅ Implemented |
| `Servo<A: Automaton>` | `rill-patchbay::control` | ✅ Implemented |
| Sensors (acoustic, physical) | `rill-patchbay::sensor` | ✅ Implemented |
| `PatchbayControl` | `rill-patchbay::control` | ✅ Centralised automata/servo/mapping API |
| `PatchbayManager` | `rill-patchbay::manager` | ✅ Multi-thread manager, tested |
| `PortCombiner` | `rill-patchbay::port_combiner` | ✅ Conflict resolution (Absolute/Modulation) |
| `PatchbayEngine` | `rill-patchbay::engine` | ✅ **Orchestrator** — green threads + patchbay + sequencer |
| **`SignalEngine`** | *removed* | ❌ Удалён. Заменён на `Port::propagate` |
| `GraphStats` | `rill-graph::graph` | ⚠️ Определён, не используется |
| `crossbeam-channel` | `rill-graph/Cargo.toml` | ⚠️ В зависимостях, не используется в коде |

### Queue systems

Две параллельные реализации:

| Система | RT-safe | Используется |
|---------|---------|-------------|
| `crossbeam_channel` (bounded) | ✅ | `CommandQueue`, `MicroControlObserver` |
| `MpscQueue`/`SpscQueue` (lock-free, `AtomicCell`) | ✅ (zero deps) | Communication audio ↔ control |

## Current processing model

Аудиограф **не имеет внешнего движка**. Обработка живёт на коллбэке `AudioIo`:

1. Drain `MpscQueue<ParameterCommand>` (параметры от control thread)
2. `Source::generate()` / `Processor::process()` / `Sink::consume()`
3. `Port::propagate()` — рекурсивный обход DAG через `downstream_input_ptrs`

`PatchbayEngine` живёт на control thread и спавнит green threads (tokio) для:
- Автоматов (LFO, envelope — `tokio::spawn` + `tokio::time::interval`)
- `PortCombiner` (приём значений от автомата и UI, разрешение конфликтов)
- `SnapshotSequencer` (приём `CLOCK_TICK` из аудиопотока)

## Action Items

1. **Удалить `crossbeam-channel`** из `rill-graph/Cargo.toml` — не используется
2. **Убрать или использовать `GraphStats`** — определён, но мёртвый код
3. **Telemetry loop** — `MicroControlObserver` готов, но не подключён ни к одному processing loop

## Thread Safety Summary

| Компонент | Поток | Требования |
|-----------|-------|------------|
| `Port::propagate` | Audio (hard RT) | zero-alloc, lock-free |
| `AudioInput::start()` / `AudioOutput::start()` | Audio (hard/soft RT) | Stack buffers, no syscalls |
| `MpscQueue::push` | Control (soft RT) | Lock-free |
| `MpscQueue::pop` | Audio (hard RT) | Lock-free |
| `PatchbayEngine::handle_event` | Control (soft RT) | May allocate |
| `Sequencer` | Control (blocking) | spawn_blocking |
