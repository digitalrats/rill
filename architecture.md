Отлично! Обновляю `architecture.md` в соответствии с текущим состоянием кодовой базы.

## Kama Audio: Архитектура проекта (актуальная версия)

### Общая концепция
Kama Audio — это модульный open-source фреймворк для создания аудиоприложений на Rust. Ключевая идея: минимальное стабильное ядро + набор специализированных крейтов, расширяющих функциональность. Все крейты успешно компилируются и проходят тесты.

### Текущее состояние проекта ✅
```
kama-core-traits/     # ✅ 6 тестов - базовые трейты (ядро)
kama-buffers/         # ✅ 21 тест - управление буферами
kama-automation/      # ✅ 17 тестов - автоматизация параметров
kama-control/         # ✅ 2 теста - управление (MIDI)
kama-signal/          # ✅ 18 тестов - сигнальная система
kama-graph/           # ✅ 12 тестов - аудиограф
kama-dsp-common/      # ✅ 11 тестов - общие утилиты для DSP
```

### Структура крейтов (реализовано)

#### 1. Базовые крейты (ядро и фундаментальные абстракции)
| Крейт | Назначение | Ключевые компоненты |
|-------|------------|---------------------|
| **kama-core-traits** | Минимальное ядро с трейтами | `AudioNode`, `ParamValue`, `NodeId`, `PortId`, `Clock`, `TimeProvider`, `SystemClock`, `AudioError` |
| **kama-signal** | Сигнальная система | `Signal` (маркер), `ParameterChanged`, `ClockTick`, `SystemEvent`, `SignalBus<T>`, `BusConfig`, `OverflowPolicy`, `SimpleSignalDispatcher` |

#### 2. Утилитарные крейты
| Крейт | Назначение | Ключевые компоненты |
|-------|------------|---------------------|
| **kama-buffers** | Работа с буферами | `RingBuffer`, `BufferPool`, `MultiHeadBuffer`, `BufferHead`, `BufferManager`, `ReadMode` (Simple, Loop, PingPong, Granular) |
| **kama-graph** | Аудиограф | `AudioGraph`, `Connection`, `GraphBufferManager`, `NodeProcessor`, топологическая сортировка |

#### 3. Автоматизация и управление
| Крейт | Назначение | Ключевые компоненты |
|-------|------------|---------------------|
| **kama-automation** | Автоматизация параметров | `Automaton` (трейт), `Servo`, `AutomationManager`, `LfoAutomaton`, `EnvelopeState`, `ParameterMapping`, интеграция с `SignalSender` |
| **kama-control** | MIDI-управление | `ControlBackend`, `MidiBackend`, `ControlNode`, `Mapping`, `EventPattern`, `Transform`, интеграция с `kama-signal` |

#### 4. DSP-инфраструктура
| Крейт | Назначение | Ключевые компоненты |
|-------|------------|---------------------|
| **kama-dsp-common** | Общие утилиты для DSP | `DspContext`, конструкторы узлов (`stateless_fn_node`, `stateful_fn_node`, `block_fn_node`), макросы (`effect!`, `filter!`, `generator!`) |

### Структура workspace (актуальная)
```toml
[workspace]
members = [
    "kama-core-traits",      # ядро
    "kama-buffers",          # буферы
    "kama-graph",            # граф
    "kama-automation",       # автоматизация
    "kama-control",          # управление
    "kama-signal",           # сигналы
    "kama-dsp-common",       # DSP инфраструктура
    # Следующие крейты в разработке:
    # "kama-digital-oscillators",
    # "kama-digital-filters",
    # "kama-digital-effects",
    # "kama-digital-eq",
]
```

### Реализованные компоненты в деталях

#### `kama-core-traits`
Базовые трейты, от которых зависят все остальные крейты:
- `AudioNode` — основной трейт для всех аудиоузлов
- `ParamValue` — типы параметров (Float, Int, Bool, String, Choice)
- `NodeId`, `PortId` — идентификаторы
- `Clock` / `TimeProvider` — работа со временем
- `SystemClock` — эталонная реализация часов

#### `kama-buffers`
Продвинутая система управления буферами:
- `RingBuffer` — кольцевой буфер с интерполяцией
- `BufferHead` — головки воспроизведения с разными режимами
- `MultiHeadBuffer` — многоголовый буфер (до 8 головок)
- `BufferManager` — пул буферов с acquire/release
- `ReadMode` — Simple, Loop, PingPong, Granular

#### `kama-graph`
Полноценная реализация аудиографа:
- Топологическая сортировка для определения порядка обработки
- Управление соединениями между узлами
- Интеграция с `BufferManager` для эффективного управления памятью
- Кэширование буферов узлов

#### `kama-automation`
Система автоматизации параметров:
- `Automaton` трейт для создания автоматов (LFO, огибающие)
- `Servo` — сервопривод, связывающий автомат с параметром
- `AutomationManager` — центральный менеджер
- Интеграция с `kama-signal` для отправки изменений

#### `kama-control`
MIDI и управление:
- `MidiBackend` — работа с MIDI-устройствами в отдельном потоке
- `ControlNode` — узел для графа, преобразующий MIDI-события в параметры
- Система маппинга с разными типами преобразований

#### `kama-signal`
Гибкая сигнальная система:
- `SignalBus<T>` — многопоточная шина с политиками переполнения
- `SimpleSignalDispatcher` — синхронный диспетчер
- Готовые типы сигналов: `ParameterChanged`, `ClockTick`, `SystemEvent`

#### `kama-dsp-common`
Фундамент для всех DSP-крейтов:
- `DspContext` — контекст выполнения с временем, параметрами, буферами
- Конструкторы функциональных узлов для быстрого создания эффектов
- Макросы для минимального бойлерплейта

### Ключевые архитектурные решения (реализованные)

1. **Минимальное ядро** (`kama-core-traits`) — только трейты и базовые типы, никакой логики
2. **Реестр узлов** — глобальный реестр в `kama-graph` для создания узлов по имени
3. **TimeProvider** — единый источник времени для всех компонентов
4. **SignalBus** — гибкая шина событий с настраиваемыми каналами
5. **DSP как функции** — возможность создавать узлы из простых функций
6. **Клиент-серверная модель** — через `kama-control` (MIDI) и будущий `kama-server`
7. **Многопоточность** — бэкенды в отдельных потоках, lock-free структуры где возможно

### Планируемые крейты (следующие шаги)

```
kama-digital-oscillators/     # осцилляторы (следующий)
kama-digital-filters/         # цифровые фильтры
kama-digital-effects/         # цифровые эффекты
kama-digital-eq/              # эквалайзеры
kama-mixer/                   # микшер (на базе kama-core-traits)
kama-server/                  # сервер с OSC API
kama-client/                  # CLI утилита
```

### Зависимости между крейтами (актуальные)
```mermaid
graph TD
    A[kama-core-traits] --> B[kama-buffers]
    A --> C[kama-signal]
    A --> D[kama-automation]
    A --> E[kama-control]
    A --> F[kama-dsp-common]
    
    B --> G[kama-graph]
    C --> D
    C --> E
    
    F --> H[kama-digital-*]    # будущие DSP-крейты
    G --> H
    B --> H
    
    D --> I[kama-server]       # будущий
    E --> I
    H --> I
    G --> I
```

### Заключение
Проект полностью готов к следующему этапу — созданию цифровых DSP-крейтов. Все базовые компоненты работают, протестированы и готовы к использованию. Архитектура обеспечивает модульность, расширяемость и производительность.