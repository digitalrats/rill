# Inter-Process Communication via Shared Memory for rill-analyzer

**Date:** 2026-07-11
**Status:** Draft

## Motivation

The current debug infrastructure is inter-thread only — the REPL thread and CollectorThread run in the same process as the signal engine. This works for local development but cannot inspect a running production process. We need a separate debugger process (`rill-analyzer`) that connects to a live rill application.

## Architecture Overview

```
┌──────────────────────────────────────────────────────────────┐
│ ПРОЦЕСС RILL (drift, или свой хост)                         │
│                                                              │
│  RT THREAD               SpscQueue        COLLECTOR THREAD   │
│  RillGraphEngine ─────────────────────→   читает пробы       │
│  probe_slots[]                            читает команды     │
│  command_queue                             ↕ shmem ring buf  │
│  debug_control ←──────── атомики ──────→   resp → отладчик   │
│                                     │     cmd ← отладчик     │
│  process()     SpscQueue            │                        │
│                ←────────────────────│                        │
└─────────────────────────────────────│────────────────────────┘
                                      │
                        /dev/shm/rill-debug-<pid> (64KB)
                        ┌─────────────────────────────┐
                        │ ControlHeader + Ring Buffers │
                        └──────────────┬──────────────┘
                                       │
┌──────────────────────────────────────│───────────────────────┐
│ ОТЛАДЧИК (rill-analyzer)            │                       │
│                                      │                       │
│  REPL THREAD                         │                       │
│  stdin → парсер → AnalyzerCommand ───┘  (CmdRingBuffer)      │
│  stdout ← форматтер ← AnalyzerResponse ← (RespRingBuffer)    │
│                                                              │
│  SIGUSR1 ────→ процесс (уведомление о команде в буфере)      │
└──────────────────────────────────────────────────────────────┘

**Signal flow:** только отладчик посылает SIGUSR1 процессу rill.
Процесс rill НЕ посылает сигналы отладчику — ответы читаются через опрос RespRingBuffer.
```

**Принцип:** один бинарник `rill-analyzer`, три режима:

| Команда | Режим | CollectorThread |
|---|---|---|
| `rill-analyzer run graph.json` | Локальный | mpsc-каналы |
| `rill-analyzer attach <pid>` | Удалённый attach | shmem ring buffers |
| `rill-analyzer launch graph.json` | Удалённый launch | shmem ring buffers |

**Безопасность:** MVP предназначен для локальной разработки в доверенном окружении. Если злоумышленник имеет доступ к `/dev/shm` и право подключиться к процессу, он уже обладает достаточными привилегиями для атаки другими способами. Аутентификация не требуется.

## Component Design

### 1. Shared Memory Layout

Файл: `/dev/shm/rill-debug-<pid>`. Размер: 64KB.

```
Offset  Size    Поле
─────────────────────────────────────
0       4       magic:          u32 = 0x52494C4C ("RILL")
4       4       version:        u32 = 1
8       8       process_pid:    u64
16      8       debugger_pid:   u64 (0 = не подключён)
24      4       flags:          AtomicU32
28      4       cmd_capacity:   u32
32      4       resp_capacity:  u32
36      4       cmd_write_pos:  AtomicU32
40      4       cmd_read_pos:   AtomicU32
44      4       resp_write_pos: AtomicU32
48      4       resp_read_pos:  AtomicU32
52      12      _reserved:      [u8; 12]
─────────────────────────────────────
64      32704   CmdRingBuffer   (bytes [64..32768))
32768   32768   RespRingBuffer  (bytes [32768..65536))
```

#### Atomic Flags

```rust
const FLAG_PAUSED:    u32 = 0x01;  // процесс приостановлен
const FLAG_ATTACHED:  u32 = 0x02;  // отладчик подключён
const FLAG_SHUTDOWN:  u32 = 0x04;  // процесс завершается
```

#### Ring Buffer Frame Format

Каждый кадр в ring buffer:

```
[0..2)   len: u16       — длина payload в байтах (0 = пустой слот)
[2..N)   payload: [u8]  — serde_cbor сериализованный AnalyzerCommand / AnalyzerResponse
```

Максимальный размер кадра: 4096 байт. `len=0` — пропускается (wrap-around маркер).

#### Ring Buffer Алгоритм (SPSC, lock-free)

**Запись (производитель → ring buffer):**
1. Читаем read_pos (Acquire)
2. Если `(write_pos + frame_len + 2 > capacity) && read_pos > 2`: записываем `len=0` в хвост, wrap-around `write_pos = 0`
3. Если буфер полон: retry или drop (debug-данные, потеря допустима)
4. Пишем `len: u16`, затем `payload`
5. `write_pos = (write_pos + 2 + frame_len) % capacity` (Release)
6. Для CmdRingBuffer: `kill(process_pid, SIGUSR1)` — уведомить процесс

**Чтение (потребитель из ring buffer):**
1. Читаем write_pos (Acquire)
2. Пока `read_pos != write_pos`:
   - Читаем `len` по `read_pos`
   - Если `len == 0`: `read_pos = 0` (wrap), continue
   - Читаем `payload[read_pos+2..read_pos+2+len]`
   - Десериализуем
   - `read_pos = (read_pos + 2 + len) % capacity` (Release)

#### Сериализация

Используется **serde_cbor** — уже присутствует в зависимостях `rill-graph`, компактный бинарный формат. `AnalyzerCommand` и `AnalyzerResponse` получают `#[derive(Serialize, Deserialize)]`.

### 2. ShmemRegion (rill-telemetry)

Новый модуль `rill-telemetry/src/debug/ipc.rs`:

```rust
/// Владеет mmap-регионом /dev/shm/rill-debug-<pid>.
/// При Drop вызывает munmap и unlink.
pub struct ShmemRegion {
    ptr: *mut u8,
    size: usize,
}

impl ShmemRegion {
    /// Открыть существующий регион (attach mode).
    pub fn open(pid: u64) -> io::Result<Self>;

    /// Создать новый регион (listen mode — процесс rill при старте).
    pub fn create() -> io::Result<Self>;

    /// Ссылка на ControlHeader.
    pub fn header(&self) -> &ControlHeader;

    /// Записать команду в CmdRingBuffer.
    /// Возвращает false если буфер полон.
    pub fn write_command(&self, cmd: &AnalyzerCommand) -> bool;

    /// Прочитать команду из CmdRingBuffer.
    pub fn read_command(&self) -> Option<AnalyzerCommand>;

    /// Записать ответ в RespRingBuffer.
    pub fn write_response(&self, resp: &AnalyzerResponse) -> bool;

    /// Прочитать ответ из RespRingBuffer.
    pub fn read_response(&self) -> Option<AnalyzerResponse>;
}

impl Drop for ShmemRegion { /* munmap, unlink */ }

// Safety: ShmemRegion владеет mmap регионом эксклюзивно.
unsafe impl Send for ShmemRegion {}
unsafe impl Sync for ShmemRegion {}
```

**unsafe-блок:** `rill-telemetry` НЕ имеет `#![deny(unsafe_code)]`, поэтому mmap и сырые указатели допустимы. `unsafe` ограничен модулем `ipc.rs`.

### 3. CollectorThread — два режима

`CollectorThread::spawn()` получает новый параметр `ipc: Option<ShmemRegion>`:

**mpsc-режим (`ipc = None`):** текущее поведение без изменений — команды через `mpsc::Receiver`, ответы через `mpsc::Sender`.

**shmem-режим (`ipc = Some(shmem)`):**

1. Вместо `cmd_rx.try_recv()` — вызывает `shmem.read_command()`
2. Вместо `resp_tx.send(resp)` — вызывает `shmem.write_response(&resp)`
3. Отслеживает `FLAG_PAUSED` в ControlHeader:
   - Установлен → `debug_control.pause()`
   - Снят → `debug_control.cont()`
4. При `FLAG_SHUTDOWN` — завершает цикл
5. Вместо `thread::sleep(5ms)` — `sleep(5ms)` или быстрый опрос при получении SIGUSR1

### 4. Инициализация в rill-adrift

В хосте (`drift` или другой пользователь rill), при `#[cfg(feature = "debug")]`:

```rust
// При старте процесса создаём shmem регион
#[cfg(feature = "debug")]
let shmem = rill_telemetry::debug::ipc::ShmemRegion::create().ok();

// Передаём в Analyzer::launch() или напрямую в CollectorThread
```

Это **не** изменение в rill-lang, rill-graph, или rill-core — только код хоста.

### 5. Сценарии

#### Attach: `rill-analyzer attach <pid>`

1. Отладчик делает `ShmemRegion::open(pid)`
2. Проверяет `magic == "RILL"`, `version == 1`
3. Записывает свой PID в `debugger_pid`
4. Устанавливает `FLAG_ATTACHED`
5. Процесс обнаруживает флаг и начинает обработку команд из CmdRingBuffer
6. Отладчик читает RespRingBuffer, выводит в REPL

#### Launch: `rill-analyzer launch <target>`

Гибридный режим с разрешением по расширению файла:

**Если `<target>` заканчивается на `.json` (граф rill):**
1. Отладчик делает `ShmemRegion::create()` (получает `/dev/shm/rill-debug-<my-pid>`)
2. `fork()` + `exec("drift", "--graph", "<target>")` с `RILL_DEBUG_SHMEM=/dev/shm/rill-debug-<ppid>` в окружении
3. Дочерний процесс (drift) делает `ShmemRegion::open_from_env("RILL_DEBUG_SHMEM")`
4. Устанавливает `FLAG_ATTACHED`
5. Отладчик ждёт флаг, затем активирует REPL

**Если `<target>` — не `.json` (произвольный бинарник/команда):**
1. Отладчик делает `ShmemRegion::create()`
2. `fork()` + `exec(<target>)` с `RILL_DEBUG_SHMEM=...` в окружении
3. Дочерний процесс должен сам создать/открыть shmem через `rill_telemetry::debug::ipc`
4. Отладчик ждёт `FLAG_ATTACHED` от дочернего процесса, затем активирует REPL

**Явная команда:** `rill-analyzer launch -- cargo run --example chiptune_stc` — всё после `--` передаётся как команда в `execvp`.

#### Local: `rill-analyzer run graph.json` (без изменений)

Текущее поведение: загрузка GraphDef, создание RillGraphEngine, `Analyzer::launch()` с `ipc = None`.

### 6. CLI (rill-analyzer/src/main.rs)

```rust
enum Commands {
    /// Локальная отладка (текущее поведение).
    Run {
        graph: PathBuf,
        #[arg(long)] no_repl: bool,
        #[arg(long)] json: bool,
        #[arg(long)] log: Option<PathBuf>,
        #[arg(long)] script: Option<PathBuf>,
    },

    /// Подключиться к работающему процессу через shmem.
    Attach {
        pid: u64,
        #[arg(long)] json: bool,
    },

    /// Запустить цель и подключиться отладчиком.
    ///
    /// Если target заканчивается на .json — запускается drift с этим графом.
    /// Иначе target трактуется как бинарник/команда.
    /// Всё после "--" передаётся как явная команда в execvp.
    Launch {
        /// Граф (.json) или бинарник для запуска.
        target: String,

        /// Дополнительные аргументы для целевого процесса (после target).
        #[arg(last = true)]
        args: Vec<String>,

        #[arg(long)] json: bool,
    },
}
```

**Примеры:**
```bash
rill-analyzer launch graph.json                    # drift --graph graph.json
rill-analyzer launch ./my-app --verbose            # exec ./my-app --verbose
rill-analyzer launch -- cargo run --example chip   # exec cargo run --example chip
rill-analyzer run graph.json                       # локально (без fork)
```

### 7. Зависимости и feature gates

- `rill-telemetry/Cargo.toml` — новый dependency: `serde_cbor` (или используется транзитивно через `rill-graph`; уточнить)
- `rill-lang` — без изменений
- `rill-graph` — без изменений
- `rill-analyzer` — добавляет IPC-режимы в CLI
- `rill-adrift` — `#[cfg(feature = "debug")]` создаёт `ShmemRegion`

### 8. Не входит в MVP

- Поддержка нескольких одновременных отладчиков
- Аутентификация / безопасность shmem
- Удалённая отладка через сеть (TCP)
- Сжатие / шифрование трафика
- Автоматический выбор свободного порта / shmem-имени

## Implementation Plan Outline

| Phase | What | Crate |
|---|---|---|
| 1 | `ShmemRegion`, ring buffer read/write, ControlHeader | `rill-telemetry` (new `debug/ipc.rs`) |
| 2 | `AnalyzerCommand` + `AnalyzerResponse` Serialize/Deserialize | `rill-telemetry` |
| 3 | CollectorThread shmem-режим (флаг `ipc`) | `rill-telemetry` |
| 4 | CLI: `attach <pid>` и `launch <graph>` команды | `rill-analyzer` |
| 5 | Инициализация shmem в `rill-adrift` под `debug` feature | `rill-adrift` |
| 6 | Интеграционные тесты | `rill-telemetry` + `rill-analyzer` |

## Open Questions

- **serde_cbor vs postcard:** serde_cbor уже в дереве зависимостей (через rill-graph), но postcard компактнее. Для MVP — serde_cbor.
- **Обработка SIGUSR1:** CollectorThread в shmem-режиме должен просыпаться по сигналу. Нужно использовать `sigaction` + `AtomicBool` или `signalfd` (Linux-only). Предпочтительнее signalfd — интеграция с poll/select.
- **Завершение shmem:** при падении процесса `unlink` не вызывается, файл остаётся в `/dev/shm`. Нужен механизм обнаружения мёртвых регионов (проверка `/proc/<pid>`).
